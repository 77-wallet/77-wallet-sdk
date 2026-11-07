use crate::harness::{self, ApiWalletBackendCall, TestEnv};
use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Barrier, mpsc};
use wallet_database::{
    ApiWalletDbPool, entities::api_wallet::ApiWalletType,
    repositories::api_wallet::wallet::ApiWalletRepo,
};
use wallet_transport_backend::response_vo::api_wallet::wallet::UidStatus;

const PHRASE: &str =
    "";
const HOLD_MS: u64 = 200;

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=");
    line.split_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))
        .map(|value| value.trim_matches('"'))
}

#[derive(Clone)]
struct TraceWriter {
    lines: Arc<Mutex<String>>,
    mark_init: mpsc::UnboundedSender<()>,
}
impl Write for TraceWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(bytes);
        if field(&text, "stage") == Some("db_mark_init") && field(&text, "event") == Some("begin") {
            let _ = self.mark_init.send(());
        }
        self.lines.lock().unwrap().push_str(&text);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct CreationScaleProbe {
    env: &'static TestEnv,
    pool: ApiWalletDbPool,
    background_address: String,
    background_uid: String,
    count: i64,
    trace: Arc<Mutex<String>>,
    mark_init: mpsc::UnboundedReceiver<()>,
}

impl CreationScaleProbe {
    pub async fn given() -> Self {
        // Same diagnostic writer pattern as the existing deletion experiment.
        let trace = Arc::new(Mutex::new(String::new()));
        let (send, mark_init) = mpsc::unbounded_channel();
        let writer = TraceWriter { lines: trace.clone(), mark_init: send };
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_env_filter("off,api_wallet_create_perf=info,wallet_database::db_pool=info")
            .with_writer(move || writer.clone())
            .try_init()
            .expect("run this experiment as a standalone exact test");
        let env = harness::ensure_env().await;
        let pool = wallet_api::testkit::mqtt::api_wallet_pool(env.manager.ctx()).unwrap();
        let background_uid = harness::next_tag("creation-background");
        let background_address = harness::upsert_wallet(
            &env.db_dir,
            &env.sn,
            &background_uid,
            ApiWalletType::SubAccount,
            None,
        )
        .await;
        Self { env, pool, background_address, background_uid, count: 0, trace, mark_init }
    }

    pub async fn given_address_count(&mut self, count: i64) {
        assert!(count > self.count && count <= 500_000);
        let setup = Instant::now();
        // SQL rows, not a 500k-element Vec; no user keys, asset balances or real addresses.
        let rows = sqlx::query("WITH RECURSIVE n(x) AS (VALUES(?) UNION ALL SELECT x+1 FROM n WHERE x < ?) INSERT INTO api_account (account_id,name,address,pubkey,address_type,wallet_address,uid,derivation_path,derivation_path_index,chain_code,api_wallet_type,status,is_init,is_expand,is_used,created_at) SELECT x,'creation-scale',printf('creation-scale-%012d',x),'synthetic-public','',?,?,'synthetic-path',x,'base',1,1,0,0,0,'2026-11-02T00:00:00Z' FROM n")
            .bind(self.count + 1).bind(count).bind(&self.background_address)
            .bind(&self.background_uid).execute(self.pool.write_ref()).await.unwrap().rows_affected();
        assert_eq!(rows as i64, count - self.count);
        self.count = count;
        self.then_background_unchanged().await;
        println!(
            "CREATE_SCALE setup_rows={count} setup_ms={:.3} (excluded)",
            setup.elapsed().as_secs_f64() * 1000.0
        );
    }

    pub async fn then_background_unchanged(&self) {
        let (count, valid): (i64, i64) = sqlx::query_as("SELECT COUNT(*),COALESCE(SUM(CASE WHEN account_id BETWEEN 1 AND ? AND address=printf('creation-scale-%012d',account_id) AND uid=? AND chain_code='base' AND name='creation-scale' AND status=1 AND is_used=0 THEN 1 ELSE 0 END),0) FROM api_account WHERE wallet_address=?")
            .bind(self.count).bind(&self.background_uid).bind(&self.background_address)
            .fetch_one(self.pool.read_ref()).await.unwrap();
        assert_eq!((count, valid), (self.count, self.count));
        let all: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_account")
            .fetch_one(self.pool.read_ref())
            .await
            .unwrap();
        assert_eq!(all, self.count, "creation must not materialize background addresses");
    }

    pub async fn when_create_pairs(&mut self, case: &str, contended: bool, repeats: usize) {
        for repeat in 1..=repeats {
            let sub = self
                .create_one(case, repeat, ApiWalletType::SubAccount, None, contended, false)
                .await;
            self.create_one(case, repeat, ApiWalletType::Withdrawal, Some(&sub), contended, false)
                .await;
            self.then_background_unchanged().await;
        }
    }

    pub async fn when_bind_info_fails(&mut self) {
        self.create_one("bind_info_failure", 1, ApiWalletType::SubAccount, None, false, true).await;
    }

    async fn create_one(
        &mut self,
        case: &str,
        repeat: usize,
        kind: ApiWalletType,
        binding: Option<&str>,
        contended: bool,
        fail_bind_info: bool,
    ) -> String {
        harness::reset_fake(self.env);
        self.env.fake_backend.enqueue_keys_uid_status(UidStatus::NotFound);
        if fail_bind_info {
            self.env.fake_backend.set_query_uid_bind_info_error(Some("synthetic bind failure"));
        } else {
            self.env.fake_backend.enqueue_query_uid_bind_info("", "", false, &self.env.sn);
        }
        let salt = harness::next_tag("creation-scale");
        let expected_uid = harness::derive_uid(PHRASE, &salt);
        let bind_query_uid = if let Some(address) = binding {
            ApiWalletRepo::find_by_address(&self.pool, address).await.unwrap().unwrap().uid
        } else {
            expected_uid.clone()
        };
        self.trace.lock().unwrap().clear();
        while self.mark_init.try_recv().is_ok() {}
        let gate = Arc::new(Barrier::new(2));
        if contended {
            self.env.fake_backend.gate_next_old_keys_init(gate.clone());
        }
        let pool = self.pool.clone();
        let background = self.background_address.clone();
        let mark_init = &mut self.mark_init;
        let competing_writer = async {
            if !contended {
                return;
            }
            // Admit a real competing SQLite/application-gate writer before mark_init.
            gate.wait().await;
            let writer = pool.lock_write().await;
            let mut tx = pool.write_ref().begin().await.unwrap();
            sqlx::query("UPDATE api_account SET is_used=is_used WHERE wallet_address=? AND account_id<=1000")
                .bind(&background).execute(&mut *tx).await.unwrap();
            gate.wait().await;
            mark_init.recv().await.expect("creation entered mark_init");
            // Artificial controlled hold, NOT real recovery-worker latency.
            tokio::time::sleep(Duration::from_millis(HOLD_MS)).await;
            tx.commit().await.unwrap();
            drop(writer);
        };
        let creation = self.env.manager.create_api_wallet(
            1,
            PHRASE,
            &salt,
            "synthetic-create",
            harness::SMOKE_WALLET_PASSWORD,
            None,
            kind,
            binding,
        );
        let started = Instant::now();
        let (result, ()) = tokio::time::timeout(Duration::from_secs(120), async {
            tokio::join!(creation, competing_writer)
        })
        .await
        .expect("creation/controlled-writer must terminate");
        let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
        if fail_bind_info {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap(), expected_uid);
        }
        let wallet = ApiWalletRepo::find_by_uid(&self.pool, &expected_uid).await.unwrap().unwrap();
        assert_eq!(wallet.api_wallet_type, kind);
        assert_eq!(wallet.is_init, 1);
        if let Some(binding) = binding {
            assert_eq!(wallet.binding_address.as_deref(), Some(binding));
        }
        self.env.fake_backend.with_calls(|calls| {
            assert_eq!(calls.len(), 4);
            assert!(matches!(&calls[0], ApiWalletBackendCall::KeysUidCheck { uid } if uid==&expected_uid));
            assert!(matches!(&calls[1], ApiWalletBackendCall::InitApiWallet(_)));
            assert!(matches!(&calls[2], ApiWalletBackendCall::OldKeysInit(_)));
            assert!(matches!(&calls[3], ApiWalletBackendCall::QueryUidBindInfo { uid } if uid==&bind_query_uid));
        });
        let logs = self.trace.lock().unwrap().clone();
        let total = logs
            .lines()
            .find(|l| field(l, "stage") == Some("total") && field(l, "event") == Some("end"))
            .expect("INFO total is emitted through actual Manager entrypoint");
        assert_eq!(field(total, "outcome"), Some(if fail_bind_info { "error" } else { "ok" }));
        let call_id = field(total, "call_id").unwrap();
        println!(
            "CREATE_SCALE case={case} repeat={repeat} kind={kind:?} rows={} wall_ms={wall_ms:.3} controlled_hold_ms={}",
            self.count,
            if contended { HOLD_MS } else { 0 }
        );
        for line in logs
            .lines()
            .filter(|l| field(l, "event") == Some("end") && field(l, "call_id") == Some(call_id))
        {
            println!(
                "CREATE_STAGE case={case} repeat={repeat} kind={kind:?} stage={} elapsed_ms={} outcome={}",
                field(line, "stage").unwrap(),
                field(line, "elapsed_ms").unwrap(),
                field(line, "outcome").unwrap()
            );
        }
        if contended {
            let wait = logs
                .lines()
                .find(|l| {
                    field(l, "metric") == Some("writer_gate_wait_ms")
                        && field(l, "op") == Some("mark_wallet_init")
                })
                .expect("controlled writer wait is observable");
            println!(
                "CREATE_GATE case={case} repeat={repeat} kind={kind:?} wait_ms={}",
                field(wait, "value_ms").unwrap()
            );
        }
        wallet.address
    }
}
