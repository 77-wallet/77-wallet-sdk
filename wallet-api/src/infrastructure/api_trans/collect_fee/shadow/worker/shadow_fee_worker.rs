// collect_fee/shadow/worker/shadow_fee_worker.rs
#![allow(deprecated)]

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::{error, info, warn};
use wallet_database::{
    ApiTransactionDbPool,
    entities::api_fee::{ApiFeeEntity, ErrCode},
    repositories::api_wallet::fee::{ApiFeeRepo, SignedFeeCandidate},
};
use wallet_types::chain::chain::ChainCode;
use wallet_utils::RetryableError as _;

use crate::{
    domain::api_wallet::{
        adapter::tx::RawTx,
        adapter_factory::ApiChainAdapterFactory,
        coin::ApiCoinDomain,
        trans::{ApiTransDomain, RecoveredTxObservation, broadcast_mechanism, shared_mechanism},
        wallet::ApiWalletDomain,
    },
    error::{
        business::api_wallet::{ApiWalletError, trans::TransError},
        service::ServiceError,
        system::SystemError,
    },
    infrastructure::api_trans::{
        AddressLockManager, collect_fee::shadow::ShadowScanner, nonce_recovery_mechanism,
        shadow_rpc_policy,
    },
    request::api_wallet::trans::{ApiBaseTransferReq, ApiTransferReq},
};

/// ShadowFeeWorker 命令
#[derive(Debug, Clone)]
pub enum ShadowFeeCommand {
    /// 构建交易
    BuildTx(String),
    /// 广播交易
    Broadcast(String),
    /// 恢复交易
    Recover(String),
}

/// ShadowFeeWorker
///
/// 负责处理链相关操作：
/// - 构建交易
/// - 广播交易
///
/// ShadowFeeWorker design invariant:
///
/// Phase 1: Address lock + fact arbitration (no network)
/// - 地址锁内进行并发裁决
/// - 占用构建槽；候选 nonce 只读观察
/// - 锁内禁止任何网络调用、sleep、await RPC
/// - 裁决依据必须基于锁内 fresh read
///
/// Phase 2: Network execution (no shared state)
/// - 锁外执行网络/RPC/构建/广播
/// - chain_rpc_guard 只限制外部世界并发
/// - 允许失败和重试
///
/// Phase 3: Address lock + irreversible fact commit
/// - 地址锁内提交不可逆事实
/// - 更新数据库状态
/// - 唤醒扫描器
///
/// 🔒 核心原则：
/// - 锁只保护地址级共享资源
/// - 锁内只做瞬时、确定的操作
/// - nonce 从"动态信息"升级为"已裁决事实"
/// - chain_rpc_guard 作为 RPC 压力阀
pub struct ShadowFeeWorker {
    ctx: &'static crate::context::Context,
    address_locks: Arc<AddressLockManager>,
    /// ShadowScanner 引用，用于直接调用 try_advance
    scanner: Arc<ShadowScanner>,
}

impl ShadowFeeWorker {
    const TRON_MISSING_CONFIRMED_AND_PENDING_REBROADCAST_TIMEOUT_SECS: i64 = 5 * 60;
    const EVM_UNCERTAIN_TIMEOUT_SECS: i64 = 5 * 60;
    const EVM_UNCERTAIN_BACKOFF_MID_SECS: i64 = 15;
    const EVM_UNCERTAIN_BACKOFF_MAX_SECS: i64 = 30;
    const BUILD_SLOT_STALE_SECS: i64 = 30;
    const TRON_POST_BROADCAST_CONFIRM_DELAYS_SECS: [u64; 5] = [3, 5, 8, 15, 30];

    fn tron_post_broadcast_confirm_eligible(
        chain_code: &str,
        transaction_time_present: bool,
        err_code_present: bool,
    ) -> bool {
        shared_mechanism::is_tron_chain(chain_code)
            && !transaction_time_present
            && !err_code_present
    }

    fn reset_tron_post_broadcast_recover_cooldown(chain_code: &str, trade_no: &str) -> bool {
        if !shared_mechanism::is_tron_chain(chain_code) {
            return false;
        }
        let recover_key = shadow_rpc_policy::recover_dispatch_key(trade_no);
        shadow_rpc_policy::reset_recover_dispatch(&recover_key)
    }

    fn schedule_tron_post_broadcast_confirms_if_needed(&self, fee: &ApiFeeEntity) {
        if !Self::tron_post_broadcast_confirm_eligible(
            &fee.chain_code,
            fee.transaction_time.is_some(),
            fee.err_code.is_some(),
        ) {
            return;
        }

        let trade_no = fee.trade_no.clone();
        let tx_hash = fee.tx_hash.as_deref().unwrap_or_default().to_string();
        for delay_secs in Self::TRON_POST_BROADCAST_CONFIRM_DELAYS_SECS {
            let scanner = self.scanner.clone();
            let trade_no = trade_no.clone();
            let tx_hash = tx_hash.clone();
            info!(
                observability_event = "tron_fee_post_broadcast_confirm_scheduled",
                trade_no = %trade_no,
                tx_hash = %tx_hash,
                delay_secs,
                source = "shadow_fee_worker",
                "Scheduled TRON fee post-broadcast confirmation"
            );
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                info!(
                    observability_event = "tron_fee_post_broadcast_confirm_triggered",
                    trade_no = %trade_no,
                    tx_hash = %tx_hash,
                    delay_secs,
                    source = "shadow_fee_worker",
                    "Triggering TRON fee post-broadcast confirmation"
                );
                scanner.try_advance(&trade_no).await;
            });
        }
    }

    pub(crate) fn is_solana_recipient_rent_error(err: &ServiceError) -> bool {
        let msg = err.to_string();
        msg.contains("recipient account is not initialized")
            || msg.contains("rent-exempt minimum")
            || msg.contains("insufficient funds for rent")
    }

    fn evm_uncertain_backoff_secs(retry_count: u32) -> i64 {
        shared_mechanism::broadcast_uncertain_backoff_secs(
            retry_count,
            Self::EVM_UNCERTAIN_BACKOFF_MID_SECS,
            Self::EVM_UNCERTAIN_BACKOFF_MAX_SECS,
        )
    }

    fn evm_uncertain_elapsed_secs(req: &ApiFeeEntity, now: chrono::DateTime<Utc>) -> Option<i64> {
        shared_mechanism::broadcast_uncertain_elapsed_secs(req.broadcast_uncertain_since_at, now)
    }

    fn is_stale_build_slot(building_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
        shared_mechanism::is_stale_slot(building_at, Self::BUILD_SLOT_STALE_SECS, now)
    }

    fn should_throttle_evm_uncertain_recover(
        req: &ApiFeeEntity,
        now: chrono::DateTime<Utc>,
    ) -> bool {
        shared_mechanism::should_throttle_broadcast_uncertain_recover(
            req.broadcast_uncertain_since_at,
            req.broadcast_uncertain_last_checked_at,
            req.broadcast_uncertain_retry_count,
            Self::EVM_UNCERTAIN_TIMEOUT_SECS,
            Self::EVM_UNCERTAIN_BACKOFF_MID_SECS,
            Self::EVM_UNCERTAIN_BACKOFF_MAX_SECS,
            now,
        )
    }

    fn should_rebroadcast_tron_missing_confirmed_and_pending(req: &ApiFeeEntity) -> bool {
        shared_mechanism::should_retry_after_tron_missing_confirmed_and_pending(
            &req.chain_code,
            req.last_broadcast_at,
            req.broadcast_uncertain_since_at,
            Self::TRON_MISSING_CONFIRMED_AND_PENDING_REBROADCAST_TIMEOUT_SECS,
            Utc::now(),
        )
    }

    pub fn new(
        ctx: &'static crate::context::Context,
        address_locks: Arc<AddressLockManager>,
        scanner: Arc<ShadowScanner>,
    ) -> Self {
        Self { ctx, address_locks, scanner }
    }
    /// 处理命令
    pub async fn handle(&self, command: ShadowFeeCommand) -> Result<(), ServiceError> {
        match command {
            ShadowFeeCommand::BuildTx(trade_no) => self.process_build_tx(trade_no).await,
            ShadowFeeCommand::Broadcast(trade_no) => self.process_broadcast(trade_no).await,
            ShadowFeeCommand::Recover(trade_no) => self.process_recover(trade_no).await,
        }
    }

    /// 执行 Recover Command - 外层wrapper，确保所有错误都被捕获
    async fn process_recover(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "shadow_fee_worker", "Processing Recover command");

        // 使用内层函数来捕获所有错误
        if let Err(err) = self.process_recover_inner(&trade_no).await {
            error!(trade_no = %trade_no, error = %err, source = "shadow_fee_worker", "Recover inner failed, handling error");
            if crate::domain::api_wallet::trans::is_confirmed_recover_chain_failure(&err) {
                self.handle_fee_tx_failed(&trade_no, err).await?;
            } else {
                warn!(trade_no = %trade_no, error = %err, "Recover observation failed; keep fee pending for retry");
            }
        }

        Ok(())
    }

    /// Recover 内部实现，可能返回错误
    async fn process_recover_inner(&self, trade_no: &str) -> Result<(), ServiceError> {
        // ====== phase 1: 锁内 · 并发裁决 ======
        // ⚠️ 锁内禁止任何网络调用、sleep、await RPC
        let req = {
            // 获取地址锁，保护地址级并发
            let initial_req = self.get_fee_entity(trade_no).await?;
            let _addr_guard = self.address_locks.acquire(&initial_req.from_addr).await;
            info!(trade_no = %trade_no, source = "shadow_fee_worker", "Acquired address lock");

            // 🔒 必须锁内重新读取，确保基于最新状态做决策
            // ⚠️ 只读"裁决字段"，不做任何业务推断
            let fresh_req = self.get_fee_entity(trade_no).await?;

            // 事实校验：Recover 只能处理 tx_hash 存在且 transaction_time 为空的交易
            // ⚠️ 这里是并发裁决的关键，确保只有一个task能通过
            if fresh_req.tx_hash.is_none()
                || fresh_req.transaction_time.is_some()
                || fresh_req.tx_exec_receipt_uploaded_at.is_some()
            {
                info!(trade_no = %trade_no, source = "shadow_fee_worker", "tx_hash empty or transaction_time exists, skipping Recover");
                return Ok(());
            }

            if shared_mechanism::tracks_broadcast_uncertain_state(&fresh_req.chain_code)
                && fresh_req.raw_tx.is_some()
                && fresh_req.last_broadcast_at.is_none()
                && fresh_req.broadcast_uncertain_since_at.is_none()
                && fresh_req.transaction_time.is_none()
                && fresh_req.tx_exec_receipt_uploaded_at.is_none()
            {
                info!(
                    trade_no = %trade_no,
                    tx_hash = %fresh_req.tx_hash.as_deref().unwrap_or_default(),
                    nonce = fresh_req.nonce,
                    source = "shadow_fee_worker",
                    "Skip Recover: raw_tx exists but not in uncertain state; broadcast should proceed"
                );
                return Ok(());
            }

            fresh_req
        };
        // 🔓 锁在这里已经释放

        if shared_mechanism::is_evm_chain(&req.chain_code) {
            let now = Utc::now();
            if Self::should_throttle_evm_uncertain_recover(&req, now) {
                let elapsed = Self::evm_uncertain_elapsed_secs(&req, now).unwrap_or_default();
                let wait_secs =
                    Self::evm_uncertain_backoff_secs(req.broadcast_uncertain_retry_count);
                let since_last = req
                    .broadcast_uncertain_last_checked_at
                    .map(|ts| now.signed_duration_since(ts).num_seconds().max(0))
                    .unwrap_or_default();
                info!(
                    observability_event = "shadow_recover_skipped",
                    reason = "evm_uncertain_backoff",
                    trade_no = %req.trade_no,
                    tx_hash = %req.tx_hash.as_deref().unwrap_or_default(),
                    nonce = req.nonce,
                    retry_count = req.broadcast_uncertain_retry_count,
                    elapsed_secs = elapsed,
                    since_last_check_secs = since_last,
                    backoff_wait_secs = wait_secs,
                    source = "shadow_fee_worker",
                    "Skip recover query due to EVM uncertain backoff"
                );
                return Ok(());
            }
        }

        // 执行恢复交易。raw wrapper 只处理外层 RPC guard/timeout，保留 recover 内层 Result。
        let recover_result =
            match crate::infrastructure::chain_rpc_guard::run_guarded_raw_with_ctx_and_log(
                self.ctx,
                &req.chain_code,
                trade_no,
                "recover",
                "shadow_fee_worker",
                crate::infrastructure::chain_rpc_guard::ChainRpcFailureLog::new()
                    .trade_no(trade_no)
                    .tx_hash(req.tx_hash.as_deref().unwrap_or_default())
                    .chain_code(&req.chain_code)
                    .source("shadow_fee_worker")
                    .op("recover"),
                self.recover_tx(&req),
            )
            .await?
            {
                Some(result) => result,
                None => return Ok(()),
            };
        self.apply_recover_observation(&req, recover_result).await
    }

    async fn apply_recover_observation(
        &self,
        req: &ApiFeeEntity,
        recover_result: Result<RecoveredTxObservation, ServiceError>,
    ) -> Result<(), ServiceError> {
        let trade_no = req.trade_no.as_str();
        match recover_result {
            Ok(RecoveredTxObservation::QueryUncertain) => {
                info!(trade_no = %trade_no, "Fee receipt query uncertain; preserve facts and retry");
                return Ok(());
            }
            Ok(RecoveredTxObservation::Confirmed(tx_resp)) => {
                info!(trade_no = %trade_no, tx_hash = %tx_resp.tx_hash, source = "shadow_fee_worker", "Transaction recover successful");

                // ====== phase 3: 锁内 · 提交不可逆事实 ======
                {
                    // 重新获取地址锁，保护地址级并发
                    let _addr_guard = self.address_locks.acquire(&req.from_addr).await;
                    info!(trade_no = %trade_no, source = "shadow_fee_worker", "Reacquired address lock for fact commit");

                    // 🔒 必须锁内重新读取，确保基于最新状态做决策
                    let fresh_req = self.get_fee_entity(trade_no).await?;

                    let existing_tx_hash =
                        shared_mechanism::normalized_tx_hash(fresh_req.tx_hash.as_deref());

                    if fresh_req.transaction_time.is_some() {
                        if existing_tx_hash.is_none() {
                            let rows_affected = ApiFeeRepo::backfill_tx_hash_if_missing(
                                &self.ctx.api_transaction_pool()?,
                                &fresh_req.trade_no,
                                &tx_resp.tx_hash,
                                "shadow_fee_worker",
                            )
                            .await
                            .map_err(|e| ServiceError::Database(e.into()))?;

                            info!(
                                trade_no = %fresh_req.trade_no,
                                tx_hash = %tx_resp.tx_hash,
                                rows_affected = %rows_affected,
                                source = "shadow_fee_worker",
                                "Recovered tx hash backfilled after concurrent missing-hash read"
                            );
                        }

                        info!(trade_no = %trade_no, source = "shadow_fee_worker", "tx_hash empty or transaction_time exists, skipping Recover fact commit");
                        info!(
                            trade_no = %trade_no,
                            source = "shadow_fee_worker",
                            "Recover facts already present; nudging scanner for downstream stages"
                        );
                        self.scanner.try_advance(&fresh_req.trade_no).await;
                        return Ok(());
                    }

                    // 🔒 事实保护：检查 tx_hash 一致性，防止事实被覆盖
                    shared_mechanism::validate_recovered_tx_hash(
                        &fresh_req.trade_no,
                        existing_tx_hash,
                        &tx_resp.tx_hash,
                        "shadow_fee_worker",
                    )?;

                    let resource_consume = if let Some(consumer) = tx_resp.consumer {
                        consumer.energy_used.to_string()
                    } else {
                        "0".to_string()
                    };

                    // 使用链上时间设置 transaction_time
                    // 必须使用链返回的时间，禁止使用本地时间作为后备
                    let transaction_time_ms = tx_resp.transaction_time_ms.ok_or_else(|| {
                        ServiceError::System(SystemError::Internal(
                            "recover_tx returned final result but missing transaction_time_ms"
                                .to_string(),
                        ))
                    })?;

                    // 将毫秒转换为ISO 8601格式
                    let transaction_time =
                        chrono::DateTime::<Utc>::from_timestamp_millis(transaction_time_ms as i64)
                            .ok_or_else(|| {
                                ServiceError::System(SystemError::Internal(
                                    "invalid transaction_time_ms from chain".to_string(),
                                ))
                            })?
                            .to_rfc3339();
                    let rows_affected = ApiFeeRepo::confirm_onchain_transaction_fact_with_recover(
                        &self.ctx.api_transaction_pool()?,
                        &fresh_req.trade_no,
                        &tx_resp.tx_hash,
                        &transaction_time,
                        &transaction_time,
                        &fresh_req.transaction_fee,
                        &resource_consume,
                    )
                    .await
                    .map_err(|e| ServiceError::Database(e.into()))?;

                    // 显式处理幂等情况：恢复已被其他并发执行
                    if rows_affected == 0 {
                        info!(
                            trade_no = %fresh_req.trade_no,
                            tx_hash = %tx_resp.tx_hash,
                            source = "shadow_fee_worker",
                            "update_after_recover skipped: recover already executed (idempotent hit)"
                        );
                    } else {
                        // 直接调用 try_advance 进行点对点唤醒
                        self.scanner.try_advance(&fresh_req.trade_no).await;
                    }
                }
            }
            Ok(RecoveredTxObservation::Missing) => {
                info!(trade_no = %trade_no, source = "shadow_fee_worker", "Transaction recover result is uncertain");
                if let Some(raw_tx_json) = req.raw_tx.as_deref() {
                    if shared_mechanism::should_invalidate_expired_tron_raw_for_recover(
                        &req.chain_code,
                        raw_tx_json,
                        req.last_broadcast_at.is_some(),
                        req.tx_hash.as_deref().is_some_and(|tx_hash| !tx_hash.trim().is_empty()),
                        Utc::now().timestamp_millis(),
                    ) {
                        warn!(
                            trade_no = %req.trade_no,
                            tx_hash = %req.tx_hash.as_deref().unwrap_or_default(),
                            source = "shadow_fee_worker",
                            "Detected expired tron raw_tx during recover; invalidating stale tx facts"
                        );
                        let rows = ApiFeeRepo::invalidate_raw_tx(
                            &self.ctx.api_transaction_pool()?,
                            &req.trade_no,
                            None,
                            None,
                            None,
                        )
                        .await
                        .map_err(|e| ServiceError::Database(e.into()))?;
                        if rows > 0 {
                            self.scanner.try_advance(&req.trade_no).await;
                        }
                        return Ok(());
                    }
                }

                if !shared_mechanism::tracks_broadcast_uncertain_state(&req.chain_code) {
                    // 查链不确定（含链上查不到 hash）后，立即尝试推进一次；
                    // 若满足广播条件会直接进入 Broadcast 重试，避免纯等待下一轮定时扫描。
                    self.scanner.try_advance(trade_no).await;
                    return Ok(());
                }

                let now = Utc::now();
                let rows_affected = ApiFeeRepo::mark_broadcast_uncertain_attempt(
                    &self.ctx.api_transaction_pool()?,
                    trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                let refreshed = self.get_fee_entity(trade_no).await?;
                info!(
                    observability_event = "shadow_broadcast_uncertain_recorded",
                    reason = "recover_result_uncertain",
                    trade_no = %refreshed.trade_no,
                    tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                    nonce = refreshed.nonce,
                    rows_affected = %rows_affected,
                    retry_count = refreshed.broadcast_uncertain_retry_count,
                    uncertain_since_at = ?refreshed.broadcast_uncertain_since_at,
                    reconciled_at = ?refreshed.broadcast_uncertain_reconciled_at,
                    rebroadcast_count = refreshed.broadcast_uncertain_rebroadcast_count,
                    source = "shadow_fee_worker",
                    "Recover uncertain state recorded"
                );

                if !shared_mechanism::is_evm_chain(&refreshed.chain_code) {
                    return Ok(());
                }

                let elapsed_secs =
                    Self::evm_uncertain_elapsed_secs(&refreshed, now).unwrap_or_default();
                if elapsed_secs >= Self::EVM_UNCERTAIN_TIMEOUT_SECS {
                    warn!(trade_no = %refreshed.trade_no, uncertain_duration_sec = elapsed_secs,
                        "EVM fee receipt still missing; preserve signed transaction and keep observing");
                }
            }
            Err(err) if shared_mechanism::is_tron_missing_confirmed_and_pending_error(&err) => {
                if !Self::should_rebroadcast_tron_missing_confirmed_and_pending(&req) {
                    info!(
                        trade_no = %trade_no,
                        tx_hash = %req.tx_hash.as_deref().unwrap_or_default(),
                        source = "shadow_fee_worker",
                        "Tron tx missing from confirmed and pending pools; keep observing before rebroadcast"
                    );
                    return Ok(());
                }

                warn!(
                    trade_no = %trade_no,
                    tx_hash = %req.tx_hash.as_deref().unwrap_or_default(),
                    source = "shadow_fee_worker",
                    "Tron tx missing from confirmed and pending pools beyond timeout; rebroadcasting"
                );
                let rows = ApiFeeRepo::invalidate_raw_tx_for_rebroadcast(
                    &self.ctx.api_transaction_pool()?,
                    &req.trade_no,
                    None,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                if rows > 0 {
                    self.scanner.try_advance(&req.trade_no).await;
                }
            }
            Err(err) => return Err(err),
        }

        Ok(())
    }

    /// 执行 BuildTx Command - 外层wrapper，确保所有错误都被捕获
    async fn process_build_tx(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "shadow_fee_worker", "Processing BuildTx command");

        // 使用内层函数来捕获所有错误
        if let Err(err) = self.process_build_tx_inner(&trade_no).await {
            error!(trade_no = %trade_no, error = %err, source = "shadow_fee_worker", "BuildTx inner failed, handling error");
            self.handle_fee_tx_failed(&trade_no, err).await?;
        }

        Ok(())
    }

    /// BuildTx 内部实现，可能返回错误
    async fn process_build_tx_inner(&self, trade_no: &str) -> Result<(), ServiceError> {
        self.build_fee_candidate(trade_no, None).await
    }

    pub(crate) async fn build_fee_candidate(
        &self,
        trade_no: &str,
        preloaded_key: Option<wallet_chain_interact::types::ChainPrivateKey>,
    ) -> Result<(), ServiceError> {
        // 1. 从数据库中获取手续费交易信息
        let fee = self.get_fee_entity(trade_no).await?;

        // check
        if fee.err_code.is_some() || fee.finished_at.is_some() || fee.transaction_time.is_some() {
            return Ok(());
        }
        if !self.check_digest(&fee).await? {
            tracing::error!(trade_no=%trade_no, "[手续费归集] 交易数据验证失败");
            return Err(ServiceError::Business(
                ApiWalletError::Trans(TransError::TransactionDigestVerificationFailed).into(),
            ));
        }
        tracing::info!(trade_no=%trade_no, "[手续费归集] 交易数据验证通过");

        // ====== phase 1: 锁内 · 快速检查 ======
        // ⚠️ 锁内禁止任何网络调用、sleep、await RPC
        let (nonce_address, nonce_chain) = {
            // 5. 获取地址锁，保护地址级并发
            let _addr_guard = self.address_locks.acquire(&fee.from_addr).await;
            info!(trade_no = %trade_no, source = "shadow_fee_worker", "Acquired address lock");

            // 🔒 必须锁内重新读取，确保基于最新状态做决策
            // ⚠️ 只读"裁决字段"，不做任何业务推断
            let fresh_fee = self.get_fee_entity(trade_no).await?;

            // 2. 事实校验：BuildTx 只能处理 raw_tx 为空的交易
            // ⚠️ 这里是并发裁决的关键，确保只有一个task能通过
            if fresh_fee.raw_tx.is_some()
                || fresh_fee.err_code.is_some()
                || fresh_fee.finished_at.is_some()
                || fresh_fee.transaction_time.is_some()
            {
                info!(trade_no = %trade_no, source = "shadow_fee_worker", "raw_tx already exists, skipping BuildTx");
                self.clear_build_slot_after_claim(trade_no).await?;
                return Ok(());
            }

            (fresh_fee.from_addr, fresh_fee.chain_code)
        };
        // 🔓 锁在这里已经释放

        // 先占位 build slot，防止同一 trade_no 在构建期间被重复推进。
        let build_slot_rows =
            ApiFeeRepo::update_building_at(&self.ctx.api_transaction_pool()?, trade_no).await?;
        if build_slot_rows == 0 {
            if self.reclaim_stale_build_slot(trade_no, fee.building_at).await? {
                let reclaimed_build_slot_rows =
                    ApiFeeRepo::update_building_at(&self.ctx.api_transaction_pool()?, trade_no)
                        .await?;
                if reclaimed_build_slot_rows > 0 {
                    info!(
                        trade_no = %trade_no,
                        source = "shadow_fee_worker",
                        "Reclaimed stale build slot, continuing BuildTx"
                    );
                } else {
                    info!(
                        trade_no = %trade_no,
                        source = "shadow_fee_worker",
                        "Build slot still unavailable after stale reclaim attempt, skipping BuildTx"
                    );
                    return Ok(());
                }
            } else {
                info!(
                    trade_no = %trade_no,
                    source = "shadow_fee_worker",
                    "Build slot already claimed or recently updated, skipping BuildTx"
                );
                return Ok(());
            }
        }

        // Observing before signing is read-only. A build failure or cancellation
        // cannot create a nonce reservation, and occupied slots do not query RPC.
        let (nonce, chain_next) = self.get_nonce(&nonce_address, &nonce_chain).await?;
        info!(trade_no = %trade_no, nonce, source = "shadow_fee_worker", "Observed nonce candidate");

        // ====== phase 2: 锁外 · 网络执行 ======
        // 通过Context获取Handles实例，然后获取私钥管理器
        let private_key = if let Some(key) = preloaded_key {
            key
        } else {
            let handles = self.ctx.get_handles_arc().await?;
            handles
                .get_global_private_key_manager()
                .get_private_key(&fee.from_addr, &fee.chain_code)
                .await?
        };
        info!(trade_no = %trade_no, source = "shadow_fee_worker", "Retrieved private key from manager");

        // 9. 生成转账请求
        // Candidate is only reserved together with signed facts in phase 3.
        let transfer_req = self.gen_transfer_req(&fee, nonce).await?;
        info!(trade_no = %trade_no, nonce = %nonce, source = "shadow_fee_worker", "Generated transfer request with nonce");

        // 10. 构建交易。公共包装负责 guarded endpoint 并发、timeout、成功/失败记录。
        let Some((tx_hash, raw_tx, fee_str)) =
            crate::infrastructure::chain_rpc_guard::run_guarded_with_ctx_and_log(
                self.ctx,
                &fee.chain_code,
                trade_no,
                "build",
                "shadow_fee_worker",
                crate::infrastructure::chain_rpc_guard::ChainRpcFailureLog::new()
                    .trade_no(trade_no)
                    .chain_code(&fee.chain_code)
                    .source("shadow_fee_worker")
                    .op("build"),
                ApiTransDomain::build_transfer_raw(
                    &self.ctx,
                    transfer_req,
                    Some(private_key), // 私钥管理
                ),
            )
            .await?
        else {
            self.clear_build_slot_after_claim(trade_no).await?;
            return Ok(());
        };
        info!(trade_no = %trade_no, tx_hash = %tx_hash, fee = %fee_str, source = "shadow_fee_worker", "Built transfer raw transaction successfully");

        // ====== phase 3: 锁内 · 提交不可逆事实 ======
        {
            // 重新获取地址锁，保护地址级并发
            let _addr_guard = self.address_locks.acquire(&fee.from_addr).await;
            info!(trade_no = %trade_no, source = "shadow_fee_worker", "Reacquired address lock for fact commit");

            // 11. 立即将tx_hash、raw_tx和nonce存储到数据库
            let raw_tx_str = wallet_utils::serde_func::serde_to_string(&raw_tx)?;
            let pool = self.ctx.api_transaction_pool()?;
            let rows_affected = if shared_mechanism::is_evm_chain(&fee.chain_code) {
                ApiFeeRepo::reserve_signed_fee(
                    &pool,
                    &SignedFeeCandidate {
                        trade_no: &fee.trade_no,
                        from_addr: &fee.from_addr,
                        chain_code: &fee.chain_code,
                        tx_hash: &tx_hash,
                        raw_tx: &raw_tx_str,
                        transaction_fee: &fee_str,
                        nonce: nonce as i64,
                        chain_next,
                    },
                )
                .await?
            } else {
                ApiFeeRepo::update_after_build(
                    &pool,
                    &fee.trade_no,
                    &tx_hash,
                    &raw_tx_str,
                    &fee_str,
                    nonce as i64,
                )
                .await?
            };

            // 显式处理幂等情况：如果影响行数为0，表示raw_tx已存在或被并发写入
            if rows_affected == 0 {
                info!(trade_no = %trade_no, source = "shadow_fee_worker", "update_after_build skipped: raw_tx already exists (idempotent hit)");
                self.clear_build_slot_after_claim(trade_no).await?;
                return Ok(());
            }

            info!(trade_no = %trade_no, source = "shadow_fee_worker", "Updated tx_hash, raw_tx and nonce to database successfully");

            // 直接调用 try_advance 进行点对点唤醒
            self.scanner.try_advance(&fee.trade_no).await;
        }

        Ok(())
    }

    /// 执行 Broadcast Command - 外层wrapper，确保所有错误都被捕获
    async fn process_broadcast(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "shadow_fee_worker", "Processing Broadcast command");

        // 使用内层函数来捕获所有错误
        if let Err(err) = self.process_broadcast_inner(&trade_no).await {
            error!(trade_no = %trade_no, error = %err, source = "shadow_fee_worker", "Broadcast inner failed, handling error");
            self.handle_fee_tx_failed(&trade_no, err).await?;
        }

        Ok(())
    }

    /// Broadcast 内部实现，可能返回错误
    async fn process_broadcast_inner(&self, trade_no: &str) -> Result<(), ServiceError> {
        // 1. 从数据库中获取手续费交易信息
        let fee = self.get_fee_entity(trade_no).await?;

        // ====== phase 1: 锁内 · 快速检查 ======
        // ⚠️ 锁内禁止任何网络调用、sleep、await RPC
        {
            // 获取地址锁，保护地址级并发
            let _addr_guard = self.address_locks.acquire(&fee.from_addr).await;
            info!(trade_no = %trade_no, source = "shadow_fee_worker", "Acquired address lock");

            // 🔒 必须锁内重新读取，确保基于最新状态做决策
            // ⚠️ 只读"裁决字段"，不做任何业务推断
            let fresh_fee = self.get_fee_entity(trade_no).await?;

            // 2. 事实校验：Broadcast 只能处理 raw_tx 存在且 last_broadcast_at 为空的交易
            // 🔒 与 predicate::can_broadcast 同构，确保模型自洽
            // ⚠️ 这里是并发裁决的关键，确保只有一个task能通过
            if fresh_fee.raw_tx.is_none()
                || fresh_fee.last_broadcast_at.is_some()
                || fresh_fee.err_code.is_some()
                || fresh_fee.finished_at.is_some()
                || fresh_fee.transaction_time.is_some()
            {
                info!(trade_no = %trade_no, source = "shadow_fee_worker", "raw_tx empty or last_broadcast_at exists, skipping Broadcast");
                return Ok(());
            }

            if shared_mechanism::tracks_broadcast_uncertain_state(&fresh_fee.chain_code)
                && fresh_fee.broadcast_uncertain_since_at.is_some()
                && fresh_fee.last_broadcast_at.is_none()
                && fresh_fee.transaction_time.is_none()
                && fresh_fee.tx_exec_receipt_uploaded_at.is_none()
                && fresh_fee.err_code.is_none()
            {
                info!(
                    observability_event = "shadow_broadcast_skipped",
                    reason = "broadcast_uncertain_in_progress",
                    trade_no = %trade_no,
                    tx_hash = %fresh_fee.tx_hash.as_deref().unwrap_or_default(),
                    nonce = fresh_fee.nonce,
                    source = "shadow_fee_worker",
                    "Skip Broadcast: uncertain state in progress; recover should proceed"
                );
                return Ok(());
            }

            // 3. 检查是否已有raw_tx和tx_hash
            if fresh_fee.tx_hash.is_none()
                || fresh_fee.raw_tx.is_none()
                || fresh_fee.raw_tx.as_ref().unwrap().is_empty()
            {
                error!(trade_no = %trade_no, source = "shadow_fee_worker", "No raw_tx or tx_hash found");
                return Err(ServiceError::Business(
                    ApiWalletError::Trans(crate::error::business::api_wallet::trans::TransError::BuildWithdrawTransactionFailed("Missing transaction data".to_string())).into(),
                ));
            }
        }
        // 🔓 锁在这里已经释放

        // 6. 反序列化raw_tx
        let raw_tx = wallet_utils::serde_func::serde_from_str(fee.raw_tx.as_deref().unwrap())?;
        info!(trade_no = %trade_no, tx_hash = %fee.tx_hash.as_deref().unwrap(), source = "shadow_fee_worker", "Deserialized raw_tx successfully");

        // 7. 广播交易
        info!(trade_no = %trade_no, tx_hash = %fee.tx_hash.as_deref().unwrap(), source = "shadow_fee_worker", "Starting to broadcast transaction");
        let tx_resp = crate::infrastructure::chain_rpc_guard::run_guarded_with_ctx_and_log(
            self.ctx,
            &fee.chain_code,
            trade_no,
            "broadcast",
            "shadow_fee_worker",
            crate::infrastructure::chain_rpc_guard::ChainRpcFailureLog::new()
                .trade_no(trade_no)
                .tx_hash(fee.tx_hash.as_deref().unwrap_or_default())
                .chain_code(&fee.chain_code)
                .source("shadow_fee_worker")
                .op("broadcast"),
            ApiTransDomain::broadcast_transfer(
                &self.ctx,
                &fee.chain_code,
                raw_tx,
                fee.tx_hash.as_deref(),
            ),
        )
        .await?
        .flatten();

        match tx_resp {
            Some(tx) => {
                info!(trade_no = %trade_no, tx_hash = %tx.tx_hash, source = "shadow_fee_worker", "Transaction broadcast successful");

                // 🔒 事实保护：检查 tx_hash 一致性，防止 build 阶段事实被覆盖
                if let Some(existing) = &fee.tx_hash {
                    if existing != &tx.tx_hash {
                        error!(
                            trade_no = %fee.trade_no,
                            existing_tx_hash = %existing,
                            broadcast_tx_hash = %tx.tx_hash,
                            source = "shadow_fee_worker",
                            "tx_hash mismatch between build and broadcast - fact integrity violated"
                        );
                        return Err(ServiceError::System(SystemError::Internal(
                            "Invariant broken - tx_hash mismatch between build and broadcast"
                                .to_string(),
                        )));
                    }
                }

                // ====== phase 3: 锁内 · 提交不可逆事实 ======
                {
                    // 重新获取地址锁，保护地址级并发
                    let _addr_guard = self.address_locks.acquire(&fee.from_addr).await;
                    info!(trade_no = %trade_no, source = "shadow_fee_worker", "Reacquired address lock for fact commit");

                    // 广播成功 = 一次不可分割的事实提交
                    let resource_consume = if let Some(consumer) = tx.consumer {
                        consumer.energy_used.to_string()
                    } else {
                        "0".to_string()
                    };

                    let rows_affected = ApiFeeRepo::mark_broadcast_executed(
                        &self.ctx.api_transaction_pool()?,
                        &fee.trade_no,
                    )
                    .await
                    .map_err(|e| ServiceError::Database(e.into()))?;

                    // 显式处理幂等情况：广播已被其他并发/恢复执行
                    if rows_affected == 0 {
                        info!(
                            trade_no = %fee.trade_no,
                            tx_hash = %tx.tx_hash,
                            source = "shadow_fee_worker",
                            "mark_broadcast_executed skipped: broadcast already executed (idempotent hit)"
                        );
                    } else {
                        let recover_key = shadow_rpc_policy::recover_dispatch_key(&fee.trade_no);
                        let cleared_pre_broadcast_cooldown =
                            Self::reset_tron_post_broadcast_recover_cooldown(
                                &fee.chain_code,
                                &fee.trade_no,
                            );
                        if shared_mechanism::is_tron_chain(&fee.chain_code) {
                            info!(
                                observability_event = "tron_fee_post_broadcast_recover_cooldown_reset",
                                trade_no = %fee.trade_no,
                                recover_key = %recover_key,
                                cleared_pre_broadcast_cooldown,
                                source = "shadow_fee_worker",
                                "Reset TRON fee confirmation cooldown after successful broadcast"
                            );
                        }
                        self.schedule_tron_post_broadcast_confirms_if_needed(&fee);
                        // 直接调用 try_advance 进行点对点唤醒
                        self.scanner.try_advance(&fee.trade_no).await;
                    }
                }

                Ok(())
            }
            None => {
                info!(trade_no = %trade_no, source = "shadow_fee_worker", "Transaction broadcast result is uncertain");
                if !shared_mechanism::tracks_broadcast_uncertain_state(&fee.chain_code) {
                    return Ok(());
                }
                let had_uncertain_since = fee.broadcast_uncertain_since_at.is_some();
                let rows_affected = ApiFeeRepo::mark_broadcast_uncertain_attempt(
                    &self.ctx.api_transaction_pool()?,
                    trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                let refreshed = self.get_fee_entity(trade_no).await?;
                info!(
                    trade_no = %refreshed.trade_no,
                    tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                    nonce = refreshed.nonce,
                    rows_affected = %rows_affected,
                    had_uncertain_since = had_uncertain_since,
                    retry_count = refreshed.broadcast_uncertain_retry_count,
                    uncertain_since_at = ?refreshed.broadcast_uncertain_since_at,
                    source = "shadow_fee_worker",
                    "Broadcast uncertain state recorded"
                );
                self.scanner.try_advance(trade_no).await;
                Ok(())
            }
        }
    }

    /// 从数据库中获取手续费交易信息
    async fn get_fee_entity(
        &self,
        trade_no: &str,
    ) -> Result<wallet_database::entities::api_fee::ApiFeeEntity, ServiceError> {
        let entity =
            ApiFeeRepo::get_api_fee_by_trade_no(&self.ctx.api_transaction_pool()?, trade_no)
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
        Ok(entity)
    }

    /// Observe an unreserved nonce candidate for transaction.
    ///
    /// Observe chain next nonce before atomic reservation. Allocation is not
    /// proof of chain inclusion; failed observations reserve nothing. Existing
    /// signed transactions keep their nonce/hash during uncertain recovery.
    pub(crate) async fn get_nonce(
        &self,
        from_addr: &str,
        chain_code: &str,
    ) -> Result<(u64, i64), ServiceError> {
        info!(from_addr = %from_addr, chain_code = %chain_code, source = "shadow_fee_worker", "Getting nonce");
        let chain: ChainCode = chain_code.try_into()?;

        // EVM candidates are read-only here; reserve_signed_fee commits them.
        // Any read-modify-write logic here is forbidden.
        match chain {
            c if crate::domain::chain::is_evm_chain(c) => {
                // EVM candidates are observed without reserving the shared counter.
                // ⚠️ INVARIANT:
                // The shared SQLite counter is checked again when signed bytes commit.
                use crate::infrastructure::nonce::nonce_engine::get_nonce_engine_with_ctx;

                let nonce_engine = get_nonce_engine_with_ctx(self.ctx)?;
                nonce_engine.observe_signed_nonce_candidate(from_addr, chain_code).await
            }
            _ => {
                // 非 EVM 链不参与 nonce 分配
                Ok((0, 0))
            }
        }
    }

    async fn check_digest(&self, req: &ApiFeeEntity) -> Result<bool, ServiceError> {
        info!(trade_no=%req.trade_no, "[手续费归集] 验证交易摘要");
        let sn = self.ctx.get_sn();
        let mut d = wallet_utils::conversion::decimal_from_str(req.value.as_str())?;
        d = d.normalize();
        let raw_data = req.from_addr.clone() + req.to_addr.as_str() + d.to_string().as_str() + sn;
        let digest = wallet_utils::bytes_to_base64(&wallet_utils::md5_vec(&raw_data));
        let is_valid = req.validate == digest;
        info!(trade_no=%req.trade_no, "[手续费归集] 摘要验证结果: {}", is_valid);
        Ok(is_valid)
    }

    /// 生成转账请求
    ///
    /// 🔒 关键语义：
    /// - nonce 由调用方传入，不再内部获取
    /// - nonce 是只读候选，构建成功后原子预留
    /// - 确保 nonce 从"动态信息"升级为"已裁决事实"
    async fn gen_transfer_req(
        &self,
        req: &ApiFeeEntity,
        nonce: u64,
    ) -> Result<ApiTransferReq, ServiceError> {
        info!(trade_no=%req.trade_no, chain_code=%req.chain_code, symbol=%req.symbol, "[手续费归集] 获取代币信息");
        let coin = ApiCoinDomain::get_coin_by_token_key_exact(
            self.ctx,
            &req.chain_code,
            req.token_addr.clone().into(),
        )
        .await?;
        info!(trade_no=%req.trade_no, token_address=?coin.token_address, decimals=%coin.decimals, "[手续费归集] 代币信息获取成功");

        info!(trade_no=%req.trade_no, from_addr=%req.from_addr, to_addr=%req.to_addr, value=%req.value, "[手续费归集] 创建基础转账请求");
        let mut params =
            ApiBaseTransferReq::new(&req.from_addr, &req.to_addr, &req.value, &req.chain_code);
        let token_address = if coin.token_address.is_native() {
            None
        } else {
            let s = coin.token_address.as_db_str().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        info!(trade_no=%req.trade_no, token_address=?token_address, "[手续费归集] 设置代币转账参数");
        params.with_token(token_address, coin.decimals, &coin.symbol);

        if req.chain_code.eq_ignore_ascii_case(ChainCode::Solana.to_string().as_str())
            && coin.token_address.is_native()
        {
            Self::bump_sol_native_transfer_value_for_rent(
                self.ctx,
                &mut params,
                &coin.symbol,
                &req.trade_no,
            )
            .await?;
        }

        info!(trade_no=%req.trade_no, "[手续费归集] 获取钱包解锁态");
        let unlock_token = ApiWalletDomain::get_wallet_unlock_token().await?;

        info!(trade_no=%req.trade_no, nonce=%nonce, "[手续费归集] 转账请求生成完成");
        Ok(ApiTransferReq { base: params, password: unlock_token, nonce })
    }

    pub(crate) async fn bump_sol_native_transfer_value_for_rent(
        ctx: &'static crate::context::Context,
        params: &mut ApiBaseTransferReq,
        symbol: &str,
        trade_no: &str,
    ) -> Result<(), ServiceError> {
        let adapter =
            ApiChainAdapterFactory::get_transaction_adapter_with_ctx(ctx, &params.chain_code)
                .await?;
        match adapter.estimate_fee(params.clone(), symbol).await {
            Ok(_) => Ok(()),
            Err(err) if Self::is_solana_recipient_rent_error(&err) => {
                let current = wallet_utils::conversion::decimal_from_str(&params.value)?;
                let rent = wallet_utils::conversion::decimal_from_str(
                    &crate::domain::chain::adapter::sol_tx::SYSTEM_ACCOUNT_RENT.to_string(),
                )?;
                let adjusted = current + rent;
                params.value = adjusted.normalize().to_string();
                info!(
                    trade_no = %trade_no,
                    source = "shadow_fee_worker",
                    current_value = %current,
                    rent = %rent,
                    adjusted_value = %params.value,
                    "SOL fee recipient not initialized; bumping transfer amount to cover rent"
                );
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// 交易恢复逻辑
    ///
    /// ⚠️ IMPORTANT:
    /// - Recover logic MUST only be triggered by Scanner commands
    /// - This method should NOT be called directly by other components
    /// - On-chain confirmation fact is owned by Scanner / Shadow Recovery ONLY
    async fn recover_tx(
        &self,
        fee: &wallet_database::entities::api_fee::ApiFeeEntity,
    ) -> Result<RecoveredTxObservation, ServiceError> {
        let tx_hash = fee.tx_hash.as_ref().unwrap();
        info!(trade_no = %fee.trade_no, tx_hash = %tx_hash, source = "shadow_fee_worker", "Processing recovered tx");

        match ApiTransDomain::observe_recovered_tx_with_ctx(
            &self.ctx,
            &fee.chain_code,
            &fee.from_addr,
            tx_hash,
            fee.nonce,
            &fee.transaction_fee,
        )
        .await
        {
            Ok(RecoveredTxObservation::Confirmed(tx_resp)) => {
                info!(trade_no = %fee.trade_no, tx_hash = %tx_hash, source = "shadow_fee_worker", "Recovered tx success");
                Ok(RecoveredTxObservation::Confirmed(tx_resp))
            }
            Ok(RecoveredTxObservation::Missing) => {
                info!(trade_no = %fee.trade_no, tx_hash = %tx_hash, source = "shadow_fee_worker", "Recovered tx result is uncertain, will retry");
                Ok(RecoveredTxObservation::Missing)
            }
            Ok(RecoveredTxObservation::QueryUncertain) => {
                Ok(RecoveredTxObservation::QueryUncertain)
            }
            Err(err) => {
                error!(trade_no = %fee.trade_no, tx_hash = %tx_hash, error = %err, source = "shadow_fee_worker", "Recovered tx failed");
                Err(err)
            }
        }
    }

    /// 处理手续费交易失败
    pub(crate) async fn handle_fee_tx_failed(
        &self,
        trade_no: &str,
        err: ServiceError,
    ) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, error = %err, source = "shadow_fee_worker", "Handling fee tx failed");

        if err.is_transient_db_pool_timeout() {
            warn!(
                trade_no = %trade_no,
                error = %err,
                source = "shadow_fee_worker",
                "Skip marking fee tx failed: local DB pool timeout is retryable infrastructure noise"
            );
            return Ok(());
        }

        if Self::is_solana_recipient_rent_error(&err) {
            warn!(
                trade_no = %trade_no,
                source = "shadow_fee_worker",
                "SOL fee tx blocked: recipient account is not initialized or transfer amount is below rent-exempt minimum"
            );
        }

        // 🔒 事实保护：检查是否已存在成功事实
        let fee = self.get_fee_entity(trade_no).await?;
        if fee.transaction_time.is_some() {
            info!(
                trade_no = %trade_no,
                source = "shadow_fee_worker",
                "Skip mark failed: transaction already confirmed (monotonicity constraint)"
            );
            return Ok(());
        }

        // Only payer shortage is terminal here. Transient build failures keep
        // releasing the slot for retry; post-broadcast facts are protected by CAS.
        if fee.raw_tx.is_none() {
            use crate::error::business::{BusinessError, chain::ChainError};
            let shortage = if Self::is_solana_recipient_rent_error(&err) {
                None
            } else {
                match &err {
                    ServiceError::Business(BusinessError::Chain(
                        ChainError::InsufficientBalance(_),
                    )) => Some(ErrCode::BalanceInsufficient),
                    ServiceError::Business(BusinessError::Chain(
                        ChainError::InsufficientFeeBalance,
                    )) => Some(ErrCode::FeeInsufficient),
                    _ => None,
                }
            };
            if let Some(code) = shortage {
                let affected = ApiFeeRepo::fail_unbuilt_fee(
                    &self.ctx.api_transaction_pool()?,
                    trade_no,
                    code,
                    &err.to_string(),
                )
                .await?;
                if affected > 0 {
                    self.scanner.try_advance(trade_no).await;
                }
                return Ok(());
            }
            let rows_affected = ApiFeeRepo::clear_building_at(&self.ctx.api_transaction_pool()?, trade_no)
                .await
                .map_err(|db_err: wallet_database::Error| {
                    error!(trade_no = %trade_no, error = %db_err, source = "shadow_fee_worker", "Failed to clear build slot for BuildTx failure");
                    ServiceError::Database(db_err.into())
                })?;

            info!(
                trade_no = %trade_no,
                rows_affected = %rows_affected,
                source = "shadow_fee_worker",
                "BuildTx failure cleared build slot"
            );

            return Ok(());
        }

        let broadcast_failure_decision = broadcast_mechanism::classify_broadcast_failure(
            &err,
            broadcast_mechanism::BroadcastFailureFacts::broadcast_stage(
                &fee.chain_code,
                fee.raw_tx.is_some(),
                fee.tx_hash.is_some(),
                fee.last_broadcast_at.is_some(),
            ),
        );

        if let Some(rebuild_reason) = broadcast_failure_decision.raw_rebuild_reason() {
            let rows_affected = ApiFeeRepo::invalidate_raw_tx(&self.ctx.api_transaction_pool()?, trade_no, None, None, None)
                .await
                .map_err(|db_err: wallet_database::Error| {
                    error!(trade_no = %trade_no, error = %db_err, source = "shadow_fee_worker", "{}", rebuild_reason.db_error_message());
                    ServiceError::Database(db_err.into())
                })?;
            info!(
                trade_no = %trade_no,
                rows_affected = %rows_affected,
                error = %err,
                source = "shadow_fee_worker",
                "{}", rebuild_reason.log_message()
            );
            if rows_affected > 0 {
                self.scanner.try_advance(trade_no).await;
            }
            return Ok(());
        }

        let error_msg = format!("{}", err);
        if let Some(accepted_reason) = broadcast_failure_decision.accepted_reason() {
            if let Some(message) = accepted_reason.pre_self_heal_log_message() {
                info!(
                    trade_no = %trade_no,
                    source = "shadow_fee_worker",
                    "{}", message
                );
            }

            if accepted_reason.requires_nonce_self_heal() {
                nonce_recovery_mechanism::try_nonce_self_heal(
                    self.ctx,
                    trade_no,
                    &fee.from_addr,
                    &fee.chain_code,
                    &error_msg,
                    "shadow_fee_worker",
                )
                .await?;
            }

            let rows_affected = ApiFeeRepo::mark_broadcast_executed(&self.ctx.api_transaction_pool()?, trade_no)
                .await
                .map_err(|db_err: wallet_database::Error| {
                    error!(trade_no = %trade_no, error = %db_err, source = "shadow_fee_worker", "{}", accepted_reason.db_error_message());
                    ServiceError::Database(db_err.into())
                })?;
            info!(
                trade_no = %trade_no,
                rows_affected = %rows_affected,
                error = %err,
                source = "shadow_fee_worker",
                "{}", accepted_reason.log_message()
            );
            self.scanner.try_advance(trade_no).await;
            return Ok(());
        }

        // 处理 nonce too low 错误
        if broadcast_failure_decision.should_fail_after_nonce_self_heal() {
            info!(trade_no = %trade_no, source = "shadow_fee_worker", "Detected nonce too low error, syncing nonce from chain");

            nonce_recovery_mechanism::try_nonce_self_heal(
                self.ctx,
                trade_no,
                &fee.from_addr,
                &fee.chain_code,
                &error_msg,
                "shadow_fee_worker",
            )
            .await?;

            let rows_affected = ApiFeeRepo::update_api_fee_status_and_err(
                &self.ctx.api_transaction_pool()?,
                trade_no,
                wallet_database::entities::api_fee::ApiFeeStatus::SendingTxFailed,
                ErrCode::SDKInternalError as u32, // 使用通用错误码
                &error_msg,
            )
            .await
            .map_err(|db_err: wallet_database::Error| {
                error!(trade_no = %trade_no, error = %db_err, source = "shadow_fee_worker", "Failed to update status to failed");
                ServiceError::Database(db_err.into())
            })?;

            info!(trade_no = %trade_no, rows_affected = %rows_affected, source = "shadow_fee_worker", "Updated status to failed with nonce error");

            // 只有第一次写入失败事实才发送 Tick
            if rows_affected > 0 {
                // 直接调用 try_advance 进行点对点唤醒
                self.scanner.try_advance(trade_no).await;
            }

            return Ok(());
        }

        // 更新数据库状态为失败
        match err.retry_policy() {
            wallet_utils::error::RetryPolicy::Never => {
                // 根据错误类型确定错误码
                let err_code = if err.is_network_error() {
                    ErrCode::NetworkException
                } else {
                    ErrCode::SDKInternalError
                };

                let rows_affected = ApiFeeRepo::update_api_fee_status_and_err(
                    &self.ctx.api_transaction_pool()?,
                    trade_no,
                    wallet_database::entities::api_fee::ApiFeeStatus::SendingTxFailed,
                    err_code as u32, // err_code - 根据错误类型设置
                    &error_msg,
                )
                .await
                .map_err(|db_err: wallet_database::Error| {
                    error!(trade_no = %trade_no, error = %db_err, source = "shadow_fee_worker", "Failed to update status to failed");
                    ServiceError::Database(db_err.into())
                })?;
                info!(trade_no = %trade_no, rows_affected = %rows_affected, source = "shadow_fee_worker", "Updated status to failed");

                // 只有第一次写入失败事实才发送 Tick
                if rows_affected > 0 {
                    // 直接调用 try_advance 进行点对点唤醒
                    self.scanner.try_advance(trade_no).await;
                }

                // 注意：Shadow Worker 是执行者，不是裁决者
                // 不设置 finished_at，因为链上事实尚未闭环
                // 只有 Scanner/Shadow Recovery 才能设置终态
            }
            wallet_utils::error::RetryPolicy::Delay => {
                info!(trade_no = %trade_no, error = %err, source = "shadow_fee_worker", "Fee tx failed, will retry later");
            }
        }

        Ok(())
    }

    async fn clear_build_slot_after_claim(&self, trade_no: &str) -> Result<(), ServiceError> {
        Self::clear_build_slot_after_claim_in_pool(&self.ctx.api_transaction_pool()?, trade_no)
            .await
    }

    async fn clear_build_slot_after_claim_in_pool(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<(), ServiceError> {
        let rows_affected = ApiFeeRepo::clear_building_at(pool, trade_no)
            .await
            .map_err(|db_err: wallet_database::Error| {
                error!(trade_no = %trade_no, error = %db_err, source = "shadow_fee_worker", "Failed to clear build slot after BuildTx early exit");
                ServiceError::Database(db_err.into())
            })?;

        info!(
            trade_no = %trade_no,
            rows_affected = %rows_affected,
            source = "shadow_fee_worker",
            "BuildTx early exit cleared build slot"
        );

        Ok(())
    }

    async fn reclaim_stale_build_slot(
        &self,
        trade_no: &str,
        building_at: Option<DateTime<Utc>>,
    ) -> Result<bool, ServiceError> {
        if !Self::is_stale_build_slot(building_at, Utc::now()) {
            return Ok(false);
        }

        Self::reclaim_stale_build_slot_in_pool(
            &self.ctx.api_transaction_pool()?,
            trade_no,
            building_at,
        )
        .await
    }

    async fn reclaim_stale_build_slot_in_pool(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
        building_at: Option<DateTime<Utc>>,
    ) -> Result<bool, ServiceError> {
        if !Self::is_stale_build_slot(building_at, Utc::now()) {
            return Ok(false);
        }

        let rows_affected = ApiFeeRepo::clear_building_at(pool, trade_no)
            .await
            .map_err(|db_err: wallet_database::Error| {
                error!(trade_no = %trade_no, error = %db_err, source = "shadow_fee_worker", "Failed to clear stale build slot");
                ServiceError::Database(db_err.into())
            })?;

        info!(
            trade_no = %trade_no,
            rows_affected = %rows_affected,
            building_at = ?building_at,
            source = "shadow_fee_worker",
            "Cleared stale build slot before retrying BuildTx"
        );

        Ok(rows_affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::ShadowFeeWorker;
    use crate::{
        config::Config,
        domain::api_wallet::adapter::tx::RawTx,
        error::{service::ServiceError, system::SystemError},
        infrastructure::api_trans::shadow_rpc_policy,
    };
    use chrono::Utc;
    use std::{fs, path::PathBuf, sync::Arc};
    use tempfile::tempdir;
    use tokio::sync::mpsc;
    use wallet_chain_interact::{BillResourceConsume, tron::operations::RawTransactionParams};
    use wallet_database::{
        ApiWalletDbPool, SqliteContext, repositories::api_wallet::fee::ApiFeeRepo,
    };

    #[test]
    fn transient_db_pool_error_is_not_terminal_fee_failure() {
        let err = ServiceError::Database(wallet_database::Error::Database(
            wallet_database::DatabaseError::Sqlx(sqlx::Error::PoolTimedOut),
        ));

        assert!(err.is_transient_db_pool_timeout());
    }

    #[test]
    fn tron_fee_post_broadcast_schedule_resets_only_tron_confirmation_cooldown() {
        assert_eq!(
            ShadowFeeWorker::TRON_POST_BROADCAST_CONFIRM_DELAYS_SECS.as_slice(),
            &[3, 5, 8, 15, 30]
        );
        assert!(ShadowFeeWorker::tron_post_broadcast_confirm_eligible("tron", false, false));
        assert!(!ShadowFeeWorker::tron_post_broadcast_confirm_eligible("tron", true, false));
        assert!(!ShadowFeeWorker::tron_post_broadcast_confirm_eligible("ethereum", false, false));

        let tron_key = shadow_rpc_policy::recover_dispatch_key("CF_TRON_RESET");
        assert!(shadow_rpc_policy::allow_recover_dispatch(&tron_key));
        assert!(ShadowFeeWorker::reset_tron_post_broadcast_recover_cooldown(
            "tron",
            "CF_TRON_RESET"
        ));
        assert!(shadow_rpc_policy::allow_broadcast_recover_dispatch(&tron_key, Some(Utc::now())));

        let evm_key = shadow_rpc_policy::recover_dispatch_key("CF_EVM_KEEP");
        assert!(shadow_rpc_policy::allow_recover_dispatch(&evm_key));
        assert!(!ShadowFeeWorker::reset_tron_post_broadcast_recover_cooldown(
            "ethereum",
            "CF_EVM_KEEP"
        ));
        assert!(!shadow_rpc_policy::allow_broadcast_recover_dispatch(&evm_key, Some(Utc::now())));

        shadow_rpc_policy::reset_recover_dispatch(&tron_key);
        shadow_rpc_policy::reset_recover_dispatch(&evm_key);
    }

    async fn test_ctx() -> &'static crate::context::Context {
        static CTX: tokio::sync::OnceCell<&'static crate::context::Context> =
            tokio::sync::OnceCell::const_new();
        *CTX.get_or_init(|| async {
            let config = Config::new(
                r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
aggregate_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
"#,
            )
            .expect("parse test config");

            let mut dir = PathBuf::from(std::env::temp_dir());
            dir.push("wallet-api-shadow-fee-worker-tests");
            fs::create_dir_all(&dir).expect("ensure temp dir");

            let dirs = crate::dirs::Dirs::new(&dir.to_string_lossy()).expect("create dirs");
            crate::context::init_context(
                "wallet_api_shadow_fee_test_sn",
                "unittest",
                dirs,
                None,
                config,
            )
            .await
            .expect("init test context")
        })
        .await
    }

    #[tokio::test]
    async fn clear_build_slot_after_claim_releases_building_at() {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();
        let collect_ctx = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db");
        let collect_pool = collect_ctx.into_transaction_db_pool().expect("transaction pool");

        let trade_no = "F_clear_build_slot_after_claim";
        ApiFeeRepo::upsert_api_fee(
            &collect_pool,
            "uid",
            "name",
            "from",
            "to",
            "1",
            "digest",
            "eth",
            None,
            "ETH",
            trade_no,
            0,
        )
        .await
        .expect("insert fee");

        let claimed = ApiFeeRepo::update_building_at(&collect_pool, trade_no)
            .await
            .expect("claim build slot");
        assert_eq!(claimed, 1);

        ShadowFeeWorker::clear_build_slot_after_claim_in_pool(&collect_pool, trade_no)
            .await
            .expect("clear build slot");

        let persisted =
            ApiFeeRepo::get_api_fee_by_trade_no(&collect_pool, trade_no).await.expect("load fee");
        assert!(persisted.building_at.is_none());
    }

    #[tokio::test]
    async fn fee_reclaim_stale_build_slot_clears_and_allows_reclaim() {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();
        let collect_ctx = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db");
        let collect_pool = collect_ctx.into_transaction_db_pool().expect("transaction pool");

        let trade_no = "F_reclaim_stale_build_slot";
        ApiFeeRepo::upsert_api_fee(
            &collect_pool,
            "uid",
            "name",
            "from",
            "to",
            "1",
            "digest",
            "eth",
            None,
            "ETH",
            trade_no,
            0,
        )
        .await
        .expect("insert fee");
        sqlx::query(
            "UPDATE api_fee
             SET building_at = datetime('now', '-31 seconds')
             WHERE trade_no = ?",
        )
        .bind(trade_no)
        .execute(collect_pool.as_ref())
        .await
        .expect("seed stale slot");

        let fee =
            ApiFeeRepo::get_api_fee_by_trade_no(&collect_pool, trade_no).await.expect("load fee");
        assert_eq!(
            fee.building_at.map(|ts| Utc::now().signed_duration_since(ts).num_seconds() >= 30),
            Some(true)
        );

        let reclaimed = ShadowFeeWorker::reclaim_stale_build_slot_in_pool(
            &collect_pool,
            trade_no,
            fee.building_at,
        )
        .await
        .expect("reclaim stale slot");
        assert!(reclaimed);

        let persisted =
            ApiFeeRepo::get_api_fee_by_trade_no(&collect_pool, trade_no).await.expect("load fee");
        assert!(persisted.building_at.is_none());

        let claimed = ApiFeeRepo::update_building_at(&collect_pool, trade_no)
            .await
            .expect("reclaim build slot");
        assert_eq!(claimed, 1);
    }

    #[tokio::test]
    async fn fee_fresh_build_slot_is_not_reclaimed() {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();
        let collect_ctx = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db");
        let collect_pool = collect_ctx.into_transaction_db_pool().expect("transaction pool");

        let trade_no = "F_fresh_build_slot";
        ApiFeeRepo::upsert_api_fee(
            &collect_pool,
            "uid",
            "name",
            "from",
            "to",
            "1",
            "digest",
            "eth",
            None,
            "ETH",
            trade_no,
            0,
        )
        .await
        .expect("insert fee");
        sqlx::query(
            "UPDATE api_fee
             SET building_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind(trade_no)
        .execute(collect_pool.as_ref())
        .await
        .expect("seed fresh slot");

        let fee =
            ApiFeeRepo::get_api_fee_by_trade_no(&collect_pool, trade_no).await.expect("load fee");
        let reclaimed = ShadowFeeWorker::reclaim_stale_build_slot_in_pool(
            &collect_pool,
            trade_no,
            fee.building_at,
        )
        .await
        .expect("reclaim fresh slot");
        assert!(!reclaimed);

        let persisted =
            ApiFeeRepo::get_api_fee_by_trade_no(&collect_pool, trade_no).await.expect("load fee");
        assert!(persisted.building_at.is_some());

        let claimed = ApiFeeRepo::update_building_at(&collect_pool, trade_no)
            .await
            .expect("claim build slot");
        assert_eq!(claimed, 0);
    }
}

#[cfg(test)]
#[path = "shadow_fee_worker/receipt_observation_tests.rs"]
mod receipt_observation_tests;
