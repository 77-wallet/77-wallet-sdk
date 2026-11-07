use super::{
    adapter::{AdapterGuard, RecordingEthAdapter, install_adapter},
    db,
};
use crate::harness::{ensure_env, next_tag};
use wallet_database::{
    ApiTransactionDbPool,
    entities::{api_trade_type::ApiTradeType, api_withdraw::ApiWithdrawStatus},
    repositories::api_wallet::{nonce::ApiNonceRepo, withdraw::ApiWithdrawRepo},
};

pub(crate) struct WithdrawBuildScenario {
    ctx: &'static wallet_api::Context,
    pool: ApiTransactionDbPool,
    trade: String,
    from: String,
    pub adapter: RecordingEthAdapter,
    _guard: AdapterGuard,
}
impl WithdrawBuildScenario {
    pub async fn given() -> anyhow::Result<Self> {
        let env = ensure_env().await;
        let from = db::ensure_bnb_transfer_fixture(env).await?;
        env.manager.set_passwd_cache(crate::harness::SMOKE_WALLET_PASSWORD).await?;
        let pool = db::open_api_transaction_pool(&env.db_dir).await;
        let trade = next_tag("withdraw-build-candidate");
        let to = super::request::BNB_TO_ADDR;
        let digest = wallet_utils::bytes_to_base64(&wallet_utils::md5_vec(
            &(from.clone() + to + "1" + env.manager.ctx().get_sn()),
        ));
        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "withdraw",
            &from,
            to,
            "1",
            &digest,
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
        sqlx::query("UPDATE api_withdraws SET tx_ack_sent_at=CURRENT_TIMESTAMP,audit_passed_at=CURRENT_TIMESTAMP WHERE trade_no=?").bind(&trade).execute(pool.write_ref()).await?;
        let adapter = RecordingEthAdapter::new();
        adapter.set_nonce_response(Some(16));
        adapter.fund_withdrawal();
        let guard = install_adapter(&adapter);
        Ok(Self { ctx: env.manager.ctx(), pool, trade, from, adapter, _guard: guard })
    }
    pub fn given_frozen(&self, frozen: bool) -> anyhow::Result<()> {
        use wallet_api::infrastructure::nonce::nonce_engine::{
            FreezeReason, get_nonce_engine_with_ctx,
        };
        let engine = get_nonce_engine_with_ctx(self.ctx)?;
        if frozen {
            engine.freeze(&self.from, "bnb", FreezeReason::Manual);
        } else {
            engine.unfreeze(&self.from, "bnb", FreezeReason::Manual);
        }
        Ok(())
    }
    pub fn then_no_nonce_query_or_build(&self) {
        assert_eq!(self.adapter.nonce_queries(), 0);
        assert_eq!(self.adapter.build_calls(), 0);
    }
    pub async fn given_saved_six(&self) -> anyhow::Result<String> {
        self.adapter.set_nonce_response(Some(5));
        db::seed_signed_collect_six(&self.pool, &self.from, "bnb").await
    }
    pub async fn then_filled_five_with_saved_six(&self, old_trade: &str) -> anyhow::Result<()> {
        let row = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &self.pool,
            &self.trade,
            ApiTradeType::Withdraw,
        )
        .await?;
        assert_eq!(row.nonce, 5);
        assert!(row.raw_tx.is_some());
        let old=wallet_database::repositories::api_wallet::collect::ApiCollectRepo::get_api_collect_by_trade_no(&self.pool,old_trade).await?;
        assert_eq!(old.raw_tx.as_deref(), Some("saved-raw-six"));
        assert!(old.last_broadcast_at.is_some());
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "bnb").await?, 6);
        Ok(())
    }
    pub async fn when_build(&self) -> anyhow::Result<()> {
        wallet_api::testkit::withdraw::build_withdraw_via_worker(self.ctx, &self.trade).await?;
        Ok(())
    }
    pub async fn then_facts(&self, signed: bool) -> anyhow::Result<()> {
        let row = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &self.pool,
            &self.trade,
            ApiTradeType::Withdraw,
        )
        .await?;
        assert_eq!(row.raw_tx.is_some(), signed, "{:?}", row.err_msg);
        assert_eq!(row.tx_hash.is_some(), signed);
        assert!(row.err_code.is_none(), "{:?}", row.err_msg);
        assert_eq!(row.building_at.is_some(), signed);
        assert_eq!(
            ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "bnb").await?,
            if signed { 16 } else { 0 }
        );
        if signed {
            assert_eq!(row.nonce, 16);
        }
        assert!(self.adapter.recorded_nonces().is_empty());
        Ok(())
    }
}
