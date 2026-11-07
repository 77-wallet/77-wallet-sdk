use wallet_api::testkit::collect::shadow_collect_check_fee;
use wallet_database::{
    ApiTransactionDbPool, SqliteContext,
    entities::api_collect::{ApiCollectEntity, ErrCode},
    repositories::api_wallet::collect::ApiCollectRepo,
};

use crate::harness::{
    AssertRole, GivenRole, LoadRole, SeedRole, ThenRole, WhenRole, WorkerTestEnv,
    ensure_worker_env, next_unique_id,
};

use super::super::super::support::{
    build_shadow_collect_worker, install_collect_test_adapter, seed_collect_order,
};

pub(crate) struct CollectBuildFeeScenario {
    env: &'static WorkerTestEnv,
    collect_pool: ApiTransactionDbPool,
}

impl CollectBuildFeeScenario {
    pub(crate) async fn new() -> Self {
        let env = ensure_worker_env().await;
        let collect_pool = open_collect_pool(env).await;
        Self { env, collect_pool }
    }

    fn seed(&self) -> SeedRole<'_, Self> {
        SeedRole::new(self)
    }

    fn load(&self) -> LoadRole<'_, Self> {
        LoadRole::new(self)
    }

    fn assert(&self) -> AssertRole<'_, Self> {
        AssertRole::new(self)
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectBuildFeeGiven {
    fn low_balance_sol_adapter(&self) -> impl Drop;

    async fn sol_collect_order(&self, trade_prefix: &str, to_addr: &str) -> ApiCollectEntity;

    async fn completed_fee_cycle_facts(&self, collect: &ApiCollectEntity);

    async fn collect_reloaded(&self, collect: &ApiCollectEntity) -> ApiCollectEntity;
}

#[async_trait::async_trait(?Send)]
impl CollectBuildFeeGiven for GivenRole<'_, CollectBuildFeeScenario> {
    fn low_balance_sol_adapter(&self) -> impl Drop {
        install_collect_test_adapter(false, 0)
    }

    async fn sol_collect_order(&self, trade_prefix: &str, to_addr: &str) -> ApiCollectEntity {
        self.scenario().seed().sol_collect_order(trade_prefix, to_addr).await
    }

    async fn completed_fee_cycle_facts(&self, collect: &ApiCollectEntity) {
        self.scenario().seed().completed_fee_cycle_facts(collect).await;
    }

    async fn collect_reloaded(&self, collect: &ApiCollectEntity) -> ApiCollectEntity {
        self.scenario().load().collect(&collect.trade_no).await
    }
}

#[async_trait::async_trait(?Send)]
trait CollectBuildFeeSeed {
    async fn sol_collect_order(&self, trade_prefix: &str, to_addr: &str) -> ApiCollectEntity;

    async fn completed_fee_cycle_facts(&self, collect: &ApiCollectEntity);
}

#[async_trait::async_trait(?Send)]
impl CollectBuildFeeSeed for SeedRole<'_, CollectBuildFeeScenario> {
    async fn sol_collect_order(&self, trade_prefix: &str, to_addr: &str) -> ApiCollectEntity {
        let trade_no = format!("{trade_prefix}_{}", next_unique_id());
        seed_collect_order(&self.scenario().collect_pool, &trade_no, to_addr).await
    }

    async fn completed_fee_cycle_facts(&self, collect: &ApiCollectEntity) {
        sqlx::query(
            r#"
            UPDATE api_collect
            SET service_fee_uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                tx_fee_res_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                need_service_fee = false,
                ever_needed_service_fee = true,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?
            "#,
        )
        .bind(&collect.trade_no)
        .execute(self.scenario().collect_pool.as_ref())
        .await
        .expect("seed fee cycle facts");
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectBuildFeeWhen {
    async fn fee_check_runs(&self, collect: &ApiCollectEntity) -> bool;

    async fn build_attempt_is_invalidated(&self, collect: &ApiCollectEntity) -> u64;
}

#[async_trait::async_trait(?Send)]
impl CollectBuildFeeWhen for WhenRole<'_, CollectBuildFeeScenario> {
    async fn fee_check_runs(&self, collect: &ApiCollectEntity) -> bool {
        let worker = build_shadow_collect_worker(self.scenario().env).await;
        shadow_collect_check_fee(&worker, collect)
            .await
            .expect("fee check should return a boolean result")
    }

    async fn build_attempt_is_invalidated(&self, collect: &ApiCollectEntity) -> u64 {
        let worker = build_shadow_collect_worker(self.scenario().env).await;
        worker
            .invalidate_build_attempt_after_fee_check_failure(collect)
            .await
            .expect("fee check failure should invalidate build attempt")
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectBuildFeeThen {
    fn fee_check_failed(&self, pass: bool, message: &str);

    fn one_row_was_affected(&self, affected: u64);

    fn fee_cycle_is_not_reopened(&self, affected: u64);

    async fn fee_cycle_is_reopened(&self, collect: &ApiCollectEntity);

    async fn completed_fee_cycle_facts_are_preserved(&self, collect: &ApiCollectEntity);
}

#[async_trait::async_trait(?Send)]
impl CollectBuildFeeThen for ThenRole<'_, CollectBuildFeeScenario> {
    fn fee_check_failed(&self, pass: bool, message: &str) {
        self.scenario().assert().fee_check_failed(pass, message);
    }

    fn one_row_was_affected(&self, affected: u64) {
        self.scenario().assert().one_row_was_affected(affected);
    }

    fn fee_cycle_is_not_reopened(&self, affected: u64) {
        self.scenario().assert().fee_cycle_is_not_reopened(affected);
    }

    async fn fee_cycle_is_reopened(&self, collect: &ApiCollectEntity) {
        let persisted = self.scenario().load().collect(&collect.trade_no).await;
        self.scenario().assert().fee_cycle_is_reopened(&persisted);
    }

    async fn completed_fee_cycle_facts_are_preserved(&self, collect: &ApiCollectEntity) {
        let persisted = self.scenario().load().collect(&collect.trade_no).await;
        self.scenario().assert().completed_fee_cycle_facts_are_preserved(collect, &persisted);
    }
}

#[async_trait::async_trait(?Send)]
trait CollectBuildFeeLoad {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity;
}

#[async_trait::async_trait(?Send)]
impl CollectBuildFeeLoad for LoadRole<'_, CollectBuildFeeScenario> {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity {
        ApiCollectRepo::get_api_collect_by_trade_no(&self.scenario().collect_pool, trade_no)
            .await
            .expect("load collect")
    }
}

trait CollectBuildFeeAssert {
    fn fee_check_failed(&self, pass: bool, message: &str);

    fn one_row_was_affected(&self, affected: u64);

    fn fee_cycle_is_not_reopened(&self, affected: u64);

    fn fee_cycle_is_reopened(&self, persisted: &ApiCollectEntity);

    fn completed_fee_cycle_facts_are_preserved(
        &self,
        original: &ApiCollectEntity,
        persisted: &ApiCollectEntity,
    );
}

impl CollectBuildFeeAssert for AssertRole<'_, CollectBuildFeeScenario> {
    fn fee_check_failed(&self, pass: bool, message: &str) {
        assert!(!pass, "{message}");
    }

    fn one_row_was_affected(&self, affected: u64) {
        assert_eq!(affected, 1);
    }

    fn fee_cycle_is_not_reopened(&self, affected: u64) {
        assert_eq!(affected, 0, "terminal failure must not reopen the fee cycle");
    }

    fn fee_cycle_is_reopened(&self, persisted: &ApiCollectEntity) {
        assert_eq!(persisted.need_service_fee, Some(true));
        assert!(persisted.service_fee_uploaded_at.is_none());
        assert!(persisted.raw_tx.is_none());
        assert!(persisted.tx_hash.is_none());
    }

    fn completed_fee_cycle_facts_are_preserved(
        &self,
        original: &ApiCollectEntity,
        persisted: &ApiCollectEntity,
    ) {
        assert_eq!(persisted.need_service_fee, Some(false));
        assert!(original.service_fee_uploaded_at.is_some());
        assert!(original.tx_fee_res_ack_sent_at.is_some());
        assert_eq!(persisted.service_fee_uploaded_at, original.service_fee_uploaded_at);
        assert_eq!(persisted.tx_fee_res_ack_sent_at, original.tx_fee_res_ack_sent_at);
        assert_eq!(persisted.ever_needed_service_fee, original.ever_needed_service_fee);
        assert_eq!(persisted.raw_tx, original.raw_tx);
        assert_eq!(persisted.tx_hash, original.tx_hash);
        assert_eq!(persisted.err_code, Some(ErrCode::BalanceInsufficient));
        assert_eq!(
            persisted.err_msg.as_deref(),
            Some("collect fee shortage after the single completed fee cycle")
        );
        assert!(persisted.transaction_time.is_none());
    }
}

async fn open_collect_pool(env: &WorkerTestEnv) -> ApiTransactionDbPool {
    let collect_pool_ctx =
        SqliteContext::new(&env.db_dir.to_string_lossy(), Some("api_transaction.db"))
            .await
            .expect("open api transaction sqlite");
    collect_pool_ctx.into_transaction_db_pool().expect("transaction pool")
}
