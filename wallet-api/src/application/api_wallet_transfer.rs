use crate::{
    context::Context,
    domain::{
        api_wallet::{coin::ApiCoinDomain, trans::ApiTransDomain},
        chain::TransferResp,
        wallet::WalletDomain,
    },
    error::{business::api_wallet::ApiWalletError, service::ServiceError},
    infrastructure::nonce::nonce_engine::get_nonce_engine_with_ctx,
    request::api_wallet::{
        trans::{ApiBaseTransferReq, ApiTransferReq},
        transfer::ApiTransferExReq,
    },
    response_vo::standard_wallet::transaction::TransactionResult,
};
use wallet_chain_interact::types::ChainPrivateKey;
use wallet_database::{
    entities::{
        api_trade_type::ApiTradeType,
        api_withdraw::{ApiWithdrawStatus, SelfTransferReceipt, SignedSelfTransfer},
        asset_token_key::AssetTokenKey,
    },
    repositories::api_wallet::{
        account::ApiAccountRepo, nonce::ApiNonceRepo, wallet::ApiWalletRepo,
        withdraw::ApiWithdrawRepo,
    },
};
use wallet_types::chain::chain::ChainCode;

pub(crate) struct ApiTransferApplication {
    ctx: &'static Context,
}

impl ApiTransferApplication {
    pub(crate) fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    /// Observe persisted ordinary transfers on startup and every 30 seconds.
    /// The owning actor cancels this loop on shutdown, including an in-flight RPC.
    pub(crate) async fn run_self_transfer_observer(
        &self,
        shutdown: tokio::sync::broadcast::Receiver<()>,
        period: std::time::Duration,
    ) {
        let receipt_shutdown = shutdown.resubscribe();
        let receipts = crate::infrastructure::runtime::time::receipt_observer::run_receipt_observer(
            receipt_shutdown,
            period,
            "SelfTransferObserver",
            (),
            |()| async {
                if let Err(error) = self.observe_self_transfer_receipts().await {
                    tracing::warn!(%error, "ordinary transfer observation round failed");
                }
            },
        );
        // Failed balance RPCs must not postpone observing newly mined receipts.
        let refreshes =
            crate::infrastructure::runtime::time::receipt_observer::run_receipt_observer(
                shutdown,
                period,
                "SelfTransferReceiptRefresh",
                (),
                |()| async {
                    if let Err(error) = self.retry_self_transfer_receipt_refreshes().await {
                        tracing::warn!(%error, "receipt asset refresh round failed");
                    }
                },
            );
        tokio::join!(receipts, refreshes);
    }

    pub(crate) async fn observe_pending_self_transfers(&self) -> Result<(), ServiceError> {
        self.observe_self_transfer_receipts().await?;
        self.retry_self_transfer_receipt_refreshes().await
    }

    async fn retry_self_transfer_receipt_refreshes(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        let mut refresh_cursor = 0;
        loop {
            let rows =
                ApiWithdrawRepo::scan_self_transfer_receipt_refresh(&pool, refresh_cursor, 32)
                    .await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                refresh_cursor = row.id;
                let outcome = crate::infrastructure::runtime::time::with_timeout(
                    std::time::Duration::from_secs(35),
                    self.retry_self_transfer_receipt_refresh(&row),
                )
                .await;
                match outcome {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(trade_no = %row.trade_no, %error, "receipt asset publication remains pending")
                    }
                    Err(_) => {
                        tracing::warn!(trade_no = %row.trade_no, "receipt asset publication timed out; retry remains pending")
                    }
                }
            }
        }
        Ok(())
    }

    async fn observe_self_transfer_receipts(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        let mut cursor = 0;
        loop {
            let rows = ApiWithdrawRepo::scan_pending_self_transfers(&pool, cursor, 32).await?;
            if rows.is_empty() {
                return Ok(());
            }
            for row in rows {
                cursor = row.id;
                // A slow provider must not stall all later transactions forever.
                let outcome = crate::infrastructure::runtime::time::with_timeout(
                    std::time::Duration::from_secs(35),
                    self.observe_self_transfer(&row),
                )
                .await;
                match outcome {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(trade_no = %row.trade_no, %error, "ordinary transfer receipt remains unresolved")
                    }
                    Err(_) => {
                        tracing::warn!(trade_no = %row.trade_no, "ordinary transfer receipt observation timed out")
                    }
                }
            }
        }
    }

    async fn observe_self_transfer(
        &self,
        row: &wallet_database::entities::api_withdraw::ApiWithdrawEntity,
    ) -> Result<(), ServiceError> {
        let engine = get_nonce_engine_with_ctx(self.ctx)?;
        let _gate = engine.acquire_transfer_gate(&row.from_addr, &row.chain_code).await;
        let pool = self.ctx.api_transaction_pool()?;
        let row = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            &row.trade_no,
            ApiTradeType::SelfWithdraw,
        )
        .await?;
        if row.finished_at.is_some()
            || row.transaction_time.is_some()
            || row.chain_success_at.is_some()
            || row.chain_failed_at.is_some()
            || row.err_code.is_some()
        {
            return Ok(());
        }
        let Some(hash) = row.tx_hash.as_deref().filter(|hash| !hash.is_empty()) else {
            return Ok(());
        };
        let adapter = crate::domain::api_wallet::adapter_factory::ApiChainAdapterFactory::get_transaction_adapter_with_ctx(
            self.ctx, &row.chain_code,
        ).await?;
        let Some(receipt) = adapter.query_tx_res(hash).await? else {
            return self.rebroadcast_saved_self_transfer(&row, adapter.as_ref()).await;
        };
        // Only an actual mined receipt is a terminal fact. Missing/pending/error
        // observations never become failure, and nonce progress proves nothing here.
        if !receipt.hash.eq_ignore_ascii_case(hash)
            || !matches!(receipt.status, 2 | 3)
            || receipt.block_height == 0
            || receipt.transaction_time == 0
            || !receipt.transaction_fee.is_finite()
            || receipt.transaction_fee < 0.0
        {
            return Err(ServiceError::Parameter(
                "invalid ordinary transfer receipt observation".into(),
            ));
        }
        let timestamp = i64::try_from(receipt.transaction_time)
            .ok()
            .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
            .ok_or_else(|| ServiceError::Parameter("invalid receipt timestamp".into()))?;
        let block_height = receipt.block_height.to_string();
        let fee = receipt.transaction_fee.to_string();
        let pool = self.ctx.api_transaction_pool()?;
        let changed = ApiWithdrawRepo::confirm_self_transfer_receipt(
            &pool,
            &SelfTransferReceipt {
                trade_no: &row.trade_no,
                chain_code: &row.chain_code,
                tx_hash: hash,
                success: receipt.status == 2,
                transaction_time: timestamp,
                block_height: &block_height,
                transaction_fee: &fee,
                resource_consume: &receipt.resource_consume,
            },
        )
        .await?;
        if changed {
            tracing::info!(trade_no = %row.trade_no, tx_hash = %hash, success = (receipt.status == 2), "ordinary transfer receipt persisted");
        }
        Ok(())
    }

    async fn retry_self_transfer_receipt_refresh(
        &self,
        row: &wallet_database::entities::api_withdraw::ApiWithdrawEntity,
    ) -> Result<(), ServiceError> {
        // The single production refresh loop is serial. Balance RPCs do not
        // mutate nonce/signing facts and must not hold the sender transfer gate.
        if ApiWithdrawRepo::self_transfer_receipt_refresh_pending(
            &self.ctx.api_transaction_pool()?,
            &row.trade_no,
        )
        .await?
        {
            self.refresh_self_transfer_receipt(row).await?;
        }
        Ok(())
    }

    async fn refresh_self_transfer_receipt(
        &self,
        row: &wallet_database::entities::api_withdraw::ApiWithdrawEntity,
    ) -> Result<(), ServiceError> {
        use crate::messaging::mqtt::topics::api_wallet::acct_change::ApiWalletAcctChange;
        use wallet_database::entities::assets::AssetsId;
        let mut ids = ApiWalletAcctChange::prepare_receipt_assets(
            self.ctx,
            &row.from_addr,
            &row.to_addr,
            &row.chain_code,
            row.token_addr.clone().into_chain_token_option(),
        )
        .await?;
        let mut native = ApiWalletAcctChange::prepare_receipt_assets(
            self.ctx,
            &row.from_addr,
            &row.from_addr,
            &row.chain_code,
            None,
        )
        .await?;
        ids.append(&mut native);
        let sender = AssetsId::new(&row.from_addr, &row.chain_code, row.token_addr.clone());
        let fee = AssetsId::new(&row.from_addr, &row.chain_code, AssetTokenKey::Native);
        if !ids.contains(&sender) || !ids.contains(&fee) {
            return Err(ServiceError::Parameter("receipt sender assets missing".into()));
        }
        let mut unique = Vec::new();
        for id in ids {
            if !unique.contains(&id) {
                unique.push(id);
            }
        }
        crate::domain::api_wallet::assets::ApiAssetsDomain::sync_receipt_assets_with_ctx(
            self.ctx,
            unique,
            &row.chain_code,
        )
        .await?;
        ApiWithdrawRepo::acknowledge_self_transfer_receipt_refresh(
            &self.ctx.api_transaction_pool()?,
            &row.trade_no,
            row.tx_hash.as_deref().unwrap_or_default(),
        )
        .await?;
        tracing::info!(trade_no = %row.trade_no, "receipt asset publication completed");
        Ok(())
    }

    /// Called while holding the sender gate. Re-send only the immutable signed
    /// bytes after independent node visibility and nonce checks, never re-sign.
    async fn rebroadcast_saved_self_transfer(
        &self,
        row: &wallet_database::entities::api_withdraw::ApiWithdrawEntity,
        adapter: &(dyn crate::domain::api_wallet::Tx + Send + Sync),
    ) -> Result<(), ServiceError> {
        let last_attempt = row
            .broadcast_uncertain_last_checked_at
            .or(row.broadcast_uncertain_since_at)
            .or(row.last_broadcast_at)
            .unwrap_or(row.created_at);
        if row.broadcast_uncertain_rebroadcast_count >= 3
            || chrono::Utc::now().signed_duration_since(last_attempt).num_seconds() < 60
            || get_nonce_engine_with_ctx(self.ctx)?.is_frozen(&row.from_addr, &row.chain_code)
        {
            return Ok(());
        }
        let (Some(hash), Some(raw_json)) = (row.tx_hash.as_deref(), row.raw_tx.as_deref()) else {
            return Ok(());
        };
        let raw: crate::domain::api_wallet::RawTx =
            wallet_utils::serde_func::serde_from_str(raw_json)?;
        let crate::domain::api_wallet::RawTx::Evm(bytes, _) = &raw else {
            return Err(ServiceError::Parameter("ordinary rebroadcast requires EVM raw".into()));
        };
        let raw_hash = format!("{:#x}", alloy::primitives::keccak256(bytes));
        if !hash.eq_ignore_ascii_case(&raw_hash) {
            return Err(ServiceError::Parameter("ordinary rebroadcast raw hash mismatch".into()));
        }
        if adapter.query_tx_seen_on_node(hash).await? {
            return Ok(());
        }
        let next = adapter.nonce(&row.from_addr).await?;
        if i64::try_from(next).ok() != Some(row.nonce) {
            return Ok(());
        }
        let pool = self.ctx.api_transaction_pool()?;
        if !ApiWithdrawRepo::claim_self_transfer_rebroadcast(&pool, &row.trade_no, hash, raw_json)
            .await?
        {
            return Ok(());
        }
        // The durable claim remains consumed even if shutdown interrupts the RPC.
        let response =
            ApiTransDomain::broadcast_transfer(self.ctx, &row.chain_code, raw, Some(hash)).await?;
        if let Some(response) = response {
            if !response.tx_hash.eq_ignore_ascii_case(hash) {
                return Err(ServiceError::Parameter(
                    "ordinary rebroadcast response hash mismatch".into(),
                ));
            }
            ApiWithdrawRepo::mark_self_transfer_accepted(&pool, &row.trade_no, hash).await?;
        }
        tracing::info!(trade_no = %row.trade_no, tx_hash = %hash, "ordinary signed transaction rebroadcast attempted");
        Ok(())
    }

    pub(crate) async fn transfer(
        &self,
        params: ApiTransferExReq,
    ) -> Result<TransactionResult, ServiceError> {
        WalletDomain::validate_password_with_context(self.ctx, &params.password).await?;

        let private_key =
            crate::domain::api_wallet::account::ApiAccountDomain::get_private_key_with_ctx(
                self.ctx,
                &params.base.from,
                &params.base.chain_code,
            )
            .await?;
        self.transfer_with_private_key(params, private_key).await
    }

    pub(crate) async fn transfer_with_private_key(
        &self,
        params: ApiTransferExReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransactionResult, ServiceError> {
        let from_addr = params.base.from.clone();
        let _gate = get_nonce_engine_with_ctx(self.ctx)?
            .acquire_transfer_gate(&from_addr, &params.base.chain_code)
            .await;

        let pool = self.ctx.api_wallet_pool()?;
        let account = ApiAccountRepo::find_one_by_address_chain_code(
            &params.base.from,
            &params.base.chain_code,
            &pool,
        )
        .await?
        .ok_or(ServiceError::Business(ApiWalletError::NotFoundAccount.into()))?;
        let wallet = ApiWalletRepo::find_by_address(&pool, &account.wallet_address).await?.ok_or(
            ServiceError::Business(
                ApiWalletError::Wallet(
                    crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
                )
                .into(),
            ),
        )?;

        let token_key = params.base.token_address.clone();
        let coin = ApiCoinDomain::get_coin_by_token_key_exact_with_ctx(
            self.ctx,
            &params.base.chain_code,
            token_key.clone(),
        )
        .await?;

        let chain_code: ChainCode = params.base.chain_code.as_str().try_into()?;
        let nonce =
            self.get_transfer_nonce(&params.base.from, &params.base.chain_code, chain_code).await?;

        let req = ApiTransferReq {
            base: Self::build_api_transfer_base(&params, &token_key, coin.decimals),
            password: params.password.to_string(),
            nonce: nonce as u64,
        };
        if crate::domain::chain::is_evm_chain(chain_code) {
            let (tx_hash, raw, fee) =
                ApiTransDomain::build_transfer_raw(self.ctx, req, Some(private_key)).await?;
            let raw_json = wallet_utils::serde_func::serde_to_string(&raw)?;
            let trade_no = uuid::Uuid::new_v4().to_string();
            let pool = self.ctx.api_transaction_pool()?;
            // Persist all signed facts before the first send; a timeout or crash
            // must leave the same bytes available for later reconciliation.
            ApiWithdrawRepo::reserve_signed_self_transfer(
                &pool,
                &SignedSelfTransfer {
                    trade_no: &trade_no,
                    uid: &wallet.uid,
                    name: &wallet.name,
                    from_addr: &params.base.from,
                    to_addr: &params.base.to,
                    chain_code: &params.base.chain_code,
                    token_addr: token_key,
                    symbol: &params.base.symbol,
                    value: &params.base.value,
                    nonce,
                    tx_hash: &tx_hash,
                    raw_tx: &raw_json,
                    transaction_fee: &fee,
                    notes: params.base.notes.as_deref(),
                },
            )
            .await?;
            let response = ApiTransDomain::broadcast_signed_self_transfer_with_ctx(
                self.ctx,
                &params.base.chain_code,
                raw,
                &tx_hash,
            )
            .await?;
            let Some(response) = response else {
                return Err(ServiceError::Parameter(format!(
                    "transfer_broadcast_uncertain: tx_hash={tx_hash}"
                )));
            };
            if !response.tx_hash.eq_ignore_ascii_case(&tx_hash) {
                return Err(ServiceError::Parameter(format!(
                    "transfer_broadcast_hash_mismatch: expected={tx_hash}"
                )));
            }
            if !ApiWithdrawRepo::mark_self_transfer_accepted(&pool, &trade_no, &tx_hash).await? {
                return Err(ServiceError::Parameter(format!(
                    "transfer_acceptance_persistence_conflict: tx_hash={tx_hash}"
                )));
            }
            if let Some(request_id) = &params.base.request_resource_id {
                let _ = self.ctx.get_global_backend_api().delegate_complete(request_id).await;
            }
            return Ok(TransactionResult { tx_hash });
        }

        let res = ApiTransDomain::transfer_with_ctx(self.ctx, req, Some(private_key)).await?;
        self.record_transfer(params, token_key, nonce, &wallet.uid, &wallet.name, &res).await?;
        Ok(TransactionResult { tx_hash: res.tx_hash })
    }

    pub(crate) fn build_api_transfer_base(
        params: &ApiTransferExReq,
        token_key: &AssetTokenKey,
        decimals: u8,
    ) -> ApiBaseTransferReq {
        ApiBaseTransferReq {
            from: params.base.from.clone(),
            to: params.base.to.clone(),
            value: params.base.value.clone(),
            chain_code: params.base.chain_code.clone(),
            token_address: token_key.clone(),
            decimals,
            symbol: params.base.symbol.clone(),
            request_resource_id: params.base.request_resource_id.clone(),
            spend_all: params.base.spend_all,
            notes: params.base.notes.clone(),
            metadata: Some(params.fee_setting.clone()),
        }
    }

    async fn get_evm_nonce(&self, from_addr: &str, chain_code: &str) -> Result<i64, ServiceError> {
        let nonce_engine = get_nonce_engine_with_ctx(self.ctx)?;
        let nonce = nonce_engine.observe_ordinary_transfer_nonce(from_addr, chain_code).await?;
        Ok(nonce as i64)
    }

    async fn get_transfer_nonce(
        &self,
        from_addr: &str,
        chain_code: &str,
        chain_code_enum: ChainCode,
    ) -> Result<i64, ServiceError> {
        if crate::domain::chain::is_evm_chain(chain_code_enum) {
            self.get_evm_nonce(from_addr, chain_code).await
        } else {
            Ok(0)
        }
    }

    async fn record_transfer(
        &self,
        params: ApiTransferExReq,
        token_key: AssetTokenKey,
        nonce: i64,
        wallet_uid: &str,
        wallet_name: &str,
        res: &TransferResp,
    ) -> Result<(), ServiceError> {
        let api_transaction_pool = self.ctx.api_transaction_pool()?;
        let resource_consume = res.resource_consume().unwrap_or_else(|_| "".to_string());
        let trade_no = uuid::Uuid::new_v4().to_string();
        ApiWithdrawRepo::upsert_api_withdraw(
            &api_transaction_pool,
            wallet_uid,
            wallet_name,
            &params.base.from,
            &params.base.to,
            &params.base.value,
            "",
            &params.base.chain_code,
            token_key,
            &params.base.symbol,
            &trade_no,
            None,
            None,
            None,
            ApiTradeType::SelfWithdraw,
            nonce,
            Some(res.tx_hash.clone()),
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            resource_consume.as_str(),
            res.fee.as_str(),
            None,
            None,
        )
        .await?;
        ApiNonceRepo::set_nonce_floor(
            &api_transaction_pool,
            &params.base.from,
            &params.base.chain_code,
            nonce,
        )
        .await?;
        Ok(())
    }
}
