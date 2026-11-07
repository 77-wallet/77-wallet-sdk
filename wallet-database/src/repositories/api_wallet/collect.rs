// ======================= 强顺序保证说明 =======================
// 本文件是 Collect 顺序链中的关键实现：
// OrderAck -> BuildTx -> Broadcast -> TxExecReceipt -> ResultAck
//
// ⚠️ 禁止修改以下事实依赖：
// - scan_can_build 必须依赖 order_ack_sent_at
// - scan_confirmed_need_result_ack 必须依赖 tx_exec_receipt_uploaded_at
//
// 修改这些条件将破坏系统的强顺序与 crash-safe 特性。
// =============================================================

use crate::{
    ApiTransactionDbPool,
    dao::api_collect::ApiCollectDao,
    entities::{
        api_collect::{
            ApiCollectEntity, ApiCollectStatus, CollectCreatedFact, ErrCode,
            PendingCollectResourceBudget,
        },
        api_resource_gate::{
            ApiResourceBlockReason, ApiResourceDependencyType, ApiResourceGateResult,
        },
        asset_token_key::AssetTokenKey,
    },
};

/// A different transaction with the same sender/nonce in a finalized EVM block.
/// The caller must verify inclusion and finality through chain RPC before calling
/// `record_finalized_nonce_conflict`. A pending nonce is NOT this evidence.
#[derive(Debug, serde::Serialize)]
pub struct FinalizedCollectNonceConflict {
    pub chain_code: String,
    pub from_addr: String,
    pub nonce: i32,
    pub conflicting_tx_hash: String,
    pub block_hash: String,
    pub block_number: u64,
}

/// Built EVM bytes awaiting atomic nonce reservation. Never broadcast before commit.
pub struct SignedCollectCandidate<'a> {
    pub trade_no: &'a str,
    pub from_addr: &'a str,
    pub chain_code: &'a str,
    pub tx_hash: &'a str,
    pub raw_tx: &'a str,
    pub transaction_fee: &'a str,
    pub nonce: i64,
    pub chain_next: i64,
}

#[cfg(test)]
#[path = "collect_signed_candidate_tests.rs"]
mod collect_signed_candidate_tests;

pub struct ApiCollectRepo;

impl ApiCollectRepo {
    /// Report an unsigned nonce hole without cancelling or releasing signed bytes.
    /// Recheck all shared nonce owners under the SQLite writer before failure.
    pub async fn fail_verified_nonce_gap(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        chain_next: i64,
        expected_nonce: i64,
    ) -> Result<u64, crate::Error> {
        use crate::{
            dao::api_nonce::ApiNonceDao,
            entities::api_nonce_claim::{COLLECT_NONCE_GAP_REPORTED, WithdrawNonceDecision},
        };
        if chain_next < 0 || expected_nonce <= chain_next {
            return Ok(0);
        }
        let mut tx =
            pool.write_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        // Acquire the writer before reading candidate and ownership facts.
        sqlx::query("UPDATE api_collect SET nonce=nonce WHERE trade_no=?")
            .bind(trade_no)
            .execute(&mut *tx)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        let row = ApiCollectDao::get_api_collect_by_trade_no(&mut *tx, trade_no).await?;
        if !crate::entities::api_withdraw::is_signed_evm_chain(&row.chain_code)
            || row.nonce as i64 != expected_nonce
            || row.err_code.is_some()
            || row.finished_at.is_some()
            || row.transaction_time.is_some()
            || row.raw_tx.is_none()
            || row.tx_hash.is_none()
        {
            return Ok(0);
        }
        let decision = ApiNonceDao::signed_nonce_decision(
            &mut tx,
            &row.from_addr,
            &row.chain_code,
            chain_next,
        )
        .await?;
        let WithdrawNonceDecision::Ready(missing) = decision else {
            return Ok(0);
        };
        if missing >= expected_nonce {
            return Ok(0);
        }
        let message = format!(
            "{COLLECT_NONCE_GAP_REPORTED} chain_next={chain_next} missing_nonce={missing} order_nonce={expected_nonce}; signed transaction retained, not cancelled"
        );
        let rows = ApiCollectDao::update_status_and_err(
            &mut *tx,
            trade_no,
            ApiCollectStatus::SendingTxFailed,
            ErrCode::SDKInternalError,
            &message,
        )
        .await?;
        tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(rows)
    }

    pub async fn list_api_collect(
        pool: &ApiTransactionDbPool,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::all_api_collect(pool.read_ref()).await
    }

    pub async fn page_api_collect(
        pool: &ApiTransactionDbPool,
        _page: i64,
        _page_size: i64,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::all_api_collect(pool.read_ref()).await
    }

    pub async fn page_api_collect_with_status(
        pool: &ApiTransactionDbPool,
        page: i64,
        page_size: i64,
        vec_status: &[ApiCollectStatus],
    ) -> Result<(i64, Vec<ApiCollectEntity>), crate::Error> {
        ApiCollectDao::page_api_collect_with_status(pool.read_ref(), page, page_size, vec_status)
            .await
    }

    pub async fn get_api_collect_by_trade_no(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<ApiCollectEntity, crate::Error> {
        ApiCollectDao::get_api_collect_by_trade_no(pool.read_ref(), trade_no).await
    }

    pub async fn find_api_collect_by_trade_no(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<Option<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::find_api_collect_by_trade_no(pool.read_ref(), trade_no).await
    }

    pub async fn find_by_resource_dependency_trade_no(
        pool: &ApiTransactionDbPool,
        dependency_trade_no: &str,
    ) -> Result<Option<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::find_by_resource_dependency_trade_no(pool.read_ref(), dependency_trade_no)
            .await
    }

    pub async fn get_api_collect_by_trade_no_status(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        vec_status: &[ApiCollectStatus],
    ) -> Result<ApiCollectEntity, crate::Error> {
        ApiCollectDao::get_api_collect_by_trade_no_status(pool.read_ref(), trade_no, vec_status)
            .await
    }

    /// Runtime repair helper: query collect candidates from acct_change facts.
    ///
    /// The caller must still perform Rust-side filtering (amount/time window/uniqueness)
    /// and decide whether to backfill `tx_hash` or defer to another onchain-confirm path.
    pub async fn find_candidates_for_acct_change_repair(
        pool: &ApiTransactionDbPool,
        chain_code: &str,
        from_addr: &str,
        to_addr: &str,
        token_addr: Option<&str>,
        symbol: &str,
        limit: i64,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::find_candidates_for_acct_change_repair(
            pool.read_ref(),
            chain_code,
            from_addr,
            to_addr,
            token_addr,
            symbol,
            limit,
        )
        .await
    }

    pub async fn upsert_api_collect(
        pool: &ApiTransactionDbPool,
        uid: &str,
        name: &str,
        from_addr: &str,
        to_addr: &str,
        value: &str,
        validate: &str,
        chain_code: &str,
        token_addr: impl Into<AssetTokenKey>,
        symbol: &str,
        trade_no: &str,
        trade_type: u8,
        status: ApiCollectStatus,
        risk_addr: u8,
    ) -> Result<(), crate::Error> {
        let token_addr = token_addr.into();
        let collect_req = CollectCreatedFact {
            uid: Some(uid.to_string()),
            name: name.to_string(),
            from_addr: from_addr.to_string(),
            to_addr: to_addr.to_string(),
            value: value.to_string(),
            validate: validate.to_string(),
            chain_code: chain_code.to_string(),
            token_addr,
            symbol: symbol.to_string(),
            trade_no: trade_no.to_string(),
            trade_type: trade_type as i64,
            risk_addr: risk_addr.to_string(),
            status,
        };
        ApiCollectDao::add(pool.write_ref(), collect_req).await
    }

    pub async fn update_api_collect_to_addr(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        to_addr: &str,
    ) -> Result<(), crate::Error> {
        ApiCollectDao::update_to_addr(pool.write_ref(), trade_no, to_addr).await
    }

    pub async fn mark_resource_blocked(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        block_reason: ApiResourceBlockReason,
        dependency_trade_no: Option<&str>,
        dependency_type: Option<ApiResourceDependencyType>,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_resource_blocked(
            pool.write_ref(),
            trade_no,
            block_reason,
            dependency_trade_no,
            dependency_type,
        )
        .await
    }

    pub async fn mark_resource_released(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        gate_result: ApiResourceGateResult,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_resource_released(pool.write_ref(), trade_no, gate_result).await
    }

    pub async fn mark_resource_released_with_estimate(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        gate_result: ApiResourceGateResult,
        estimated_resource_consume: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_resource_released_with_estimate(
            pool.write_ref(),
            trade_no,
            gate_result,
            estimated_resource_consume,
        )
        .await
    }

    pub async fn pending_resource_budget_by_owner(
        pool: &ApiTransactionDbPool,
        chain_code: &str,
        from_addr: &str,
        exclude_trade_no: &str,
    ) -> Result<PendingCollectResourceBudget, crate::Error> {
        ApiCollectDao::pending_resource_budget_by_owner(
            pool.read_ref(),
            chain_code,
            from_addr,
            exclude_trade_no,
        )
        .await
    }

    pub async fn invalidate_resource_ready_for_reeval(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::invalidate_resource_ready_for_reeval(pool.write_ref(), trade_no).await
    }

    pub async fn clear_account_activation_block_for_reeval(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        activation_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::clear_account_activation_block_for_reeval(
            pool.write_ref(),
            trade_no,
            activation_trade_no,
        )
        .await
    }

    pub async fn update_api_collect_tx_status(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        tx_hash: &str,
        resource_consume: &str,
        transaction_fee: &str,
        status: ApiCollectStatus,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::update_tx_status(
            pool.write_ref(),
            trade_no,
            tx_hash,
            resource_consume,
            transaction_fee,
            status,
        )
        .await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    #[deprecated(
        since = "0.1.0",
        note = "Legacy state-machine API. Do not use in fact-driven system. This will be removed in future versions."
    )]
    #[allow(deprecated)]
    pub async fn legacy_update_api_collect_status_and_err(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        status: ApiCollectStatus,
        err_code: ErrCode,
        err_msg: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::update_status_and_err(pool.write_ref(), trade_no, status, err_code, err_msg)
            .await
    }

    // 兼容旧代码，标记为 deprecated
    #[allow(deprecated)]
    #[deprecated(since = "0.1.0", note = "Use legacy_update_api_collect_status_and_err instead.")]
    pub async fn update_api_collect_status_and_err(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        status: ApiCollectStatus,
        err_code: ErrCode,
        err_msg: &str,
    ) -> Result<u64, crate::Error> {
        Self::legacy_update_api_collect_status_and_err(pool, trade_no, status, err_code, err_msg)
            .await
    }

    #[deprecated(
        since = "0.1.0",
        note = "LEGACY STATE MACHINE API. Do not use in Shadow / Scanner / fact-driven paths. Use fact-based APIs instead."
    )]
    #[allow(deprecated)]
    pub async fn legacy_update_api_collect_next_status(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        status: ApiCollectStatus,
        next_status: ApiCollectStatus,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::legacy_update_next_status(pool.write_ref(), trade_no, status, next_status)
            .await
    }

    // 兼容旧代码，标记为 deprecated
    #[allow(deprecated)]
    #[deprecated(since = "0.1.0", note = "Use legacy_update_api_collect_next_status instead.")]
    pub async fn update_api_collect_next_status(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        status: ApiCollectStatus,
        next_status: ApiCollectStatus,
    ) -> Result<u64, crate::Error> {
        Self::legacy_update_api_collect_next_status(pool, trade_no, status, next_status).await
    }

    #[deprecated(
        since = "0.1.0",
        note = "LEGACY STATE MACHINE API. Do not use in Shadow / Scanner / fact-driven paths. Use fact-based APIs instead."
    )]
    #[allow(deprecated)]
    pub async fn legacy_update_api_collect_next_status_and_err(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        status: ApiCollectStatus,
        next_status: ApiCollectStatus,
        err_code: u32,
        err_msg: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::legacy_update_next_status_and_err(
            pool.write_ref(),
            trade_no,
            status,
            next_status,
            err_code,
            err_msg,
        )
        .await
    }

    // 兼容旧代码，标记为 deprecated
    #[allow(deprecated)]
    #[deprecated(
        since = "0.1.0",
        note = "Use legacy_update_api_collect_next_status_and_err instead."
    )]
    pub async fn update_api_collect_next_status_and_err(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        status: ApiCollectStatus,
        next_status: ApiCollectStatus,
        err_code: u32,
        err_msg: &str,
    ) -> Result<u64, crate::Error> {
        Self::legacy_update_api_collect_next_status_and_err(
            pool,
            trade_no,
            status,
            next_status,
            err_code,
            err_msg,
        )
        .await
    }

    pub async fn update_api_collect_post_tx_count(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::update_post_tx_count(pool.write_ref(), trade_no).await
    }

    /// 标记已收到 SER TxRes 推送（AWM_ORDER_TRANS_RES）
    ///
    /// 语义：
    /// - 仅表示“SDK 已收到并持久化 SER 的交易执行结果推送”
    /// - 与链上确认（transaction_time）不是同一事实
    /// - 用于强顺序屏障：TX_RES ACK 禁止早于该事实发送
    pub async fn update_tx_res_received_at(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::update_tx_res_received_at(pool.write_ref(), trade_no).await
    }

    pub async fn update_api_collect_post_confirm_tx_count(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::update_post_confirm_tx_count(pool.write_ref(), trade_no).await
    }

    /// Save signed facts and reserve the candidate in one SQLite transaction.
    /// The shared counter write serializes all senders, including legacy allocators.
    pub async fn reserve_signed_collect(
        pool: &ApiTransactionDbPool,
        input: &SignedCollectCandidate<'_>,
    ) -> Result<u64, crate::Error> {
        use crate::dao::api_nonce::ApiNonceDao;
        if !crate::entities::api_withdraw::is_signed_evm_chain(input.chain_code)
            || input.nonce < 0
            || input.chain_next < 0
            || input.nonce < input.chain_next
            || input.tx_hash.is_empty()
            || input.raw_tx.is_empty()
            || input.from_addr.is_empty()
            || input.trade_no.is_empty()
        {
            return Err(crate::Error::Other("invalid signed EVM collect candidate".into()));
        }
        let mut tx =
            pool.write_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        ApiNonceDao::upsert_nonce_floor(
            &mut *tx,
            input.from_addr,
            input.chain_code,
            input.chain_next - 1,
        )
        .await?;
        let row = ApiCollectDao::get_api_collect_by_trade_no(&mut *tx, input.trade_no).await?;
        if row.from_addr != input.from_addr || row.chain_code != input.chain_code {
            return Err(crate::Error::Other("collect candidate identity conflict".into()));
        }
        if row.raw_tx.is_some() {
            if row.raw_tx.as_deref() == Some(input.raw_tx)
                && row.tx_hash.as_deref() == Some(input.tx_hash)
                && row.nonce == input.nonce
                && row.transaction_fee == input.transaction_fee
            {
                return Ok(0);
            }
            return Err(crate::Error::Other("collect candidate signed facts conflict".into()));
        }
        let decision = ApiNonceDao::signed_nonce_decision(
            &mut tx,
            input.from_addr,
            input.chain_code,
            input.chain_next,
        )
        .await?;
        let replacement = super::collect_replacement::CollectReplacementRepo::permits_signed(
            &mut tx,
            input.trade_no,
            input.nonce,
            input.chain_next,
        )
        .await?;
        if (decision != crate::entities::api_nonce_claim::WithdrawNonceDecision::Ready(input.nonce)
            && !replacement)
            || row.building_at.is_none()
            || row.order_ack_sent_at.is_none()
            || row.is_fact_terminal()
            || row.transaction_time.is_some()
            || row.err_code.is_some()
            || row.tx_hash.is_some()
            || row.last_broadcast_at.is_some()
            || row.broadcast_uncertain_since_at.is_some()
            || row.tx_exec_receipt_uploaded_at.is_some()
            || row.need_service_fee == Some(true)
            || (row.ever_needed_service_fee && row.tx_fee_res_ack_sent_at.is_none())
        {
            return Err(crate::Error::Other(
                "collect nonce candidate or lifecycle conflict".into(),
            ));
        }
        let rows = ApiCollectDao::update_after_build(
            &mut *tx,
            input.trade_no,
            input.tx_hash,
            input.raw_tx,
            input.transaction_fee,
            input.nonce,
        )
        .await?;
        if rows != 1 {
            return Err(crate::Error::Other("collect candidate commit conflict".into()));
        }
        super::collect_replacement::CollectReplacementRepo::record_signed(
            &mut tx,
            input.trade_no,
            input.tx_hash,
            input.raw_tx,
        )
        .await?;
        let signed = ApiCollectDao::get_api_collect_by_trade_no(&mut *tx, input.trade_no).await?;
        ApiCollectDao::update_status(&mut *tx, input.trade_no, signed.recompute_status()).await?;
        ApiNonceDao::upsert_nonce_floor(&mut *tx, input.from_addr, input.chain_code, input.nonce)
            .await?;
        tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(rows)
    }

    pub async fn update_after_build(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        tx_hash: &str,
        raw_tx: &str,
        transaction_fee: &str,
        nonce: i64,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::update_after_build(
            pool.write_ref(),
            trade_no,
            tx_hash,
            raw_tx,
            transaction_fee,
            nonce,
        )
        .await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    pub async fn set_order_ack_sent(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<(), crate::Error> {
        ApiCollectDao::mark_order_ack_sent(pool.write_ref(), trade_no).await.map(|_| ())
    }

    pub async fn get_ack_times(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<
        (
            Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
            Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
        ),
        crate::Error,
    > {
        ApiCollectDao::get_ack_times(pool.read_ref(), trade_no).await
    }

    /// 扫描可构建的交易
    ///
    /// `building_at` 作为 worker 入口的 build-slot 占位，不参与 scanner 过滤。
    /// 如果曾经缺过手续费，则必须先完成 TxFeeResAck。
    pub async fn scan_can_build(
        pool: &ApiTransactionDbPool,
        limit: usize,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::scan_can_build(pool.read_ref(), limit).await
    }

    pub async fn scan_unexecuted_created_before(
        pool: &ApiTransactionDbPool,
        created_before: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
        limit: usize,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::scan_unexecuted_created_before(pool.read_ref(), created_before, limit).await
    }

    pub async fn mark_unexecuted_expired(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        created_before: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
        err_code: ErrCode,
        err_msg: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_unexecuted_expired(
            pool.write_ref(),
            trade_no,
            created_before,
            err_code,
            err_msg,
        )
        .await
    }

    pub async fn scan_need_resource_gate(
        pool: &ApiTransactionDbPool,
        limit: usize,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::scan_need_resource_gate(pool.read_ref(), limit).await
    }

    /// 扫描可广播的交易
    ///
    /// 事实条件：
    /// - raw_tx IS NOT NULL
    /// - (last_broadcast_at IS NULL) OR (last_broadcast_at IS NOT NULL AND tx_exec_receipt_uploaded_at IS NOT NULL)
    /// - finished_at IS NULL
    /// - 只有当前周期真的进入过服务费上传，才需要等待 TxFeeResAck
    /// - (service_fee_uploaded_at IS NULL OR tx_fee_res_ack_sent_at IS NOT NULL)
    ///
    /// ⚠️ 重要约束：
    /// - SQL必须100%等价于scanner中的can_broadcast predicate
    /// - 特别注意：service_fee_uploaded_at IS NOT NULL 的记录
    ///   在tx_fee_res_ack_sent_at IS NULL时永远不能被扫出来
    pub async fn scan_can_broadcast(
        pool: &ApiTransactionDbPool,
        limit: usize,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::scan_can_broadcast(pool.read_ref(), limit).await
    }

    /// 扫描已确认且需要发送Result ACK的交易
    pub async fn scan_confirmed_need_result_ack(
        pool: &ApiTransactionDbPool,
        limit: usize,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::scan_confirmed_need_result_ack(pool.read_ref(), limit).await
    }

    /// 扫描已确认但未上传服务费的交易
    pub async fn scan_confirmed_need_service_fee_upload(
        pool: &ApiTransactionDbPool,
        limit: usize,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::scan_confirmed_need_service_fee_upload(pool.read_ref(), limit).await
    }

    /// 扫描需要发送手续费结果确认 ACK 的交易
    pub async fn scan_confirmed_need_tx_fee_res_ack(
        pool: &ApiTransactionDbPool,
        limit: usize,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::scan_confirmed_need_tx_fee_res_ack(pool.read_ref(), limit).await
    }

    /// 更新building_at时间
    pub async fn update_building_at(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::update_building_at(pool.write_ref(), trade_no).await
    }

    /// 清除 build slot 占位
    pub async fn clear_building_at(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::clear_building_at(pool.write_ref(), trade_no).await
    }

    /// 更新last_broadcast_at时间
    pub async fn update_last_broadcast_at(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::update_last_broadcast_at(pool.write_ref(), trade_no).await
    }

    /// Mark successful broadcast execution
    ///
    /// Semantics:
    /// - Represents a successful broadcast attempt
    /// - NOT a chain confirmation
    /// - Idempotent, overwrite allowed
    pub async fn mark_broadcast_executed(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_broadcast_executed(pool.write_ref(), trade_no).await
    }

    /// 记录 EVM 广播/恢复不确定态（RPC 返回 hash 但同节点不可见）
    pub async fn mark_broadcast_uncertain_attempt(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_broadcast_uncertain_attempt(pool.write_ref(), trade_no).await
    }

    /// Record a verified conflict only while the original transaction is unresolved.
    /// Returns zero if another worker advanced/replaced the order. Never rebuilds
    /// or changes transaction identity; existing receipt-upload scanning reports it.
    pub async fn record_finalized_nonce_conflict(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        expected_tx_hash: &str,
        evidence: &FinalizedCollectNonceConflict,
    ) -> Result<u64, crate::Error> {
        let is_hex = |value: &str, len: usize| {
            value.len() == len
                && value.starts_with("0x")
                && value[2..].bytes().all(|b| b.is_ascii_hexdigit())
        };
        if !matches!(evidence.chain_code.as_str(), "eth" | "bnb")
            || evidence.nonce < 0
            || evidence.block_number == 0
            || !is_hex(&evidence.from_addr, 42)
            || !is_hex(expected_tx_hash, 66)
            || !is_hex(&evidence.conflicting_tx_hash, 66)
            || !is_hex(&evidence.block_hash, 66)
            || expected_tx_hash.eq_ignore_ascii_case(&evidence.conflicting_tx_hash)
        {
            return Err(crate::Error::Other(
                "Invalid finalized collect nonce conflict evidence".into(),
            ));
        }
        let evidence_json =
            serde_json::to_string(evidence).map_err(|e| crate::Error::Other(e.to_string()))?;
        let message =
            format!("EVM nonce consumed by another finalized transaction: {evidence_json}");
        ApiCollectDao::record_finalized_nonce_conflict(
            pool.write_ref(),
            trade_no,
            expected_tx_hash,
            &evidence.chain_code,
            &evidence.from_addr,
            evidence.nonce,
            &message,
        )
        .await
    }

    /// 标记已执行不确定态超时 reconcile（每个生命周期最多一次）
    pub async fn mark_broadcast_uncertain_reconciled(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_broadcast_uncertain_reconciled(pool.write_ref(), trade_no).await
    }

    /// 记录一次不确定态超时后的自动重建/重播尝试
    pub async fn mark_broadcast_uncertain_rebroadcast_attempted(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_broadcast_uncertain_rebroadcast_attempted(pool.write_ref(), trade_no)
            .await
    }

    /// 原子放行一次 EVM uncertain 原始交易重播；不修改 raw/hash/nonce。
    pub async fn arm_uncertain_same_raw_rebroadcast_once(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::arm_uncertain_same_raw_rebroadcast_once(pool.write_ref(), trade_no).await
    }

    /// 清理不确定态追踪字段（广播可见/链上确认后）
    pub async fn clear_broadcast_uncertain_tracking(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::clear_broadcast_uncertain_tracking(pool.write_ref(), trade_no).await
    }

    /// 标记 Result ACK 尝试（行为事实）
    ///
    /// 语义：
    /// - 只记录第一次尝试时间（COALESCE 幂等写）
    /// - confirmed 之后不再变化
    /// - 这是"行为事实"，不是"推进事实"
    pub async fn mark_result_ack_attempted(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_result_ack_attempted(pool.write_ref(), trade_no).await
    }

    /// Confirm on-chain transaction finality (fact-based)
    ///
    /// Semantics:
    /// - On-chain transaction has been proven finalized
    /// - This is a fact write, NOT a state-machine transition
    /// - Idempotent
    ///
    /// Does NOT:
    /// - imply broadcast success
    /// - write finished_at
    /// - trigger side effects
    ///
    /// Who can write transaction_time:
    /// | Scenario             | Write transaction_time | Who writes         |
    /// | -------------------- | --------------------- | ------------------ |
    /// | Broadcast success    | ❌                     | No one             |
    /// | MQTT TxRes           | ❌                     | No one             |
    /// | Scanner chain check  | ✅                     | Scanner / Shadow   |
    /// | Recovery chain check | ❌                     | Use confirm_onchain_transaction_fact_with_recover |
    pub async fn confirm_onchain_transaction_fact(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        tx_hash: &str,
        transaction_time: &str,
        transaction_fee: &str,
        resource_consume: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::confirm_onchain_transaction_fact(
            pool.write_ref(),
            trade_no,
            tx_hash,
            transaction_time,
            transaction_fee,
            resource_consume,
        )
        .await
    }

    /// Confirm on-chain transaction finality with recover (fact-based)
    ///
    /// Semantics (RECOVER FACT COMPLETION):
    /// - On-chain transaction has been proven finalized via recover
    /// - This implies broadcast MUST have happened (behavior fact)
    /// - Atomically completes both behavior and chain facts
    /// - Idempotent
    ///
    /// Fact completion guarantee:
    /// - If transaction_time is set, last_broadcast_at MUST also be set
    /// - Uses COALESCE to preserve existing broadcast timestamps
    /// - Only updates when transaction_time IS NULL (幂等)
    ///
    /// Who can call this:
    /// | Scenario             | Can call | Reason               |
    /// | -------------------- | -------- | -------------------- |
    /// | Recovery chain check | ✅        | Recover fact completion |
    /// | Scanner chain check  | ❌        | Use regular confirm  |
    /// | Broadcast success    | ❌        | Use mark_broadcast_executed |
    pub async fn confirm_onchain_transaction_fact_with_recover(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        tx_hash: &str,
        last_broadcast_at: &str,
        transaction_time: &str,
        transaction_fee: &str,
        resource_consume: &str,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::confirm_onchain_transaction_fact_with_recover(
            pool.write_ref(),
            trade_no,
            tx_hash,
            last_broadcast_at,
            transaction_time,
            transaction_fee,
            resource_consume,
        )
        .await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// Repair-only: backfill tx_hash when local fact is missing but execution has progressed.
    ///
    /// Safety:
    /// - never overwrites non-empty tx_hash
    /// - requires transaction_time or last_broadcast_at to exist
    pub async fn backfill_tx_hash_if_missing(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        tx_hash: &str,
        source: &str,
    ) -> Result<u64, crate::Error> {
        let normalized = tx_hash.trim();
        if normalized.is_empty() {
            return Err(crate::Error::Other("tx_hash must not be empty".to_string()));
        }

        let before = Self::get_api_collect_by_trade_no(pool, trade_no).await.ok();
        let rows =
            ApiCollectDao::backfill_tx_hash_if_missing(pool.write_ref(), trade_no, normalized)
                .await?;
        let after = if rows > 0 {
            Self::get_api_collect_by_trade_no(pool, trade_no).await.ok()
        } else {
            None
        };

        tracing::warn!(
            trade_no = %trade_no,
            source = %source,
            tx_hash = %normalized,
            rows_affected = %rows,
            before_tx_hash = ?before.as_ref().and_then(|r| r.tx_hash.as_ref()),
            before_last_broadcast_at_present = %before.as_ref().and_then(|r| r.last_broadcast_at.as_ref()).is_some(),
            before_transaction_time_present = %before.as_ref().and_then(|r| r.transaction_time.as_ref()).is_some(),
            after_tx_hash = ?after.as_ref().and_then(|r| r.tx_hash.as_ref()),
            after_last_broadcast_at_present = %after.as_ref().and_then(|r| r.last_broadcast_at.as_ref()).is_some(),
            after_transaction_time_present = %after.as_ref().and_then(|r| r.transaction_time.as_ref()).is_some(),
            "backfill_tx_hash_if_missing attempted"
        );

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// 标记 Result ACK 确认（推进事实）
    ///
    /// 语义：
    /// - 只能在 attempted 之后调用
    /// - 防止重复确认
    pub async fn mark_result_ack_confirmed(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::mark_result_ack_confirmed(pool.write_ref(), trade_no).await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// 标记 Result ACK 已确认并标记链上终态（原子操作）
    ///
    /// 语义：
    /// - Result ACK 已成功发送到后端（result_ack_sent_at）
    /// - 同时标记链上终态（finished_at）
    /// - 单条 SQL 原子更新，防止 kill -9 产生"半完成事实"
    pub async fn mark_result_ack_confirmed_and_mark_chain_finished(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        let rows =
            ApiCollectDao::mark_result_ack_confirmed_and_chain_finished(pool.write_ref(), trade_no)
                .await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// 标记ACK尝试，并设置终态
    pub async fn mark_result_ack_sent(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::mark_result_ack_sent(pool.write_ref(), trade_no).await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// 标记手续费结果确认 ACK 已发送
    ///
    /// 语义：
    /// - 手续费结果确认 ACK 已成功发送到后端
    /// - 这是副作用完成的事实
    ///
    /// ⚠️ 调用约束：
    /// - 仅允许调用一次（tx_fee_res_ack_sent_at IS NULL）
    /// - 由 SideEffectWorker 调用
    pub async fn mark_tx_fee_res_ack_sent(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::mark_tx_fee_res_ack_sent(pool.write_ref(), trade_no).await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// 标记服务费上传尝试（行为事实）
    ///
    /// 语义：
    /// - 只记录第一次尝试时间（COALESCE 幂等写）
    /// - 上传成功后不再变化（WHERE service_fee_uploaded_at IS NULL）
    /// - 这是"行为事实"，不是"推进事实"
    /// - 由 SideEffectWorker 调用
    pub async fn mark_service_fee_attempted(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_service_fee_attempted(pool.write_ref(), trade_no).await
    }

    /// 标记已收到后端手续费订单
    ///
    /// 语义：
    /// - 已收到 AWM_ORDER_TRANS trade_type=3
    /// - 这是 collect 侧前置事实，不代表服务费已上传
    pub async fn mark_service_fee_order_received(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        let rows =
            ApiCollectDao::mark_service_fee_order_received(pool.write_ref(), trade_no).await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// 标记服务费已上传
    ///
    /// 语义：
    /// - 服务费记录已成功上传到后端
    /// - 这是副作用完成的事实
    ///
    /// ⚠️ 调用约束：
    /// - 仅允许在服务费已上传的前提下调用
    /// - 仅允许调用一次（service_fee_uploaded_at IS NULL）
    /// - 由 SideEffectWorker 调用
    pub async fn mark_service_fee_uploaded(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::mark_service_fee_uploaded(pool.write_ref(), trade_no).await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// 标记交易执行回执上传尝试（行为事实）
    ///
    /// 语义：
    /// - 只记录第一次尝试时间（COALESCE 幂等写）
    /// - 上传成功后不再变化（WHERE tx_exec_receipt_uploaded_at IS NULL）
    /// - 这是"行为事实"，不是"推进事实"
    /// - 由 SideEffectWorker 调用
    pub async fn mark_tx_exec_receipt_attempted(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_tx_exec_receipt_attempted(pool.write_ref(), trade_no).await
    }

    /// 标记交易执行回执已上传
    ///
    /// 语义：
    /// - 交易执行回执已成功上传到后端
    /// - 这是副作用完成的事实
    ///
    /// ⚠️ 调用约束：
    /// - 仅允许在回执已上传的前提下调用
    /// - 仅允许调用一次（tx_exec_receipt_uploaded_at IS NULL）
    /// - 由 SideEffectWorker 调用
    pub async fn mark_tx_exec_receipt_uploaded(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::mark_tx_exec_receipt_uploaded(pool.write_ref(), trade_no).await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// 扫描需要上传交易执行回执的交易
    ///
    /// 事实条件直接翻译：
    /// - last_broadcast_at IS NOT NULL：交易已成功广播
    /// - finished_at IS NULL：系统生命周期未结束
    /// - tx_exec_receipt_uploaded_at IS NULL：尚未上传执行回执
    pub async fn scan_need_tx_exec_receipt_upload(
        pool: &ApiTransactionDbPool,
        limit: usize,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::scan_need_tx_exec_receipt_upload(pool.read_ref(), limit).await
    }

    /// 标记订单 ACK 尝试（行为事实）
    ///
    /// 语义：
    /// - 只记录第一次尝试时间（COALESCE 幂等写）
    /// - 发送成功后不再变化（WHERE order_ack_sent_at IS NULL）
    /// - 这是"行为事实"，不是"推进事实"
    /// - 由 SideEffectWorker 调用
    pub async fn mark_order_ack_attempted(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::mark_order_ack_attempted(pool.write_ref(), trade_no).await
    }

    /// 标记订单 ACK 已发送（推进事实）
    ///
    /// 语义：
    /// - 订单 ACK 已成功发送到后端
    /// - 这是副作用完成的事实
    ///
    /// ⚠️ 调用约束：
    /// - 仅允许在订单 ACK 已尝试的前提下调用
    /// - 仅允许调用一次（order_ack_sent_at IS NULL）
    /// - 由 SideEffectWorker 调用
    pub async fn mark_order_ack_sent(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::mark_order_ack_sent(pool.write_ref(), trade_no).await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// 扫描需要发送订单 ACK 的交易
    ///
    /// 事实条件直接翻译：
    /// - order_ack_sent_at IS NULL：尚未发送订单 ACK
    ///
    /// ⚠️ 注意：
    /// - 仅基于推进事实判断，不依赖 attempted 行为中间态
    pub async fn scan_need_order_ack(
        pool: &ApiTransactionDbPool,
        limit: usize,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::scan_need_order_ack(pool.read_ref(), limit).await
    }

    /// 扫描需要恢复交易的记录
    ///
    /// 事实条件：
    /// - tx_hash IS NOT NULL
    /// - transaction_time IS NULL
    /// - last_broadcast_at IS NULL
    /// - tx_exec_receipt_uploaded_at IS NULL
    /// - finished_at IS NULL
    /// - err_code IS NULL
    ///
    /// ⚠️ 重要约束：
    /// - SQL必须100%等价于scanner中的need_recover predicate
    /// MUST be equivalent to scanner::need_recover()
    pub async fn scan_need_recover(
        pool: &ApiTransactionDbPool,
        limit: usize,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::scan_need_recover(pool.read_ref(), limit).await
    }

    /// 扫描可能卡住的交易
    ///
    /// 事实条件：
    /// - finished_at IS NULL：系统生命周期未结束
    /// - err_code IS NULL：无错误
    /// - created_at < now() - interval '5 minutes'：至少等待 5 分钟
    /// - (order_ack_sent_at IS NOT NULL OR raw_tx IS NOT NULL OR last_broadcast_at IS NOT NULL)：有一定进展
    ///
    /// ⚠️ 重要约束：
    /// - 只返回可能卡住的交易
    /// - 使用 LIMIT 控制返回数量
    /// - ORDER BY created_at 优先处理 older 的交易
    pub async fn scan_possible_stuck(
        pool: &ApiTransactionDbPool,
        limit: usize,
    ) -> Result<Vec<ApiCollectEntity>, crate::Error> {
        ApiCollectDao::scan_possible_stuck(pool.read_ref(), limit).await
    }

    /// 标记链上终态
    ///
    /// 语义：
    /// - 链上已确认不可逆（success / failure）
    /// - 系统生命周期结束
    ///
    /// ⚠️ 调用约束：
    /// - 仅允许在链上事实已确认的前提下调用（transaction_time IS NOT NULL）
    /// - 仅允许调用一次（finished_at IS NULL）
    /// - 由链终态确认模块调用
    pub async fn mark_chain_finished(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::mark_chain_finished(pool.write_ref(), trade_no).await?;

        Ok(rows)
    }

    /// 确认交易时间（如果不存在）
    ///
    /// 语义：
    /// - 只写入 transaction_time 字段
    /// - 仅当 transaction_time IS NULL 时才写入
    /// - 幂等
    /// - 用于 MQTT TxRes 等只知道最终结果已确认的场景
    pub async fn confirm_transaction_time_if_absent(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        transaction_time: &str,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::confirm_transaction_time_if_absent(
            pool.write_ref(),
            trade_no,
            transaction_time,
        )
        .await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// 重新计算并更新状态
    ///
    /// ⚠️ Repo 写事实铁律
    /// - 任何 *_at / raw_tx / tx_hash 等“事实字段”的成功写入
    /// - 必须紧随 recompute_and_update_status
    ///
    /// ❌ 禁止在 Worker / Scanner / Dispatcher 中直接写 status
    /// ✅ status 只能由 Repo 根据事实统一推导
    pub async fn recompute_and_update_status(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<(), crate::Error> {
        let entity = Self::get_api_collect_by_trade_no(pool, trade_no).await?;

        let new_status = entity.recompute_status();

        if entity.status != new_status {
            ApiCollectDao::update_status(pool.write_ref(), trade_no, new_status).await?;

            tracing::info!(
                trade_no = %trade_no,
                old_status = ?entity.status,
                new_status = ?new_status,
                "collect status recomputed"
            );
        }

        Ok(())
    }

    /// 作废当前 raw_tx 及其 tx_hash
    ///
    /// ⚠️ 设计铁律：
    /// - 一旦 raw_tx 被判定为不可再广播 / 不可再构建（如手续费不足、前置条件变化）
    /// - 必须同时清空 tx_hash
    /// - 并写入 need_service_fee = true 事实
    /// - 确保 scanner / recover 只基于有效事实工作
    ///
    /// 本方法是"事实回滚"，不是状态流转。
    /// 该方法不是重试控制，而是事实作废。
    ///
    /// ⚠️ 调用约束：
    /// - 仅允许对尚未广播的交易调用（transaction_time IS NULL）
    /// - status 仅用于错误标注，不得用于流程推进
    /// - 📌 必须检查返回值 rows_affected()：
    ///   * rows_affected() == 0：表示事实已变更，无需处理
    ///   * rows_affected() == 1：表示成功作废事实
    ///   * 不建议直接忽略返回值
    pub async fn invalidate_raw_tx_need_service_fee(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        status: Option<ApiCollectStatus>,
    ) -> Result<u64, crate::Error> {
        Self::invalidate_raw_tx(pool, trade_no, status).await
    }

    /// 作废当前 raw_tx/tx_hash，仅用于触发重建，不写 need_service_fee 事实。
    pub async fn invalidate_raw_tx_for_rebuild(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        status: Option<ApiCollectStatus>,
    ) -> Result<u64, crate::Error> {
        let before = Self::get_api_collect_by_trade_no(pool, trade_no).await.ok();

        let rows = ApiCollectDao::invalidate_raw_tx_for_rebuild(pool.write_ref(), trade_no, status)
            .await?;

        let after = if rows > 0 {
            Self::get_api_collect_by_trade_no(pool, trade_no).await.ok()
        } else {
            None
        };

        tracing::warn!(
            trade_no = %trade_no,
            requested_status = ?status,
            rows_affected = %rows,
            before_need_service_fee = ?before.as_ref().and_then(|r| r.need_service_fee),
            before_service_fee_uploaded_at = ?before.as_ref().and_then(|r| r.service_fee_uploaded_at.as_ref()),
            before_tx_fee_res_ack_sent_at = ?before.as_ref().and_then(|r| r.tx_fee_res_ack_sent_at.as_ref()),
            before_raw_tx_present = %before.as_ref().and_then(|r| r.raw_tx.as_ref()).is_some(),
            before_tx_hash_present = %before.as_ref().and_then(|r| r.tx_hash.as_ref()).is_some(),
            before_last_broadcast_at_present = %before.as_ref().and_then(|r| r.last_broadcast_at.as_ref()).is_some(),
            before_transaction_time_present = %before.as_ref().and_then(|r| r.transaction_time.as_ref()).is_some(),
            before_status = ?before.as_ref().map(|r| &r.status),
            after_need_service_fee = ?after.as_ref().and_then(|r| r.need_service_fee),
            after_service_fee_uploaded_at = ?after.as_ref().and_then(|r| r.service_fee_uploaded_at.as_ref()),
            after_tx_fee_res_ack_sent_at = ?after.as_ref().and_then(|r| r.tx_fee_res_ack_sent_at.as_ref()),
            after_raw_tx_present = %after.as_ref().and_then(|r| r.raw_tx.as_ref()).is_some(),
            after_tx_hash_present = %after.as_ref().and_then(|r| r.tx_hash.as_ref()).is_some(),
            after_last_broadcast_at_present = %after.as_ref().and_then(|r| r.last_broadcast_at.as_ref()).is_some(),
            after_transaction_time_present = %after.as_ref().and_then(|r| r.transaction_time.as_ref()).is_some(),
            after_status = ?after.as_ref().map(|r| &r.status),
            "invalidate_raw_tx_for_rebuild applied (rebuild-only invalidation, fee facts preserved)"
        );

        Ok(rows)
    }

    pub async fn invalidate_raw_tx(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        status: Option<ApiCollectStatus>,
    ) -> Result<u64, crate::Error> {
        let before = Self::get_api_collect_by_trade_no(pool, trade_no).await.ok();

        let rows =
            ApiCollectDao::invalidate_raw_tx_need_service_fee(pool.write_ref(), trade_no, status)
                .await?;

        let after = if rows > 0 {
            Self::get_api_collect_by_trade_no(pool, trade_no).await.ok()
        } else {
            None
        };

        tracing::warn!(
            trade_no = %trade_no,
            requested_status = ?status,
            rows_affected = %rows,
            before_need_service_fee = ?before.as_ref().and_then(|r| r.need_service_fee),
            before_service_fee_uploaded_at = ?before.as_ref().and_then(|r| r.service_fee_uploaded_at.as_ref()),
            before_tx_fee_res_ack_sent_at = ?before.as_ref().and_then(|r| r.tx_fee_res_ack_sent_at.as_ref()),
            before_raw_tx_present = %before.as_ref().and_then(|r| r.raw_tx.as_ref()).is_some(),
            before_tx_hash_present = %before.as_ref().and_then(|r| r.tx_hash.as_ref()).is_some(),
            before_last_broadcast_at_present = %before.as_ref().and_then(|r| r.last_broadcast_at.as_ref()).is_some(),
            before_transaction_time_present = %before.as_ref().and_then(|r| r.transaction_time.as_ref()).is_some(),
            before_status = ?before.as_ref().map(|r| &r.status),
            after_need_service_fee = ?after.as_ref().and_then(|r| r.need_service_fee),
            after_service_fee_uploaded_at = ?after.as_ref().and_then(|r| r.service_fee_uploaded_at.as_ref()),
            after_tx_fee_res_ack_sent_at = ?after.as_ref().and_then(|r| r.tx_fee_res_ack_sent_at.as_ref()),
            after_raw_tx_present = %after.as_ref().and_then(|r| r.raw_tx.as_ref()).is_some(),
            after_tx_hash_present = %after.as_ref().and_then(|r| r.tx_hash.as_ref()).is_some(),
            after_last_broadcast_at_present = %after.as_ref().and_then(|r| r.last_broadcast_at.as_ref()).is_some(),
            after_transaction_time_present = %after.as_ref().and_then(|r| r.transaction_time.as_ref()).is_some(),
            after_status = ?after.as_ref().map(|r| &r.status),
            "invalidate_raw_tx applied (fee cycle facts reset on reopen)"
        );

        Ok(rows)
    }

    /// 作废广播后失联的 raw_tx 及其 tx_hash，用于重新构建并重广播。
    pub async fn invalidate_raw_tx_for_rebroadcast(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        status: Option<ApiCollectStatus>,
    ) -> Result<u64, crate::Error> {
        let rows =
            ApiCollectDao::invalidate_raw_tx_for_rebroadcast(pool.write_ref(), trade_no, status)
                .await?;

        if rows > 0 {
            Self::recompute_and_update_status(pool, trade_no).await?;
        }

        Ok(rows)
    }

    /// 清除构建阻断标记
    ///
    /// ⚠️ 设计约束：
    /// - 仅允许在"外部事实已发生"的前提下调用（如 fee 到账）
    /// - 本方法不会构建 raw_tx，只是解除构建阻断
    /// - 语义是：解除"不可构建"的事实，允许重新构建
    ///
    /// ⚠️ 调用约定：
    /// - 必须由产生新事实的一方调用（如 fee mqtt 处理器）
    /// - 禁止在 scanner / worker / retry 逻辑中调用
    pub async fn resolve_need_service_fee(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiCollectDao::resolve_need_service_fee(pool.write_ref(), trade_no).await
    }

    /// 清除服务费需求标记（recover 专用）
    ///
    /// 语义：
    /// - 修复“手续费不足”这一事实，使交易重新具备构建条件
    /// - 不做任何状态回滚，不保证一定继续推进
    ///
    /// 调用场景：
    /// - 手续费问题已解决，需要重新构建交易
    pub async fn clear_need_service_fee(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<u64, crate::Error> {
        let rows = ApiCollectDao::clear_need_service_fee(pool.write_ref(), trade_no).await?;
        tracing::info!(
            trade_no = %trade_no,
            rows_affected = %rows,
            "clear_need_service_fee applied"
        );
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::ApiCollectRepo;
    use crate::{
        dao::api_collect::ApiCollectDao,
        entities::{
            api_collect::{ApiCollectStatus, CollectCreatedFact},
            api_resource_delegation::{
                ApiResourceDelegationResultStatus, NewApiResourceDelegation,
            },
            api_resource_gate::{
                ApiResourceBlockReason, ApiResourceDependencyType, ApiResourceGateResult,
            },
            api_trade_type::ApiTradeType,
        },
        error::Error,
        repositories::{
            api_wallet::resource_delegation::ApiResourceDelegationRepo,
            test_helper::{setup_api_resource_pool, setup_api_transaction_pool},
        },
    };

    #[tokio::test]
    async fn collect_upsert_and_get_success() {
        let pool = setup_api_transaction_pool("wallet_db_collect_success").await;
        let trade_no = "collect_trade_success_1";
        let from_addr = "0xfrom_collect_s";
        let to_addr = "0xto_collect_s";

        ApiCollectRepo::upsert_api_collect(
            &pool,
            "u1",
            "collect_name",
            from_addr,
            to_addr,
            "100",
            "v",
            wallet_types::constant::chain_code::ETHEREUM,
            None,
            "ETH",
            trade_no,
            0,
            ApiCollectStatus::Init,
            0,
        )
        .await
        .unwrap();

        let got = ApiCollectRepo::get_api_collect_by_trade_no(&pool, trade_no).await.unwrap();
        assert_eq!(got.trade_no, trade_no);
        assert_eq!(got.from_addr, from_addr);
        assert_eq!(got.to_addr, to_addr);
        assert_eq!(got.symbol, "ETH");
        assert_eq!(got.value, "100");
        assert_eq!(got.status, ApiCollectStatus::Init);

        let (count, rows) =
            ApiCollectRepo::page_api_collect_with_status(&pool, 1, 20, &[ApiCollectStatus::Init])
                .await
                .unwrap();
        assert_eq!(count, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].trade_no, trade_no);
    }

    #[tokio::test]
    async fn collect_missing_trade_no_returns_database_error() {
        let pool = setup_api_transaction_pool("wallet_db_collect_edge").await;
        let err = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "collect_missing_trade_no")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Database(_)));
    }

    #[tokio::test]
    async fn collect_tx_rollback_keeps_db_unchanged() {
        let pool = setup_api_transaction_pool("wallet_db_collect_rollback").await;
        let trade_no = "collect_trade_rollback_1";

        let mut tx = pool.write_ref().begin().await.unwrap();
        let fact = CollectCreatedFact {
            uid: Some("u2".to_string()),
            name: "collect_rb".to_string(),
            from_addr: "0xfrom_collect_rb".to_string(),
            to_addr: "0xto_collect_rb".to_string(),
            symbol: "ETH".to_string(),
            value: "88".to_string(),
            validate: "v".to_string(),
            chain_code: wallet_types::constant::chain_code::ETHEREUM.to_string(),
            token_addr: crate::entities::asset_token_key::AssetTokenKey::Native,
            trade_no: trade_no.to_string(),
            trade_type: 0,
            risk_addr: "0".to_string(),
            status: ApiCollectStatus::Init,
        };
        ApiCollectDao::add(tx.as_mut(), fact).await.unwrap();
        tx.rollback().await.unwrap();

        let got = ApiCollectRepo::get_api_collect_by_trade_no(&pool, trade_no).await;
        assert!(matches!(got, Err(Error::Database(_))));

        let (count, rows) =
            ApiCollectRepo::page_api_collect_with_status(&pool, 1, 20, &[ApiCollectStatus::Init])
                .await
                .unwrap();
        assert_eq!(count, 0);
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn collect_resource_gate_records_block_and_release_facts() {
        let pool = setup_api_transaction_pool("wallet_db_collect_resource_gate").await;
        let trade_no = "collect_resource_gate_1";

        ApiCollectRepo::upsert_api_collect(
            &pool,
            "u1",
            "collect_name",
            "from",
            "to",
            "100",
            "v",
            wallet_types::constant::chain_code::TRON,
            None,
            "TRX",
            trade_no,
            2,
            ApiCollectStatus::Init,
            0,
        )
        .await
        .unwrap();

        ApiCollectRepo::mark_resource_blocked(
            &pool,
            trade_no,
            ApiResourceBlockReason::NeedPlatformDelegate,
            Some("resource_trade_1"),
            Some(ApiResourceDependencyType::PlatformDelegate),
        )
        .await
        .unwrap();

        let blocked = ApiCollectRepo::get_api_collect_by_trade_no(&pool, trade_no).await.unwrap();
        assert!(blocked.resource_check_at.is_some());
        assert!(blocked.resource_gate_released_at.is_none());
        assert_eq!(
            blocked.resource_block_reason,
            Some(ApiResourceBlockReason::NeedPlatformDelegate)
        );
        assert_eq!(blocked.resource_dependency_trade_no.as_deref(), Some("resource_trade_1"));
        assert_eq!(
            blocked.resource_dependency_type,
            Some(ApiResourceDependencyType::PlatformDelegate)
        );

        ApiCollectRepo::mark_resource_released(
            &pool,
            trade_no,
            ApiResourceGateResult::ResourceReady,
        )
        .await
        .unwrap();

        let released = ApiCollectRepo::get_api_collect_by_trade_no(&pool, trade_no).await.unwrap();
        assert!(released.resource_gate_released_at.is_some());
        assert_eq!(released.resource_gate_result, Some(ApiResourceGateResult::ResourceReady));
        assert!(released.resource_block_reason.is_none());
    }

    #[tokio::test]
    async fn collect_account_activation_clear_requeues_resource_gate() {
        let pool = setup_api_transaction_pool("wallet_db_collect_activation_recheck").await;
        let trade_no = "collect_activation_recheck_1";
        let activation_trade_no = "act_collect_activation_recheck_1";

        ApiCollectRepo::upsert_api_collect(
            &pool,
            "u1",
            "collect_name",
            "from",
            "to",
            "100",
            "v",
            wallet_types::constant::chain_code::TRON,
            None,
            "TRX",
            trade_no,
            2,
            ApiCollectStatus::Init,
            0,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_collect
             SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE trade_no = ?",
        )
        .bind(trade_no)
        .execute(pool.as_ref())
        .await
        .unwrap();

        ApiCollectRepo::mark_resource_blocked(
            &pool,
            trade_no,
            ApiResourceBlockReason::NeedAccountActivation,
            Some(activation_trade_no),
            Some(ApiResourceDependencyType::AccountActivation),
        )
        .await
        .unwrap();

        let rows = ApiCollectRepo::clear_account_activation_block_for_reeval(
            &pool,
            trade_no,
            activation_trade_no,
        )
        .await
        .unwrap();
        assert_eq!(rows, 1);

        let collect = ApiCollectRepo::get_api_collect_by_trade_no(&pool, trade_no).await.unwrap();
        assert!(collect.resource_gate_released_at.is_none());
        assert!(collect.resource_gate_result.is_none());
        assert!(collect.resource_block_reason.is_none());
        assert!(collect.resource_dependency_trade_no.is_none());
        assert!(collect.resource_dependency_type.is_none());

        let need_resource_gate = ApiCollectRepo::scan_need_resource_gate(&pool, 10).await.unwrap();
        assert_eq!(need_resource_gate.len(), 1);
        assert_eq!(need_resource_gate[0].trade_no, trade_no);
    }

    #[tokio::test]
    async fn mark_resource_result_ack_sent_and_release_collect_gate_updates_facts_together() {
        let pool =
            setup_api_transaction_pool("wallet_db_collect_resource_result_ack_release").await;
        let resource_pool =
            setup_api_resource_pool("wallet_db_collect_resource_result_ack_release").await;
        let trade_no = "collect_resource_result_ack_release_1";

        ApiCollectRepo::upsert_api_collect(
            &pool,
            "u1",
            "collect_name",
            "from",
            "to",
            "100",
            "v",
            wallet_types::constant::chain_code::TRON,
            None,
            "USDT",
            trade_no,
            2,
            ApiCollectStatus::Init,
            0,
        )
        .await
        .unwrap();

        ApiCollectRepo::mark_resource_blocked(
            &pool,
            trade_no,
            ApiResourceBlockReason::NeedPlatformDelegate,
            Some(trade_no),
            Some(ApiResourceDependencyType::PlatformDelegate),
        )
        .await
        .unwrap();

        ApiResourceDelegationRepo::upsert_original_order_result_fact(
            &resource_pool,
            NewApiResourceDelegation::platform_delegate(
                "u1",
                trade_no,
                trade_no,
                ApiTradeType::Collect as i64,
                "",
                "to",
                "0",
            ),
            ApiResourceDelegationResultStatus::Fail,
            Some(3),
            Some(r#"{"tradeNo":"collect_resource_result_ack_release_1","status":false}"#),
        )
        .await
        .unwrap();

        let ack_rows = ApiResourceDelegationRepo::mark_result_ack_sent(&resource_pool, trade_no)
            .await
            .unwrap();
        assert_eq!(ack_rows, 1);

        let release_rows = ApiCollectRepo::mark_resource_released(
            &pool,
            trade_no,
            ApiResourceGateResult::ResourceDelegationFailedBypass,
        )
        .await
        .unwrap();
        assert_eq!(release_rows, 1);

        let resource_task =
            ApiResourceDelegationRepo::get_by_resource_trade_no(&resource_pool, trade_no)
                .await
                .unwrap();
        assert!(resource_task.result_ack_sent_at.is_some());

        let released = ApiCollectRepo::get_api_collect_by_trade_no(&pool, trade_no).await.unwrap();
        assert!(released.resource_gate_released_at.is_some());
        assert_eq!(
            released.resource_gate_result,
            Some(ApiResourceGateResult::ResourceDelegationFailedBypass)
        );
        assert!(released.resource_block_reason.is_none());

        let ack_rows = ApiResourceDelegationRepo::mark_result_ack_sent(&resource_pool, trade_no)
            .await
            .unwrap();
        assert_eq!(ack_rows, 0);

        let still_released =
            ApiCollectRepo::get_api_collect_by_trade_no(&pool, trade_no).await.unwrap();
        assert_eq!(
            still_released.resource_gate_result,
            Some(ApiResourceGateResult::ResourceDelegationFailedBypass)
        );
        assert!(still_released.resource_gate_released_at.is_some());
    }
}

#[cfg(test)]
#[path = "collect_nonce_conflict_tests.rs"]
mod collect_nonce_conflict_tests;

#[cfg(test)]
#[path = "collect_recover_hash_tests.rs"]
mod collect_recover_hash_tests;
