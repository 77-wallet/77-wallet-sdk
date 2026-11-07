use alloy::primitives::U256;
use wallet_api::{error::service::ServiceError, testkit::collect::shadow_collect_check_fee};
use wallet_database::{
    ApiTransactionDbPool, SqliteContext,
    entities::api_collect::{ApiCollectEntity, ApiCollectStatus},
    repositories::api_wallet::collect::ApiCollectRepo,
};

use crate::harness::{
    AssertRole, GivenRole, LoadRole, SeedRole, ThenRole, WhenRole, WorkerTestEnv,
    ensure_worker_env, next_unique_id,
};

use super::super::super::support::{
    build_eth_shadow_collect_worker, build_shadow_collect_worker, install_collect_eth_test_adapter,
    install_collect_test_adapter, install_collect_test_adapter_fee_shortage, seed_collect_order,
    seed_eth_collect_order,
};

pub(crate) struct CollectFeeCheckScenario {
    env: &'static WorkerTestEnv,
    collect_pool: ApiTransactionDbPool,
}

impl CollectFeeCheckScenario {
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
pub(crate) trait CollectFeeCheckGiven {
    fn sol_recipient_adapter(&self, recipient_missing: bool, balance: u64) -> impl Drop;

    fn sol_fee_shortage_adapter(&self, recipient_missing: bool, balance: u64) -> impl Drop;

    fn eth_fee_adapter(&self, balance_wei: u128, fee_amount: f64) -> impl Drop;

    async fn sol_collect_order(&self, trade_prefix: &str, to_addr: &str) -> ApiCollectEntity;

    async fn eth_collect_order(
        &self,
        trade_prefix: &str,
        from_addr: &str,
        to_addr: &str,
        value: &str,
    ) -> ApiCollectEntity;
}

#[async_trait::async_trait(?Send)]
impl CollectFeeCheckGiven for GivenRole<'_, CollectFeeCheckScenario> {
    fn sol_recipient_adapter(&self, recipient_missing: bool, balance: u64) -> impl Drop {
        install_collect_test_adapter(recipient_missing, balance)
    }

    fn sol_fee_shortage_adapter(&self, recipient_missing: bool, balance: u64) -> impl Drop {
        install_collect_test_adapter_fee_shortage(recipient_missing, balance)
    }

    fn eth_fee_adapter(&self, balance_wei: u128, fee_amount: f64) -> impl Drop {
        install_collect_eth_test_adapter(U256::from(balance_wei), fee_amount)
    }

    async fn sol_collect_order(&self, trade_prefix: &str, to_addr: &str) -> ApiCollectEntity {
        self.scenario().seed().sol_collect_order(trade_prefix, to_addr).await
    }

    async fn eth_collect_order(
        &self,
        trade_prefix: &str,
        from_addr: &str,
        to_addr: &str,
        value: &str,
    ) -> ApiCollectEntity {
        self.scenario().seed().eth_collect_order(trade_prefix, from_addr, to_addr, value).await
    }
}

#[async_trait::async_trait(?Send)]
trait CollectFeeCheckSeed {
    async fn sol_collect_order(&self, trade_prefix: &str, to_addr: &str) -> ApiCollectEntity;

    async fn eth_collect_order(
        &self,
        trade_prefix: &str,
        from_addr: &str,
        to_addr: &str,
        value: &str,
    ) -> ApiCollectEntity;
}

#[async_trait::async_trait(?Send)]
impl CollectFeeCheckSeed for SeedRole<'_, CollectFeeCheckScenario> {
    async fn sol_collect_order(&self, trade_prefix: &str, to_addr: &str) -> ApiCollectEntity {
        let trade_no = format!("{trade_prefix}_{}", next_unique_id());
        seed_collect_order(&self.scenario().collect_pool, &trade_no, to_addr).await
    }

    async fn eth_collect_order(
        &self,
        trade_prefix: &str,
        from_addr: &str,
        to_addr: &str,
        value: &str,
    ) -> ApiCollectEntity {
        let trade_no = format!("{trade_prefix}_{}", next_unique_id());
        seed_eth_collect_order(&self.scenario().collect_pool, &trade_no, from_addr, to_addr, value)
            .await
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectFeeCheckWhen {
    async fn sol_fee_check_runs(&self, collect: &ApiCollectEntity) -> bool;

    async fn sol_fee_check_fails(&self, collect: &ApiCollectEntity) -> ServiceError;

    async fn eth_fee_check_runs(&self, collect: &ApiCollectEntity) -> bool;

    async fn build_attempt_is_invalidated(&self, collect: &ApiCollectEntity) -> u64;
}

#[async_trait::async_trait(?Send)]
impl CollectFeeCheckWhen for WhenRole<'_, CollectFeeCheckScenario> {
    async fn sol_fee_check_runs(&self, collect: &ApiCollectEntity) -> bool {
        let worker = build_shadow_collect_worker(self.scenario().env).await;
        shadow_collect_check_fee(&worker, collect)
            .await
            .expect("collect fee check should return a boolean result")
    }

    async fn sol_fee_check_fails(&self, collect: &ApiCollectEntity) -> ServiceError {
        let worker = build_shadow_collect_worker(self.scenario().env).await;
        shadow_collect_check_fee(&worker, collect).await.expect_err("collect fee check should fail")
    }

    async fn eth_fee_check_runs(&self, collect: &ApiCollectEntity) -> bool {
        let worker = build_eth_shadow_collect_worker(self.scenario().env).await;
        shadow_collect_check_fee(&worker, collect)
            .await
            .expect("ETH collect fee check should return a boolean result")
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
pub(crate) trait CollectFeeCheckThen {
    fn error_mentions_uninitialized_recipient(&self, err: ServiceError);

    fn fee_check_passed(&self, pass: bool);

    fn fee_check_failed(&self, pass: bool, message: &str);

    fn one_row_was_affected(&self, affected: u64);

    async fn collect_status_is_init(&self, collect: &ApiCollectEntity);

    async fn fee_cycle_is_reopened(&self, collect: &ApiCollectEntity);
}

#[async_trait::async_trait(?Send)]
impl CollectFeeCheckThen for ThenRole<'_, CollectFeeCheckScenario> {
    fn error_mentions_uninitialized_recipient(&self, err: ServiceError) {
        self.scenario().assert().error_mentions_uninitialized_recipient(err);
    }

    fn fee_check_passed(&self, pass: bool) {
        self.scenario().assert().fee_check_passed(pass);
    }

    fn fee_check_failed(&self, pass: bool, message: &str) {
        self.scenario().assert().fee_check_failed(pass, message);
    }

    fn one_row_was_affected(&self, affected: u64) {
        self.scenario().assert().one_row_was_affected(affected);
    }

    async fn collect_status_is_init(&self, collect: &ApiCollectEntity) {
        let persisted = self.scenario().load().collect(&collect.trade_no).await;
        self.scenario().assert().collect_status_is_init(&persisted);
    }

    async fn fee_cycle_is_reopened(&self, collect: &ApiCollectEntity) {
        let persisted = self.scenario().load().collect(&collect.trade_no).await;
        self.scenario().assert().fee_cycle_is_reopened(&persisted);
    }
}

#[async_trait::async_trait(?Send)]
trait CollectFeeCheckLoad {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity;
}

#[async_trait::async_trait(?Send)]
impl CollectFeeCheckLoad for LoadRole<'_, CollectFeeCheckScenario> {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity {
        ApiCollectRepo::get_api_collect_by_trade_no(&self.scenario().collect_pool, trade_no)
            .await
            .expect("load collect")
    }
}

trait CollectFeeCheckAssert {
    fn error_mentions_uninitialized_recipient(&self, err: ServiceError);

    fn fee_check_passed(&self, pass: bool);

    fn fee_check_failed(&self, pass: bool, message: &str);

    fn one_row_was_affected(&self, affected: u64);

    fn collect_status_is_init(&self, persisted: &ApiCollectEntity);

    fn fee_cycle_is_reopened(&self, persisted: &ApiCollectEntity);
}

impl CollectFeeCheckAssert for AssertRole<'_, CollectFeeCheckScenario> {
    fn error_mentions_uninitialized_recipient(&self, err: ServiceError) {
        let msg = err.to_string();
        assert!(msg.contains("recipient account is not initialized"));
        assert!(msg.contains("rent-exempt minimum"));
    }

    fn fee_check_passed(&self, pass: bool) {
        assert!(pass);
    }

    fn fee_check_failed(&self, pass: bool, message: &str) {
        assert!(!pass, "{message}");
    }

    fn one_row_was_affected(&self, affected: u64) {
        assert_eq!(affected, 1);
    }

    fn collect_status_is_init(&self, persisted: &ApiCollectEntity) {
        assert_eq!(persisted.status, ApiCollectStatus::Init);
    }

    fn fee_cycle_is_reopened(&self, persisted: &ApiCollectEntity) {
        assert_eq!(persisted.need_service_fee, Some(true));
        assert!(persisted.service_fee_uploaded_at.is_none());
        assert!(persisted.raw_tx.is_none());
        assert!(persisted.tx_hash.is_none());
    }
}

async fn open_collect_pool(env: &WorkerTestEnv) -> ApiTransactionDbPool {
    let collect_pool_ctx =
        SqliteContext::new(&env.db_dir.to_string_lossy(), Some("api_transaction.db"))
            .await
            .expect("open api transaction sqlite");
    collect_pool_ctx.into_transaction_db_pool().expect("transaction pool")
}
