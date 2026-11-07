use super::{
    adapter::{AdapterGuard, RecordingEthAdapter, install_adapter},
    db,
};
use crate::harness::{
    ensure_env, next_tag,
    worker::{MockBackendRecorder, worker_context_without_actors},
};
use wallet_api::{domain::api_wallet::RawTx, testkit::withdraw::broadcast_withdraw_via_worker};
use wallet_database::{
    entities::{api_trade_type::ApiTradeType, api_withdraw::ApiWithdrawStatus},
    repositories::api_wallet::withdraw::ApiWithdrawRepo,
};

pub(crate) struct WithdrawBroadcastScenario {
    ctx: &'static wallet_api::Context,
    db_dir: std::path::PathBuf,
    backend: Option<MockBackendRecorder>,
    pub(crate) adapter: RecordingEthAdapter,
    _guard: AdapterGuard,
    orders: Vec<String>,
    from: String,
}

impl WithdrawBroadcastScenario {
    pub(crate) async fn given_signed_batch() -> anyhow::Result<Self> {
        let env = ensure_env().await;
        let from = db::ensure_bnb_transfer_fixture(env).await?;
        Self::signed_batch(env.manager.ctx(), env.db_dir.clone(), from, None).await
    }

    pub(crate) async fn given_signed_batch_with_receipts() -> anyhow::Result<Self> {
        let (ctx, db_dir, recorder) = worker_context_without_actors().await;
        Self::signed_batch(ctx, db_dir, next_tag("nonce-gap-sender"), Some(recorder)).await
    }

    async fn signed_batch(
        ctx: &'static wallet_api::Context,
        db_dir: std::path::PathBuf,
        from: String,
        backend: Option<MockBackendRecorder>,
    ) -> anyhow::Result<Self> {
        let adapter = RecordingEthAdapter::new();
        adapter.set_nonce_response(Some(14));
        adapter.require_next_nonce();
        let guard = install_adapter(&adapter);
        let pool = db::open_api_transaction_pool(&db_dir).await;
        let mut orders = Vec::new();
        for nonce in 14_u64..=16 {
            let trade = next_tag("withdraw-broadcast-order");
            ApiWithdrawRepo::upsert_api_withdraw(
                &pool,
                "uid",
                "withdraw",
                &from,
                super::request::BNB_TO_ADDR,
                "1",
                "digest",
                "bnb",
                None,
                "BNB",
                &trade,
                None,
                None,
                None,
                ApiTradeType::Withdraw,
                1,
                None,
                ApiWithdrawStatus::AuditPass,
                ApiWithdrawStatus::InitOrder,
                "",
                "",
                None,
                None,
            )
            .await?;
            let raw = serde_json::to_string(&RawTx::Evm(
                nonce.to_be_bytes().to_vec(),
                alloy::primitives::U256::ZERO,
            ))?;
            sqlx::query("UPDATE api_withdraws SET raw_tx=?, tx_hash=?, nonce=?, tx_ack_sent_at=CURRENT_TIMESTAMP, audit_passed_at=CURRENT_TIMESTAMP WHERE trade_no=?")
                .bind(raw).bind(adapter.tx_hash(nonce)).bind(nonce as i32).bind(&trade)
                .execute(pool.write_ref()).await?;
            orders.push(trade);
        }
        wallet_database::repositories::api_wallet::nonce::ApiNonceRepo::set_nonce_floor(
            &pool, &from, "bnb", 16,
        )
        .await?;
        Ok(Self { ctx, db_dir, backend, adapter, _guard: guard, orders, from })
    }

    pub(crate) async fn given_predecessor_failed(&self) -> anyhow::Result<()> {
        let pool = db::open_api_transaction_pool(&self.db_dir).await;
        sqlx::query("UPDATE api_withdraws SET err_code=6008,err_msg='deterministic failure',failure_stage=2,finished_at=CURRENT_TIMESTAMP WHERE trade_no=?")
            .bind(&self.orders[0]).execute(pool.write_ref()).await?;
        Ok(())
    }

    pub(crate) async fn then_gap_failed(&self, nonce: usize) -> anyhow::Result<()> {
        let pool = db::open_api_transaction_pool(&self.db_dir).await;
        let row = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            &self.orders[nonce - 14],
            ApiTradeType::Withdraw,
        )
        .await?;
        assert_eq!(
            row.err_code,
            Some(wallet_database::entities::api_withdraw::ErrCode::SDKInternalError)
        );
        assert!(row.err_msg.as_deref().unwrap().starts_with("nonce_gap_predecessor_failed:"));
        assert!(row.last_broadcast_at.is_none());
        assert!(row.broadcast_uncertain_since_at.is_none());
        assert!(row.raw_tx.is_some());
        assert_eq!(row.nonce, nonce as i64);
        Ok(())
    }

    pub(crate) async fn when_failure_receipt_uploaded(&self, nonce: usize) -> anyhow::Result<()> {
        wallet_api::testkit::withdraw::upload_withdraw_tx_exec_receipt_via_worker(
            self.ctx,
            &self.orders[nonce - 14],
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn then_failure_receipts_uploaded_once(&self) -> anyhow::Result<()> {
        let requests = self.backend.as_ref().unwrap().snapshot();
        let receipts: Vec<_> = requests
            .iter()
            .filter(|r| r.path.contains("/awallet/aw/trans/executeComplete"))
            .collect();
        assert_eq!(receipts.len(), 2);
        let pool = db::open_api_transaction_pool(&self.db_dir).await;
        for trade in &self.orders[1..] {
            let row =
                ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, trade, ApiTradeType::Withdraw)
                    .await?;
            assert!(row.tx_exec_receipt_uploaded_at.is_some());
            assert!(row.finished_at.is_some());
            let payload = receipts
                .iter()
                .map(|r| crate::harness::decrypt_captured_api_backend_body(&r.body))
                .find(|p| p["tradeNo"].as_str() == Some(trade))
                .expect("failure receipt");
            assert_eq!(payload["status"], "FAIL");
            assert_eq!(payload["errorCode"], "ERR_6008");
        }
        Ok(())
    }

    pub(crate) async fn then_next_nonce_reusable(&self) -> anyhow::Result<()> {
        use wallet_database::entities::api_nonce_claim::WithdrawNonceDecision;
        let pool = db::open_api_transaction_pool(&self.db_dir).await;
        assert_eq!(
            ApiWithdrawRepo::observe_withdraw_nonce(&pool, &self.from, "bnb", 14).await?,
            WithdrawNonceDecision::Ready(14)
        );
        Ok(())
    }

    pub(crate) async fn given_broadcast_persistence_fails(&self) -> anyhow::Result<()> {
        let pool = db::open_api_transaction_pool(&self.db_dir).await;
        sqlx::query("CREATE TRIGGER reject_broadcast_fact BEFORE UPDATE OF last_broadcast_at ON api_withdraws WHEN NEW.last_broadcast_at IS NOT NULL BEGIN SELECT RAISE(ABORT,'broadcast fact storage unavailable'); END").execute(pool.write_ref()).await?;
        Ok(())
    }

    pub(crate) async fn then_send_is_uncertain(&self, nonce: usize) -> anyhow::Result<()> {
        let pool = db::open_api_transaction_pool(&self.db_dir).await;
        let row = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            &self.orders[nonce - 14],
            ApiTradeType::Withdraw,
        )
        .await?;
        assert!(row.broadcast_uncertain_since_at.is_some());
        assert_eq!(row.broadcast_uncertain_retry_count, 1);
        assert!(row.err_code.is_none());
        assert!(row.last_broadcast_at.is_none());
        assert!(row.raw_tx.is_some());
        Ok(())
    }

    pub(crate) async fn when_broadcast(&self, nonce: usize) -> anyhow::Result<()> {
        broadcast_withdraw_via_worker(self.ctx, &self.orders[nonce - 14]).await?;
        Ok(())
    }

    pub(crate) fn when_broadcast_starts(
        &self,
        nonce: usize,
    ) -> tokio::task::JoinHandle<Result<(), wallet_api::error::service::ServiceError>> {
        let ctx = self.ctx;
        let trade = self.orders[nonce - 14].clone();
        tokio::spawn(async move { broadcast_withdraw_via_worker(ctx, &trade).await })
    }

    pub(crate) async fn then_order_is(&self, nonce: usize, sent: bool) -> anyhow::Result<()> {
        let row = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &db::open_api_transaction_pool(&self.db_dir).await,
            &self.orders[nonce - 14],
            ApiTradeType::Withdraw,
        )
        .await?;
        assert_eq!(
            wallet_database::repositories::api_wallet::nonce::ApiNonceRepo::get_api_nonce(
                &db::open_api_transaction_pool(&self.db_dir).await,
                &self.from,
                "bnb"
            )
            .await?,
            16
        );
        assert_eq!(row.nonce, nonce as i64);
        assert_eq!(row.tx_hash.as_deref(), Some(self.adapter.tx_hash(nonce as u64).as_str()));
        let raw: RawTx = serde_json::from_str(row.raw_tx.as_deref().unwrap())?;
        let RawTx::Evm(bytes, _) = raw else { panic!("EVM raw expected") };
        assert_eq!(bytes, (nonce as u64).to_be_bytes());
        assert_eq!(row.last_broadcast_at.is_some(), sent);
        assert!(row.err_code.is_none(), "waiting must not become ERR_6008: {:?}", row.err_msg);
        assert!(row.finished_at.is_none());
        assert!(row.tx_exec_receipt_uploaded_at.is_none());
        assert!(row.broadcast_uncertain_since_at.is_none());
        Ok(())
    }
}
