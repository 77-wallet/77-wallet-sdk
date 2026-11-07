use super::{
    adapter::{AdapterGuard, RecordingEthAdapter, install_adapter_for_chain},
    db,
};
use crate::harness::{ensure_env, next_tag};
use wallet_api::{error::service::ServiceError, testkit::collect_fee::FeeBuildTestWorker};
use wallet_database::{
    ApiTransactionDbPool,
    repositories::api_wallet::{fee::ApiFeeRepo, nonce::ApiNonceRepo},
};

pub(crate) struct FeeBuildScenario {
    pub trade: String,
    address: String,
    pool: ApiTransactionDbPool,
    pub adapter: RecordingEthAdapter,
    _guard: AdapterGuard,
    worker: FeeBuildTestWorker,
}
impl FeeBuildScenario {
    pub async fn given() -> anyhow::Result<Self> {
        let env = ensure_env().await;
        let address = db::ensure_evm_transfer_fixture(env, "arbitrum").await?;
        env.manager.set_passwd_cache(crate::harness::SMOKE_WALLET_PASSWORD).await?;
        let pool = db::open_api_transaction_pool(&env.db_dir).await;
        let trade = next_tag("fee-candidate");
        let to = super::request::BNB_TO_ADDR;
        let digest = wallet_utils::bytes_to_base64(&wallet_utils::md5_vec(
            &(address.clone() + to + "1" + env.manager.ctx().get_sn()),
        ));
        ApiFeeRepo::upsert_api_fee(
            &pool, "uid", "fee", &address, to, "1", &digest, "arbitrum", None, "ETH", &trade, 0,
        )
        .await?;
        ApiFeeRepo::set_tx_ack_sent(&pool, &trade).await?;
        let adapter = RecordingEthAdapter::new();
        adapter.set_nonce_response(Some(6));
        let guard = install_adapter_for_chain(&adapter, "arbitrum");
        let worker = FeeBuildTestWorker::new(env.manager.ctx());
        Ok(Self { trade, address, pool, adapter, _guard: guard, worker })
    }
    pub async fn given_saved_six(&self) -> anyhow::Result<String> {
        self.adapter.set_nonce_response(Some(5));
        db::seed_signed_collect_six(&self.pool, &self.address, "arbitrum").await
    }
    pub async fn then_filled_five_with_saved_six(&self, old_trade: &str) -> anyhow::Result<()> {
        let row = ApiFeeRepo::get_api_fee_by_trade_no(&self.pool, &self.trade).await?;
        assert_eq!(row.nonce, 5);
        assert!(row.raw_tx.is_some());
        let old=wallet_database::repositories::api_wallet::collect::ApiCollectRepo::get_api_collect_by_trade_no(&self.pool,old_trade).await?;
        assert_eq!(old.raw_tx.as_deref(), Some("saved-raw-six"));
        assert!(old.last_broadcast_at.is_some());
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.address, "arbitrum").await?, 6);
        Ok(())
    }
    pub async fn when_build(&self) -> Result<(), ServiceError> {
        self.worker.build(&self.trade).await
    }

    pub async fn when_inflight_build_loses_to_shortage(&self) -> Result<(), ServiceError> {
        self.adapter.block_build(true);
        let building = self.when_build();
        tokio::pin!(building);
        tokio::select! {
            result = &mut building => panic!("build must wait at adapter boundary: {result:?}"),
            _ = self.adapter.wait_for_first_entry() => {}
        }
        use wallet_api::error::business::{
            BusinessError,
            chain::{ChainError, InsufficientBalanceDetail},
        };
        self.worker
            .fail(
                &self.trade,
                ServiceError::Business(BusinessError::Chain(
                    ChainError::insufficient_balance_with_detail(
                        InsufficientBalanceDetail::new()
                            .reason("main coin balance is insufficient"),
                    ),
                )),
            )
            .await?;
        self.adapter.release_first();
        building.await
    }

    pub async fn when_recipient_rent_error_occurs(&self) -> Result<(), ServiceError> {
        use wallet_api::error::business::{
            BusinessError,
            chain::{ChainError, InsufficientBalanceDetail},
        };
        self.worker.fail(&self.trade, ServiceError::Business(BusinessError::Chain(
            ChainError::insufficient_balance_with_detail(
                InsufficientBalanceDetail::new().reason("recipient account is not initialized and transfer amount is below rent-exempt minimum"),
            ),
        ))).await
    }
    pub async fn given_occupied_slot(&self) {
        assert_eq!(ApiFeeRepo::update_building_at(&self.pool, &self.trade).await.unwrap(), 1);
    }
    pub async fn given_competing_reservation(&self) {
        ApiNonceRepo::set_nonce_floor(&self.pool, &self.address, "arbitrum", 6).await.unwrap();
    }
    pub async fn then_facts(&self, nonce: i64, has_raw: bool) {
        assert_eq!(
            ApiNonceRepo::get_api_nonce(&self.pool, &self.address, "arbitrum").await.unwrap(),
            nonce
        );
        let row = ApiFeeRepo::get_api_fee_by_trade_no(&self.pool, &self.trade).await.unwrap();
        assert_eq!(row.raw_tx.is_some(), has_raw);
        assert_eq!(row.tx_hash.is_some(), has_raw);
        if has_raw {
            assert_eq!(row.nonce, 6);
        }
        assert!(row.last_broadcast_at.is_none());
        assert!(row.transaction_time.is_none());
        assert_eq!(self.adapter.recorded_nonces().len(), 0);
    }
    pub async fn then_slot_released(&self) {
        assert!(
            ApiFeeRepo::get_api_fee_by_trade_no(&self.pool, &self.trade)
                .await
                .unwrap()
                .building_at
                .is_none()
        );
    }

    pub async fn then_failed_for_balance(&self) {
        use wallet_database::entities::api_fee::{ApiFeeStatus, ErrCode};
        let row = ApiFeeRepo::get_api_fee_by_trade_no(&self.pool, &self.trade).await.unwrap();
        assert_eq!(row.err_code, Some(ErrCode::BalanceInsufficient));
        assert_eq!(row.status, ApiFeeStatus::SendingTxFailed);
        assert!(row.err_msg.as_deref().unwrap().contains("main coin balance is insufficient"));
        assert!(row.finished_at.is_none(), "failure still needs receipt upload");
        let buildable = ApiFeeRepo::scan_can_build(&self.pool, 1000).await.unwrap();
        assert!(!buildable.iter().any(|row| row.trade_no == self.trade));
        let reportable =
            ApiFeeRepo::scan_need_tx_exec_receipt_upload(&self.pool, 1000).await.unwrap();
        assert!(reportable.iter().any(|row| row.trade_no == self.trade));
    }

    pub async fn then_retryable(&self) {
        let row = ApiFeeRepo::get_api_fee_by_trade_no(&self.pool, &self.trade).await.unwrap();
        assert!(row.err_code.is_none());
        assert!(row.finished_at.is_none());
        assert!(row.building_at.is_none());
        let buildable = ApiFeeRepo::scan_can_build(&self.pool, 1000).await.unwrap();
        assert!(buildable.iter().any(|row| row.trade_no == self.trade));
    }
}
