use std::{cell::RefCell, time::Duration};

use tokio::task::JoinHandle;
use wallet_api::error::service::ServiceError;

use crate::harness::{
    AssertRole, GivenRole, LoadRole, SMOKE_WALLET_PASSWORD, SeedRole, TestEnv, ThenRole, WhenRole,
    ensure_env,
};

use super::{
    adapter::{AdapterGuard, RecordingEthAdapter, install_adapter},
    db::{ensure_bnb_transfer_fixture, load_bnb_nonce_floor},
    request::{BNB_TO_ADDR, make_transfer_req},
};

pub(crate) type TransferTask = JoinHandle<Result<String, ServiceError>>;

pub(crate) struct TransferNonceScenario {
    env: &'static TestEnv,
    from_addr: RefCell<String>,
    adapter: RecordingEthAdapter,
    guard: RefCell<Option<AdapterGuard>>,
}

impl TransferNonceScenario {
    pub(crate) async fn new() -> Self {
        Self {
            env: ensure_env().await,
            from_addr: RefCell::new(String::new()),
            adapter: RecordingEthAdapter::new(),
            guard: RefCell::new(None),
        }
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
pub(crate) trait TransferNonceGiven {
    async fn bnb_transfer_fixture(&self) -> anyhow::Result<()>;

    fn first_transfer_blocks(&self);

    fn transfer_fails(&self);

    fn fake_chain_adapter(&self);

    async fn wallet_password_cached(&self);
}

#[async_trait::async_trait(?Send)]
impl TransferNonceGiven for GivenRole<'_, TransferNonceScenario> {
    async fn bnb_transfer_fixture(&self) -> anyhow::Result<()> {
        self.scenario().seed().bnb_transfer_fixture().await
    }

    fn first_transfer_blocks(&self) {
        self.scenario().seed().first_transfer_blocks();
    }

    fn transfer_fails(&self) {
        self.scenario().seed().transfer_fails();
    }

    fn fake_chain_adapter(&self) {
        self.scenario().seed().fake_chain_adapter();
    }

    async fn wallet_password_cached(&self) {
        self.scenario().seed().wallet_password_cached().await;
    }
}

#[async_trait::async_trait(?Send)]
trait TransferNonceSeed {
    async fn bnb_transfer_fixture(&self) -> anyhow::Result<()>;

    fn first_transfer_blocks(&self);

    fn transfer_fails(&self);

    fn fake_chain_adapter(&self);

    async fn wallet_password_cached(&self);
}

#[async_trait::async_trait(?Send)]
impl TransferNonceSeed for SeedRole<'_, TransferNonceScenario> {
    async fn bnb_transfer_fixture(&self) -> anyhow::Result<()> {
        let from_addr = ensure_bnb_transfer_fixture(self.scenario().env).await?;
        self.scenario().from_addr.replace(from_addr);
        Ok(())
    }

    fn first_transfer_blocks(&self) {
        self.scenario().adapter.block_first_transfer();
    }

    fn transfer_fails(&self) {
        self.scenario().adapter.fail_on_transfer();
    }

    fn fake_chain_adapter(&self) {
        self.scenario().guard.replace(Some(install_adapter(&self.scenario().adapter)));
    }

    async fn wallet_password_cached(&self) {
        let _ = self.scenario().env.manager.set_passwd_cache(SMOKE_WALLET_PASSWORD).await;
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait TransferNonceWhen {
    fn transfer_starts(&self) -> TransferTask;

    async fn transfer_fails(&self) -> ServiceError;

    fn first_transfer_is_released(&self);
}

#[async_trait::async_trait(?Send)]
impl TransferNonceWhen for WhenRole<'_, TransferNonceScenario> {
    fn transfer_starts(&self) -> TransferTask {
        let wallet_manager = self.scenario().env.manager.clone();
        let from_addr = self.scenario().from_addr.borrow().clone();
        let req = make_transfer_req(&from_addr, BNB_TO_ADDR);

        tokio::spawn(async move {
            let resp =
                wallet_manager.api_transfer_with_preloaded_private_key(req, "00".into()).await?;
            Ok(resp.tx_hash)
        })
    }

    async fn transfer_fails(&self) -> ServiceError {
        let from_addr = self.scenario().from_addr.borrow().clone();
        let req = make_transfer_req(&from_addr, BNB_TO_ADDR);
        self.scenario()
            .env
            .manager
            .api_transfer_with_preloaded_private_key(req, "00".into())
            .await
            .expect_err("transfer should fail")
    }

    fn first_transfer_is_released(&self) {
        self.scenario().adapter.release_first();
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait TransferNonceThen {
    async fn first_transfer_has_entered(&self);

    async fn only_first_nonce_is_recorded_while_second_waits(&self);

    async fn serial_transfer_results_are(
        &self,
        first: TransferTask,
        second: TransferTask,
    ) -> anyhow::Result<()>;

    async fn failure_keeps_reserved_nonce(&self, err: ServiceError) -> anyhow::Result<()>;
}

#[async_trait::async_trait(?Send)]
impl TransferNonceThen for ThenRole<'_, TransferNonceScenario> {
    async fn first_transfer_has_entered(&self) {
        self.scenario().adapter.wait_for_first_entry().await;
    }

    async fn only_first_nonce_is_recorded_while_second_waits(&self) {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let recorded_nonces = self.scenario().adapter.recorded_nonces();
        self.scenario().assert().only_first_nonce_is_recorded_while_second_waits(&recorded_nonces);
    }

    async fn serial_transfer_results_are(
        &self,
        first: TransferTask,
        second: TransferTask,
    ) -> anyhow::Result<()> {
        let first_result = first.await.expect("first join")?;
        let second_result = second.await.expect("second join")?;
        let recorded_nonces = self.scenario().adapter.recorded_nonces();

        self.scenario().assert().serial_transfer_results_are(
            &first_result,
            &second_result,
            &recorded_nonces,
        );

        Ok(())
    }

    async fn failure_keeps_reserved_nonce(&self, err: ServiceError) -> anyhow::Result<()> {
        let from_addr = self.scenario().from_addr.borrow().clone();
        let floor = self.scenario().load().bnb_nonce_floor(&from_addr).await?;
        let recorded_nonces = self.scenario().adapter.recorded_nonces();

        self.scenario().assert().failure_keeps_reserved_nonce(err, floor, &recorded_nonces);

        Ok(())
    }
}

#[async_trait::async_trait(?Send)]
trait TransferNonceLoad {
    async fn bnb_nonce_floor(&self, from_addr: &str) -> anyhow::Result<i64>;
}

#[async_trait::async_trait(?Send)]
impl TransferNonceLoad for LoadRole<'_, TransferNonceScenario> {
    async fn bnb_nonce_floor(&self, from_addr: &str) -> anyhow::Result<i64> {
        load_bnb_nonce_floor(self.scenario().env, from_addr).await
    }
}

trait TransferNonceAssert {
    fn only_first_nonce_is_recorded_while_second_waits(&self, recorded_nonces: &[u64]);

    fn serial_transfer_results_are(
        &self,
        first_result: &str,
        second_result: &str,
        recorded_nonces: &[u64],
    );

    fn failure_keeps_reserved_nonce(
        &self,
        err: ServiceError,
        nonce_floor: i64,
        recorded_nonces: &[u64],
    );
}

impl TransferNonceAssert for AssertRole<'_, TransferNonceScenario> {
    fn only_first_nonce_is_recorded_while_second_waits(&self, recorded_nonces: &[u64]) {
        assert_eq!(recorded_nonces, [1], "second transfer should stay blocked");
    }

    fn serial_transfer_results_are(
        &self,
        first_result: &str,
        second_result: &str,
        recorded_nonces: &[u64],
    ) {
        assert_eq!(first_result, format!("0x{:064x}", 1u64));
        assert_eq!(second_result, format!("0x{:064x}", 2u64));
        assert_eq!(recorded_nonces, [1, 2]);
    }

    fn failure_keeps_reserved_nonce(
        &self,
        err: ServiceError,
        nonce_floor: i64,
        recorded_nonces: &[u64],
    ) {
        assert!(err.to_string().contains("simulated transfer failure"), "unexpected error: {err}");
        assert_eq!(nonce_floor, 1, "nonce floor should stay advanced after failure");
        assert_eq!(recorded_nonces, [1]);
    }
}

impl TransferNonceScenario {
    pub(crate) fn given_chain_next_nonce(&self, next: Option<u64>) {
        self.adapter.set_nonce_response(next);
    }

    pub(crate) async fn when_shared_nonce_is_observed(&self) -> Result<i32, ServiceError> {
        let address = self.from_addr.borrow().clone();
        // Collect and fee share this read-only candidate engine.
        wallet_api::testkit::collect_fee::allocate_collect_fee_nonce(
            self.env.manager.ctx(),
            &address,
            "bnb",
        )
        .await
        .map(|nonce| nonce as i32)
    }

    pub(crate) async fn given_nonce_floor(&self, floor: i64) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        let pool = super::db::open_api_transaction_pool(&self.env.db_dir).await;
        wallet_database::repositories::api_wallet::nonce::ApiNonceRepo::set_nonce_floor(
            &pool, &address, "bnb", floor,
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn then_nonce_floor_and_queries_are(&self, floor: i64, queries: usize) {
        let address = self.from_addr.borrow().clone();
        assert_eq!(load_bnb_nonce_floor(self.env, &address).await.unwrap(), floor);
        assert_eq!(self.adapter.nonce_queries(), queries);
        assert!(
            self.adapter.recorded_nonces().is_empty(),
            "allocation must not send a transaction"
        );
    }
}

impl TransferNonceScenario {
    pub(crate) async fn when_nonce_alignment_runs(&self) -> Result<u64, ServiceError> {
        use wallet_api::infrastructure::nonce::nonce_engine::{
            ReconcileReason, get_nonce_engine_with_ctx,
        };
        let engine = get_nonce_engine_with_ctx(self.env.manager.ctx())?;
        let address = self.from_addr.borrow().clone();
        engine.force_align_to_chain_next_nonce(&address, "bnb", ReconcileReason::Manual).await
    }

    pub(crate) fn given_nonce_allocation_frozen(&self, frozen: bool) {
        use wallet_api::infrastructure::nonce::nonce_engine::{
            FreezeReason, get_nonce_engine_with_ctx,
        };
        let engine = get_nonce_engine_with_ctx(self.env.manager.ctx()).unwrap();
        let address = self.from_addr.borrow().clone();
        if frozen {
            engine.freeze(&address, "bnb", FreezeReason::Manual);
        } else {
            engine.unfreeze(&address, "bnb", FreezeReason::Manual);
        }
    }
}

impl TransferNonceScenario {
    pub(crate) async fn then_transfer_nonces_and_floor_are(&self, expected: &[u64], floor: i64) {
        assert_eq!(self.adapter.recorded_nonces(), expected);
        let address = self.from_addr.borrow().clone();
        assert_eq!(load_bnb_nonce_floor(self.env, &address).await.unwrap(), floor);
    }
}

impl TransferNonceScenario {
    pub(crate) async fn when_withdrawal_nonce_is_observed(&self) -> Result<u64, ServiceError> {
        let address = self.from_addr.borrow().clone();
        wallet_api::testkit::withdraw::observe_withdraw_nonce(
            self.env.manager.ctx(),
            &address,
            "bnb",
        )
        .await
    }
}

impl TransferNonceScenario {
    pub(crate) async fn when_collect_fee_nonce_is_allocated(&self) -> Result<u64, ServiceError> {
        let address = self.from_addr.borrow().clone();
        wallet_api::testkit::collect_fee::allocate_collect_fee_nonce(
            self.env.manager.ctx(),
            &address,
            "bnb",
        )
        .await
    }
}

impl TransferNonceScenario {
    pub(crate) async fn when_nonce_error_is_handled(
        &self,
        error: &str,
    ) -> Result<(), ServiceError> {
        let engine = wallet_api::infrastructure::nonce::nonce_engine::get_nonce_engine_with_ctx(
            self.env.manager.ctx(),
        )?;
        let address = self.from_addr.borrow().clone();
        engine.handle_nonce_error(&address, "bnb", error).await
    }
}

impl TransferNonceScenario {
    pub(crate) fn given_transfer_can_succeed(&self) {
        self.adapter.allow_transfer();
    }

    pub(crate) fn given_transfer_times_out(&self) {
        self.adapter.timeout_transfer();
    }
}

impl TransferNonceScenario {
    pub(crate) fn given_transfer_is_invisible(&self) {
        self.adapter.hide_transfer();
    }
    pub(crate) fn given_returned_hash_mismatches(&self) {
        self.adapter.mismatch_hash();
    }

    pub(crate) async fn given_signed_persistence_fails(&self) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        super::db::reject_signed_persistence(self.env, &address).await
    }

    pub(crate) async fn then_durable_transfer_is(
        &self,
        nonce: u64,
        accepted: bool,
    ) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        // Reopen SQLite so the assertion does not depend on adapter/application memory.
        let rows = super::db::pending_ordinary_transfers(self.env, &address).await?;
        assert_eq!(rows.iter().filter(|r| r.nonce == nonce as i64).count(), 1);
        let row = rows.iter().find(|r| r.nonce == nonce as i64).unwrap();
        assert_eq!(row.tx_hash.as_deref(), Some(self.adapter.tx_hash(nonce).as_str()));
        let raw: wallet_api::domain::api_wallet::RawTx =
            serde_json::from_str(row.raw_tx.as_deref().expect("signed raw persisted"))?;
        let wallet_api::domain::api_wallet::RawTx::Evm(bytes, _) = raw else {
            panic!("expected EVM raw")
        };
        assert_eq!(bytes, nonce.to_be_bytes());
        assert_eq!(row.last_broadcast_at.is_some(), accepted);
        assert_eq!(row.broadcast_uncertain_since_at.is_some(), !accepted);
        assert!(row.transaction_time.is_none());
        assert!(row.chain_success_at.is_none());
        assert!(row.finished_at.is_none());
        Ok(())
    }

    pub(crate) async fn then_no_signed_transfer_exists(&self) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        assert!(super::db::pending_ordinary_transfers(self.env, &address).await?.is_empty());
        Ok(())
    }
}

impl TransferNonceScenario {
    pub(crate) async fn given_acceptance_persistence_fails(&self) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        super::db::reject_acceptance_persistence(self.env, &address).await
    }
}

impl TransferNonceScenario {
    pub(crate) fn given_receipt_behavior(&self, nonce: u64, mode: i8) {
        self.adapter.receipt_behavior(nonce, mode);
    }

    pub(crate) async fn when_receipts_are_observed(&self) -> anyhow::Result<()> {
        wallet_api::testkit::withdraw::observe_pending_self_transfers(self.env.manager.ctx())
            .await?;
        Ok(())
    }

    pub(crate) fn when_observer_starts(
        &self,
    ) -> (tokio::sync::broadcast::Sender<()>, tokio::task::JoinHandle<()>) {
        let (shutdown, rx) = tokio::sync::broadcast::channel(1);
        let ctx = self.env.manager.ctx();
        let handle = tokio::spawn(wallet_api::testkit::withdraw::run_self_transfer_observer(
            ctx,
            rx,
            Duration::from_millis(20),
        ));
        (shutdown, handle)
    }

    pub(crate) async fn then_observer_has_queried(&self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.adapter.receipt_queries() == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("background observer did not query the saved hash");
    }

    pub(crate) async fn then_receipt_is_persisted(
        &self,
        nonce: u64,
        success: bool,
    ) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        let row =
            super::db::ordinary_transfer_by_hash(self.env, &address, &self.adapter.tx_hash(nonce))
                .await?;
        assert_eq!(
            row.status,
            if success {
                wallet_database::entities::api_withdraw::ApiWithdrawStatus::Success
            } else {
                wallet_database::entities::api_withdraw::ApiWithdrawStatus::Failure
            }
        );
        assert_eq!(
            self.adapter.receipt_queries(),
            1,
            "terminal rows must leave the observation scan"
        );
        self.then_bill_status_is(nonce, if success { 2 } else { 3 }).await?;
        assert_eq!(row.transaction_fee, "0.00002");
        assert_eq!(row.block_height.as_deref(), Some("1234"));
        assert!(row.finished_at.is_some());
        assert!(row.transaction_time.is_some());
        assert_eq!(row.chain_success_at.is_some(), success);
        assert_eq!(row.chain_failed_at.is_some(), !success);
        assert!(row.broadcast_uncertain_since_at.is_none());
        assert!(row.raw_tx.is_some());
        self.then_transfer_nonces_and_floor_are(&[nonce], nonce as i64).await;
        Ok(())
    }

    pub(crate) async fn then_background_receipt_finishes(&self, nonce: u64) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if super::db::ordinary_transfer_by_hash(
                    self.env,
                    &address,
                    &self.adapter.tx_hash(nonce),
                )
                .await
                .unwrap()
                .finished_at
                .is_some()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        self.then_receipt_is_persisted(nonce, true).await
    }

    pub(crate) async fn when_observer_stops(
        &self,
        observer: (tokio::sync::broadcast::Sender<()>, tokio::task::JoinHandle<()>),
    ) -> anyhow::Result<()> {
        observer.0.send(())?;
        tokio::time::timeout(Duration::from_secs(1), observer.1).await??;
        Ok(())
    }
}

impl TransferNonceScenario {
    pub(crate) fn given_rebroadcast_fixture(&self) {
        self.adapter.canonical_rebroadcast_fixture(&self.from_addr.borrow());
    }
    pub(crate) fn given_node_visibility(&self, visible: bool, error: bool) {
        self.adapter.set_visibility(visible, error);
    }
    pub(crate) async fn given_rebroadcast_cooldown_elapsed(&self) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        super::db::age_self_transfer(self.env, &address).await
    }
    pub(crate) async fn given_saved_raw_corrupted(&self) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        super::db::corrupt_self_transfer_raw(self.env, &address).await
    }
    pub(crate) async fn then_rebroadcast_counts_are(
        &self,
        nonce: u64,
        sends: usize,
        retries: i64,
    ) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        let row =
            super::db::ordinary_transfer_by_hash(self.env, &address, &self.adapter.tx_hash(nonce))
                .await?;
        assert_eq!(row.broadcast_uncertain_rebroadcast_count as i64, retries);
        assert_eq!(row.tx_hash.as_deref(), Some(self.adapter.tx_hash(nonce).as_str()));
        assert!(row.finished_at.is_none());
        assert_eq!(self.adapter.build_calls(), 1, "recovery must never sign again");
        assert_eq!(
            self.adapter.recorded_raw(),
            vec![nonce.to_be_bytes().to_vec(); sends],
            "every send must use identical bytes"
        );
        self.then_transfer_nonces_and_floor_are(&vec![nonce; sends], nonce as i64).await;
        Ok(())
    }
}

impl TransferNonceScenario {
    pub(crate) fn given_broadcast_hangs(&self) {
        self.adapter.hang_send();
    }

    pub(crate) async fn then_send_count_reaches(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.adapter.recorded_nonces().len() < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("observer did not reach broadcast");
    }
}

impl TransferNonceScenario {
    pub(crate) async fn then_bill_status_is(&self, nonce: u64, expected: i8) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        let detail =
            self.env.manager.api_bill_detail(&self.adapter.tx_hash(nonce), &address).await?;
        assert_eq!(detail.bill.status, expected, "bill must expose receipt outcome");
        Ok(())
    }

    pub(crate) async fn given_legacy_success_overwrite(&self) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        super::db::overwrite_legacy_status(self.env, &address).await
    }
}

impl TransferNonceScenario {
    pub(crate) async fn then_case_alias_shares_nonce_gate_and_freeze(&self) -> anyhow::Result<()> {
        use wallet_api::infrastructure::nonce::nonce_engine::{
            FreezeReason, get_nonce_engine_with_ctx,
        };
        let engine = get_nonce_engine_with_ctx(self.env.manager.ctx())?;
        let lower = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let upper = "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let permit = engine.acquire_transfer_gate(lower, "bnb").await;
        assert!(
            tokio::time::timeout(
                Duration::from_millis(30),
                engine.acquire_transfer_gate(upper, "bnb")
            )
            .await
            .is_err()
        );
        drop(permit);
        let alias_permit = tokio::time::timeout(
            Duration::from_secs(1),
            engine.acquire_transfer_gate(upper, "bnb"),
        )
        .await?;
        drop(alias_permit);
        engine.freeze(lower, "bnb", FreezeReason::Manual);
        assert!(engine.is_frozen(upper, "bnb"));
        engine.unfreeze(upper, "bnb", FreezeReason::Manual);
        assert!(!engine.is_frozen(lower, "bnb"));
        Ok(())
    }
}

impl TransferNonceScenario {
    pub(crate) async fn collect_receipt_notifications(
        &self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<wallet_api::messaging::notify::FrontendNotifyEvent>
    {
        let (tx, mut all_events) = tokio::sync::mpsc::unbounded_channel::<
            wallet_api::messaging::notify::FrontendNotifyEvent,
        >();
        self.env.manager.set_frontend_notify_sender(tx).await.unwrap();
        let (only_this_account, rx) = tokio::sync::mpsc::unbounded_channel();
        let address = self.from_addr.borrow().clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = only_this_account.closed() => break,
                    event = all_events.recv() => {
                        let Some(event) = event else { break; };
                        if serde_json::to_string(&event).unwrap().contains(&address) {
                            if only_this_account.send(event).is_err() { break; }
                        }
                    }
                }
            }
        });
        rx
    }

    pub(crate) async fn then_receipt_refresh_pending(&self, expected: bool) -> anyhow::Result<()> {
        let pool = super::db::open_api_transaction_pool(&self.env.db_dir).await;
        let address = self.from_addr.borrow().clone();
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM api_withdraws WHERE from_addr = ? AND receipt_refresh_pending = 1")
            .bind(&address).fetch_one(pool.read_ref()).await?;
        assert_eq!(pending > 0, expected);
        Ok(())
    }

    pub(crate) async fn seed_receipt_native_asset(&self) -> anyhow::Result<()> {
        use wallet_database::{
            entities::{
                api_assets::ApiCreateAssetsVo, asset_token_key::AssetTokenKey, assets::AssetsId,
            },
            repositories::api_wallet::assets::ApiAssetsRepo,
        };
        let pool = crate::harness::open_api_wallet_pool(&self.env.db_dir).await;
        wallet_database::repositories::api_wallet::chain::ApiChainRepo::upsert_multi_chain(
            &pool,
            vec![wallet_database::entities::api_chain::ApiChainCreateVo::new(
                "BNB",
                "bnb",
                &[],
                wallet_database::entities::api_chain::NodeBindType::AutoBackend,
                "BNB",
            )],
        )
        .await?;
        let address = self.from_addr.borrow().clone();
        ApiAssetsRepo::upsert_assets_multi(
            &pool,
            vec![ApiCreateAssetsVo::new(
                AssetsId::new(&address, "bnb", AssetTokenKey::Native),
                "BNB",
                18,
                None,
                0,
            )],
        )
        .await?;
        Ok(())
    }
}

impl TransferNonceScenario {
    pub(crate) fn given_receipt_balance_fails(&self, fail: bool) {
        self.adapter.fail_balance(fail);
    }
    pub(crate) async fn when_self_transfer_account_change_arrives(
        &self,
        nonce: u64,
    ) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        wallet_api::testkit::mqtt::replay_api_wallet_account_change(self.env.manager.ctx(), &serde_json::json!({
            "txHash": self.adapter.tx_hash(nonce), "chainCode": "bnb", "symbol": "BNB",
            "transferType": 1, "txKind": 1, "fromAddr": address, "toAddr": super::request::BNB_TO_ADDR,
            "token": null, "transactionFee": 0.00002, "transactionTime": "2026-10-30T10:05:49Z",
            "status": true, "blockHeight": 1234
        }), "receipt-refresh-test").await
    }
}

impl TransferNonceScenario {
    pub(crate) async fn when_token_transfer_starts(&self) -> anyhow::Result<()> {
        let address = self.from_addr.borrow().clone();
        let mut req = make_transfer_req(&address, BNB_TO_ADDR);
        req.base.with_token(Some("0x0123456789012345678901234567890123456789".into()), 18, "USDT");
        self.env.manager.api_transfer_with_preloaded_private_key(req, "00".into()).await?;
        Ok(())
    }

    pub(crate) async fn given_receipt_token_asset(&self) -> anyhow::Result<()> {
        use wallet_database::{
            entities::{api_assets::ApiCreateAssetsVo, assets::AssetsId},
            repositories::api_wallet::assets::ApiAssetsRepo,
        };
        let pool = crate::harness::open_api_wallet_pool(&self.env.db_dir).await;
        wallet_database::repositories::api_wallet::coin::ApiCoinRepo::upsert_multi_coin(
            &pool,
            vec![wallet_database::entities::api_coin::ApiCoinData::new(
                Some("USDT".into()),
                "USDT",
                "bnb",
                "0x0123456789012345678901234567890123456789".into(),
                Some("1".into()),
                None,
                18,
                1,
                1,
                1,
                chrono::Utc::now(),
                Some(chrono::Utc::now()),
            )],
        )
        .await?;
        let address = self.from_addr.borrow().clone();
        ApiAssetsRepo::upsert_assets_multi(
            &pool,
            vec![ApiCreateAssetsVo::new(
                AssetsId::new(&address, "bnb", "0x0123456789012345678901234567890123456789".into()),
                "USDT",
                18,
                None,
                0,
            )],
        )
        .await?;
        Ok(())
    }
    pub(crate) fn given_only_token_balance_fails(&self, fail: bool) {
        self.adapter.fail_token_balance(fail);
    }
}

impl TransferNonceScenario {
    pub(crate) async fn given_frontend_notification_closed(&self) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);
        self.env.manager.set_frontend_notify_sender(tx).await.unwrap();
    }
}

impl TransferNonceScenario {
    pub(crate) fn given_other_receipt_balance_hangs(&self, other: &Self) {
        self.adapter.block_balance_for(other.from_addr.borrow().clone());
    }
    pub(crate) async fn then_old_receipt_balance_has_started(&self) -> anyhow::Result<()> {
        tokio::time::timeout(Duration::from_secs(5), self.adapter.wait_for_blocked_balance())
            .await?;
        Ok(())
    }
    pub(crate) async fn then_background_receipt_commits(&self, nonce: u64) -> anyhow::Result<()> {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let row = super::db::ordinary_transfer_by_hash(
                    self.env,
                    &self.from_addr.borrow().clone(),
                    &self.adapter.tx_hash(nonce),
                )
                .await
                .unwrap();
                if row.finished_at.is_some() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }
}

impl TransferNonceScenario {
    pub(crate) async fn then_receipt_refresh_pending_for(&self, nonce: u64) -> anyhow::Result<()> {
        let pool = super::db::open_api_transaction_pool(&self.env.db_dir).await;
        let address = self.from_addr.borrow().clone();
        let pending: i64 = sqlx::query_scalar(
            "SELECT receipt_refresh_pending FROM api_withdraws WHERE from_addr = ? AND tx_hash = ?",
        )
        .bind(address)
        .bind(self.adapter.tx_hash(nonce))
        .fetch_one(pool.read_ref())
        .await?;
        assert_eq!(pending, 1, "the specific interrupted receipt must retain its refresh debt");
        Ok(())
    }
}
