use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use chrono::Utc;
use wallet_api::{Context, infrastructure::api_trans::ShadowCollectCommand};
use wallet_database::{
    ApiTransactionDbPool, SqliteContext,
    entities::api_collect::{ApiCollectEntity, ApiCollectStatus},
    repositories::api_wallet::collect::ApiCollectRepo,
};

use crate::harness::{
    AssertRole, CountRole, GivenRole, LoadRole, SeedRole, ThenRole, WhenRole, open_api_wallet_pool,
};

use super::{
    adapters::{
        TronRecoverProbeGuard, expired_tron_raw_tx_json, install_collect_tron_recover_probe_adapter,
    },
    fixtures::CollectRecoveryFixture,
    worker::{build_shadow_collect_worker_from_pools, ensure_sol_main_coin, seed_tron_collect},
};

pub(crate) struct ShadowCollectRecoveryScenario {
    ctx: &'static Context,
    db_dir: PathBuf,
    query_count: RefCell<Option<Arc<AtomicUsize>>>,
    adapter_guard: RefCell<Option<TronRecoverProbeGuard>>,
    conflict_count: Arc<AtomicUsize>,
}

impl ShadowCollectRecoveryScenario {
    pub(crate) async fn new() -> Self {
        // Recovery needs chain/DB context, not an unlocked wallet. Share the same
        // transaction test context as this flow's scanner scenarios.
        let ctx = wallet_api::testkit::context::api_trans_test_ctx().await;
        let db_dir = ctx.get_global_dirs().db_dir.clone();
        let core_pool = open_api_wallet_pool(&db_dir).await;
        ensure_sol_main_coin(&core_pool).await;

        Self {
            ctx,
            db_dir,
            query_count: RefCell::new(None),
            adapter_guard: RefCell::new(None),
            conflict_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(crate) async fn given_evm_uncertain_collect(
        &self,
        fixture: &CollectRecoveryFixture,
        missing_receipt: bool,
        proof: Option<bool>,
    ) {
        use wallet_database::{
            entities::{api_coin::ApiCoinData, asset_token_key::AssetTokenKey},
            repositories::api_wallet::coin::ApiCoinRepo,
        };
        let core = open_api_wallet_pool(&self.db_dir).await;
        let now = Utc::now();
        ApiCoinRepo::upsert_multi_coin(
            &core,
            vec![ApiCoinData::new(
                Some("Ethereum".to_owned()),
                "ETH",
                "eth",
                AssetTokenKey::Native,
                Some("0".to_owned()),
                None,
                18,
                1,
                1,
                1,
                now,
                Some(now),
            )],
        )
        .await
        .expect("seed ETH native coin");
        let pool = self.collect_pool().await;
        seed_tron_collect(&pool, fixture).await;
        sqlx::query(
            "UPDATE api_collect SET chain_code='eth',
            from_addr=' 0x08932ba7a633FdEA40Dfd68afab610340e6375BF ', nonce=1,
            raw_tx='original-signed-raw', tx_hash=?, err_code=NULL, err_msg=NULL,
            transaction_time=NULL, finished_at=NULL, tx_exec_receipt_uploaded_at=NULL,
            last_broadcast_at=strftime('%Y-%m-%dT%H:%M:%SZ','now','-15 minutes'),
            broadcast_uncertain_since_at=strftime('%Y-%m-%dT%H:%M:%SZ','now','-15 minutes'),
            broadcast_uncertain_last_checked_at=NULL, broadcast_uncertain_retry_count=0
            WHERE trade_no=?",
        )
        .bind(&fixture.tx_hash)
        .bind(&fixture.trade_no)
        .execute(pool.as_ref())
        .await
        .expect("seed expired uncertain EVM collect");
        let queries = Arc::new(AtomicUsize::new(0));
        let guard = super::adapters::install_evm_recover_probe(
            &fixture.tx_hash,
            missing_receipt,
            proof,
            queries.clone(),
            self.conflict_count.clone(),
        );
        self.query_count.replace(Some(queries));
        self.adapter_guard.replace(Some(guard));
    }

    pub(crate) async fn given_future_nonce(&self, fixture: &CollectRecoveryFixture) {
        let pool = self.collect_pool().await;
        sqlx::query("UPDATE api_collect SET nonce=3 WHERE trade_no=?")
            .bind(&fixture.trade_no)
            .execute(pool.as_ref())
            .await
            .unwrap();
    }

    pub(crate) async fn given_query_uncertain_gap(
        &self,
        fixture: &CollectRecoveryFixture,
        nonce: Option<u64>,
        recent: bool,
    ) {
        self.given_evm_uncertain_collect(fixture, true, None).await;
        self.given_future_nonce(fixture).await;
        let pool = self.collect_pool().await;
        sqlx::query("UPDATE api_collect SET broadcast_uncertain_since_at=NULL, last_broadcast_at=strftime('%Y-%m-%dT%H:%M:%SZ','now',?) WHERE trade_no=?")
            .bind(if recent { "-1 minute" } else { "-15 minutes" }).bind(&fixture.trade_no).execute(pool.as_ref()).await.unwrap();
        wallet_database::repositories::api_wallet::nonce::ApiNonceRepo::set_nonce_floor(
            &pool,
            " 0x08932ba7a633FdEA40Dfd68afab610340e6375BF ",
            "eth",
            153,
        )
        .await
        .unwrap();
        self.adapter_guard.take();
        let queries = Arc::new(AtomicUsize::new(0));
        let guard = super::adapters::install_evm_recover_probe_with_nonce(
            &fixture.tx_hash,
            true,
            None,
            queries.clone(),
            self.conflict_count.clone(),
            nonce,
            true,
        );
        self.query_count.replace(Some(queries));
        self.adapter_guard.replace(Some(guard));
    }

    pub(crate) async fn then_query_uncertain_preserves_order(
        &self,
        fixture: &CollectRecoveryFixture,
    ) {
        let pool = self.collect_pool().await;
        let row =
            ApiCollectRepo::get_api_collect_by_trade_no(&pool, &fixture.trade_no).await.unwrap();
        assert!(row.err_code.is_none());
        assert!(row.finished_at.is_none());
        assert_eq!(row.raw_tx.as_deref(), Some("original-signed-raw"));
        assert!(
            !ApiCollectRepo::scan_need_tx_exec_receipt_upload(&pool, 10000)
                .await
                .unwrap()
                .iter()
                .any(|r| r.trade_no == fixture.trade_no)
        );
    }

    pub(crate) async fn then_gap_is_failed_without_releasing_signed_nonce(
        &self,
        fixture: &CollectRecoveryFixture,
    ) {
        let pool = self.collect_pool().await;
        let row =
            ApiCollectRepo::get_api_collect_by_trade_no(&pool, &fixture.trade_no).await.unwrap();
        assert!(row.err_msg.as_deref().unwrap_or_default().starts_with("nonce_gap_reported:"));
        assert!(row.err_code.is_some());
        assert_eq!(row.raw_tx.as_deref(), Some("original-signed-raw"));
        assert_eq!(row.tx_hash.as_deref(), Some(fixture.tx_hash.as_str()));
        assert_eq!(row.nonce, 3);
        assert!(row.transaction_time.is_none());
        assert!(
            ApiCollectRepo::scan_need_tx_exec_receipt_upload(&pool, 10_000)
                .await
                .unwrap()
                .iter()
                .any(|r| r.trade_no == fixture.trade_no)
        );
        assert!(
            !ApiCollectRepo::scan_need_recover(&pool, 10_000)
                .await
                .unwrap()
                .iter()
                .any(|r| r.trade_no == fixture.trade_no)
        );
        assert!(
            !ApiCollectRepo::scan_can_broadcast(&pool, 10_000)
                .await
                .unwrap()
                .iter()
                .any(|r| r.trade_no == fixture.trade_no)
        );
        assert_eq!(
            wallet_database::repositories::api_wallet::nonce::ApiNonceRepo::observe_signed_nonce(
                &pool,
                &row.from_addr,
                "eth",
                2
            )
            .await
            .unwrap(),
            wallet_database::entities::api_nonce_claim::WithdrawNonceDecision::Ready(2)
        );
    }

    pub(crate) async fn then_evm_recovery_facts(
        &self,
        fixture: &CollectRecoveryFixture,
        conflict: bool,
        confirmed: bool,
    ) {
        let pool = self.collect_pool().await;
        let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &fixture.trade_no)
            .await
            .expect("read recovered EVM collect");
        assert_eq!(row.tx_hash.as_deref(), Some(fixture.tx_hash.as_str()));
        assert_eq!(row.raw_tx.as_deref(), Some("original-signed-raw"));
        assert_eq!(row.nonce, 1);
        assert_eq!(row.transaction_time.is_some(), confirmed);
        assert_eq!(row.err_code.is_some(), conflict);
        assert!(row.finished_at.is_none());
        assert!(row.tx_exec_receipt_uploaded_at.is_none());
        assert_eq!(self.query_count.borrow().as_ref().unwrap().load(Ordering::Relaxed), 1);
        assert_eq!(self.conflict_count.load(Ordering::Relaxed), usize::from(!confirmed));
        let receipt = ApiCollectRepo::scan_need_tx_exec_receipt_upload(&pool, 10_000)
            .await
            .unwrap()
            .iter()
            .any(|r| r.trade_no == fixture.trade_no);
        assert_eq!(receipt, conflict || confirmed);
        if conflict {
            assert_eq!(row.status, ApiCollectStatus::SendingTxFailed);
            assert!(row.err_msg.as_deref().unwrap().contains(&format!("0x{}", "22".repeat(32))));
            assert!(
                !ApiCollectRepo::scan_need_recover(&pool, 10_000)
                    .await
                    .unwrap()
                    .iter()
                    .any(|r| r.trade_no == fixture.trade_no)
            );
            assert!(
                !ApiCollectRepo::scan_can_broadcast(&pool, 10_000)
                    .await
                    .unwrap()
                    .iter()
                    .any(|r| r.trade_no == fixture.trade_no)
            );
        } else if !confirmed {
            assert_eq!(row.status, ApiCollectStatus::SendingTx);
            assert!(row.err_msg.is_none());
            assert!(row.broadcast_uncertain_retry_count > 0);
        }
    }

    async fn collect_pool(&self) -> ApiTransactionDbPool {
        let tx_ctx = SqliteContext::new(&self.db_dir.to_string_lossy(), Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db");
        tx_ctx.into_transaction_db_pool().expect("transaction pool")
    }

    fn seed(&self) -> SeedRole<'_, Self> {
        SeedRole::new(self)
    }

    fn load(&self) -> LoadRole<'_, Self> {
        LoadRole::new(self)
    }

    fn count(&self) -> CountRole<'_, Self> {
        CountRole::new(self)
    }

    fn assert(&self) -> AssertRole<'_, Self> {
        AssertRole::new(self)
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectRecoveryGiven {
    fn chain_probe_confirms_tx(&self, fixture: &CollectRecoveryFixture);

    fn chain_query_clears_hash_then_confirms(&self, fixture: &CollectRecoveryFixture);

    async fn expired_raw_tx_collect(&self, fixture: &CollectRecoveryFixture);

    async fn recoverable_collect_with_tx_hash(&self, fixture: &CollectRecoveryFixture);
}

#[async_trait::async_trait(?Send)]
impl CollectRecoveryGiven for GivenRole<'_, ShadowCollectRecoveryScenario> {
    fn chain_probe_confirms_tx(&self, fixture: &CollectRecoveryFixture) {
        self.scenario().seed().chain_probe_confirms_tx(fixture);
    }

    fn chain_query_clears_hash_then_confirms(&self, fixture: &CollectRecoveryFixture) {
        self.scenario().seed().chain_query_clears_hash_then_confirms(fixture);
    }

    async fn expired_raw_tx_collect(&self, fixture: &CollectRecoveryFixture) {
        self.scenario().seed().expired_raw_tx_collect(fixture).await;
    }

    async fn recoverable_collect_with_tx_hash(&self, fixture: &CollectRecoveryFixture) {
        self.scenario().seed().recoverable_collect_with_tx_hash(fixture).await;
    }
}

#[async_trait::async_trait(?Send)]
trait CollectRecoverySeed {
    fn chain_probe_confirms_tx(&self, fixture: &CollectRecoveryFixture);

    fn chain_query_clears_hash_then_confirms(&self, fixture: &CollectRecoveryFixture);

    async fn expired_raw_tx_collect(&self, fixture: &CollectRecoveryFixture);

    async fn recoverable_collect_with_tx_hash(&self, fixture: &CollectRecoveryFixture);
}

#[async_trait::async_trait(?Send)]
impl CollectRecoverySeed for SeedRole<'_, ShadowCollectRecoveryScenario> {
    fn chain_probe_confirms_tx(&self, fixture: &CollectRecoveryFixture) {
        let query_count = Arc::new(AtomicUsize::new(0));
        let adapter_guard = install_collect_tron_recover_probe_adapter(
            query_count.clone(),
            None,
            &fixture.tx_hash,
            0.25,
            r#"{"net_used":0,"energy_used":0}"#,
            1_700_000_000_000,
            99,
        );
        self.scenario().query_count.replace(Some(query_count));
        self.scenario().adapter_guard.replace(Some(adapter_guard));
    }

    fn chain_query_clears_hash_then_confirms(&self, fixture: &CollectRecoveryFixture) {
        let clear_trade_no = fixture.trade_no.clone();
        let db_dir = self.scenario().db_dir.clone();
        let query_hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let db_dir = db_dir.clone();
            let trade_no = clear_trade_no.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("create helper runtime");
                rt.block_on(async move {
                    let tx_ctx =
                        SqliteContext::new(&db_dir.to_string_lossy(), Some("api_transaction.db"))
                            .await
                            .expect("open api_transaction.db");
                    let pool = tx_ctx.into_transaction_db_pool().expect("transaction pool");
                    let _ = sqlx::query(
                        r#"
                        UPDATE api_collect
                        SET tx_hash = ''
                        WHERE trade_no = ?
                        "#,
                    )
                    .bind(&trade_no)
                    .execute(pool.as_ref())
                    .await;
                });
            })
            .join()
            .expect("clear hash hook");
        });

        let query_count = Arc::new(AtomicUsize::new(0));
        let adapter_guard = install_collect_tron_recover_probe_adapter(
            query_count.clone(),
            Some(query_hook),
            &fixture.tx_hash,
            0.25,
            r#"{"net_used":0,"energy_used":0}"#,
            1_700_000_000_000,
            99,
        );
        self.scenario().query_count.replace(Some(query_count));
        self.scenario().adapter_guard.replace(Some(adapter_guard));
    }

    async fn expired_raw_tx_collect(&self, fixture: &CollectRecoveryFixture) {
        let collect_pool = self.scenario().collect_pool().await;
        seed_tron_collect(&collect_pool, fixture).await;

        let expired_raw_tx = expired_tron_raw_tx_json(Utc::now().timestamp_millis() - 60_000);
        sqlx::query(
            r#"
            UPDATE api_collect
            SET raw_tx = $2,
                tx_hash = $3,
                last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                status = $4,
                transaction_time = NULL,
                tx_exec_receipt_uploaded_at = NULL,
                err_code = NULL,
                err_msg = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
            "#,
        )
        .bind(&fixture.trade_no)
        .bind(&expired_raw_tx)
        .bind(&fixture.tx_hash)
        .bind(ApiCollectStatus::SendingTx)
        .execute(collect_pool.as_ref())
        .await
        .expect("seed expired raw tx facts");
    }

    async fn recoverable_collect_with_tx_hash(&self, fixture: &CollectRecoveryFixture) {
        let collect_pool = self.scenario().collect_pool().await;
        seed_tron_collect(&collect_pool, fixture).await;

        sqlx::query(
            r#"
            UPDATE api_collect
            SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                raw_tx = '{"tx":true}',
                tx_hash = ?,
                last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                transaction_time = NULL,
                tx_exec_receipt_uploaded_at = NULL,
                err_code = NULL,
                err_msg = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?
            "#,
        )
        .bind(&fixture.tx_hash)
        .bind(&fixture.trade_no)
        .execute(collect_pool.as_ref())
        .await
        .expect("seed recoverable collect row");
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectRecoveryWhen {
    async fn recover_runs(&self, fixture: &CollectRecoveryFixture);
}

#[async_trait::async_trait(?Send)]
impl CollectRecoveryWhen for WhenRole<'_, ShadowCollectRecoveryScenario> {
    async fn recover_runs(&self, fixture: &CollectRecoveryFixture) {
        let worker = build_shadow_collect_worker_from_pools(self.scenario().ctx);
        worker
            .handle(ShadowCollectCommand::Recover(fixture.trade_no.clone()))
            .await
            .expect("recover command should succeed");
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectRecoveryThen {
    fn chain_was_queried_once(&self);

    async fn expired_raw_tx_is_confirmed_without_rebuild(&self, fixture: &CollectRecoveryFixture);

    async fn tx_hash_is_backfilled_and_receipt_upload_needed(
        &self,
        fixture: &CollectRecoveryFixture,
    );
}

#[async_trait::async_trait(?Send)]
impl CollectRecoveryThen for ThenRole<'_, ShadowCollectRecoveryScenario> {
    fn chain_was_queried_once(&self) {
        let query_count = self.scenario().count().chain_queries();
        self.scenario().assert().chain_was_queried_once(query_count);
    }

    async fn expired_raw_tx_is_confirmed_without_rebuild(&self, fixture: &CollectRecoveryFixture) {
        let after = self.scenario().load().collect(&fixture.trade_no).await;
        self.scenario().assert().expired_raw_tx_is_confirmed_without_rebuild(&after);
    }

    async fn tx_hash_is_backfilled_and_receipt_upload_needed(
        &self,
        fixture: &CollectRecoveryFixture,
    ) {
        let after = self.scenario().load().collect(&fixture.trade_no).await;
        let needs_receipt_upload =
            self.scenario().load().has_receipt_upload_candidate(&fixture.trade_no).await;
        self.scenario().assert().tx_hash_is_backfilled_and_receipt_upload_needed(
            &after,
            fixture,
            needs_receipt_upload,
        );
    }
}

#[async_trait::async_trait(?Send)]
trait CollectRecoveryLoad {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity;

    async fn has_receipt_upload_candidate(&self, trade_no: &str) -> bool;
}

#[async_trait::async_trait(?Send)]
impl CollectRecoveryLoad for LoadRole<'_, ShadowCollectRecoveryScenario> {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity {
        let collect_pool = self.scenario().collect_pool().await;
        ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("reload collect after recover")
    }

    async fn has_receipt_upload_candidate(&self, trade_no: &str) -> bool {
        let collect_pool = self.scenario().collect_pool().await;
        ApiCollectRepo::scan_need_tx_exec_receipt_upload(&collect_pool, 10_000)
            .await
            .expect("scan need tx exec receipt upload")
            .iter()
            .any(|record| record.trade_no == trade_no)
    }
}

trait CollectRecoveryCount {
    fn chain_queries(&self) -> usize;
}

impl CollectRecoveryCount for CountRole<'_, ShadowCollectRecoveryScenario> {
    fn chain_queries(&self) -> usize {
        let query_count = self.scenario().query_count.borrow();
        query_count.as_ref().expect("probe query count installed").load(Ordering::Relaxed)
    }
}

trait CollectRecoveryAssert {
    fn chain_was_queried_once(&self, query_count: usize);

    fn expired_raw_tx_is_confirmed_without_rebuild(&self, after: &ApiCollectEntity);

    fn tx_hash_is_backfilled_and_receipt_upload_needed(
        &self,
        after: &ApiCollectEntity,
        fixture: &CollectRecoveryFixture,
        needs_receipt_upload: bool,
    );
}

impl CollectRecoveryAssert for AssertRole<'_, ShadowCollectRecoveryScenario> {
    fn chain_was_queried_once(&self, query_count: usize) {
        assert_eq!(query_count, 1, "recover must query chain first");
    }

    fn expired_raw_tx_is_confirmed_without_rebuild(&self, after: &ApiCollectEntity) {
        assert!(after.transaction_time.is_some(), "recover must persist chain confirmation");
        assert!(after.last_broadcast_at.is_some(), "broadcast evidence must be preserved");
        assert!(
            after.raw_tx.is_some(),
            "expired raw tx must not be invalidated before final confirmation"
        );
    }

    fn tx_hash_is_backfilled_and_receipt_upload_needed(
        &self,
        after: &ApiCollectEntity,
        fixture: &CollectRecoveryFixture,
        needs_receipt_upload: bool,
    ) {
        assert_eq!(after.tx_hash.as_deref(), Some(fixture.tx_hash.as_str()));
        assert!(after.transaction_time.is_some());
        assert!(
            needs_receipt_upload,
            "recovered collect with backfilled hash must enter receipt upload scan"
        );
    }
}
