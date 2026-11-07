use super::DeletionScenario;

// Standalone ignored latency experiment: observe real production stage events,
// without adding timing hooks or sleeps to the deletion implementation.
pub struct DeletionPhase {
    pub(super) name: &'static str,
    pub(super) at: std::time::Instant,
}

pub fn deletion_phase_trace() -> tokio::sync::mpsc::UnboundedReceiver<DeletionPhase> {
    let (send, receive) = tokio::sync::mpsc::unbounded_channel();
    struct PhaseWriter(tokio::sync::mpsc::UnboundedSender<DeletionPhase>);
    impl std::io::Write for PhaseWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let text = String::from_utf8_lossy(bytes);
            for phase in ["read_snapshot_opened", "writer_reserved", "writer_occupied"] {
                if text.contains(&format!("stage=\"{phase}\"")) {
                    let _ =
                        self.0.send(DeletionPhase { name: phase, at: std::time::Instant::now() });
                }
            }
            std::io::Write::write_all(&mut std::io::stdout(), bytes)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    tracing_subscriber::fmt()
        .with_env_filter("off,wallet_deletion_perf=info")
        .with_ansi(false)
        .with_writer(move || PhaseWriter(send.clone()))
        .try_init()
        .expect("run latency experiment as a standalone exact test");
    receive
}

impl DeletionScenario {
    pub async fn when_delete_with_read_write_probes(
        &self,
        mut phases: tokio::sync::mpsc::UnboundedReceiver<DeletionPhase>,
    ) {
        use std::time::{Duration, Instant};
        // Use the SAME application pool/gate, plus a separate SQLite writer.
        let pool = wallet_api::testkit::mqtt::api_wallet_pool(self.env.manager.ctx()).unwrap();
        let external = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                pool.write_ref()
                    .connect_options()
                    .as_ref()
                    .clone()
                    .busy_timeout(Duration::from_secs(60)),
            )
            .await
            .unwrap();
        let original_name: String =
            sqlx::query_scalar("SELECT name FROM api_wallet WHERE address=?")
                .bind(&self.other_address)
                .fetch_one(pool.read_ref())
                .await
                .unwrap();
        let monitor = async {
            for expected in ["read_snapshot_opened", "writer_reserved"] {
                let phase = tokio::time::timeout(Duration::from_secs(120), phases.recv())
                    .await
                    .unwrap()
                    .expect("deletion stage event");
                let phase = phase.name;
                assert_eq!(phase, expected);
                let reader = async {
                    let start = Instant::now();
                    let present: i64 =
                        sqlx::query_scalar("SELECT COUNT(*) FROM api_wallet WHERE address=?")
                            .bind(&self.other_address)
                            .fetch_one(pool.read_ref())
                            .await
                            .unwrap();
                    assert_eq!(present, 1);
                    start.elapsed()
                };
                let writer = async {
                    let start = Instant::now();
                    let _guard = pool.lock_write().await;
                    sqlx::query("UPDATE api_wallet SET name=name || '-probe' WHERE address=?")
                        .bind(&self.other_address)
                        .execute(pool.write_ref())
                        .await
                        .unwrap();
                    start.elapsed()
                };
                let outside_writer = async {
                    let start = Instant::now();
                    sqlx::query("UPDATE api_wallet SET name=name || '-probe' WHERE address=?")
                        .bind(&self.other_address)
                        .execute(&external)
                        .await
                        .unwrap();
                    start.elapsed()
                };
                let (read, write, outside) = tokio::join!(reader, writer, outside_writer);
                println!(
                    "WALLET-2980 operation_wait phase={phase} read_ms={} pooled_write_ms={} external_write_ms={}",
                    read.as_millis(),
                    write.as_millis(),
                    outside.as_millis()
                );
                assert!(read < Duration::from_secs(5), "reader must progress in WAL mode");
                if phase == "read_snapshot_opened" {
                    assert!(
                        write < Duration::from_secs(5),
                        "first snapshot must leave app writer available"
                    );
                    assert!(
                        outside < Duration::from_secs(5),
                        "first snapshot must leave SQLite writer available"
                    );
                }
            }
        };
        let started = Instant::now();
        let (result, ()) = tokio::join!(self.when_delete_subaccount(), monitor);
        result.unwrap();
        println!(
            "WALLET-2980 contended deletion addresses=500000 elapsed_ms={}",
            started.elapsed().as_millis()
        );
        let updated_name: String =
            sqlx::query_scalar("SELECT name FROM api_wallet WHERE address=?")
                .bind(&self.other_address)
                .fetch_one(pool.read_ref())
                .await
                .unwrap();
        assert_eq!(
            updated_name,
            format!("{original_name}{}", "-probe".repeat(4)),
            "all four competing writes must actually commit changed values"
        );
        external.close().await;
    }
}
