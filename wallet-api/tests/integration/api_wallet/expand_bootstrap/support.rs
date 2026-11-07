use crate::harness::worker::{
    MockBackendRecorder, decrypt_captured_api_backend_body, worker_context_without_actors,
};
use wallet_database::{
    entities::expand_batch::ExpandBatchStatus,
    repositories::api_wallet::expand_batch::ExpandBatchRepo,
};

pub fn run_in_child(name: &str) -> bool {
    if std::env::var("WALLET_EXPAND_BOOTSTRAP_CHILD").as_deref() == Ok(name) {
        return false;
    }
    let full = format!("integration::api_wallet::expand_bootstrap::{name}");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([full.as_str(), "--exact", "--nocapture"])
        .env("WALLET_EXPAND_BOOTSTRAP_CHILD", name)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

pub struct Scenario {
    ctx: &'static wallet_api::Context,
    recorder: MockBackendRecorder,
    pool: wallet_database::ApiWalletDbPool,
    batch: String,
    uid: String,
}

impl Scenario {
    pub async fn given_done_batch() -> Self {
        let (ctx, db_dir, recorder) = worker_context_without_actors().await;
        let uid = format!("expand-bootstrap-{}", uuid::Uuid::new_v4().simple());
        let batch = format!("batch-{uid}");
        let pool = crate::harness::open_api_wallet_pool(&db_dir).await;
        ExpandBatchRepo::create_batch(&pool, &uid, &batch, "fixture-serial", "base", 1)
            .await
            .unwrap();
        sqlx::query("UPDATE expand_batch SET status=2, local_complete_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE batch_id=?")
            .bind(&batch).execute(pool.write_ref()).await.unwrap();
        Self { ctx, recorder, pool, batch, uid }
    }

    pub fn given_next_connection_drops(&self) {
        self.recorder.disconnect_next_backend_call();
    }

    pub async fn when_unlock_bootstrap_runs(
        &self,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        wallet_api::testkit::expand::start_after_unlock(self.ctx).await
    }

    pub async fn then_scanner_running_and_batch_recovers(&self) {
        let handle = wallet_api::testkit::expand::scanner_handle(self.ctx)
            .await
            .expect("scanner channel must be installed");
        wallet_api::infrastructure::runtime::time::with_timeout(
            std::time::Duration::from_secs(10),
            async {
                loop {
                    let batch =
                        ExpandBatchRepo::get_batch(&self.pool, &self.batch).await.unwrap().unwrap();
                    if batch.status == ExpandBatchStatus::Notified {
                        assert!(batch.local_complete_at.is_some());
                        assert!(batch.expand_complete_at.is_some());
                        break;
                    }
                    handle.hint().await;
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            },
        )
        .await
        .expect("scanner must retry and finish notification after connection recovery");
    }

    pub async fn then_repeated_unlock_preserves_scanner(&self) {
        let first = wallet_api::testkit::expand::scanner_handle(self.ctx).await.unwrap();
        self.when_unlock_bootstrap_runs().await.unwrap();
        let second = wallet_api::testkit::expand::scanner_handle(self.ctx).await.unwrap();
        assert!(first.same_channel(&second), "repeat unlock must not replace scanner");
    }

    pub async fn then_completed_with_matching_backend_requests(&self, expected: usize) {
        let requests = self.recorder.snapshot();
        assert_eq!(
            requests.len(),
            expected,
            "notification must retry only failed request and not duplicate success"
        );
        for req in requests {
            let payload = decrypt_captured_api_backend_body(&req.body);
            assert_eq!(payload["uid"], self.uid);
            assert_eq!(payload["batchId"], self.batch);
            assert_eq!(payload["serialNo"], "fixture-serial");
        }
    }
}
