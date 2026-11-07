use super::{
    adapter::{AdapterGuard, RecordingEthAdapter, install_adapter_for_chain},
    db, request,
};
use crate::harness::{TestEnv, ensure_env};
use wallet_database::{
    entities::api_withdraw::ApiWithdrawStatus,
    repositories::api_wallet::{nonce::ApiNonceRepo, withdraw::ApiWithdrawRepo},
};

pub(crate) struct EvmTransferScenario {
    env: &'static TestEnv,
    from: String,
    chain: String,
    adapter: RecordingEthAdapter,
    _guards: Vec<AdapterGuard>,
}

impl EvmTransferScenario {
    pub(crate) async fn given_chain(chain: &str) -> anyhow::Result<Self> {
        let env = ensure_env().await;
        let from = db::ensure_evm_transfer_fixture(env, chain).await?;
        let adapter = RecordingEthAdapter::new();
        // The observer scans shared fixture rows, not just this scenario's chain.
        // Override every supported EVM route to prevent production RPC fallback.
        let guards = request::evm_chain_codes()
            .into_iter()
            .map(|code| {
                let route_adapter =
                    if code == chain { adapter.clone() } else { RecordingEthAdapter::new() };
                install_adapter_for_chain(&route_adapter, &code)
            })
            .collect();
        Ok(Self { env, from, chain: chain.to_string(), adapter, _guards: guards })
    }
    pub(crate) async fn given_persistence_fails(&self) -> anyhow::Result<()> {
        db::reject_signed_persistence(self.env, &self.from).await
    }
    pub(crate) fn given_build_blocks(&self) {
        self.adapter.block_build(true);
    }
    pub(crate) async fn then_build_has_entered(&self) {
        self.adapter.wait_for_first_entry().await;
    }
    pub(crate) fn when_build_is_released(&self) {
        self.adapter.block_build(false);
        self.adapter.release_first();
    }
    pub(crate) fn when_transfer_starts(&self) -> super::scenario::TransferTask {
        let req = request::make_evm_transfer_req(&self.from, &self.chain);
        let manager = &self.env.manager;
        tokio::spawn(async move {
            manager
                .api_transfer_with_preloaded_private_key(req, "00".into())
                .await
                .map(|r| r.tx_hash)
        })
    }
    pub(crate) async fn when_foreign_counter_advances(&self) -> anyhow::Result<i64> {
        // Inject a concurrent committed counter change, not a preallocation API.
        let pool = db::open_api_transaction_pool(&self.env.db_dir).await;
        Ok(ApiNonceRepo::set_nonce_floor(&pool, &self.from, &self.chain, 1).await?)
    }
    pub(crate) async fn then_nonce_floor_is(&self, floor: i64) -> anyhow::Result<()> {
        let pool = db::open_api_transaction_pool(&self.env.db_dir).await;
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, &self.from, &self.chain).await?, floor);
        Ok(())
    }
    pub(crate) fn given_build_fails(&self, fail: bool) {
        self.adapter.fail_build(fail);
    }
    pub(crate) async fn given_persistence_recovers(&self) -> anyhow::Result<()> {
        db::allow_signed_persistence(self.env, &self.from).await
    }
    pub(crate) async fn then_pre_send_failure_has_no_reservation(&self) -> anyhow::Result<()> {
        self.then_nothing_was_sent_or_saved().await?;
        let pool = db::open_api_transaction_pool(&self.env.db_dir).await;
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, &self.from, &self.chain).await?, 0);
        Ok(())
    }
    pub(crate) fn given_visibility_tls_error(&self) {
        self.adapter.fail_visibility_with_tls();
    }
    pub(crate) fn then_no_visibility_query_was_needed(&self) {
        assert_eq!(self.adapter.visibility_queries(), 0);
    }
    pub(crate) fn given_visibility(&self, visible: bool, error: bool) {
        self.adapter.set_visibility(visible, error);
    }
    pub(crate) fn given_duplicate_broadcast(&self) {
        self.adapter.duplicate_transfer();
    }
    pub(crate) fn given_hash_mismatch(&self) {
        self.adapter.mismatch_hash();
    }
    pub(crate) async fn when_shared_broadcast_runs(&self) -> anyhow::Result<bool> {
        let response = wallet_api::testkit::withdraw::broadcast_transfer_for_test(
            self.env.manager.ctx(),
            &self.chain,
            wallet_api::domain::api_wallet::RawTx::Evm(
                1_u64.to_be_bytes().to_vec(),
                alloy::primitives::U256::ZERO,
            ),
            &self.expected_hash(),
        )
        .await?;
        Ok(response.is_some())
    }
    pub(crate) fn then_visibility_queries_are(&self, count: usize) {
        assert_eq!(self.adapter.visibility_queries(), count);
    }
    pub(crate) fn expected_hash(&self) -> String {
        self.adapter.tx_hash(1)
    }
    pub(crate) fn given_timeout(&self) {
        self.adapter.timeout_transfer();
    }
    pub(crate) async fn when_transfer_runs(
        &self,
    ) -> Result<String, wallet_api::error::service::ServiceError> {
        let req = request::make_evm_transfer_req(&self.from, &self.chain);
        self.env
            .manager
            .api_transfer_with_preloaded_private_key(req, "00".into())
            .await
            .map(|r| r.tx_hash)
    }
    pub(crate) async fn then_signed_fact_and_send_are(&self, accepted: bool) -> anyhow::Result<()> {
        let row =
            db::ordinary_transfer_by_hash(self.env, &self.from, &self.adapter.tx_hash(1)).await?;
        assert_eq!(row.chain_code, self.chain);
        assert_eq!(row.nonce, 1);
        assert_eq!(row.last_broadcast_at.is_some(), accepted);
        assert_eq!(row.broadcast_uncertain_since_at.is_some(), !accepted);
        assert!(row.finished_at.is_none());
        assert!(row.chain_success_at.is_none());
        let raw: wallet_api::domain::api_wallet::RawTx =
            serde_json::from_str(row.raw_tx.as_deref().unwrap())?;
        let wallet_api::domain::api_wallet::RawTx::Evm(bytes, _) = raw else {
            panic!("EVM raw expected")
        };
        assert_eq!(self.adapter.recorded_raw(), vec![bytes]);
        assert_eq!(self.adapter.recorded_nonces(), vec![1]);
        Ok(())
    }
    pub(crate) async fn when_receipt_is_observed(&self, success: bool) -> anyhow::Result<()> {
        self.adapter.receipt_behavior(1, if success { 2 } else { 3 });
        wallet_api::testkit::withdraw::observe_pending_self_transfers(self.env.manager.ctx())
            .await?;
        Ok(())
    }
    pub(crate) async fn then_confirmed_without_resend(&self, success: bool) -> anyhow::Result<()> {
        let row =
            db::ordinary_transfer_by_hash(self.env, &self.from, &self.adapter.tx_hash(1)).await?;
        assert_eq!(
            row.status,
            if success { ApiWithdrawStatus::Success } else { ApiWithdrawStatus::Failure }
        );
        assert!(row.finished_at.is_some());
        assert_eq!(row.chain_success_at.is_some(), success);
        assert_eq!(row.chain_failed_at.is_some(), !success);
        assert_eq!(self.adapter.recorded_nonces(), vec![1]);
        let pool = db::open_api_transaction_pool(&self.env.db_dir).await;
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, &self.from, &self.chain).await?, 1);
        assert!(
            ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, &self.from, &self.chain)
                .await?
                .is_empty()
        );
        Ok(())
    }
    pub(crate) async fn then_nothing_was_sent_or_saved(&self) -> anyhow::Result<()> {
        assert!(self.adapter.recorded_nonces().is_empty());
        let pool = db::open_api_transaction_pool(&self.env.db_dir).await;
        assert!(
            ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, &self.from, &self.chain)
                .await?
                .is_empty()
        );
        Ok(())
    }
}
