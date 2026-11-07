use crate::{
    context::Context,
    domain::{bill::BillDomain, chain::adapter::ChainAdapterFactory},
    error::{business::api_wallet::ApiWalletError, service::ServiceError},
    infrastructure::inner_event::{InnerEvent, SyncAssetsData, SyncPriority},
    messaging::{
        mqtt::topics::AcctChange,
        notify::{FrontendNotifyEvent, event::NotifyEvent, transaction::AcctChangeFrontend},
    },
};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use std::str::FromStr;
use wallet_database::{
    entities::{
        api_assets::ApiCreateAssetsVo,
        api_coin::ApiCoinData,
        api_collect::ApiCollectEntity,
        api_fee::ApiFeeEntity,
        api_trade_type::ApiTradeType,
        api_wallet::ApiWalletType,
        api_withdraw::{ApiWithdrawEntity, ApiWithdrawStatus},
        asset_token_key::AssetTokenKey,
        assets::AssetsId,
    },
    repositories::api_wallet::{
        account::ApiAccountRepo, api_asset_sync_state::ApiAssetSyncStateRepo,
        assets::ApiAssetsRepo, coin::ApiCoinRepo, collect::ApiCollectRepo, fee::ApiFeeRepo,
        wallet::ApiWalletRepo, withdraw::ApiWithdrawRepo,
    },
};

// biz_type = ACCT_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiWalletAcctChange(AcctChange);

impl From<&ApiWalletAcctChange> for AcctChangeFrontend {
    fn from(value: &ApiWalletAcctChange) -> Self {
        Self {
            tx_hash: value.0.tx_hash.clone(),
            chain_code: value.0.chain_code.clone(),
            symbol: value.0.symbol.clone(),
            transfer_type: value.0.transfer_type,
            tx_kind: value.0.tx_kind,
            from_addr: value.0.from_addr.clone(),
            to_addr: value.0.to_addr.clone(),
            token: value.0.token.clone().into(),
            value: value.0.value,
            transaction_fee: value.0.transaction_fee,
            transaction_time: value.0.transaction_time.clone(),
            status: value.0.status,
            is_multisig: value.0.is_multisig,
            queue_id: value.0.queue_id.clone(),
            block_height: value.0.block_height,
            notes: value.0.notes.clone(),
        }
    }
}

impl ApiWalletAcctChange {
    const COLLECT_REPAIR_CANDIDATE_LIMIT: i64 = 10;
    const COLLECT_REPAIR_TIME_WINDOW_HOURS: i64 = 24;

    pub(crate) async fn exec(
        &self,
        ctx: &'static Context,
        _msg_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        // let event_name = self.name();
        tracing::debug!("处理帐变: {:?}", self);
        let pool = ctx.api_wallet_pool()?;

        if let Some(token_str) = &self.0.token {
            let has_coin = ApiCoinRepo::has_coin(
                &self.0.chain_code,
                AssetTokenKey::from_raw(Some(token_str)),
                &pool,
            )
            .await?;
            if !has_coin {
                if let Err(e) = Self::try_create_coin_for_address(
                    ctx,
                    &self.0.chain_code,
                    token_str,
                    &self.0.symbol,
                )
                .await
                {
                    tracing::error!("3deposit_acct_change 自动创建代币失败: to_addr {}", e);
                }
            }
        }

        if let Err(e) = self.try_repair_collect_from_acct_change(ctx).await {
            tracing::warn!(error = %e, "acct_change collect runtime repair failed (best-effort)");
        }

        // 充值帐变消息
        self.deposit_acct_change(ctx).await?;

        // 自己转账帐变
        self.self_transfer_acct_change(ctx).await?;

        if let Err(e) = self.try_repair_fee_from_acct_change(ctx).await {
            tracing::warn!(error = %e, "acct_change fee runtime repair failed (best-effort)");
        }
        if let Err(e) = self.try_repair_withdraw_from_acct_change(ctx).await {
            tracing::warn!(error = %e, "acct_change withdraw runtime repair failed (best-effort)");
        }

        // The receipt observer owns publication for locally signed EVM transfers,
        // including fee-only messages carrying the same transaction hash.
        let defer_assets = ApiWithdrawRepo::should_defer_self_transfer_asset_refresh(
            &ctx.api_transaction_pool()?,
            &self.0.from_addr,
            &self.0.tx_hash,
            &self.0.chain_code,
        )
        .await?;
        if defer_assets {
            Self::prepare_asset_addresses(ctx, self).await?;
            tracing::info!(tx_hash = %self.0.tx_hash, "self transfer asset publication reserved for receipt observer");
        }

        // 更新资产,不进行新增(垃圾币)
        tracing::info!(
            "账变进入资产同步阶段: tx_hash={}, chain_code={}, symbol={}, from_addr={}, to_addr={}, token={:?}",
            self.0.tx_hash,
            self.0.chain_code,
            self.0.symbol,
            self.0.from_addr,
            self.0.to_addr,
            self.0.token
        );
        if !defer_assets {
            Self::sync_assets(ctx, &self).await?;
        }
        tracing::info!(
            "账变资产同步阶段完成: tx_hash={}, chain_code={}, symbol={}, from_addr={}, to_addr={}, token={:?}",
            self.0.tx_hash,
            self.0.chain_code,
            self.0.symbol,
            self.0.from_addr,
            self.0.to_addr,
            self.0.token
        );

        // send acct_change to frontend
        let change_frontend = AcctChangeFrontend::from(self);
        let data = NotifyEvent::ApiWalletAcctChange(change_frontend);
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
        Ok(())
    }

    /// Best-effort runtime repair for collect records whose `tx_hash` is missing.
    ///
    /// Design constraints:
    /// - Driven by external acct_change facts (do not rely on local collect.tx_hash)
    /// - Do not write `transaction_time` here; another MQTT path owns onchain-confirm facts
    /// - Only repair when a unique candidate can be identified
    async fn try_repair_collect_from_acct_change(
        &self,
        ctx: &'static Context,
    ) -> Result<(), ServiceError> {
        // Normalize chain-specific hash formats (e.g. TON) before matching/writing.
        let normalized_hash = BillDomain::handle_hash(&self.0.tx_hash).trim().to_string();
        // Treat empty token as native coin to align with collect.token_addr NULL / empty storage.
        let normalized_token =
            self.0.token.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

        tracing::debug!(
            source = "api_wallet_acct_change_collect_repair",
            chain_code = %self.0.chain_code,
            transfer_type = %self.0.transfer_type,
            tx_kind = %self.0.tx_kind,
            status = %self.0.status,
            from_addr = %self.0.from_addr,
            to_addr = %self.0.to_addr,
            symbol = %self.0.symbol,
            token = ?normalized_token,
            acct_change_tx_hash_raw = %self.0.tx_hash,
            acct_change_tx_hash_normalized = %normalized_hash,
            "Evaluating acct_change for collect runtime repair"
        );

        if !self.0.status {
            tracing::info!(
                source = "api_wallet_acct_change_collect_repair",
                skip_reason = "status_false",
                acct_change_tx_hash_raw = %self.0.tx_hash,
                "Skip collect runtime repair"
            );
            return Ok(());
        }
        if self.0.transfer_type != 1 {
            tracing::debug!(
                source = "api_wallet_acct_change_collect_repair",
                skip_reason = "transfer_type_not_outgoing",
                transfer_type = %self.0.transfer_type,
                "Skip collect runtime repair"
            );
            return Ok(());
        }
        if self.0.tx_kind != 1 {
            tracing::debug!(
                source = "api_wallet_acct_change_collect_repair",
                skip_reason = "tx_kind_not_normal",
                tx_kind = %self.0.tx_kind,
                "Skip collect runtime repair"
            );
            return Ok(());
        }
        if normalized_hash.is_empty() {
            tracing::info!(
                source = "api_wallet_acct_change_collect_repair",
                skip_reason = "empty_normalized_hash",
                acct_change_tx_hash_raw = %self.0.tx_hash,
                "Skip collect runtime repair"
            );
            return Ok(());
        }
        if self.0.from_addr.trim().is_empty() || self.0.to_addr.trim().is_empty() {
            tracing::info!(
                source = "api_wallet_acct_change_collect_repair",
                skip_reason = "empty_from_or_to",
                from_addr = %self.0.from_addr,
                to_addr = %self.0.to_addr,
                "Skip collect runtime repair"
            );
            return Ok(());
        }

        // We still parse acct_change time for candidate narrowing and logging, but do not
        // persist onchain confirmation facts in this path.
        let acct_change_time = match self.convert_transaction_time(self.0.transaction_time.as_str())
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    source = "api_wallet_acct_change_collect_repair",
                    skip_reason = "transaction_time_parse_failed",
                    transaction_time = %self.0.transaction_time,
                    error = %e,
                    "Skip collect runtime repair"
                );
                return Ok(());
            }
        };

        let acct_change_value_str = self.0.value.to_string();
        let acct_change_value = match Decimal::from_str(&acct_change_value_str) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    source = "api_wallet_acct_change_collect_repair",
                    skip_reason = "invalid_acct_change_value",
                    value = %self.0.value,
                    value_str = %acct_change_value_str,
                    error = %e,
                    "Skip collect runtime repair"
                );
                return Ok(());
            }
        };

        let wallet_pool = ctx.api_wallet_pool()?;
        let api_transaction_pool = ctx.api_transaction_pool()?;

        // Restrict to subaccount-originated transfers; destination is intentionally NOT
        // constrained to a local withdrawal wallet because collect targets can vary.
        let from_account = ApiAccountRepo::find_one_by_address_chain_code(
            &self.0.from_addr,
            &self.0.chain_code,
            &wallet_pool,
        )
        .await?;
        let Some(from_account) = from_account else {
            tracing::debug!(
                source = "api_wallet_acct_change_collect_repair",
                skip_reason = "from_account_not_found",
                from_addr = %self.0.from_addr,
                chain_code = %self.0.chain_code,
                "Skip collect runtime repair"
            );
            return Ok(());
        };
        if from_account.api_wallet_type != ApiWalletType::SubAccount {
            tracing::debug!(
                source = "api_wallet_acct_change_collect_repair",
                skip_reason = "from_account_not_subaccount",
                from_addr = %self.0.from_addr,
                api_wallet_type = ?from_account.api_wallet_type,
                "Skip collect runtime repair"
            );
            return Ok(());
        }

        // Broad SQL candidate query first; exactness is enforced by Rust-side amount/time/unique checks.
        let candidates = ApiCollectRepo::find_candidates_for_acct_change_repair(
            &api_transaction_pool,
            &self.0.chain_code,
            &self.0.from_addr,
            &self.0.to_addr,
            normalized_token.as_deref(),
            &self.0.symbol,
            Self::COLLECT_REPAIR_CANDIDATE_LIMIT,
        )
        .await?;

        let candidate_trade_nos: Vec<String> =
            candidates.iter().map(|c| c.trade_no.clone()).collect();

        let mut amount_match_count = 0usize;
        let mut time_window_match_count = 0usize;
        let mut matched = Vec::new();
        let mut execution_evidence_match_count = 0usize;

        for candidate in candidates {
            let Ok(candidate_value) = Decimal::from_str(candidate.value.trim()) else {
                tracing::debug!(
                    source = "api_wallet_acct_change_collect_repair",
                    trade_no = %candidate.trade_no,
                    collect_value = %candidate.value,
                    "Skip candidate due to invalid collect value format"
                );
                continue;
            };

            if candidate_value != acct_change_value {
                continue;
            }
            amount_match_count += 1;

            if !Self::collect_time_window_match(&candidate, acct_change_time) {
                continue;
            }
            time_window_match_count += 1;
            // MQTT can be out-of-order: acct_change may arrive before transaction_time is written.
            // For hash-only repair we only require local execution evidence, not chain time.
            let has_execution_evidence =
                candidate.transaction_time.is_some() || candidate.last_broadcast_at.is_some();
            if !has_execution_evidence {
                tracing::debug!(
                    source = "api_wallet_acct_change_collect_repair",
                    trade_no = %candidate.trade_no,
                    skip_reason = "candidate_missing_execution_evidence_defer",
                    "Skip acct_change collect repair candidate"
                );
                continue;
            }
            execution_evidence_match_count += 1;
            matched.push(candidate);
        }

        tracing::info!(
            source = "api_wallet_acct_change_collect_repair",
            chain_code = %self.0.chain_code,
            acct_change_tx_hash_raw = %self.0.tx_hash,
            acct_change_tx_hash_normalized = %normalized_hash,
            candidate_count = %candidate_trade_nos.len(),
            candidate_trade_nos = ?candidate_trade_nos,
            amount_match_count = %amount_match_count,
            time_window_match_count = %time_window_match_count,
            execution_evidence_match_count = %execution_evidence_match_count,
            "Collect acct_change repair candidate evaluation finished"
        );

        // Safety rule: only repair on a unique candidate. Ambiguous matches are logged and skipped.
        let candidate = match matched.len() {
            0 => {
                tracing::info!(
                    source = "api_wallet_acct_change_collect_repair",
                    skip_reason = "no_candidate",
                    acct_change_tx_hash_normalized = %normalized_hash,
                    "Skip collect runtime repair: no matched candidate"
                );
                return Ok(());
            }
            1 => matched.pop().unwrap(),
            _ => {
                let trade_nos: Vec<String> = matched.iter().map(|c| c.trade_no.clone()).collect();
                tracing::warn!(
                    source = "api_wallet_acct_change_collect_repair",
                    skip_reason = "ambiguous_candidates",
                    candidate_trade_nos = ?trade_nos,
                    acct_change_tx_hash_normalized = %normalized_hash,
                    "Skip collect runtime repair: ambiguous candidates"
                );
                return Ok(());
            }
        };

        // If local hash already exists but conflicts with acct_change, do not overwrite facts.
        if let Some(existing_hash) =
            candidate.tx_hash.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            if existing_hash != normalized_hash {
                tracing::error!(
                    source = "api_wallet_acct_change_collect_repair",
                    trade_no = %candidate.trade_no,
                    existing_tx_hash = %existing_hash,
                    acct_change_tx_hash = %normalized_hash,
                    "Collect acct_change repair tx_hash conflict"
                );
                return Ok(());
            }
        }

        let acct_change_time_rfc3339 = acct_change_time.to_rfc3339();

        let tx_hash_missing =
            candidate.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
        let (repair_mode, rows_affected) = if tx_hash_missing {
            // Safe backfill only: DAO refuses overwrite and requires execution evidence.
            let rows = ApiCollectRepo::backfill_tx_hash_if_missing(
                &api_transaction_pool,
                &candidate.trade_no,
                &normalized_hash,
                "acct_change_runtime_repair",
            )
            .await?;
            ("acct_change_backfill_tx_hash", rows)
        } else {
            tracing::debug!(
                source = "api_wallet_acct_change_collect_repair",
                trade_no = %candidate.trade_no,
                skip_reason = "candidate_no_repair_needed_after_filters",
                "Skip collect runtime repair"
            );
            return Ok(());
        };

        tracing::warn!(
            source = "api_wallet_acct_change_collect_repair",
            trade_no = %candidate.trade_no,
            repair_mode = %repair_mode,
            rows_affected = %rows_affected,
            chain_code = %self.0.chain_code,
            tx_hash = %normalized_hash,
            transaction_time = %acct_change_time_rfc3339,
            mqtt_out_of_order_tolerated = %(candidate.transaction_time.is_none()
                && candidate.last_broadcast_at.is_some()),
            "Collect runtime repair from acct_change attempted"
        );

        Ok(())
    }

    fn collect_time_window_match(
        candidate: &ApiCollectEntity,
        acct_change_time: DateTime<Utc>,
    ) -> bool {
        // Prefer transaction_time; fall back to last_broadcast_at. If both are absent, leave
        // the candidate in and rely on uniqueness + other fields to decide.
        let ref_time = candidate
            .transaction_time
            .as_ref()
            .cloned()
            .or_else(|| candidate.last_broadcast_at.as_ref().cloned());

        let Some(ref_time) = ref_time else {
            return true;
        };

        let diff = if acct_change_time >= ref_time {
            acct_change_time - ref_time
        } else {
            ref_time - acct_change_time
        };

        diff <= Duration::hours(Self::COLLECT_REPAIR_TIME_WINDOW_HOURS)
    }

    /// Best-effort runtime repair for fee records whose `tx_hash` is missing.
    ///
    /// Design constraints:
    /// - Driven by external acct_change facts (do not rely on local fee.tx_hash)
    /// - Hash-only repair (do NOT write transaction_time here)
    /// - Only repair when a unique candidate can be identified
    async fn try_repair_fee_from_acct_change(
        &self,
        ctx: &'static Context,
    ) -> Result<(), ServiceError> {
        let normalized_hash = BillDomain::handle_hash(&self.0.tx_hash).trim().to_string();
        let normalized_token =
            self.0.token.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

        tracing::debug!(
            source = "api_wallet_acct_change_fee_repair",
            chain_code = %self.0.chain_code,
            transfer_type = %self.0.transfer_type,
            tx_kind = %self.0.tx_kind,
            status = %self.0.status,
            from_addr = %self.0.from_addr,
            to_addr = %self.0.to_addr,
            symbol = %self.0.symbol,
            token = ?normalized_token,
            acct_change_tx_hash_raw = %self.0.tx_hash,
            acct_change_tx_hash_normalized = %normalized_hash,
            "Evaluating acct_change for fee runtime repair"
        );

        if !self.0.status {
            tracing::info!(
                source = "api_wallet_acct_change_fee_repair",
                skip_reason = "status_false",
                acct_change_tx_hash_raw = %self.0.tx_hash,
                "Skip fee runtime repair"
            );
            return Ok(());
        }
        if self.0.transfer_type != 1 {
            tracing::debug!(
                source = "api_wallet_acct_change_fee_repair",
                skip_reason = "transfer_type_not_outgoing",
                transfer_type = %self.0.transfer_type,
                "Skip fee runtime repair"
            );
            return Ok(());
        }
        if self.0.tx_kind != 1 {
            tracing::debug!(
                source = "api_wallet_acct_change_fee_repair",
                skip_reason = "tx_kind_not_normal",
                tx_kind = %self.0.tx_kind,
                "Skip fee runtime repair"
            );
            return Ok(());
        }
        if normalized_hash.is_empty() {
            tracing::info!(
                source = "api_wallet_acct_change_fee_repair",
                skip_reason = "empty_normalized_hash",
                acct_change_tx_hash_raw = %self.0.tx_hash,
                "Skip fee runtime repair"
            );
            return Ok(());
        }
        if self.0.from_addr.trim().is_empty() || self.0.to_addr.trim().is_empty() {
            tracing::info!(
                source = "api_wallet_acct_change_fee_repair",
                skip_reason = "empty_from_or_to",
                from_addr = %self.0.from_addr,
                to_addr = %self.0.to_addr,
                "Skip fee runtime repair"
            );
            return Ok(());
        }

        let acct_change_time = match self.convert_transaction_time(self.0.transaction_time.as_str())
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    source = "api_wallet_acct_change_fee_repair",
                    skip_reason = "transaction_time_parse_failed",
                    transaction_time = %self.0.transaction_time,
                    error = %e,
                    "Skip fee runtime repair"
                );
                return Ok(());
            }
        };

        let acct_change_value_str = self.0.value.to_string();
        let acct_change_value = match Decimal::from_str(&acct_change_value_str) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    source = "api_wallet_acct_change_fee_repair",
                    skip_reason = "invalid_acct_change_value",
                    value = %self.0.value,
                    value_str = %acct_change_value_str,
                    error = %e,
                    "Skip fee runtime repair"
                );
                return Ok(());
            }
        };

        let wallet_pool = ctx.api_wallet_pool()?;
        let api_transaction_pool = ctx.api_transaction_pool()?;

        let from_account = ApiAccountRepo::find_one_by_address_chain_code(
            &self.0.from_addr,
            &self.0.chain_code,
            &wallet_pool,
        )
        .await?;
        let Some(from_account) = from_account else {
            tracing::debug!(
                source = "api_wallet_acct_change_fee_repair",
                skip_reason = "from_account_not_found",
                from_addr = %self.0.from_addr,
                chain_code = %self.0.chain_code,
                "Skip fee runtime repair"
            );
            return Ok(());
        };
        if from_account.api_wallet_type != ApiWalletType::Withdrawal {
            tracing::debug!(
                source = "api_wallet_acct_change_fee_repair",
                skip_reason = "from_account_not_withdrawal",
                from_addr = %self.0.from_addr,
                api_wallet_type = ?from_account.api_wallet_type,
                "Skip fee runtime repair"
            );
            return Ok(());
        }

        let candidates = ApiFeeRepo::find_candidates_for_acct_change_hash_backfill(
            &api_transaction_pool,
            &self.0.chain_code,
            &self.0.from_addr,
            &self.0.to_addr,
            normalized_token.as_deref(),
            &self.0.symbol,
            Self::COLLECT_REPAIR_CANDIDATE_LIMIT,
        )
        .await?;

        let candidate_trade_nos: Vec<String> =
            candidates.iter().map(|c| c.trade_no.clone()).collect();

        let mut amount_match_count = 0usize;
        let mut time_window_match_count = 0usize;
        let mut execution_evidence_match_count = 0usize;
        let mut matched = Vec::new();

        for candidate in candidates {
            let Ok(candidate_value) = Decimal::from_str(candidate.value.trim()) else {
                tracing::debug!(
                    source = "api_wallet_acct_change_fee_repair",
                    trade_no = %candidate.trade_no,
                    fee_value = %candidate.value,
                    "Skip candidate due to invalid fee value format"
                );
                continue;
            };

            if candidate_value != acct_change_value {
                continue;
            }
            amount_match_count += 1;

            if !Self::fee_time_window_match(&candidate, acct_change_time) {
                continue;
            }
            time_window_match_count += 1;

            let has_execution_evidence =
                candidate.transaction_time.is_some() || candidate.last_broadcast_at.is_some();
            if !has_execution_evidence {
                tracing::debug!(
                    source = "api_wallet_acct_change_fee_repair",
                    trade_no = %candidate.trade_no,
                    skip_reason = "candidate_missing_execution_evidence_defer",
                    "Skip acct_change fee repair candidate"
                );
                continue;
            }
            execution_evidence_match_count += 1;
            matched.push(candidate);
        }

        tracing::info!(
            source = "api_wallet_acct_change_fee_repair",
            chain_code = %self.0.chain_code,
            acct_change_tx_hash_raw = %self.0.tx_hash,
            acct_change_tx_hash_normalized = %normalized_hash,
            candidate_count = %candidate_trade_nos.len(),
            candidate_trade_nos = ?candidate_trade_nos,
            amount_match_count = %amount_match_count,
            time_window_match_count = %time_window_match_count,
            execution_evidence_match_count = %execution_evidence_match_count,
            "Fee acct_change repair candidate evaluation finished"
        );

        let candidate = match matched.len() {
            0 => {
                tracing::info!(
                    source = "api_wallet_acct_change_fee_repair",
                    skip_reason = "no_candidate",
                    acct_change_tx_hash_normalized = %normalized_hash,
                    "Skip fee runtime repair: no matched candidate"
                );
                return Ok(());
            }
            1 => matched.pop().unwrap(),
            _ => {
                let trade_nos: Vec<String> = matched.iter().map(|c| c.trade_no.clone()).collect();
                tracing::warn!(
                    source = "api_wallet_acct_change_fee_repair",
                    skip_reason = "ambiguous_candidates",
                    candidate_trade_nos = ?trade_nos,
                    acct_change_tx_hash_normalized = %normalized_hash,
                    "Skip fee runtime repair: ambiguous candidates"
                );
                return Ok(());
            }
        };

        if let Some(existing_hash) =
            candidate.tx_hash.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            if existing_hash != normalized_hash {
                tracing::error!(
                    source = "api_wallet_acct_change_fee_repair",
                    trade_no = %candidate.trade_no,
                    existing_tx_hash = %existing_hash,
                    acct_change_tx_hash = %normalized_hash,
                    "Fee acct_change repair tx_hash conflict"
                );
                return Ok(());
            }
        }

        let acct_change_time_rfc3339 = acct_change_time.to_rfc3339();
        let tx_hash_missing =
            candidate.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
        if !tx_hash_missing {
            tracing::debug!(
                source = "api_wallet_acct_change_fee_repair",
                trade_no = %candidate.trade_no,
                skip_reason = "candidate_no_repair_needed_after_filters",
                "Skip fee runtime repair"
            );
            return Ok(());
        }

        let rows_affected = ApiFeeRepo::backfill_tx_hash_if_missing(
            &api_transaction_pool,
            &candidate.trade_no,
            &normalized_hash,
            "acct_change_runtime_repair",
        )
        .await?;

        tracing::warn!(
            source = "api_wallet_acct_change_fee_repair",
            trade_no = %candidate.trade_no,
            repair_mode = "acct_change_backfill_tx_hash",
            rows_affected = %rows_affected,
            chain_code = %self.0.chain_code,
            tx_hash = %normalized_hash,
            transaction_time = %acct_change_time_rfc3339,
            mqtt_out_of_order_tolerated = %(candidate.transaction_time.is_none()
                && candidate.last_broadcast_at.is_some()),
            "Fee runtime repair from acct_change attempted"
        );

        Ok(())
    }

    /// Best-effort runtime repair for withdraw records whose `tx_hash` is missing.
    ///
    /// Design constraints:
    /// - Driven by external acct_change facts (do not rely on local withdraw.tx_hash)
    /// - Hash-only repair (do NOT write transaction_time here)
    /// - Only repair normal withdraw orders on success path
    async fn try_repair_withdraw_from_acct_change(
        &self,
        ctx: &'static Context,
    ) -> Result<(), ServiceError> {
        let normalized_hash = BillDomain::handle_hash(&self.0.tx_hash).trim().to_string();
        let normalized_token =
            self.0.token.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

        tracing::debug!(
            source = "api_wallet_acct_change_withdraw_repair",
            chain_code = %self.0.chain_code,
            transfer_type = %self.0.transfer_type,
            tx_kind = %self.0.tx_kind,
            status = %self.0.status,
            from_addr = %self.0.from_addr,
            to_addr = %self.0.to_addr,
            symbol = %self.0.symbol,
            token = ?normalized_token,
            acct_change_tx_hash_raw = %self.0.tx_hash,
            acct_change_tx_hash_normalized = %normalized_hash,
            "Evaluating acct_change for withdraw runtime repair"
        );

        if !self.0.status {
            tracing::info!(
                source = "api_wallet_acct_change_withdraw_repair",
                skip_reason = "status_false",
                acct_change_tx_hash_raw = %self.0.tx_hash,
                "Skip withdraw runtime repair"
            );
            return Ok(());
        }
        if self.0.transfer_type != 1 {
            tracing::debug!(
                source = "api_wallet_acct_change_withdraw_repair",
                skip_reason = "transfer_type_not_outgoing",
                transfer_type = %self.0.transfer_type,
                "Skip withdraw runtime repair"
            );
            return Ok(());
        }
        if self.0.tx_kind != 1 {
            tracing::debug!(
                source = "api_wallet_acct_change_withdraw_repair",
                skip_reason = "tx_kind_not_normal",
                tx_kind = %self.0.tx_kind,
                "Skip withdraw runtime repair"
            );
            return Ok(());
        }
        if normalized_hash.is_empty() {
            tracing::info!(
                source = "api_wallet_acct_change_withdraw_repair",
                skip_reason = "empty_normalized_hash",
                acct_change_tx_hash_raw = %self.0.tx_hash,
                "Skip withdraw runtime repair"
            );
            return Ok(());
        }
        if self.0.from_addr.trim().is_empty() || self.0.to_addr.trim().is_empty() {
            tracing::info!(
                source = "api_wallet_acct_change_withdraw_repair",
                skip_reason = "empty_from_or_to",
                from_addr = %self.0.from_addr,
                to_addr = %self.0.to_addr,
                "Skip withdraw runtime repair"
            );
            return Ok(());
        }

        let acct_change_time = match self.convert_transaction_time(self.0.transaction_time.as_str())
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    source = "api_wallet_acct_change_withdraw_repair",
                    skip_reason = "transaction_time_parse_failed",
                    transaction_time = %self.0.transaction_time,
                    error = %e,
                    "Skip withdraw runtime repair"
                );
                return Ok(());
            }
        };

        let acct_change_value_str = self.0.value.to_string();
        let acct_change_value = match Decimal::from_str(&acct_change_value_str) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    source = "api_wallet_acct_change_withdraw_repair",
                    skip_reason = "invalid_acct_change_value",
                    value = %self.0.value,
                    value_str = %acct_change_value_str,
                    error = %e,
                    "Skip withdraw runtime repair"
                );
                return Ok(());
            }
        };

        let wallet_pool = ctx.api_wallet_pool()?;
        let api_transaction_pool = ctx.api_transaction_pool()?;

        let from_account = ApiAccountRepo::find_one_by_address_chain_code(
            &self.0.from_addr,
            &self.0.chain_code,
            &wallet_pool,
        )
        .await?;
        let Some(from_account) = from_account else {
            tracing::debug!(
                source = "api_wallet_acct_change_withdraw_repair",
                skip_reason = "from_account_not_found",
                from_addr = %self.0.from_addr,
                chain_code = %self.0.chain_code,
                "Skip withdraw runtime repair"
            );
            return Ok(());
        };
        if from_account.api_wallet_type != ApiWalletType::Withdrawal {
            tracing::debug!(
                source = "api_wallet_acct_change_withdraw_repair",
                skip_reason = "from_account_not_withdrawal",
                from_addr = %self.0.from_addr,
                api_wallet_type = ?from_account.api_wallet_type,
                "Skip withdraw runtime repair"
            );
            return Ok(());
        }

        let candidates = ApiWithdrawRepo::find_candidates_for_acct_change_hash_backfill(
            &api_transaction_pool,
            &self.0.chain_code,
            &self.0.from_addr,
            &self.0.to_addr,
            normalized_token.as_deref(),
            &self.0.symbol,
            Self::COLLECT_REPAIR_CANDIDATE_LIMIT,
        )
        .await?;

        let candidate_trade_nos: Vec<String> =
            candidates.iter().map(|c| c.trade_no.clone()).collect();

        let mut amount_match_count = 0usize;
        let mut time_window_match_count = 0usize;
        let mut execution_evidence_match_count = 0usize;
        let mut matched = Vec::new();

        for candidate in candidates {
            let Ok(candidate_value) = Decimal::from_str(candidate.value.trim()) else {
                tracing::debug!(
                    source = "api_wallet_acct_change_withdraw_repair",
                    trade_no = %candidate.trade_no,
                    withdraw_value = %candidate.value,
                    "Skip candidate due to invalid withdraw value format"
                );
                continue;
            };

            if candidate_value != acct_change_value {
                continue;
            }
            amount_match_count += 1;

            if !Self::withdraw_time_window_match(&candidate, acct_change_time) {
                continue;
            }
            time_window_match_count += 1;

            let has_execution_evidence = candidate.chain_success_at.is_some()
                || candidate.transaction_time.is_some()
                || candidate.last_broadcast_at.is_some();
            if !has_execution_evidence {
                tracing::debug!(
                    source = "api_wallet_acct_change_withdraw_repair",
                    trade_no = %candidate.trade_no,
                    skip_reason = "candidate_missing_execution_evidence_defer",
                    "Skip acct_change withdraw repair candidate"
                );
                continue;
            }
            execution_evidence_match_count += 1;
            matched.push(candidate);
        }

        tracing::info!(
            source = "api_wallet_acct_change_withdraw_repair",
            chain_code = %self.0.chain_code,
            acct_change_tx_hash_raw = %self.0.tx_hash,
            acct_change_tx_hash_normalized = %normalized_hash,
            candidate_count = %candidate_trade_nos.len(),
            candidate_trade_nos = ?candidate_trade_nos,
            amount_match_count = %amount_match_count,
            time_window_match_count = %time_window_match_count,
            execution_evidence_match_count = %execution_evidence_match_count,
            "Withdraw acct_change repair candidate evaluation finished"
        );

        let candidate = match matched.len() {
            0 => {
                tracing::info!(
                    source = "api_wallet_acct_change_withdraw_repair",
                    skip_reason = "no_candidate",
                    acct_change_tx_hash_normalized = %normalized_hash,
                    "Skip withdraw runtime repair: no matched candidate"
                );
                return Ok(());
            }
            1 => matched.pop().unwrap(),
            _ => {
                let trade_nos: Vec<String> = matched.iter().map(|c| c.trade_no.clone()).collect();
                tracing::warn!(
                    source = "api_wallet_acct_change_withdraw_repair",
                    skip_reason = "ambiguous_candidates",
                    candidate_trade_nos = ?trade_nos,
                    acct_change_tx_hash_normalized = %normalized_hash,
                    "Skip withdraw runtime repair: ambiguous candidates"
                );
                return Ok(());
            }
        };

        if let Some(existing_hash) =
            candidate.tx_hash.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            if existing_hash != normalized_hash {
                tracing::error!(
                    source = "api_wallet_acct_change_withdraw_repair",
                    trade_no = %candidate.trade_no,
                    existing_tx_hash = %existing_hash,
                    acct_change_tx_hash = %normalized_hash,
                    "Withdraw acct_change repair tx_hash conflict"
                );
                return Ok(());
            }
        }

        let acct_change_time_rfc3339 = acct_change_time.to_rfc3339();
        let tx_hash_missing =
            candidate.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
        if !tx_hash_missing {
            tracing::debug!(
                source = "api_wallet_acct_change_withdraw_repair",
                trade_no = %candidate.trade_no,
                skip_reason = "candidate_no_repair_needed_after_filters",
                "Skip withdraw runtime repair"
            );
            return Ok(());
        }

        let rows_affected = ApiWithdrawRepo::backfill_tx_hash_if_missing(
            &api_transaction_pool,
            &candidate.trade_no,
            &normalized_hash,
            "acct_change_runtime_repair",
        )
        .await?;

        tracing::warn!(
            source = "api_wallet_acct_change_withdraw_repair",
            trade_no = %candidate.trade_no,
            repair_mode = "acct_change_backfill_tx_hash",
            rows_affected = %rows_affected,
            chain_code = %self.0.chain_code,
            tx_hash = %normalized_hash,
            transaction_time = %acct_change_time_rfc3339,
            mqtt_out_of_order_tolerated = %(candidate.transaction_time.is_none()
                && (candidate.last_broadcast_at.is_some() || candidate.chain_success_at.is_some())),
            "Withdraw runtime repair from acct_change attempted"
        );

        Ok(())
    }

    fn fee_time_window_match(candidate: &ApiFeeEntity, acct_change_time: DateTime<Utc>) -> bool {
        let ref_time = candidate
            .transaction_time
            .as_ref()
            .cloned()
            .or_else(|| candidate.last_broadcast_at.as_ref().cloned());

        let Some(ref_time) = ref_time else {
            return false;
        };

        let diff = if acct_change_time >= ref_time {
            acct_change_time - ref_time
        } else {
            ref_time - acct_change_time
        };

        diff <= Duration::hours(Self::COLLECT_REPAIR_TIME_WINDOW_HOURS)
    }

    fn withdraw_time_window_match(
        candidate: &ApiWithdrawEntity,
        acct_change_time: DateTime<Utc>,
    ) -> bool {
        let ref_time = candidate
            .transaction_time
            .as_ref()
            .cloned()
            .or_else(|| candidate.chain_success_at.as_ref().cloned())
            .or_else(|| candidate.last_broadcast_at.as_ref().cloned());

        let Some(ref_time) = ref_time else {
            return false;
        };

        let diff = if acct_change_time >= ref_time {
            acct_change_time - ref_time
        } else {
            ref_time - acct_change_time
        };

        diff <= Duration::hours(Self::COLLECT_REPAIR_TIME_WINDOW_HOURS)
    }

    pub(crate) async fn prepare_receipt_assets(
        ctx: &'static Context,
        from: &str,
        to: &str,
        chain: &str,
        token: Option<String>,
    ) -> Result<Vec<AssetsId>, ServiceError> {
        let change = Self(AcctChange {
            tx_hash: String::new(),
            chain_code: chain.into(),
            symbol: String::new(),
            transfer_type: 1,
            tx_kind: 1,
            from_addr: from.into(),
            to_addr: to.into(),
            token,
            value: 0.0,
            transaction_fee: 0.0,
            transaction_time: String::new(),
            status: true,
            is_multisig: 0,
            queue_id: String::new(),
            block_height: 0,
            notes: String::new(),
            net_used: 0,
            energy_used: None,
            extra: None,
        });
        let addresses = Self::prepare_asset_addresses(ctx, &change).await?;
        Ok(addresses
            .into_iter()
            .map(|addr| AssetsId::new(&addr, chain, change.0.token.clone().into()))
            .collect())
    }

    async fn prepare_asset_addresses(
        ctx: &'static Context,
        acct_change: &Self,
    ) -> Result<Vec<String>, ServiceError> {
        let pool = ctx.api_wallet_pool()?;

        // 记录帐变信息用于调试
        tracing::info!(
            "开始同步资产: tx_hash={}, chain_code={}, symbol={}, from_addr={}, to_addr={}, status={}, token={:?}",
            acct_change.0.tx_hash,
            acct_change.0.chain_code,
            acct_change.0.symbol,
            acct_change.0.from_addr,
            acct_change.0.to_addr,
            acct_change.0.status,
            acct_change.0.token
        );

        // 优化：即使 status=false，也尝试同步（可能是失败交易但余额已变化）
        if !acct_change.0.status {
            tracing::warn!(
                "帐变状态为失败，但仍尝试同步资产: tx_hash={}, chain_code={}",
                acct_change.0.tx_hash,
                acct_change.0.chain_code
            );
        }

        // 尝试获取 coin 信息（用于创建资产记录），但不强制要求
        let token_key = AssetTokenKey::from_raw(acct_change.0.token.as_deref())
            .normalized_for_chain(&acct_change.0.chain_code);
        let coin = ApiCoinRepo::coin_by_chain_token_key_opt(
            &acct_change.0.chain_code,
            token_key.clone(),
            &pool,
        )
        .await?;

        // 如果 coin 不存在，尝试自动创建
        let coin = if (coin.is_none() && acct_change.0.token.is_some())
            || (coin.is_some()
                && coin.clone().unwrap().price.parse::<f64>().is_ok()
                && coin.clone().unwrap().price.parse::<f64>().unwrap() == 0.0f64)
        {
            tracing::info!(
                "coin 信息 有误，尝试自动创建代币或者更新: chain_code={}, token={:?}。{coin:?}",
                acct_change.0.chain_code,
                acct_change.0.token
            );

            // 重新查询 coin
            ApiCoinRepo::coin_by_chain_token_key_opt(&acct_change.0.chain_code, token_key, &pool)
                .await?
        } else {
            coin
        };

        if coin.is_none() {
            tracing::warn!(
                "未找到 coin 信息，将跳过资产记录创建，但仍尝试同步已存在的资产: chain_code={}, token={:?}",
                acct_change.0.chain_code,
                acct_change.0.token
            );
        }

        let addrs = vec![acct_change.0.from_addr.clone(), acct_change.0.to_addr.clone()];
        let mut sync_addrs = Vec::new();
        tracing::info!(
            "开始筛选资产同步地址: tx_hash={}, chain_code={}, token={:?}, addrs={:?}",
            acct_change.0.tx_hash,
            acct_change.0.chain_code,
            acct_change.0.token,
            addrs
        );

        // 优化：即使找不到 account，如果数据库中有资产记录，也应该同步
        for addr in addrs.iter() {
            let account = ApiAccountRepo::find_one_by_address_chain_code(
                addr,
                &acct_change.0.chain_code,
                &pool,
            )
            .await?;

            // 如果找到 account，尝试创建资产记录（如果不存在）
            if let Some(account) = &account {
                if let Some(ref coin) = coin {
                    let assets_id_vo = AssetsId::new(
                        addr,
                        &acct_change.0.chain_code,
                        acct_change.0.token.clone().into(),
                    );
                    let assets = ApiAssetsRepo::find_by_id(&pool, &assets_id_vo).await?;
                    if assets.is_none() {
                        let assets_id = AssetsId::new(
                            &account.address,
                            &account.chain_code,
                            coin.token_address.clone(),
                        );
                        let assets = ApiCreateAssetsVo::new(
                            assets_id,
                            &coin.symbol,
                            coin.decimals,
                            coin.protocol.clone(),
                            0,
                        )
                        .with_name(&coin.name)
                        .with_u256(alloy::primitives::U256::default(), coin.decimals)?;
                        ApiAssetsRepo::upsert_assets_multi(&pool, vec![assets]).await?;
                        tracing::info!(
                            "创建资产记录: address={}, chain_code={}, symbol={}",
                            account.address,
                            account.chain_code,
                            coin.symbol
                        );
                    }
                }
            }

            // 优化：即使找不到 account，如果数据库中有该地址的资产记录，也应该同步
            let assets_id_vo =
                AssetsId::new(addr, &acct_change.0.chain_code, acct_change.0.token.clone().into());
            let existing_assets = ApiAssetsRepo::find_by_id(&pool, &assets_id_vo).await?;
            let has_account = account.is_some();
            let has_existing_assets = existing_assets.is_some();

            if has_account || has_existing_assets {
                sync_addrs.push(addr.to_string());
            } else {
                tracing::warn!(
                    "跳过地址（无 account 且无资产记录）: address={}, chain_code={}",
                    addr,
                    acct_change.0.chain_code
                );
            }

            tracing::info!(
                "资产同步地址判定: tx_hash={}, address={}, chain_code={}, token={:?}, has_account={}, has_existing_assets={}, will_sync={}",
                acct_change.0.tx_hash,
                addr,
                acct_change.0.chain_code,
                acct_change.0.token,
                has_account,
                has_existing_assets,
                has_account || has_existing_assets
            );
        }

        sync_addrs.sort();
        sync_addrs.dedup();
        Ok(sync_addrs)
    }

    async fn sync_assets(
        ctx: &'static Context,
        acct_change: &ApiWalletAcctChange,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let sync_addrs = Self::prepare_asset_addresses(ctx, acct_change).await?;
        if sync_addrs.is_empty() {
            tracing::warn!(
                "没有需要同步的地址: tx_hash={}, chain_code={}",
                acct_change.0.tx_hash,
                acct_change.0.chain_code
            );
            return Ok(());
        }

        let sync_token_key = AssetTokenKey::from_raw(acct_change.0.token.as_deref())
            .normalized_for_chain(&acct_change.0.chain_code);
        for addr in &sync_addrs {
            ApiAssetSyncStateRepo::upsert_pending(
                &pool,
                addr,
                &acct_change.0.chain_code,
                sync_token_key.clone(),
            )
            .await?;
        }
        tracing::info!(
            "API 钱包账变资产同步任务已持久化: tx_hash={}, addrs={:?}, chain_code={}, token={:?}",
            acct_change.0.tx_hash,
            sync_addrs,
            acct_change.0.chain_code,
            acct_change.0.token
        );

        let handles = ctx.get_global_handles().await;
        if let Some(handles) = handles.upgrade() {
            let inner_event_handle = handles.get_global_inner_event_handle();

            let data = SyncAssetsData::new_with_token_key(
                sync_addrs.clone(),
                acct_change.0.chain_code.clone(),
                sync_token_key,
            )
            .with_priority(SyncPriority::High);
            tracing::info!(
                "发送到账优先资产同步事件: tx_hash={}, addrs={:?}, chain_code={}, token={:?}",
                acct_change.0.tx_hash,
                sync_addrs,
                acct_change.0.chain_code,
                acct_change.0.token
            );

            inner_event_handle.send(InnerEvent::ApiWalletSyncAssets(data))?;
            tracing::info!(
                "资产同步事件已发送: tx_hash={}, addrs={:?}, chain_code={}, token={:?}",
                acct_change.0.tx_hash,
                sync_addrs,
                acct_change.0.chain_code,
                acct_change.0.token
            );
        } else {
            tracing::error!(
                "Handles 已释放，无法发送资产同步事件: tx_hash={}",
                acct_change.0.tx_hash
            );
        }

        Ok(())
    }

    // 尝试为地址创建代币
    async fn try_create_coin_for_address(
        ctx: &'static Context,
        chain_code: &str,
        token_address: &str,
        symbol: &str,
    ) -> Result<(), ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        tracing::error!("为地址创建代币22: chain_code={}, token={}", chain_code, token_address);
        if token_address.is_empty() {
            return Ok(());
        }

        let chain_instance =
            ChainAdapterFactory::get_transaction_adapter_with_ctx(ctx, chain_code).await?;
        let backend_api = ctx.get_global_backend_api();
        let coins_finds = backend_api.fetch_all_api_tokens(None, None).await?;
        tracing::error!(
            "1try_create_coin_for_address find token coin , price is :{:?}",
            coins_finds
        );

        let coin_find = coins_finds.iter().find(|o| {
            AssetTokenKey::from_raw(o.token_address.as_deref())
                .matches_for_chain(&AssetTokenKey::from_raw(Some(token_address)), chain_code)
                && o.chain_code == Some(chain_code.to_string())
        });

        tracing::error!(
            "2try_create_coin_for_address Create new token coin , price is :{:?}",
            coin_find
        );
        let cus_coin = Self::build_acct_change_coin(
            &chain_instance,
            chain_code,
            token_address,
            symbol,
            coin_find.and_then(|x| x.price.map(|p| p.to_string())),
        )
        .await?;
        let coin = vec![cus_coin];
        tracing::error!("[55customize_coin] coin: {:?} ", coin);
        ApiCoinRepo::upsert_multi_coin(&pool, coin).await?;
        tracing::error!("成功创建代币: chain_code={}, token={}", chain_code, token_address);

        Ok(())
    }

    async fn build_acct_change_coin(
        chain_instance: &crate::domain::chain::adapter::TransactionAdapter,
        chain_code: &str,
        token_address: &str,
        symbol: &str,
        price: Option<String>,
    ) -> Result<ApiCoinData, ServiceError> {
        if symbol.trim().is_empty() {
            return Err(ServiceError::Parameter("acct_change token symbol is empty".into()));
        }
        let time = wallet_utils::time::now();
        // The notification supplies the display symbol; only precision comes from RPC.
        let coin = ApiCoinData::new(
            Some(symbol.to_string()),
            symbol,
            chain_code,
            AssetTokenKey::from_raw(Some(token_address)),
            Some(price.unwrap_or_else(|| "0".into())),
            None,
            chain_instance.decimals(token_address).await?,
            1,
            0,
            1,
            time,
            Some(time),
        )
        .with_custom(0)
        .with_status(1);
        Ok(coin)
    }

    async fn deposit_acct_change(&self, ctx: &'static Context) -> Result<(), ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let api_transaction_pool = ctx.api_transaction_pool()?;
        let to_account = ApiAccountRepo::find_one_by_address_chain_code(
            &self.0.to_addr,
            &self.0.chain_code,
            &pool,
        )
        .await?;
        if let Some(to_account) = to_account {
            if to_account.api_wallet_type == ApiWalletType::Withdrawal {
                let from_account = ApiAccountRepo::find_one_by_address_chain_code(
                    &self.0.from_addr,
                    &self.0.chain_code,
                    &pool,
                )
                .await?;
                if let None = from_account {
                    let wallet =
                        ApiWalletRepo::find_by_address(&pool, &to_account.wallet_address).await?;
                    if let Some(wallet) = wallet {
                        let datetime =
                            self.convert_transaction_time(self.0.transaction_time.as_str())?;
                        let resource_consume = if let Some(energy_used) = self.0.energy_used {
                            energy_used.to_string()
                        } else {
                            "".to_string()
                        };
                        let trade_no = uuid::Uuid::new_v4().to_string();
                        ApiWithdrawRepo::upsert_api_withdraw(
                            &api_transaction_pool,
                            &wallet.uid,
                            &wallet.name,
                            self.0.from_addr.as_str(),
                            self.0.to_addr.as_str(),
                            self.0.value.to_string().as_str(),
                            "",
                            &self.0.chain_code,
                            self.0.token.clone(),
                            self.0.symbol.as_str(),
                            &trade_no,
                            None,
                            None,
                            None,
                            ApiTradeType::SelfRecharge,
                            0,
                            Some(self.0.tx_hash.clone()),
                            ApiWithdrawStatus::ConfirmSuccessReport,
                            ApiWithdrawStatus::ConfirmSuccessReport,
                            resource_consume.as_str(),
                            self.0.transaction_fee.to_string().as_str(),
                            Some(datetime),
                            Some(self.0.block_height.to_string()),
                        )
                        .await?;
                    }
                } else {
                    tracing::warn!(to_account=%to_account.address, "from account found:");
                }
            }
        }
        Ok(())
    }

    async fn self_transfer_acct_change(&self, ctx: &'static Context) -> Result<(), ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let api_transaction_pool = ctx.api_transaction_pool()?;
        let from_account = ApiAccountRepo::find_one_by_address_chain_code(
            &self.0.from_addr,
            &self.0.chain_code,
            &pool,
        )
        .await?;
        if let Some(from_account) = from_account {
            if from_account.api_wallet_type == ApiWalletType::Withdrawal {
                let res = ApiWithdrawRepo::get_by_hash_and_owner(
                    &api_transaction_pool,
                    self.0.from_addr.as_str(),
                    &self.0.tx_hash,
                )
                .await;
                match res {
                    Ok(tx) => {
                        if tx.trade_type == ApiTradeType::SelfWithdraw {
                            // A signed EVM submission is resolved by its receipt observer.
                            // Account-change success can describe a fee/native event for
                            // the same hash, not the token transfer's execution result.
                            if crate::domain::api_wallet::trans::shared_mechanism::is_evm_chain(
                                &tx.chain_code,
                            ) && tx.raw_tx.as_deref().is_some_and(|raw| !raw.is_empty())
                            {
                                tracing::info!(trade_no = %tx.trade_no, tx_hash = %self.0.tx_hash,
                                    message_success = self.0.status,
                                    "signed EVM self transfer status reserved for receipt observation");
                                return Ok(());
                            }
                            let status = if self.0.status {
                                ApiWithdrawStatus::ConfirmSuccessReport
                            } else {
                                ApiWithdrawStatus::ConfirmFailureReport
                            };
                            let datetime =
                                self.convert_transaction_time(self.0.transaction_time.as_str())?;
                            let resource_consume = if let Some(energy_used) = self.0.energy_used {
                                energy_used.to_string()
                            } else {
                                "0".to_string()
                            };
                            ApiWithdrawRepo::update_api_withdraw_tx_status(
                                &api_transaction_pool,
                                &tx.trade_no,
                                0,
                                &tx.tx_hash.unwrap_or_default(),
                                &resource_consume,
                                self.0.transaction_fee.to_string().as_str(),
                                Some(datetime),
                                self.0.block_height.to_string().as_str(),
                                status,
                            )
                            .await?;
                        }
                        //  else if tx.trade_type == ApiTradeType::Withdraw {
                        //     let datetime =
                        //         self.convert_transaction_time(self.0.transaction_time.as_str())?;
                        //     let resource_consume = if let Some(energy_used) = self.0.energy_used {
                        //         energy_used.to_string()
                        //     } else {
                        //         "0".to_string()
                        //     };
                        //     ApiWithdrawRepo::update_api_withdraw_tx(
                        //         &api_transaction_pool,
                        //         &tx.trade_no,
                        //         &resource_consume,
                        //         self.0.transaction_fee.to_string().as_str(),
                        //         Some(datetime),
                        //         self.0.block_height.to_string().as_str(),
                        //     )
                        //     .await?;
                        // }
                        else {
                            tracing::warn!("api_wallet_type == {:?} is not found:", tx.trade_type);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("api_wallet_type == Withdrawal is not found: {}", e);
                    }
                }
            }
        }
        Ok(())
    }

    fn convert_transaction_time(
        &self,
        transaction_time: &str,
    ) -> Result<DateTime<Utc>, ServiceError> {
        let naive =
            NaiveDateTime::parse_from_str(transaction_time, "%Y-%m-%d %H:%M:%S").map_err(|_| {
                ServiceError::Business(
                    ApiWalletError::DataTimeParseError(transaction_time.to_string()).into(),
                )
            })?;
        let datetime: DateTime<Utc> = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
        Ok(datetime)
    }
}

#[cfg(test)]
#[path = "acct_change/coin_metadata_tests.rs"]
mod coin_metadata_tests;
