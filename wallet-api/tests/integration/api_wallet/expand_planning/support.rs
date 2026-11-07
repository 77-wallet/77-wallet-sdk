use wallet_database::{
    entities::expand_batch::ExpandBatchStatus,
    repositories::api_wallet::expand_batch::ExpandBatchRepo,
};

pub struct Scenario {
    pub ctx: &'static wallet_api::Context,
    db_dir: std::path::PathBuf,
    pool: wallet_database::ApiWalletDbPool,
    uid: String,
    batch: String,
}
impl Scenario {
    pub async fn given(count: i32) -> Self {
        let ctx = wallet_api::testkit::context::api_trans_test_ctx().await;
        let db_dir = ctx.get_global_dirs().db_dir.clone();
        let pool = crate::harness::open_api_wallet_pool(&db_dir).await;
        let uid = format!("planning-{}", uuid::Uuid::new_v4().simple());
        let batch = format!("batch-{uid}");
        ExpandBatchRepo::create_batch(&pool, &uid, &batch, "serial", "base", count).await.unwrap();
        Self { ctx, db_dir, pool, uid, batch }
    }
    pub async fn given_running_without_items(&self) {
        assert!(ExpandBatchRepo::mark_running_if_pending(&self.pool, &self.batch).await.unwrap());
    }
    pub async fn given_second_chunk_fails(&self) {
        let sql = format!(
            "CREATE TRIGGER fail_planning_insert BEFORE INSERT ON expand_batch_item WHEN NEW.batch_id='{}' AND NEW.input_index>=150 BEGIN SELECT RAISE(FAIL,'injected second chunk failure'); END",
            self.batch
        );
        sqlx::query(&sql).execute(self.pool.write_ref()).await.unwrap();
    }
    pub async fn given_writes_recover(&self) {
        sqlx::query("DROP TRIGGER fail_planning_insert")
            .execute(self.pool.write_ref())
            .await
            .unwrap();
    }
    pub async fn when_plan_batch_runs(
        &self,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        wallet_api::testkit::expand::plan_batch(self.ctx, &self.batch).await
    }
    pub async fn when_reopened_planner_runs(
        &self,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        let reopened = crate::harness::open_api_wallet_pool(&self.db_dir).await;
        assert!(ExpandBatchRepo::get_batch(&reopened, &self.batch).await.unwrap().is_some());
        wallet_api::testkit::expand::plan_all(self.ctx).await
    }
    async fn indices(&self, batch: &str) -> Vec<i32> {
        sqlx::query_scalar(
            "SELECT input_index FROM expand_batch_item WHERE batch_id=? ORDER BY input_index",
        )
        .bind(batch)
        .fetch_all(self.pool.read_ref())
        .await
        .unwrap()
    }
    pub async fn then_pending_without_items_after_reopen(&self) {
        let reopened = crate::harness::open_api_wallet_pool(&self.db_dir).await;
        let batch = ExpandBatchRepo::get_batch(&reopened, &self.batch).await.unwrap().unwrap();
        assert_eq!(
            batch.status,
            ExpandBatchStatus::Pending,
            "failed planning must remain retryable after reopen"
        );
        assert!(
            self.indices(&self.batch).await.is_empty(),
            "no committed first chunk may survive failure"
        );
    }
    pub async fn then_running_with_exact_items(&self, count: usize) {
        let batch = ExpandBatchRepo::get_batch(&self.pool, &self.batch).await.unwrap().unwrap();
        assert_eq!(batch.status, ExpandBatchStatus::Running);
        let indices = self.indices(&self.batch).await;
        assert_eq!(indices, (0..count as i32).collect::<Vec<_>>());
    }
    pub async fn given_another_batch(&self) -> String {
        let batch = format!("other-{}", self.batch);
        ExpandBatchRepo::create_batch(&self.pool, &self.uid, &batch, "serial", "base", 3)
            .await
            .unwrap();
        batch
    }
    pub async fn then_two_batches_have_disjoint_indices(&self, other: &str) {
        let a = self.indices(&self.batch).await;
        let b = self.indices(other).await;
        assert_eq!(a.len(), 3);
        assert_eq!(b.len(), 3);
        assert!(
            a.iter().all(|idx| !b.contains(idx)),
            "concurrent reservations must be disjoint: {a:?} / {b:?}"
        );
    }
}
