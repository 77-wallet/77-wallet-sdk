use super::{
    adapter::{AdapterGuard, RecordingEthAdapter, install_adapter_for_chain},
    db,
};
use crate::harness::{
    WorkerTestEnv, decrypt_captured_api_backend_body, ensure_worker_env, next_tag,
};
use wallet_api::{
    error::{
        business::{BusinessError, chain::ChainError},
        service::ServiceError,
    },
    testkit::collect_fee::{FeeBuildTestWorker, upload_fee_receipt_via_worker},
};
use wallet_database::{
    ApiTransactionDbPool, entities::api_fee::ErrCode, repositories::api_wallet::fee::ApiFeeRepo,
};

pub(crate) struct FeeFailureScenario {
    env: &'static WorkerTestEnv,
    ctx: &'static wallet_api::Context,
    pool: ApiTransactionDbPool,
    trade: String,
    worker: FeeBuildTestWorker,
    adapter: RecordingEthAdapter,
    _guard: AdapterGuard,
}

impl FeeFailureScenario {
    pub async fn given() -> Self {
        let env = ensure_worker_env().await;
        env.recorder.reset();
        // The shared manager Context may already belong to another harness.
        // Reuse the isolated Context constructor instead of the global OnceCell.
        let root = env.db_dir.join(next_tag("fee-receipt-context"));
        let dirs = wallet_api::dirs::Dirs::new(root.to_str().unwrap()).unwrap();
        let backend = &env.backend_url;
        let config = wallet_api::config::Config::new(&format!(
            r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "{backend}"
  test_url: "{backend}"
  prod_url: "{backend}"
aggregate_api:
  dev_url: "{backend}"
  test_url: "{backend}"
  prod_url: "{backend}"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
"#
        ))
        .unwrap();
        let pool = db::open_api_transaction_pool(&dirs.db_dir).await;
        let ctx = Box::leak(Box::new(
            wallet_api::Context::new_without_handles_for_test(dirs, config).await.unwrap(),
        ));
        let trade = next_tag("fee-terminal-shortage");
        ApiFeeRepo::upsert_api_fee(
            &pool, "uid", "fee", "payer", "receiver", "1", "v", "arbitrum", None, "ETH", &trade, 3,
        )
        .await
        .unwrap();
        ApiFeeRepo::set_tx_ack_sent(&pool, &trade).await.unwrap();
        ApiFeeRepo::update_building_at(&pool, &trade).await.unwrap();
        let adapter = RecordingEthAdapter::new();
        let guard = install_adapter_for_chain(&adapter, "arbitrum");
        let worker = FeeBuildTestWorker::new(ctx);
        Self { env, ctx, pool, trade, worker, adapter, _guard: guard }
    }

    pub async fn when_gas_balance_shortage(&self) {
        self.worker
            .fail(
                &self.trade,
                ServiceError::Business(BusinessError::Chain(ChainError::InsufficientFeeBalance)),
            )
            .await
            .unwrap();
    }

    pub fn given_receipt_upload_failure(&self) {
        self.env.recorder.fail_next_api_backend_call(500, "temporary backend error");
    }

    pub async fn when_upload_receipt(&self) {
        upload_fee_receipt_via_worker(self.ctx, &self.trade).await;
    }

    pub async fn when_stale_execution_is_queued(&self) {
        self.worker.build(&self.trade).await.unwrap();
        self.worker.broadcast(&self.trade).await.unwrap();
    }

    pub async fn when_restarted_execution_is_queued(&self) {
        let restarted = FeeBuildTestWorker::new(self.ctx);
        restarted.build(&self.trade).await.unwrap();
        restarted.broadcast(&self.trade).await.unwrap();
    }

    pub async fn then_failure_state(&self, uploaded: bool) {
        let fee = ApiFeeRepo::get_api_fee_by_trade_no(&self.pool, &self.trade).await.unwrap();
        assert_eq!(fee.err_code, Some(ErrCode::FeeInsufficient));
        assert_eq!(fee.tx_exec_receipt_uploaded_at.is_some(), uploaded);
        assert_eq!(fee.finished_at.is_some(), uploaded);
        assert!(fee.raw_tx.is_none());
        assert!(fee.tx_hash.is_none());
        assert!(fee.last_broadcast_at.is_none());
        assert!(fee.building_at.is_none());
        assert_eq!(self.adapter.build_calls(), 0);
        assert_eq!(self.adapter.recorded_nonces().len(), 0);
    }

    pub fn then_failure_receipts(&self, count: usize) {
        let requests = self.env.recorder.snapshot();
        assert_eq!(requests.len(), count);
        for request in requests {
            assert!(request.path.ends_with("aw/trans/executeComplete"));
            let payload = decrypt_captured_api_backend_body(&request.body);
            assert_eq!(payload["tradeNo"], self.trade);
            assert_eq!(payload["type"], "COL_FEE");
            assert_eq!(payload["status"], "FAIL");
            assert_eq!(payload["hash"], "");
        }
    }
}
