use super::{
    adapter::{AdapterGuard, RecordingEthAdapter, install_adapter_for_chain},
    db,
};
use crate::harness::{ensure_env, next_tag, open_api_wallet_pool};
use wallet_database::{
    ApiTransactionDbPool,
    entities::{
        api_collect::ApiCollectStatus, api_collect_strategy::ApiCollectStrategyEntity,
        api_collect_strategy_chain_config::ApiCollectStrategyChainConfigEntity,
    },
    repositories::api_wallet::{
        account::ApiAccountRepo, collect::ApiCollectRepo, collect_strategy::ApiCollectStrategyRepo,
        collect_strategy_chain_config::ApiCollectStrategyChainConfigRepo, nonce::ApiNonceRepo,
    },
};

pub(crate) struct CollectBuildScenario {
    ctx: &'static wallet_api::Context,
    pool: ApiTransactionDbPool,
    trade: String,
    from: String,
    pub adapter: RecordingEthAdapter,
    _guard: AdapterGuard,
}
impl CollectBuildScenario {
    pub async fn given() -> anyhow::Result<Self> {
        let env = ensure_env().await;
        let from = db::ensure_evm_transfer_fixture(env, "scroll").await?;
        let api_pool = open_api_wallet_pool(&env.db_dir).await;
        let account = ApiAccountRepo::find_one_by_address_chain_code(&from, "scroll", &api_pool)
            .await?
            .unwrap();
        let now = chrono::Utc::now();
        ApiCollectStrategyRepo::upsert(
            &api_pool,
            ApiCollectStrategyEntity {
                id: 0,
                uid: account.uid.clone(),
                threshold: 1,
                created_at: now,
                updated_at: None,
            },
        )
        .await?;
        let strategy = ApiCollectStrategyRepo::get_by_uid(&api_pool, &account.uid).await?.unwrap();
        let to = super::request::BNB_TO_ADDR;
        ApiCollectStrategyChainConfigRepo::upsert(
            &api_pool,
            ApiCollectStrategyChainConfigEntity {
                id: 0,
                strategy_id: strategy.id,
                chain_code: "scroll".into(),
                chain_address_type: None,
                normal_idx: None,
                normal_address: to.into(),
                risk_idx: None,
                risk_address: to.into(),
                created_at: now,
                updated_at: None,
            },
        )
        .await?;
        let pool = db::open_api_transaction_pool(&env.db_dir).await;
        let trade = next_tag("collect-main-build");
        let digest = wallet_utils::bytes_to_base64(&wallet_utils::md5_vec(
            &(from.clone() + "1" + env.manager.ctx().get_sn()),
        ));
        ApiCollectRepo::upsert_api_collect(
            &pool,
            &account.uid,
            "collect",
            &from,
            "",
            "1",
            &digest,
            "scroll",
            None,
            "ETH",
            &trade,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await?;
        ApiCollectRepo::set_order_ack_sent(&pool, &trade).await?;
        let adapter = RecordingEthAdapter::new();
        adapter.set_nonce_response(Some(5));
        adapter.fund_withdrawal();
        let guard = install_adapter_for_chain(&adapter, "scroll");
        Ok(Self { ctx: env.manager.ctx(), pool, trade, from, adapter, _guard: guard })
    }
    pub async fn given_saved_six(&self) -> anyhow::Result<()> {
        self.seed_saved_six("saved-six").await
    }
    async fn seed_saved_six(&self, old_trade: &str) -> anyhow::Result<()> {
        use wallet_database::repositories::api_wallet::collect::SignedCollectCandidate;
        let row = ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade).await?;
        ApiCollectRepo::upsert_api_collect(
            &self.pool,
            &row.uid,
            "old-collect",
            &self.from,
            "receiver",
            "1",
            "v",
            "scroll",
            None,
            "ETH",
            old_trade,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await?;
        ApiCollectRepo::set_order_ack_sent(&self.pool, old_trade).await?;
        ApiCollectRepo::update_building_at(&self.pool, old_trade).await?;
        ApiCollectRepo::reserve_signed_collect(
            &self.pool,
            &SignedCollectCandidate {
                trade_no: old_trade,
                from_addr: &self.from,
                chain_code: "scroll",
                nonce: 6,
                chain_next: 6,
                tx_hash: "saved-hash-six",
                raw_tx: "saved-raw-six",
                transaction_fee: "0.1",
            },
        )
        .await?;
        sqlx::query("UPDATE api_collect SET last_broadcast_at=CURRENT_TIMESTAMP WHERE trade_no=?")
            .bind(old_trade)
            .execute(self.pool.write_ref())
            .await?;
        Ok(())
    }
    pub async fn given_replacement_at_frontier(&self) -> anyhow::Result<()> {
        use wallet_database::repositories::api_wallet::collect_replacement::{
            CollectReplacementRepo, CollectReplacementScope,
        };
        let old_trade = format!("old-{}", self.trade);
        self.seed_saved_six(&old_trade).await?;
        let target = super::request::BNB_TO_ADDR;
        sqlx::query("UPDATE api_collect SET to_addr=? WHERE trade_no IN (?, ?)")
            .bind(target)
            .bind(&self.trade)
            .bind(&old_trade)
            .execute(self.pool.write_ref())
            .await?;
        ApiCollectRepo::fail_verified_nonce_gap(&self.pool, &old_trade, 5, 6).await?;
        ApiNonceRepo::set_nonce_floor(&self.pool, &self.from, "scroll", 153).await?;
        self.adapter.set_nonce_response(Some(6));
        self.adapter.set_latest_nonce_response(Some(6));
        let scope = CollectReplacementScope {
            chain_code: "scroll".into(),
            from_addr: self.from.clone(),
            old_trade_no: old_trade,
            old_hash: "saved-hash-six".into(),
            nonce: 6,
            legacy_counter: 153,
        };
        assert!(
            CollectReplacementRepo::prepare(&self.pool, &self.trade, &scope, 6).await?.is_some()
        );
        Ok(())
    }
    pub async fn given_automatic_replacement_without_configuration(&self) -> anyhow::Result<()> {
        self.given_replacement_at_frontier().await?;
        sqlx::query("DELETE FROM api_collect_replacement WHERE new_trade_no=?")
            .bind(&self.trade)
            .execute(self.pool.write_ref())
            .await?;
        sqlx::query(
            "UPDATE api_collect SET tx_exec_receipt_uploaded_at=CURRENT_TIMESTAMP WHERE trade_no=?",
        )
        .bind(format!("old-{}", self.trade))
        .execute(self.pool.write_ref())
        .await?;
        assert!(wallet_database::repositories::api_wallet::collect_replacement::CollectReplacementRepo::discover(&self.pool, &self.trade).await?.is_some(), "synthetic old/new identities should be automatically eligible");
        self.adapter.set_nonce_response(Some(7));
        self.adapter.require_replacement_quote();
        Ok(())
    }
    pub async fn given_restarted_context_without_recovery_override(
        &mut self,
    ) -> anyhow::Result<()> {
        let config = wallet_api::config::Config::new(
            "app_code: test\ncrypto: { aes_key: 1234567890abcdef, aes_iv: abcdef1234567890 }\nbackend_api: { dev_url: 'http://127.0.0.1:9', test_url: 'http://127.0.0.1:9', prod_url: 'http://127.0.0.1:9' }\naggregate_api: { dev_url: 'http://127.0.0.1:9', test_url: 'http://127.0.0.1:9', prod_url: 'http://127.0.0.1:9' }\n",
        )?;
        let ctx = wallet_api::Context::new_without_handles_for_test(
            (*self.ctx.get_global_dirs()).clone(),
            config,
        )
        .await?;
        self.ctx = Box::leak(Box::new(ctx));
        let digest = wallet_utils::bytes_to_base64(&wallet_utils::md5_vec(
            &(self.from.clone() + "1" + self.ctx.get_sn()),
        ));
        sqlx::query("UPDATE api_collect SET validate=? WHERE trade_no=?")
            .bind(digest)
            .bind(&self.trade)
            .execute(self.pool.write_ref())
            .await?;
        Ok(())
    }
    pub async fn given_next_collect_after_replacement(&mut self) -> anyhow::Result<()> {
        let old = ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade).await?;
        self.trade = next_tag("after-replacement");
        let digest = wallet_utils::bytes_to_base64(&wallet_utils::md5_vec(
            &(self.from.clone() + "1" + self.ctx.get_sn()),
        ));
        ApiCollectRepo::upsert_api_collect(
            &self.pool,
            &old.uid,
            "collect",
            &self.from,
            "",
            "1",
            &digest,
            "scroll",
            None,
            "ETH",
            &self.trade,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await?;
        ApiCollectRepo::set_order_ack_sent(&self.pool, &self.trade).await?;
        self.adapter.set_nonce_response(Some(7));
        self.adapter.set_latest_nonce_response(Some(7));
        self.adapter.allow_ordinary_quote();
        Ok(())
    }
    pub async fn then_next_collect_owns_seven_without_replacement(&self) -> anyhow::Result<()> {
        let row = ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade).await?;
        assert_eq!(row.nonce, 7);
        assert!(row.raw_tx.is_some());
        assert_eq!(self.adapter.build_calls(), 2);
        assert!(wallet_database::repositories::api_wallet::collect_replacement::CollectReplacementRepo::find(&self.pool,&self.trade).await?.is_none());
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await?, 7);
        Ok(())
    }
    pub async fn when_replacement_build_command(&self) -> anyhow::Result<()> {
        use std::sync::Arc;
        use wallet_api::infrastructure::api_trans::{
            AddressLockManager, ShadowAdvancer, ShadowCollectCommand, ShadowCollectWorker,
        };
        let (tx, _) = tokio::sync::mpsc::channel(32);
        let worker = ShadowCollectWorker::new(
            self.ctx,
            Arc::new(AddressLockManager::new()),
            Arc::new(ShadowAdvancer::new(self.ctx, tx, None)),
        );
        worker.handle(ShadowCollectCommand::BuildTx(self.trade.clone())).await?;
        Ok(())
    }
    pub async fn then_replacement_fee_failure_does_not_sign_or_top_up(&self) -> anyhow::Result<()> {
        let row = ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade).await?;
        assert!(row.raw_tx.is_none());
        assert!(row.tx_hash.is_none());
        assert_eq!(
            row.err_code,
            Some(wallet_database::entities::api_collect::ErrCode::FeeInsufficient)
        );
        assert_ne!(row.need_service_fee, Some(true));
        assert_eq!(self.adapter.build_calls(), 0);
        let r=wallet_database::repositories::api_wallet::collect_replacement::CollectReplacementRepo::find(&self.pool,&self.trade).await?.unwrap();
        assert!(r.abandoned_at.is_some());
        assert!(r.new_hash.is_none());
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await?, 153);
        Ok(())
    }
    pub async fn then_automatic_replacement_waits_unsigned(&self) -> anyhow::Result<()> {
        let row = ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade).await?;
        assert!(row.raw_tx.is_none());
        assert!(row.tx_hash.is_none());
        assert!(row.err_code.is_none());
        assert_eq!(self.adapter.build_calls(), 0);
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await?, 153);
        Ok(())
    }
    pub async fn then_replacement_waits_unsigned(&self) -> anyhow::Result<()> {
        let row = ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade).await?;
        assert!(row.raw_tx.is_none());
        assert!(row.tx_hash.is_none());
        assert!(row.err_code.is_none());
        assert_eq!(self.adapter.build_calls(), 0);
        let r=wallet_database::repositories::api_wallet::collect_replacement::CollectReplacementRepo::find(&self.pool,&self.trade).await?.unwrap();
        assert!(r.winner_hash.is_none());
        assert!(r.new_hash.is_none());
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await?, 153);
        Ok(())
    }
    pub async fn then_replacement_is_signed_once(&self) -> anyhow::Result<()> {
        let row = ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade).await?;
        assert_eq!(row.nonce, 6);
        assert!(row.raw_tx.is_some());
        let relation=wallet_database::repositories::api_wallet::collect_replacement::CollectReplacementRepo::find(&self.pool,&self.trade).await?.unwrap();
        assert_eq!(relation.new_hash, row.tx_hash);
        assert_eq!(relation.new_raw, row.raw_tx);
        assert!(relation.winner_hash.is_none());
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await?, 153);
        assert_eq!(
            ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &format!("old-{}", self.trade))
                .await?
                .raw_tx
                .as_deref(),
            Some("saved-raw-six")
        );
        assert!(self.adapter.recorded_nonces().is_empty());
        assert_eq!(self.adapter.build_calls(), 1);
        let raw: wallet_api::domain::api_wallet::RawTx =
            serde_json::from_str(row.raw_tx.as_deref().unwrap())?;
        let wallet_api::domain::api_wallet::RawTx::Evm(bytes, _) = raw else {
            panic!("expected EVM")
        };
        assert_eq!(u64::from_be_bytes(bytes.try_into().unwrap()), 6);
        Ok(())
    }

    pub async fn given_replacement_winner(&self, old: bool, success: bool) -> anyhow::Result<()> {
        let r=wallet_database::repositories::api_wallet::collect_replacement::CollectReplacementRepo::find(&self.pool,&self.trade).await?.unwrap();
        self.adapter.set_replacement_receipt(
            wallet_api::testkit::adapter_factory::ReplacementReceipt {
                transaction_fee: "0.000000000000000123".into(),
                resource_consume: "21000".into(),
                tx_hash: if old { r.old_hash } else { r.new_hash.unwrap() },
                success,
                block_hash: format!("0x{}", "ab".repeat(32)),
                block_number: 42,
                transaction_time: "2026-10-19T00:00:00Z".into(),
            },
        );
        self.adapter.set_nonce_response(Some(7));
        Ok(())
    }
    pub async fn when_replacement_recovers(&self) -> anyhow::Result<()> {
        use std::sync::Arc;
        use wallet_api::infrastructure::api_trans::{
            AddressLockManager, ShadowAdvancer, ShadowCollectCommand, ShadowCollectWorker,
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        let advancer = Arc::new(ShadowAdvancer::new(self.ctx, tx, None));
        let worker =
            ShadowCollectWorker::new(self.ctx, Arc::new(AddressLockManager::new()), advancer);
        worker.handle(ShadowCollectCommand::Recover(self.trade.clone())).await?;
        Ok(())
    }
    pub async fn then_replacement_reconciled(
        &self,
        old: bool,
        success: bool,
    ) -> anyhow::Result<()> {
        use wallet_database::entities::api_nonce_claim::WithdrawNonceDecision;
        let r=wallet_database::repositories::api_wallet::collect_replacement::CollectReplacementRepo::find(&self.pool,&self.trade).await?.unwrap();
        let new = ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade).await?;
        assert_eq!(
            r.winner_hash.as_deref(),
            if old { Some(r.old_hash.as_str()) } else { r.new_hash.as_deref() }
        );
        assert_eq!(new.transaction_time.is_some(), !old && success);
        if !old && success {
            assert_eq!(new.transaction_fee, "0.000000000000000123");
            assert_eq!(new.resource_consume, "21000");
        }
        assert_eq!(new.err_code.is_some(), old || !success);
        assert_eq!(new.tx_hash, r.new_hash);
        assert_eq!(
            ApiNonceRepo::observe_signed_nonce(&self.pool, &self.from, "scroll", 7).await?,
            WithdrawNonceDecision::Ready(7)
        );
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await?, 6);
        assert!(self.adapter.recorded_nonces().is_empty());
        assert!(
            ApiCollectRepo::scan_need_tx_exec_receipt_upload(&self.pool, 10000)
                .await?
                .iter()
                .any(|x| x.trade_no == self.trade)
        );
        Ok(())
    }

    pub async fn then_hole_filled_and_six_preserved(&self) -> anyhow::Result<()> {
        let five = ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade).await?;
        assert_eq!(five.nonce, 5);
        assert!(five.raw_tx.is_some());
        let six = ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, "saved-six").await?;
        assert_eq!(six.nonce, 6);
        assert_eq!(six.raw_tx.as_deref(), Some("saved-raw-six"));
        assert!(six.last_broadcast_at.is_some());
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await?, 6);
        Ok(())
    }
    pub async fn when_build(&self) -> anyhow::Result<()> {
        wallet_api::testkit::collect::build_collect_candidate_for_test(self.ctx, &self.trade)
            .await?;
        Ok(())
    }
    pub async fn then_no_signed_reservation(&self) {
        let nonce = ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await.unwrap();
        assert!(nonce < 5, "failed or cancelled build consumed nonce {nonce}");
        assert!(
            ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade)
                .await
                .unwrap()
                .raw_tx
                .is_none()
        );
    }
    pub async fn then_signed_once(&self) {
        let row =
            ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade).await.unwrap();
        assert!(row.raw_tx.is_some());
        assert_eq!(row.nonce, 5);
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await.unwrap(), 5);
    }
    pub async fn when_cancel_inflight(&self) {
        self.adapter.block_build(true);
        let build = self.when_build();
        tokio::pin!(build);
        tokio::select! {r=&mut build=>panic!("expected blocked build: {r:?}"),_=self.adapter.wait_for_first_entry()=>{}}
    }
    pub async fn given_stale_build_slot(&self) {
        sqlx::query(
            "UPDATE api_collect SET building_at=datetime('now','-1 hour') WHERE trade_no=?",
        )
        .bind(&self.trade)
        .execute(self.pool.write_ref())
        .await
        .unwrap();
        self.adapter.block_build(false);
    }
    pub async fn when_build_loses_reservation_race(&self) {
        self.adapter.block_build(true);
        let build = self.when_build();
        tokio::pin!(build);
        tokio::select! {r=&mut build=>panic!("expected blocked build: {r:?}"),_=self.adapter.wait_for_first_entry()=>{}}
        self.given_competing_reservation().await;
        self.adapter.release_first();
        assert!(build.await.is_err());
        self.then_no_raw().await;
    }
    pub async fn then_no_raw(&self) {
        let row =
            ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, &self.trade).await.unwrap();
        assert!(row.raw_tx.is_none());
        assert!(row.tx_hash.is_none());
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await.unwrap(), 5);
    }
    pub async fn given_competing_reservation(&self) {
        ApiNonceRepo::set_nonce_floor(&self.pool, &self.from, "scroll", 5).await.unwrap();
    }
}

impl CollectBuildScenario {
    pub async fn then_ordinary_seven_is_free_until_signed_commit(&self) -> anyhow::Result<()> {
        use wallet_database::{
            entities::{api_withdraw::SignedSelfTransfer, asset_token_key::AssetTokenKey},
            repositories::api_wallet::withdraw::ApiWithdrawRepo,
        };
        let observed =
            wallet_api::testkit::ordinary_business::observe_nonce(self.ctx, &self.from, "scroll")
                .await?;
        assert_eq!(observed, 7);
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await?, 6);
        let trade = format!("ordinary-{}", self.trade);
        // Synthetic persisted signed facts only: no signer or broadcaster runs.
        ApiWithdrawRepo::reserve_signed_self_transfer(
            &self.pool,
            &SignedSelfTransfer {
                trade_no: &trade,
                uid: "uid",
                name: "ordinary",
                from_addr: &self.from,
                to_addr: super::request::BNB_TO_ADDR,
                chain_code: "scroll",
                token_addr: AssetTokenKey::Native,
                symbol: "ETH",
                value: "1",
                nonce: 7,
                tx_hash: "synthetic-ordinary-seven",
                raw_tx: "synthetic-seven-raw",
                transaction_fee: "0.1",
                notes: None,
            },
        )
        .await?;
        assert_eq!(ApiNonceRepo::get_api_nonce(&self.pool, &self.from, "scroll").await?, 7);
        assert!(
            wallet_api::testkit::ordinary_business::observe_nonce(self.ctx, &self.from, "scroll")
                .await
                .is_err()
        );
        assert!(self.adapter.recorded_nonces().is_empty());
        Ok(())
    }
}
