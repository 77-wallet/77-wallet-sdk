use super::{DeletionPhase, DeletionScenario};
use std::time::{Duration, Instant};

pub fn normal_scale_parameters() -> (usize, String) {
    let count: usize = std::env::var("WALLET_DELETION_TEST_ADDRESSES")
        .expect("set characterization address count")
        .parse()
        .unwrap();
    let mode = std::env::var("WALLET_DELETION_TEST_MODE").expect("set plain/fast/changed");
    assert!([100, 1000, 10000, 500000].contains(&count));
    assert!(["plain", "fast", "changed"].contains(&mode.as_str()));
    (count, mode)
}

impl DeletionScenario {
    pub async fn when_normal_scale_timing(
        &self,
        mut phases: tokio::sync::mpsc::UnboundedReceiver<DeletionPhase>,
        addresses: usize,
        mode: &str,
    ) {
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
            let mut jobs = Vec::new();
            let writer_end = loop {
                let event = tokio::time::timeout(Duration::from_secs(30), phases.recv())
                    .await
                    .unwrap()
                    .expect("deletion phase event");
                if event.name == "writer_occupied" {
                    break event.at;
                }
                if mode == "plain" {
                    continue;
                }
                let write = event.name == "writer_reserved" || mode == "changed";
                let pool = pool.clone();
                let external = external.clone();
                let address = self.other_address.clone();
                // Independent phase jobs: initial writes must not postpone the
                // final-window probe until after a small deletion has finished.
                jobs.push(tokio::spawn(async move {
                    let reader = async {
                        let start = Instant::now();
                        let present: i64 =
                            sqlx::query_scalar("SELECT COUNT(*) FROM api_wallet WHERE address=?")
                                .bind(&address)
                                .fetch_one(pool.read_ref())
                                .await
                                .unwrap();
                        assert_eq!(present, 1);
                        (start, start.elapsed().as_micros())
                    };
                    let writer = async {
                        let start = Instant::now();
                        if write {
                            let _gate = pool.lock_write().await;
                            let row = sqlx::query(
                                "UPDATE api_wallet SET name=name || '-probe' WHERE address=?",
                            )
                            .bind(&address)
                            .execute(pool.write_ref())
                            .await
                            .unwrap();
                            assert_eq!(row.rows_affected(), 1);
                        }
                        (start, start.elapsed().as_micros())
                    };
                    let outside = async {
                        let start = Instant::now();
                        if write {
                            let row = sqlx::query(
                                "UPDATE api_wallet SET name=name || '-probe' WHERE address=?",
                            )
                            .bind(&address)
                            .execute(&external)
                            .await
                            .unwrap();
                            assert_eq!(row.rows_affected(), 1);
                        }
                        (start, start.elapsed().as_micros())
                    };
                    let (read, pooled, outside) = tokio::join!(reader, writer, outside);
                    (event, read, pooled, outside, write)
                }));
            };
            let mut observations = Vec::new();
            for job in jobs {
                let (event, read, pooled, outside, write) = job.await.unwrap();
                let overlap = [read.0, pooled.0, outside.0].iter().all(|at| *at < writer_end);
                if event.name == "writer_reserved" {
                    assert!(overlap, "probe missed the writer window; cannot claim no waiting");
                }
                observations.push(serde_json::json!({
                    "phase":event.name,"read_us":read.1,
                    "pooled_write_us":if write {Some(pooled.1)} else {None},
                    "external_write_us":if write {Some(outside.1)} else {None},
                    "read_dispatch_us":read.0.duration_since(event.at).as_micros(),
                    "pooled_dispatch_us":pooled.0.duration_since(event.at).as_micros(),
                    "external_dispatch_us":outside.0.duration_since(event.at).as_micros(),
                    "started_before_writer_end":overlap
                }));
            }
            observations
        };
        let deletion = async {
            let started = Instant::now();
            self.when_delete_subaccount().await.unwrap();
            started.elapsed().as_micros()
        };
        let (delete_us, observations) = tokio::time::timeout(Duration::from_secs(60), async {
            tokio::join!(deletion, monitor)
        })
        .await
        .expect("normal-scale deletion/observers must finish");
        let suffixes = match mode {
            "plain" => 0,
            "fast" => 2,
            "changed" => 4,
            _ => unreachable!(),
        };
        let name: String = sqlx::query_scalar("SELECT name FROM api_wallet WHERE address=?")
            .bind(&self.other_address)
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
        assert_eq!(
            name,
            format!("{original_name}{}", "-probe".repeat(suffixes)),
            "all competing writes must commit actual changes"
        );
        println!(
            "NORMAL_SCALE {}",
            serde_json::json!({
                "addresses":addresses,"mode":mode,"delete_us":delete_us,"probes":observations
            })
        );
        external.close().await;
    }
}
