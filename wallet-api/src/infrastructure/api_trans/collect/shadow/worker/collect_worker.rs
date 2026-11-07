#![allow(deprecated)]
#[path = "collect_replacement.rs"]
mod replacement_flow;

use crate::domain::api_wallet::trans::RecoveredTxObservation;
// collect/shadow/worker/collect_worker.rs

// Architecture Rule:
// - Broadcast success MUST only update last_broadcast_at
// - transaction_time is an irreversible on-chain confirmation fact
// - Only Scanner / Shadow Recovery may write transaction_time
use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use tracing::{error, info, warn};
use wallet_chain_interact::BillResourceConsume;
use wallet_database::{
    ApiResourceDbPool, ApiTransactionDbPool, ApiWalletDbPool,
    entities::{
        api_collect::{ApiCollectEntity, ApiCollectStatus, ErrCode, PendingCollectResourceBudget},
        api_resource_delegation::{
            ApiResourceDelegationEntity, ApiResourceDelegationOperationType,
            ApiResourceDelegationResultStatus, ApiResourceDelegationSource,
            NewApiResourceDelegation,
        },
        api_resource_gate::{
            ApiResourceBlockReason, ApiResourceDependencyType, ApiResourceGateResult,
        },
        api_resource_type::ApiResourceType,
        api_tron_account_activation::{
            ApiTronAccountActivationEntity, ApiTronAccountActivationRecoverStatus,
            NewApiTronAccountActivation,
        },
        asset_token_key::AssetTokenKey,
    },
    repositories::api_wallet::{
        account::ApiAccountRepo,
        collect::{ApiCollectRepo, SignedCollectCandidate},
        resource_delegation::ApiResourceDelegationRepo,
        tron_account_activation::ApiTronAccountActivationRepo,
        wallet::ApiWalletRepo,
    },
};
use wallet_transport_backend::request::api_wallet::{
    resource_delegation::{ResourceApplyReq, ResourceType},
    strategy::ChainConfig,
    transaction::TransType,
};
use wallet_types::chain::chain::ChainCode;
use wallet_utils::{RetryableError as _, conversion, unit};

// 从crate::response_vo导入必要的Fee类型
use crate::{
    domain::api_wallet::wallet::ApiWalletDomain,
    error::{business::api_wallet::trans::TransError, system::SystemError},
    infrastructure::nonce::nonce_engine::get_nonce_engine_with_ctx,
    request::api_wallet::trans::ApiTransferReq,
    response_vo::{
        CommonFeeDetails, TronFeeDetails, api_wallet::transaction::ApiWalletResourceConsume,
    },
};

use crate::{
    domain::{
        api_wallet::{
            adapter::tx::RawTx,
            adapter_factory::ApiChainAdapterFactory,
            chain::ApiChainTransDomain,
            coin::ApiCoinDomain,
            strategy::StrategyDomain,
            trans::{ApiTransDomain, broadcast_mechanism, shared_mechanism},
        },
        chain::adapter::{ChainAdapterFactory, sol_tx::SYSTEM_ACCOUNT_RENT},
    },
    error::{
        business::{
            BusinessError,
            api_wallet::ApiWalletError,
            chain::{ChainError, InsufficientBalanceDetail},
        },
        service::ServiceError,
    },
    infrastructure::api_trans::{
        AddressLockManager, nonce_recovery_mechanism,
        resource_amount::energy_shortfall_to_apply_amounts,
        resource_apply_outcome::PlatformApplyOutcome,
        resource_delegation::{
            resource_delegation_failure_fact, resource_delegation_retry_wait_secs,
        },
        resource_reservation::{
            CollectFeeReservationManager, FeeReservationDecision, ResourceBudget,
            ResourceReservationDecision, WithdrawResourceReservationManager,
        },
        resource_rpc_auth, shadow_rpc_policy,
    },
    request::api_wallet::trans::{ApiBaseTransferReq, COLLECT_IGNORE_SENDER_RENT_METADATA},
};

/// Shadow Worker Command 结构
/// 只表达："对某个 trade_no 执行某个确定动作"
#[derive(Debug)]
pub enum ShadowCollectCommand {
    /// 终结超过执行租期且从未进入链上执行的订单
    ExpireUnexecuted(String),
    /// 评估资源闸门
    EvalResourceGate(String),
    /// 构建交易
    BuildTx(String),
    /// 广播交易
    Broadcast(String),
    /// 恢复交易
    Recover(String),
    /// 执行平台代理资源代理任务，trade_no 是资源任务号
    ExecuteLocalResourceDelegation(String),
    /// 执行 TRON 账户激活任务，trade_no 是激活任务号
    ExecuteAccountActivation(String),
}

#[derive(Debug, Clone, Copy)]
struct ResourceGateSnapshot {
    /// Energy 代理成功后仍不可避免的 TRX 手续费估算，用于代理前资金预占。
    estimated_fee_trx: f64,
    /// BuildTx 前当前归集交易需要的能量
    required_energy: u64,
    /// BuildTx 前当前归集交易需要的带宽
    required_bandwidth: u64,
    /// 子账户当前可直接使用的能量
    available_energy: i64,
    /// 子账户当前可直接使用的带宽
    available_bandwidth: i64,
    /// 当前链上每 1 TRX 可换算的 Energy，用于本地代理时把 energy 缺口换算为 TRX 数量
    energy_price: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceDelegationBlockPath {
    PlatformFallback,
    LocalFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceGateNextStep {
    Release,
    BlockOnAccountActivation,
    BlockOnPlatform,
    BlockOnLocal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TronMissingPoolsRecoveryAction {
    ObserveWithPointToPointRetry,
    Rebroadcast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvmUncertainNonceAction {
    PreserveFutureNonce,
    RebroadcastSameRaw,
    PreserveConsumedNonce,
}

/// Shadow Worker
/// 纯执行型、无状态假设、可随时 kill -9 的 Worker
///
/// Shadow Worker 约束：
/// - 永远不发送 ACK
/// - 永远不做业务决策
/// - 永远只执行 DB 已确认允许的动作
/// - DB 是唯一真理源
/// - 可随时被 kill-9，不影响系统一致性
/// - 只负责执行链动作：build / broadcast / confirm
/// - 不依赖任何外部业务系统
/// - 不产生任何业务承诺
/// - 只执行链相关操作，不涉及业务逻辑
/// - 不做任何 in-flight 管理，并发与去重完全由 DB 状态机保证
///
/// Shadow Worker design invariant:
///
/// Phase 1: Address lock + fact arbitration (no network)
/// - 地址锁内进行并发裁决
/// - 分配 nonce（确保同一地址串行）
/// - 锁内禁止任何网络调用、sleep、await RPC
/// - 裁决依据必须基于锁内 fresh read
///
/// Phase 2: Network execution (no shared state)
/// - 锁外执行网络/RPC/构建/广播
/// - chain_rpc_guard 只限制外部世界并发
/// - 允许失败和重试
///
/// Phase 3: DB commit (with address lock)
/// - 持锁写事实，保证原子性
/// - 只写事实，不做决策
/// - 写事实后必须调用 try_advance 唤醒 Scanner
use crate::infrastructure::api_trans::collect::shadow::ShadowAdvancer;

pub struct ShadowCollectWorker {
    ctx: &'static crate::context::Context,
    /// 地址锁管理器，保护地址级并发
    address_locks: Arc<AddressLockManager>,
    /// ShadowAdvancer 引用，用于统一推进执行
    advancer: Arc<ShadowAdvancer>,
    resource_reservations: WithdrawResourceReservationManager,
    fee_reservations: CollectFeeReservationManager,
}

impl ShadowCollectWorker {
    const UNEXECUTED_ORDER_TTL_HOURS: i64 = 24;
    const UNEXECUTED_ORDER_EXPIRED_MESSAGE: &'static str =
        "Collect order expired after 24 hours without chain execution";
    fn collect_broadcast_guard_skip_requires_retry<T>(
        outcome: &crate::infrastructure::chain_rpc_guard::GuardedCallOutcome<T>,
    ) -> bool {
        matches!(
            outcome,
            crate::infrastructure::chain_rpc_guard::GuardedCallOutcome::SkippedBeforeExecution
        )
    }

    const LOCAL_DELEGATION_POST_BROADCAST_CONFIRM_DELAYS_SECS: [u64; 3] = [3, 8, 15];

    const TRON_MISSING_CONFIRMED_AND_PENDING_REBROADCAST_TIMEOUT_SECS: i64 = 5 * 60;
    const EVM_UNCERTAIN_TIMEOUT_SECS: i64 = 5 * 60;
    const EVM_UNCERTAIN_BACKOFF_MID_SECS: i64 = 15;
    const EVM_UNCERTAIN_BACKOFF_MAX_SECS: i64 = 30;
    const EVM_UNCERTAIN_AUTO_FAIL_ERR_CODE: ErrCode = ErrCode::TransactionOnChainException;
    const BUILD_SLOT_STALE_SECS: i64 = 30;

    fn should_spend_all_native_collect(chain_code: &str, token_key: &AssetTokenKey) -> bool {
        shared_mechanism::is_sol_chain(chain_code) && token_key.is_native()
    }

    fn sol_token_collect_sender_rent_reserve(
        chain_code: &str,
        token_key: &AssetTokenKey,
    ) -> Result<Decimal, ServiceError> {
        if shared_mechanism::is_sol_chain(chain_code) && token_key.is_contract() {
            return Ok(conversion::decimal_from_str(&SYSTEM_ACCOUNT_RENT.to_string())?);
        }

        Ok(Decimal::ZERO)
    }

    fn collect_balance_need(
        fee: Decimal,
        value: Decimal,
        spend_all_native: bool,
    ) -> Result<Decimal, ServiceError> {
        if spend_all_native { Ok(fee) } else { Ok(fee + value) }
    }

    fn is_collect_amount_shortage(balance: &str, value: &str) -> Result<bool, ServiceError> {
        let balance = conversion::decimal_from_str(balance)?;
        let value = conversion::decimal_from_str(value)?;
        Ok(balance < value)
    }

    fn tron_token_collect_resource_gate_covers_native_fee(req: &ApiCollectEntity) -> bool {
        shared_mechanism::is_tron_chain(&req.chain_code)
            && req.token_addr.is_contract()
            && req.resource_gate_released_at.is_some()
            && matches!(
                req.resource_gate_result,
                Some(ApiResourceGateResult::ResourceDelegationSuccess)
            )
    }

    fn evm_uncertain_backoff_secs(retry_count: u32) -> i64 {
        shared_mechanism::broadcast_uncertain_backoff_secs(
            retry_count,
            Self::EVM_UNCERTAIN_BACKOFF_MID_SECS,
            Self::EVM_UNCERTAIN_BACKOFF_MAX_SECS,
        )
    }

    fn broadcast_uncertain_elapsed_secs(req: &ApiCollectEntity, now: DateTime<Utc>) -> Option<i64> {
        shared_mechanism::broadcast_uncertain_elapsed_secs(req.broadcast_uncertain_since_at, now)
    }

    fn should_auto_fail_broadcast_uncertain(req: &ApiCollectEntity, now: DateTime<Utc>) -> bool {
        shared_mechanism::is_broadcast_uncertain_timed_out(
            req.broadcast_uncertain_since_at,
            Self::EVM_UNCERTAIN_TIMEOUT_SECS,
            now,
        )
    }

    fn evm_uncertain_nonce_action(
        chain_nonce: u64,
        order_nonce: u64,
        can_rebroadcast_same_raw: bool,
    ) -> EvmUncertainNonceAction {
        match chain_nonce.cmp(&order_nonce) {
            std::cmp::Ordering::Less => EvmUncertainNonceAction::PreserveFutureNonce,
            std::cmp::Ordering::Equal if can_rebroadcast_same_raw => {
                EvmUncertainNonceAction::RebroadcastSameRaw
            }
            std::cmp::Ordering::Equal | std::cmp::Ordering::Greater => {
                EvmUncertainNonceAction::PreserveConsumedNonce
            }
        }
    }

    fn should_throttle_evm_uncertain_recover(req: &ApiCollectEntity, now: DateTime<Utc>) -> bool {
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

    fn is_stale_build_slot(building_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
        shared_mechanism::is_stale_slot(building_at, Self::BUILD_SLOT_STALE_SECS, now)
    }

    fn should_retry_collect_build_service_unavailable(
        req: &ApiCollectEntity,
        err: &ServiceError,
    ) -> bool {
        req.raw_tx.is_none()
            && req.tx_hash.is_none()
            && crate::infrastructure::chain_rpc_guard::ChainRpcErrorContext::from_display(err)
                .is_service_unavailable()
    }

    fn tron_collect_required_budget(
        required_energy: u64,
        _required_bandwidth: u64,
    ) -> ResourceBudget {
        // The collect resource gate only owns Energy delegation. Bandwidth
        // shortage is paid/bypassed by the existing collect main flow, so it
        // must not trigger `/resourceDl/apply` for an Energy task.
        ResourceBudget::new(required_energy, 0)
    }

    fn tron_fee_estimate_resource_json(
        energy: u64,
        bandwidth: u64,
    ) -> Result<String, ServiceError> {
        ApiWalletResourceConsume::new(bandwidth, energy).to_json_string()
    }

    fn effective_resource_after_pending_collects(
        available_energy: i64,
        available_bandwidth: i64,
        pending_budget: PendingCollectResourceBudget,
    ) -> ResourceBudget {
        let effective_energy =
            available_energy.saturating_sub(pending_budget.energy.min(i64::MAX as u64) as i64);
        let effective_bandwidth = available_bandwidth
            .saturating_sub(pending_budget.bandwidth.min(i64::MAX as u64) as i64);
        ResourceBudget::new(effective_energy.max(0) as u64, effective_bandwidth.max(0) as u64)
    }

    fn collect_local_delegate_trade_no(origin_trade_no: &str) -> String {
        // 本地代理是 SDK 自己执行的 fallback 任务，所以需要确定性任务号；
        // 否则 collect 每次重新评估时都会重复创建本地代理任务。
        format!("rsc_local_delegate_{}", origin_trade_no)
    }

    fn resource_shortfall(required: u64, available: i64) -> u64 {
        required.saturating_sub(available.max(0) as u64)
    }

    fn collect_delegation_energy_amount(required_energy: u64) -> u64 {
        required_energy.max(1)
    }

    fn is_insufficient_fee_balance_error(err: &ServiceError) -> bool {
        matches!(
            err,
            ServiceError::Business(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::InsufficientFeeBalance
            ))
        )
    }

    fn should_open_fee_cycle_for_tron_resource_ready(
        req: &ApiCollectEntity,
        err: &ServiceError,
    ) -> bool {
        Self::is_tron_resource_ready_candidate(req) && Self::is_insufficient_fee_balance_error(err)
    }

    fn should_fail_tron_platform_delegated_collect_after_build_resource_shortage(
        req: &ApiCollectEntity,
        err: &ServiceError,
    ) -> bool {
        shared_mechanism::is_tron_chain(&req.chain_code)
            && req.token_addr.is_contract()
            && req.raw_tx.is_none()
            && req.resource_gate_released_at.is_some()
            && req.resource_gate_result == Some(ApiResourceGateResult::PlatformDelegateSuccess)
            && Self::is_insufficient_fee_balance_error(err)
    }

    fn should_precheck_collect_amount_before_resource_gate(req: &ApiCollectEntity) -> bool {
        shared_mechanism::is_tron_chain(&req.chain_code)
            && req.raw_tx.is_none()
            && req.transaction_time.is_none()
            && req.finished_at.is_none()
            && req.err_code.is_none()
            && req.resource_gate_released_at.is_none()
    }

    fn is_tron_resource_ready_candidate(req: &ApiCollectEntity) -> bool {
        shared_mechanism::is_tron_chain(&req.chain_code)
            && req.token_addr.is_contract()
            && req.resource_gate_result == Some(ApiResourceGateResult::ResourceReady)
    }

    fn should_reopen_fee_cycle_for_solana_token_collect(
        req: &ApiCollectEntity,
        err: &ServiceError,
    ) -> bool {
        shared_mechanism::is_sol_chain(&req.chain_code)
            && req.token_addr.is_contract()
            && Self::is_solana_rent_exempt_reserve_balance_error(req, err)
    }

    fn should_terminal_fail_solana_token_collect_rent_shortage(
        req: &ApiCollectEntity,
        err: &ServiceError,
    ) -> bool {
        Self::should_reopen_fee_cycle_for_solana_token_collect(req, err)
            && Self::service_fee_cycle_completed(req)
    }

    fn service_fee_cycle_completed(req: &ApiCollectEntity) -> bool {
        req.service_fee_uploaded_at.is_some() && req.tx_fee_res_ack_sent_at.is_some()
    }

    fn should_terminal_fail_fee_shortage_after_completed_fee_cycle(req: &ApiCollectEntity) -> bool {
        Self::should_terminal_fail_fee_shortage_at(req, Utc::now())
    }

    fn should_terminal_fail_fee_shortage_at(req: &ApiCollectEntity, now: DateTime<Utc>) -> bool {
        const EVM_REESTIMATE_WINDOW_SECS: i64 = 180;
        Self::service_fee_cycle_completed(req)
            && (!shared_mechanism::is_evm_chain(&req.chain_code)
                || req.tx_fee_res_ack_sent_at.is_some_and(|ack_at| {
                    now.signed_duration_since(ack_at).num_seconds() >= EVM_REESTIMATE_WINDOW_SECS
                }))
    }

    fn is_solana_rent_exempt_reserve_balance_error(
        req: &ApiCollectEntity,
        err: &ServiceError,
    ) -> bool {
        if !shared_mechanism::is_sol_chain(&req.chain_code) {
            return false;
        }

        match err {
            ServiceError::Business(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::InsufficientBalance(detail),
            )) => detail
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("rent-exempt reserve")),
            ServiceError::Business(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::InsufficientFundsRent,
            )) => true,
            _ => false,
        }
    }

    fn is_collect_amount_insufficient_error(err: &ServiceError) -> bool {
        match err {
            ServiceError::Business(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::InsufficientBalance(detail),
            )) => detail
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("collect amount is insufficient")),
            _ => false,
        }
    }

    fn should_rebroadcast_tron_missing_confirmed_and_pending(req: &ApiCollectEntity) -> bool {
        shared_mechanism::should_retry_after_tron_missing_confirmed_and_pending(
            &req.chain_code,
            req.last_broadcast_at,
            req.broadcast_uncertain_since_at,
            Self::TRON_MISSING_CONFIRMED_AND_PENDING_REBROADCAST_TIMEOUT_SECS,
            Utc::now(),
        )
    }

    fn tron_missing_pools_recovery_action(
        req: &ApiCollectEntity,
    ) -> TronMissingPoolsRecoveryAction {
        if Self::should_rebroadcast_tron_missing_confirmed_and_pending(req) {
            TronMissingPoolsRecoveryAction::Rebroadcast
        } else {
            TronMissingPoolsRecoveryAction::ObserveWithPointToPointRetry
        }
    }

    fn apply_exec_to_addr(req: &mut ApiCollectEntity, exec_to_addr: &str) -> bool {
        if req.to_addr == exec_to_addr {
            return false;
        }
        req.to_addr = exec_to_addr.to_string();
        true
    }

    fn collect_account_activation_trade_no(origin_trade_no: &str) -> String {
        format!("act_{origin_trade_no}")
    }

    fn resource_snapshot_needs_account_activation(snapshot: &ResourceGateSnapshot) -> bool {
        snapshot.required_energy > snapshot.available_energy.max(0) as u64
            && snapshot.energy_price <= 0.0
    }

    /// 创建新的 Shadow Collect Worker
    pub fn new(
        ctx: &'static crate::context::Context,
        address_locks: Arc<AddressLockManager>,
        advancer: Arc<ShadowAdvancer>,
    ) -> Self {
        Self::new_with_fee_reservations(ctx, address_locks, advancer, Default::default())
    }

    pub(crate) fn new_with_fee_reservations(
        ctx: &'static crate::context::Context,
        address_locks: Arc<AddressLockManager>,
        advancer: Arc<ShadowAdvancer>,
        fee_reservations: CollectFeeReservationManager,
    ) -> Self {
        Self {
            ctx,
            address_locks,
            advancer,
            resource_reservations: Default::default(),
            fee_reservations,
        }
    }

    /// Keep the large command state out of every dispatcher branch. This is
    /// a heap boundary, not a spawned task: dropping the caller still cancels
    /// the same operation and releases its existing guards.
    pub fn handle(
        &self,
        cmd: ShadowCollectCommand,
    ) -> impl std::future::Future<Output = Result<(), ServiceError>> + Send + '_ {
        Box::pin(self.handle_inner(cmd))
    }

    async fn handle_inner(&self, cmd: ShadowCollectCommand) -> Result<(), ServiceError> {
        // 提取 trade_no 用于日志
        let trade_no = match &cmd {
            ShadowCollectCommand::ExpireUnexecuted(trade_no) => trade_no,
            ShadowCollectCommand::EvalResourceGate(trade_no) => trade_no,
            ShadowCollectCommand::BuildTx(trade_no) => trade_no,
            ShadowCollectCommand::Broadcast(trade_no) => trade_no,
            ShadowCollectCommand::Recover(trade_no) => trade_no,
            ShadowCollectCommand::ExecuteLocalResourceDelegation(trade_no) => trade_no,
            ShadowCollectCommand::ExecuteAccountActivation(trade_no) => trade_no,
        };

        info!(trade_no = %trade_no, command = ?cmd, source = "shadow_worker_v2", "Received shadow collect command");

        match cmd {
            ShadowCollectCommand::ExpireUnexecuted(trade_no) => {
                self.process_expire_unexecuted(trade_no).await
            }
            ShadowCollectCommand::EvalResourceGate(trade_no) => {
                self.process_resource_gate(trade_no).await
            }
            ShadowCollectCommand::BuildTx(trade_no) => self.process_build_tx(trade_no).await,
            ShadowCollectCommand::Broadcast(trade_no) => self.process_broadcast(trade_no).await,
            ShadowCollectCommand::Recover(trade_no) => self.process_recover(trade_no).await,
            ShadowCollectCommand::ExecuteLocalResourceDelegation(trade_no) => {
                let result = self.process_local_resource_delegation_execute(trade_no.clone()).await;
                self.handle_local_resource_delegation_failure_if_needed(&trade_no, result).await
            }
            ShadowCollectCommand::ExecuteAccountActivation(trade_no) => {
                self.process_account_activation(trade_no).await
            }
        }
    }

    async fn process_expire_unexecuted(&self, trade_no: String) -> Result<(), ServiceError> {
        let cutoff = Utc::now() - chrono::Duration::hours(Self::UNEXECUTED_ORDER_TTL_HOURS);
        let rows = ApiCollectRepo::mark_unexecuted_expired(
            &self.ctx.api_transaction_pool()?,
            &trade_no,
            cutoff,
            ErrCode::UnknownError,
            Self::UNEXECUTED_ORDER_EXPIRED_MESSAGE,
        )
        .await
        .map_err(|error| ServiceError::Database(error.into()))?;

        if rows > 0 {
            info!(
                trade_no = %trade_no,
                ttl_hours = Self::UNEXECUTED_ORDER_TTL_HOURS,
                source = "shadow_worker_v2",
                "Expired unexecuted collect order"
            );
            self.fee_reservations.release(&trade_no);
            self.advancer.try_advance(&trade_no).await;
        }

        Ok(())
    }

    async fn process_account_activation(
        &self,
        activation_trade_no: String,
    ) -> Result<(), ServiceError> {
        let Some(task) = ApiTronAccountActivationRepo::find_by_activation_trade_no(
            &self.ctx.api_resource_pool()?,
            &activation_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?
        else {
            warn!(
                activation_trade_no = %activation_trade_no,
                source = "shadow_worker_v2",
                "Account activation task not found"
            );
            return Ok(());
        };

        if task.account_verified_at.is_some() {
            return Ok(());
        }
        if task.tx_hash.is_some() {
            return self.recover_or_verify_account_activation(task).await;
        }

        if let Err(err) = self.execute_account_activation(task.clone()).await {
            let next_retry_at = (Utc::now()
                + chrono::Duration::seconds(resource_delegation_retry_wait_secs(task.retry_count)))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            ApiTronAccountActivationRepo::mark_retry_wait(
                &self.ctx.api_resource_pool()?,
                &task.activation_trade_no,
                Some("activation_execute_failed"),
                Some(&err.to_string()),
                ApiTronAccountActivationRecoverStatus::RetryBuild,
                &next_retry_at,
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
            warn!(
                activation_trade_no = %task.activation_trade_no,
                error = %err,
                next_retry_at = %next_retry_at,
                source = "shadow_worker_v2",
                "Account activation execution failed; scheduled retry"
            );
        }

        Ok(())
    }

    async fn schedule_account_activation_retry(
        &self,
        task: &ApiTronAccountActivationEntity,
        err_code: &'static str,
        err_msg: &str,
        recover_status: ApiTronAccountActivationRecoverStatus,
    ) -> Result<(), ServiceError> {
        let next_retry_at = (Utc::now()
            + chrono::Duration::seconds(resource_delegation_retry_wait_secs(task.retry_count)))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        ApiTronAccountActivationRepo::mark_retry_wait(
            &self.ctx.api_resource_pool()?,
            &task.activation_trade_no,
            Some(err_code),
            Some(err_msg),
            recover_status,
            &next_retry_at,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        Ok(())
    }

    async fn execute_account_activation(
        &self,
        task: ApiTronAccountActivationEntity,
    ) -> Result<(), ServiceError> {
        let claimed = ApiTronAccountActivationRepo::claim_build_slot(
            &self.ctx.api_resource_pool()?,
            &task.activation_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if claimed == 0 {
            info!(
                activation_trade_no = %task.activation_trade_no,
                source = "shadow_worker_v2",
                "Account activation task already claimed or no longer executable"
            );
            return Ok(());
        }

        let handles = self.ctx.get_handles_arc().await?;
        let private_key_manager = handles.get_global_private_key_manager();
        let private_key =
            private_key_manager.get_private_key(&task.payer_address, &task.chain_code).await?;
        let unlock_token = ApiWalletDomain::get_wallet_unlock_token().await?;

        let mut params = ApiBaseTransferReq::new(
            &task.payer_address,
            &task.target_address,
            &task.amount,
            &task.chain_code,
        );
        params.with_token(AssetTokenKey::Native, 6, "TRX");
        let transfer_req = ApiTransferReq { base: params, password: unlock_token, nonce: 0 };
        let Some((tx_hash, raw_tx, _fee)) =
            crate::infrastructure::chain_rpc_guard::run_guarded_with_ctx_and_log(
                self.ctx,
                &task.chain_code,
                &task.activation_trade_no,
                "account_activation_build",
                "shadow_worker_v2",
                crate::infrastructure::chain_rpc_guard::ChainRpcFailureLog::new()
                    .trade_no(&task.activation_trade_no)
                    .chain_code(&task.chain_code)
                    .source("shadow_worker_v2")
                    .op("account_activation_build"),
                crate::domain::api_wallet::trans::ApiTransDomain::build_transfer_raw(
                    &self.ctx,
                    transfer_req,
                    Some(private_key),
                ),
            )
            .await?
        else {
            self.schedule_account_activation_retry(
                &task,
                "activation_build_skipped",
                "account activation build skipped by RPC guard",
                ApiTronAccountActivationRecoverStatus::RetryBuild,
            )
            .await?;
            return Ok(());
        };

        let raw_tx_str = wallet_utils::serde_func::serde_to_string(&raw_tx)?;
        let tx_resp = crate::infrastructure::chain_rpc_guard::run_guarded_with_ctx_and_log(
            self.ctx,
            &task.chain_code,
            &task.activation_trade_no,
            "account_activation_broadcast",
            "shadow_worker_v2",
            crate::infrastructure::chain_rpc_guard::ChainRpcFailureLog::new()
                .trade_no(&task.activation_trade_no)
                .tx_hash(&tx_hash)
                .chain_code(&task.chain_code)
                .source("shadow_worker_v2")
                .op("account_activation_broadcast"),
            crate::domain::api_wallet::trans::ApiTransDomain::broadcast_transfer(
                &self.ctx,
                &task.chain_code,
                raw_tx,
                Some(&tx_hash),
            ),
        )
        .await?
        .flatten();

        let Some(resp) = tx_resp else {
            self.schedule_account_activation_retry(
                &task,
                "activation_broadcast_uncertain",
                "account activation broadcast returned no committed response",
                ApiTronAccountActivationRecoverStatus::RetryBuild,
            )
            .await?;
            return Ok(());
        };

        ApiTronAccountActivationRepo::mark_broadcast_success(
            &self.ctx.api_resource_pool()?,
            &task.activation_trade_no,
            &resp.tx_hash,
            &raw_tx_str,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(
            activation_trade_no = %task.activation_trade_no,
            tx_hash = %resp.tx_hash,
            target_address = %task.target_address,
            source = "shadow_worker_v2",
            "Account activation broadcast committed"
        );
        self.advancer.try_advance(&task.origin_trade_no).await;

        Ok(())
    }

    async fn verify_account_activation(
        &self,
        task: ApiTronAccountActivationEntity,
    ) -> Result<(), ServiceError> {
        let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(self.ctx).await?;
        let account = chain.account_info(&task.target_address).await?;
        if account.address.is_empty() {
            info!(
                activation_trade_no = %task.activation_trade_no,
                target_address = %task.target_address,
                source = "shadow_worker_v2",
                "Account activation not visible on chain yet"
            );
            return Ok(());
        }

        let rows = ApiTronAccountActivationRepo::mark_account_verified(
            &self.ctx.api_resource_pool()?,
            &task.activation_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if rows == 0 {
            return Ok(());
        }

        if let Some(collect) = ApiCollectRepo::find_by_resource_dependency_trade_no(
            &self.ctx.api_transaction_pool()?,
            &task.activation_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?
        {
            let rows = ApiCollectRepo::clear_account_activation_block_for_reeval(
                &self.ctx.api_transaction_pool()?,
                &collect.trade_no,
                &task.activation_trade_no,
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
            if rows > 0 {
                self.advancer.try_advance(&collect.trade_no).await;
            }
        }

        info!(
            activation_trade_no = %task.activation_trade_no,
            target_address = %task.target_address,
            source = "shadow_worker_v2",
            "Account activation verified and collect resource gate requeued"
        );

        Ok(())
    }

    async fn recover_or_verify_account_activation(
        &self,
        task: ApiTronAccountActivationEntity,
    ) -> Result<(), ServiceError> {
        if task.chain_success_at.is_some() {
            return self.verify_account_activation(task).await;
        }

        let Some(tx_hash) = task.tx_hash.as_deref().filter(|s| !s.trim().is_empty()) else {
            return Ok(());
        };

        match crate::domain::api_wallet::trans::ApiTransDomain::process_recovered_tx(
            &self.ctx,
            &task.chain_code,
            &task.payer_address,
            tx_hash,
            0,
            "0",
        )
        .await
        {
            Ok(Some(_resp)) => {
                ApiTronAccountActivationRepo::mark_chain_success(
                    &self.ctx.api_resource_pool()?,
                    &task.activation_trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                let refreshed = ApiTronAccountActivationRepo::find_by_activation_trade_no(
                    &self.ctx.api_resource_pool()?,
                    &task.activation_trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?
                .unwrap_or(task);
                self.verify_account_activation(refreshed).await
            }
            Ok(None) => {
                self.schedule_account_activation_retry(
                    &task,
                    "activation_recover_uncertain",
                    "account activation recover returned no final result",
                    ApiTronAccountActivationRecoverStatus::RetryRecover,
                )
                .await
            }
            Err(err) if Self::is_recovered_tx_terminal_chain_failure(&err) => {
                let next_retry_at = (Utc::now()
                    + chrono::Duration::seconds(resource_delegation_retry_wait_secs(
                        task.retry_count,
                    )))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
                ApiTronAccountActivationRepo::mark_chain_failed_retry(
                    &self.ctx.api_resource_pool()?,
                    &task.activation_trade_no,
                    Some("activation_chain_failed"),
                    Some(&err.to_string()),
                    ApiTronAccountActivationRecoverStatus::RetryBuild,
                    &next_retry_at,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                warn!(
                    activation_trade_no = %task.activation_trade_no,
                    tx_hash = %tx_hash,
                    error = %err,
                    next_retry_at = %next_retry_at,
                    source = "shadow_worker_v2",
                    "Account activation tx failed on chain; scheduled rebuild retry"
                );
                Ok(())
            }
            Err(err) => {
                self.schedule_account_activation_retry(
                    &task,
                    "activation_recover_error",
                    &err.to_string(),
                    ApiTronAccountActivationRecoverStatus::RetryRecover,
                )
                .await
            }
        }
    }

    async fn handle_local_resource_delegation_failure_if_needed(
        &self,
        resource_trade_no: &str,
        result: Result<(), ServiceError>,
    ) -> Result<(), ServiceError> {
        let Err(err) = result else {
            return Ok(());
        };

        if err.is_transient_db_pool_timeout() {
            self.schedule_local_resource_delegation_rebuild_retry(resource_trade_no, &err).await?;
            warn!(
                resource_trade_no = %resource_trade_no,
                error = %err,
                source = "shadow_worker_v2",
                "Resource delegation hit local DB pool timeout, scheduled retry instead of reporting failure"
            );
            return Ok(());
        }

        match err.retry_policy() {
            wallet_utils::RetryPolicy::Never => {
                warn!(
                    resource_trade_no = %resource_trade_no,
                    error = %err,
                    source = "shadow_worker_v2",
                    "Resource delegation execution failed permanently"
                );
                if self.mark_local_resource_delegation_failed(resource_trade_no, &err).await? {
                    self.release_collect_gate_after_local_delegation_failure(resource_trade_no)
                        .await?;
                }
                Ok(())
            }
            wallet_utils::RetryPolicy::Delay => {
                self.schedule_local_resource_delegation_rebuild_retry(resource_trade_no, &err)
                    .await?;
                info!(
                    resource_trade_no = %resource_trade_no,
                    error = %err,
                    source = "shadow_worker_v2",
                    "Resource delegation terminal step failed, will retry later"
                );
                Ok(())
            }
        }
    }

    async fn schedule_local_resource_delegation_rebuild_retry(
        &self,
        resource_trade_no: &str,
        err: &ServiceError,
    ) -> Result<(), ServiceError> {
        Self::schedule_local_resource_delegation_rebuild_retry_in_pool(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
            err,
        )
        .await
    }

    async fn schedule_local_resource_delegation_rebuild_retry_in_pool(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
        err: &ServiceError,
    ) -> Result<(), ServiceError> {
        let task = ApiResourceDelegationRepo::get_by_resource_trade_no(pool, resource_trade_no)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
        let affected = ApiResourceDelegationRepo::reset_for_rebuild(pool, resource_trade_no)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;

        info!(
            resource_trade_no = %resource_trade_no,
            origin_trade_no = ?task.origin_trade_no,
            affected = %affected,
            error = %err,
            source = "shadow_worker_v2",
            "Resource delegation reset for rebuild"
        );

        Ok(())
    }

    async fn process_local_resource_delegation_execute(
        &self,
        resource_trade_no: String,
    ) -> Result<(), ServiceError> {
        use crate::infrastructure::api_trans::resource_delegate::local_execution::{
            LocalDelegateExecutionOutcome, execute_local_delegate_step,
        };

        let outcome = execute_local_delegate_step(
            self.ctx,
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
            "collect_resource_delegation",
        )
        .await?;
        match outcome {
            LocalDelegateExecutionOutcome::Skipped => {}
            LocalDelegateExecutionOutcome::Broadcasted { tx_hash } => {
                info!(
                    resource_trade_no,
                    tx_hash = %tx_hash,
                    source = "shadow_worker_v2",
                    "Resource delegation broadcast fact committed"
                );
                self.schedule_local_delegation_post_broadcast_confirms(
                    &resource_trade_no,
                    &tx_hash,
                );
            }
            LocalDelegateExecutionOutcome::RecoveredSuccess(refreshed) => {
                self.release_collect_gate_after_local_delegation_success(&refreshed).await?;
            }
            LocalDelegateExecutionOutcome::RecoveryUncertain { rebroadcasted, origin_trade_no } => {
                if rebroadcasted {
                    if let Some(origin_trade_no) = origin_trade_no.as_deref() {
                        self.advancer.try_advance(origin_trade_no).await;
                    }
                }
            }
            LocalDelegateExecutionOutcome::RecoveredFailure(failed) => {
                self.release_collect_gate_after_local_delegation_failure(&failed.resource_trade_no)
                    .await?;
            }
        }

        Ok(())
    }

    fn schedule_local_delegation_post_broadcast_confirms(
        &self,
        resource_trade_no: &str,
        tx_hash: &str,
    ) {
        for delay_secs in Self::LOCAL_DELEGATION_POST_BROADCAST_CONFIRM_DELAYS_SECS {
            let advancer = self.advancer.clone();
            let resource_trade_no = resource_trade_no.to_string();
            let tx_hash = tx_hash.to_string();
            info!(
                resource_trade_no = %resource_trade_no,
                tx_hash = %tx_hash,
                delay_secs = delay_secs,
                source = "shadow_worker_v2",
                "Scheduled local delegation post-broadcast confirmation"
            );
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                info!(
                    resource_trade_no = %resource_trade_no,
                    tx_hash = %tx_hash,
                    delay_secs = delay_secs,
                    source = "shadow_worker_v2",
                    "Triggering local delegation post-broadcast confirmation"
                );
                advancer.trigger_local_delegation_fast_path(&resource_trade_no);
            });
        }
    }

    async fn mark_local_resource_delegation_failed(
        &self,
        resource_trade_no: &str,
        err: &ServiceError,
    ) -> Result<bool, ServiceError> {
        if err.is_transient_db_pool_timeout() {
            warn!(
                resource_trade_no = %resource_trade_no,
                error = %err,
                source = "shadow_worker_v2",
                "Skip marking resource delegation failed: local DB pool timeout is retryable infrastructure noise"
            );
            return Ok(false);
        }

        // 统一失败 fact 映射，保持与其他 resource delegation 统一上报约定。
        let (err_code, err_msg) = resource_delegation_failure_fact(err);
        let payload = format!("local_delegation_terminal_failure:{err_msg}");
        let affected = ApiResourceDelegationRepo::mark_failed_with_result_if_unfinished(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
            None,
            &err_code,
            &err_msg,
            &payload,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if affected == 0 {
            info!(
                resource_trade_no = %resource_trade_no,
                err_code = %err_code,
                err_msg = %err_msg,
                source = "shadow_worker_v2",
                "Resource delegation failure fact already committed or no longer eligible"
            );
        } else {
            info!(
                resource_trade_no = %resource_trade_no,
                err_code = %err_code,
                err_msg = %err_msg,
                source = "shadow_worker_v2",
                "Resource delegation failure fact committed"
            );
        }

        if affected > 0 {
            return Ok(true);
        }

        let persisted = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        Ok(persisted.result_status == Some(ApiResourceDelegationResultStatus::Fail)
            && persisted.finished_at.is_some())
    }

    /// 执行资源闸门检查，入参是原归集订单号，不是资源任务号。
    ///
    /// 边界说明：
    /// - 这里的“评估资源闸门”是一个真实操作步骤
    /// - `resource_ready` / `need_platform_delegate` 只是这一步落下的结果事实
    /// - worker 负责提交事实，不负责继续调度；后续推进由 scanner/advancer 基于事实完成
    async fn process_resource_gate(&self, origin_trade_no: String) -> Result<(), ServiceError> {
        info!(origin_trade_no = %origin_trade_no, source = "shadow_worker_v2", "Processing resource gate command");

        if let Err(err) = self.process_resource_gate_inner(&origin_trade_no).await {
            self.fee_reservations.release(&origin_trade_no);
            error!(origin_trade_no = %origin_trade_no, error = %err, source = "shadow_worker_v2", "Resource gate check failed");
            return Err(err);
        }

        Ok(())
    }

    async fn process_resource_gate_inner(&self, origin_trade_no: &str) -> Result<(), ServiceError> {
        let req = self.get_collect_entity(origin_trade_no).await?;

        if !shared_mechanism::is_tron_chain(&req.chain_code) {
            return Ok(());
        }

        if !Self::resource_gate_prerequisites_ready(&req) {
            info!(
                origin_trade_no = %origin_trade_no,
                source = "shadow_worker_v2",
                "Collect resource gate skipped until order ACK is persisted"
            );
            self.advancer.try_advance(origin_trade_no).await;
            return Ok(());
        }

        if Self::resource_gate_already_resolved(&req) {
            info!(
                origin_trade_no = %origin_trade_no,
                source = "shadow_worker_v2",
                "Resource gate already resolved or collect is no longer eligible"
            );
            return Ok(());
        }

        if Self::should_precheck_collect_amount_before_resource_gate(&req)
            && !self.check_collect_amount(&req).await?
        {
            info!(
                origin_trade_no = %origin_trade_no,
                source = "shadow_worker_v2",
                "Collect amount insufficient before resource gate; marking collect failed without delegation"
            );
            self.mark_collect_amount_insufficient(&req).await?;
            return Ok(());
        }

        let exec_to_addr = self.resolve_collect_to_addr(&req).await?;
        let Some(snapshot) = self
            .eval_collect_resource_gate_snapshot_guarded(&req, &exec_to_addr, origin_trade_no)
            .await?
        else {
            return Ok(());
        };

        if !self.reserve_collect_service_fee_before_resource_gate(&req, snapshot).await? {
            self.mark_collect_fee_funding_insufficient(&req).await?;
            return Ok(());
        }

        // 资源顺序只处理到“允许回到旧 collect 主链”为止：
        // 自身资源 -> 平台代理 -> 本地代理 fallback -> release gate。
        // release 之后，主币不足 / 补币 / 原失败收口仍走上一版已经稳定的旧闭环。
        let (next_step, effective_snapshot) = self
            .decide_collect_resource_gate_next_step(&req, snapshot.clone(), origin_trade_no)
            .await?;
        self.apply_collect_resource_gate_next_step(
            next_step,
            origin_trade_no,
            &req,
            &exec_to_addr,
            effective_snapshot,
        )
        .await?;

        info!(
            origin_trade_no,
            required_energy = %snapshot.required_energy,
            available_energy = %snapshot.available_energy,
            required_bandwidth = %snapshot.required_bandwidth,
            available_bandwidth = %snapshot.available_bandwidth,
            source = "shadow_worker_v2",
            "TRON collect resource gate evaluated"
        );

        Ok(())
    }

    fn resource_gate_already_resolved(req: &ApiCollectEntity) -> bool {
        req.resource_gate_released_at.is_some()
            || req.raw_tx.is_some()
            || req.transaction_time.is_some()
            || req.finished_at.is_some()
            || req.err_code.is_some()
    }

    fn resource_gate_prerequisites_ready(req: &ApiCollectEntity) -> bool {
        req.order_ack_sent_at.is_some()
    }

    async fn process_release_resource_gate(
        &self,
        origin_trade_no: String,
        snapshot: Option<ResourceGateSnapshot>,
    ) -> Result<(), ServiceError> {
        let req = self.get_collect_entity(&origin_trade_no).await?;
        if !shared_mechanism::is_tron_chain(&req.chain_code) {
            return Ok(());
        }
        if req.resource_gate_released_at.is_some() {
            info!(
                origin_trade_no = %origin_trade_no,
                source = "shadow_worker_v2",
                "Resource gate already released"
            );
            return Ok(());
        }
        if req.raw_tx.is_some()
            || req.transaction_time.is_some()
            || req.finished_at.is_some()
            || req.err_code.is_some()
        {
            info!(
                origin_trade_no = %origin_trade_no,
                source = "shadow_worker_v2",
                "Skip release resource gate: collect no longer eligible"
            );
            return Ok(());
        }

        // The resource gate is a pre-BuildTx fact. Releasing it only records
        // that the collect order may enter the existing BuildTx flow; raw_tx
        // and tx_hash are still produced by the normal build path.
        let rows = if let Some(snapshot) = snapshot {
            let estimated_resource_consume = Self::tron_fee_estimate_resource_json(
                snapshot.required_energy,
                snapshot.required_bandwidth,
            )?;
            ApiCollectRepo::mark_resource_released_with_estimate(
                &self.ctx.api_transaction_pool()?,
                &origin_trade_no,
                ApiResourceGateResult::ResourceReady,
                &estimated_resource_consume,
            )
            .await
        } else {
            ApiCollectRepo::mark_resource_released(
                &self.ctx.api_transaction_pool()?,
                &origin_trade_no,
                ApiResourceGateResult::ResourceReady,
            )
            .await
        }
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(
            origin_trade_no = %origin_trade_no,
            rows = %rows,
            source = "shadow_worker_v2",
            "TRON collect resource gate released"
        );
        self.advancer.try_advance(&origin_trade_no).await;
        Ok(())
    }

    async fn process_block_on_platform_delegation(
        &self,
        origin_trade_no: String,
    ) -> Result<(), ServiceError> {
        let req = self.get_collect_entity(&origin_trade_no).await?;
        if !shared_mechanism::is_tron_chain(&req.chain_code) {
            return Ok(());
        }
        if Self::resource_gate_already_resolved(&req) {
            info!(
                origin_trade_no = %origin_trade_no,
                source = "shadow_worker_v2",
                "Skip block on platform delegation: resource gate already resolved"
            );
            return Ok(());
        }

        let exec_to_addr = self.resolve_collect_to_addr(&req).await?;
        let fee_details = self
            .eval_collect_resource_gate_snapshot_guarded(&req, &exec_to_addr, &origin_trade_no)
            .await?;
        let Some(snapshot) = fee_details else {
            return Ok(());
        };

        self.commit_platform_delegation_block(
            &origin_trade_no,
            &req,
            &exec_to_addr,
            snapshot.required_energy,
            snapshot.required_bandwidth,
            snapshot.available_energy,
            snapshot.available_bandwidth,
            snapshot.energy_price,
        )
        .await
    }

    async fn eval_collect_resource_gate_snapshot_guarded(
        &self,
        req: &ApiCollectEntity,
        exec_to_addr: &str,
        origin_trade_no: &str,
    ) -> Result<Option<ResourceGateSnapshot>, ServiceError> {
        crate::infrastructure::chain_rpc_guard::run_guarded_with_ctx_and_log(
            self.ctx,
            &req.chain_code,
            origin_trade_no,
            "resource_gate",
            "shadow_worker_v2",
            crate::infrastructure::chain_rpc_guard::ChainRpcFailureLog::new()
                .trade_no(origin_trade_no)
                .chain_code(&req.chain_code)
                .exec_to_addr(exec_to_addr)
                .source("shadow_worker_v2")
                .op("resource_gate"),
            self.eval_collect_resource_gate_snapshot(req, exec_to_addr),
        )
        .await
    }

    async fn eval_collect_resource_gate_snapshot(
        &self,
        req: &ApiCollectEntity,
        exec_to_addr: &str,
    ) -> Result<ResourceGateSnapshot, ServiceError> {
        let resource_gate_semaphore =
            crate::infrastructure::api_trans::shadow_rpc_policy::shared_resource_gate_semaphore();
        let wait_started = std::time::Instant::now();
        info!(
            origin_trade_no = %req.trade_no,
            available_permits = resource_gate_semaphore.available_permits(),
            source = "shadow_worker_v2",
            "Collect resource gate worker wait started after chain RPC admission"
        );
        let _resource_gate_permit = resource_gate_semaphore
            .acquire_owned()
            .await
            .map_err(|_| ServiceError::Parameter("shared resource gate semaphore closed".into()))?;
        info!(
            origin_trade_no = %req.trade_no,
            queue_wait_ms = wait_started.elapsed().as_millis(),
            source = "shadow_worker_v2",
            "Collect resource gate worker permit acquired"
        );

        let ctx = self.ctx;
        // 这里只做“评估快照”：
        // - 估算如果现在 BuildTx，需要多少资源
        // - 读取子账户当前链上资源余额
        // 不在这里决定走平台代理还是本地代理，也不直接写 blocked/released 事实。
        let main_coin = ApiChainTransDomain::main_coin(ctx, &req.chain_code).await?;
        let (token_symbol, token_key, token_decimals) = if req.token_addr.is_contract() {
            let token_coin = ApiCoinDomain::get_coin_by_token_key_exact(
                self.ctx,
                &req.chain_code,
                req.token_addr.clone(),
            )
            .await?;
            (token_coin.symbol, token_coin.token_address, token_coin.decimals)
        } else {
            (main_coin.symbol.clone(), AssetTokenKey::Native, main_coin.decimals)
        };

        let fee_details = self
            .estimate_tron_fee_details(
                &req.from_addr,
                exec_to_addr,
                &req.value,
                &token_symbol,
                &main_coin.symbol,
                token_key,
                token_decimals,
            )
            .await?;
        let adapter =
            ApiChainAdapterFactory::get_transaction_adapter_with_ctx(self.ctx, &req.chain_code)
                .await?;
        let resource = adapter.account_resource(&req.from_addr).await?;

        Ok(ResourceGateSnapshot {
            estimated_fee_trx: Self::collect_pre_delegation_fee_trx(
                fee_details.bandwidth,
                fee_details.user_bandwidth,
                fee_details.bandwidth_price,
                fee_details.extra_fee,
            ),
            required_energy: fee_details.energy,
            required_bandwidth: fee_details.bandwidth,
            available_energy: resource.available_energy(),
            available_bandwidth: resource.available_bandwidth(),
            energy_price: resource.energy_price(),
        })
    }

    fn trx_to_sun_ceil(amount: Decimal) -> Result<i64, ServiceError> {
        if amount <= Decimal::ZERO {
            return Ok(0);
        }
        (amount * Decimal::from(1_000_000u64))
            .ceil()
            .to_i64()
            .ok_or_else(|| ServiceError::Parameter("TRON fee reservation exceeds i64".into()))
    }

    fn collect_pre_delegation_fee_trx(
        required_bandwidth: u64,
        user_bandwidth: u64,
        bandwidth_price: f64,
        extra_fee: f64,
    ) -> f64 {
        let bandwidth_fee = if user_bandwidth >= required_bandwidth {
            0.0
        } else {
            required_bandwidth as f64 * bandwidth_price
        };
        bandwidth_fee + extra_fee
    }

    fn trx_balance_to_sun(amount: Decimal) -> Result<i64, ServiceError> {
        if amount <= Decimal::ZERO {
            return Ok(0);
        }
        (amount * Decimal::from(1_000_000u64))
            .floor()
            .to_i64()
            .ok_or_else(|| ServiceError::Parameter("TRON funding balance exceeds i64".into()))
    }

    async fn reserve_collect_service_fee_before_resource_gate(
        &self,
        req: &ApiCollectEntity,
        snapshot: ResourceGateSnapshot,
    ) -> Result<bool, ServiceError> {
        if !req.token_addr.is_contract() {
            return Ok(true);
        }

        let estimated_fee = conversion::decimal_from_f64(snapshot.estimated_fee_trx)?;
        let child_balance = conversion::decimal_from_str(
            &self.query_balance(&req.from_addr, ChainCode::Tron, AssetTokenKey::Native, 6).await?,
        )?;
        let fee_top_up_sun =
            Self::trx_to_sun_ceil((estimated_fee - child_balance).max(Decimal::ZERO))?;
        if fee_top_up_sun == 0 {
            return Ok(true);
        }

        let funding_addr =
            Self::resolve_withdraw_from_addr(self.ctx, &self.ctx.api_wallet_pool()?, req).await?;
        let funding_tx_fee = self
            .estimate_tron_fee_details(
                &funding_addr,
                &req.from_addr,
                &Decimal::new(fee_top_up_sun, 6).to_string(),
                "TRX",
                "TRX",
                AssetTokenKey::Native,
                6,
            )
            .await?;
        let funding_tx_fee_sun = Self::trx_to_sun_ceil(conversion::decimal_from_f64(
            funding_tx_fee.estimate_fee.amount,
        )?)?;
        let required_sun = fee_top_up_sun
            .checked_add(funding_tx_fee_sun)
            .ok_or_else(|| ServiceError::Parameter("TRON fee reservation overflow".into()))?;
        let funding_balance = conversion::decimal_from_str(
            &self.query_balance(&funding_addr, ChainCode::Tron, AssetTokenKey::Native, 6).await?,
        )?;
        let available_sun = Self::trx_balance_to_sun(funding_balance)?;

        let funding_key = format!("{}:{}", req.chain_code.to_ascii_lowercase(), funding_addr);
        match self.fee_reservations.reserve_if_available(
            funding_key,
            req.trade_no.clone(),
            required_sun as u64,
            available_sun as u64,
        ) {
            FeeReservationDecision::Reserved { total_reserved } => {
                info!(
                    trade_no = %req.trade_no,
                    funding_addr = %funding_addr,
                    fee_top_up_sun,
                    funding_tx_fee_sun,
                    required_sun,
                    available_sun,
                    total_reserved_sun = total_reserved,
                    source = "shadow_worker_v2",
                    "Reserved collect service fee before resource delegation"
                );
                Ok(true)
            }
            FeeReservationDecision::Insufficient { reserved, .. } => {
                warn!(
                    trade_no = %req.trade_no,
                    funding_addr = %funding_addr,
                    fee_top_up_sun,
                    funding_tx_fee_sun,
                    required_sun,
                    available_sun,
                    reserved_sun = reserved,
                    source = "shadow_worker_v2",
                    "Withdrawal wallet cannot reserve collect service fee before resource delegation"
                );
                Ok(false)
            }
        }
    }

    async fn mark_collect_fee_funding_insufficient(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<(), ServiceError> {
        ApiCollectRepo::update_api_collect_status_and_err(
            &self.ctx.api_transaction_pool()?,
            &req.trade_no,
            ApiCollectStatus::InsufficientBalance,
            ErrCode::BalanceInsufficient,
            "withdrawal wallet balance is insufficient for collect service fee reservation",
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        self.advancer.try_advance(&req.trade_no).await;
        Ok(())
    }

    async fn decide_collect_resource_gate_next_step(
        &self,
        req: &ApiCollectEntity,
        mut snapshot: ResourceGateSnapshot,
        origin_trade_no: &str,
    ) -> Result<(ResourceGateNextStep, ResourceGateSnapshot), ServiceError> {
        let pending_budget = ApiCollectRepo::pending_resource_budget_by_owner(
            &self.ctx.api_transaction_pool()?,
            &req.chain_code,
            &req.from_addr,
            origin_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        let required = Self::tron_collect_required_budget(
            snapshot.required_energy,
            snapshot.required_bandwidth,
        );
        let available = Self::effective_resource_after_pending_collects(
            snapshot.available_energy,
            snapshot.available_bandwidth,
            pending_budget,
        );
        let decision = self.resource_reservations.reserve_if_available(
            &req.from_addr,
            origin_trade_no.to_string(),
            required,
            available,
        );

        if let ResourceReservationDecision::Reserved { effective_available, reserved } = decision {
            info!(
                origin_trade_no,
                required_energy = %snapshot.required_energy,
                available_energy = %snapshot.available_energy,
                pending_energy = %pending_budget.energy,
                reserved_energy = %reserved.energy,
                effective_energy = %effective_available.energy,
                source = "shadow_worker_v2",
                "TRON collect resource gate reserved address-level ResourceReady budget"
            );
            snapshot.available_energy = effective_available.energy.min(i64::MAX as u64) as i64;
            snapshot.available_bandwidth =
                effective_available.bandwidth.min(i64::MAX as u64) as i64;
            return Ok((ResourceGateNextStep::Release, snapshot));
        }

        let ResourceReservationDecision::NeedMore(shortfall) = decision else {
            unreachable!("reserved decision returned early")
        };
        snapshot.available_energy =
            shortfall.effective_available.energy.min(i64::MAX as u64) as i64;
        snapshot.available_bandwidth =
            shortfall.effective_available.bandwidth.min(i64::MAX as u64) as i64;

        if Self::resource_snapshot_needs_account_activation(&snapshot) {
            return Ok((ResourceGateNextStep::BlockOnAccountActivation, snapshot));
        }

        let delegations = ApiResourceDelegationRepo::list_by_origin_trade_no(
            &self.ctx.api_resource_pool()?,
            origin_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        let next_step = match Self::decide_collect_resource_block_path(req, &delegations) {
            ResourceDelegationBlockPath::LocalFallback => ResourceGateNextStep::BlockOnLocal,
            ResourceDelegationBlockPath::PlatformFallback => ResourceGateNextStep::BlockOnPlatform,
        };

        Ok((next_step, snapshot))
    }

    /// Main collect resource-gate skeleton:
    /// 1. evaluate current resource snapshot
    /// 2. decide the next step from facts
    /// 3. commit only the facts for that one step
    async fn apply_collect_resource_gate_next_step(
        &self,
        next_step: ResourceGateNextStep,
        origin_trade_no: &str,
        req: &ApiCollectEntity,
        exec_to_addr: &str,
        snapshot: ResourceGateSnapshot,
    ) -> Result<(), ServiceError> {
        match next_step {
            ResourceGateNextStep::Release => {
                self.process_release_resource_gate(origin_trade_no.to_string(), Some(snapshot))
                    .await
            }
            ResourceGateNextStep::BlockOnAccountActivation => {
                self.commit_account_activation_block(
                    origin_trade_no,
                    req,
                    exec_to_addr,
                    snapshot.required_energy,
                    snapshot.available_energy,
                    snapshot.energy_price,
                )
                .await
            }
            ResourceGateNextStep::BlockOnPlatform => {
                self.commit_platform_delegation_block(
                    origin_trade_no,
                    req,
                    exec_to_addr,
                    snapshot.required_energy,
                    snapshot.required_bandwidth,
                    snapshot.available_energy,
                    snapshot.available_bandwidth,
                    snapshot.energy_price,
                )
                .await
            }
            ResourceGateNextStep::BlockOnLocal => {
                self.commit_local_delegation_block(
                    origin_trade_no,
                    req,
                    exec_to_addr,
                    snapshot.required_energy,
                    snapshot.available_energy,
                    snapshot.energy_price,
                )
                .await
            }
        }
    }

    async fn commit_account_activation_block(
        &self,
        origin_trade_no: &str,
        req: &ApiCollectEntity,
        payer_address: &str,
        required_energy: u64,
        available_energy: i64,
        energy_price: f64,
    ) -> Result<(), ServiceError> {
        Self::commit_account_activation_block_in_pools(
            &self.ctx.api_transaction_pool()?,
            &self.ctx.api_resource_pool()?,
            origin_trade_no,
            req,
            payer_address,
            required_energy,
            available_energy,
            energy_price,
        )
        .await
    }

    async fn commit_account_activation_block_in_pools(
        transaction_pool: &ApiTransactionDbPool,
        resource_pool: &ApiResourceDbPool,
        origin_trade_no: &str,
        req: &ApiCollectEntity,
        payer_address: &str,
        required_energy: u64,
        available_energy: i64,
        energy_price: f64,
    ) -> Result<(), ServiceError> {
        let activation_trade_no = Self::collect_account_activation_trade_no(origin_trade_no);
        ApiTronAccountActivationRepo::upsert(
            resource_pool,
            NewApiTronAccountActivation::collect(
                req.uid.clone(),
                origin_trade_no.to_string(),
                activation_trade_no.clone(),
                payer_address.to_string(),
                req.from_addr.clone(),
            ),
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        let rows = ApiCollectRepo::mark_resource_blocked(
            transaction_pool,
            origin_trade_no,
            ApiResourceBlockReason::NeedAccountActivation,
            Some(&activation_trade_no),
            Some(ApiResourceDependencyType::AccountActivation),
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        warn!(
            origin_trade_no = %origin_trade_no,
            rows = %rows,
            activation_trade_no = %activation_trade_no,
            payer_address = %payer_address,
            inactive_address = %req.from_addr,
            required_energy = %required_energy,
            available_energy = %available_energy,
            energy_price = %energy_price,
            source = "shadow_worker_v2",
            "TRON collect resource gate blocked by inactive account"
        );
        Ok(())
    }

    fn decide_collect_resource_block_path(
        req: &ApiCollectEntity,
        delegations: &[ApiResourceDelegationEntity],
    ) -> ResourceDelegationBlockPath {
        // 文档顺序要求：
        // 子账户自身能量 -> 平台资源代理 -> 出款地址本地代理 -> 后续主链
        //
        // 所以只要平台代理已经失败，或者本地 fallback 已经存在，
        // 下一次 EvalResourceGate 就必须把 blocked 事实切到 local_delegate。
        let has_local_fallback = delegations.iter().any(|delegation| {
            delegation.source == ApiResourceDelegationSource::Local
                && delegation.operation_type == ApiResourceDelegationOperationType::Delegate
        });
        let platform_failed = delegations.iter().any(|delegation| {
            delegation.source == ApiResourceDelegationSource::Platform
                && delegation.operation_type == ApiResourceDelegationOperationType::Delegate
                && (delegation.err_code.is_some() || delegation.chain_failed_at.is_some())
        });

        if req.resource_block_reason == Some(ApiResourceBlockReason::NeedLocalDelegate)
            || has_local_fallback
            || platform_failed
        {
            ResourceDelegationBlockPath::LocalFallback
        } else {
            ResourceDelegationBlockPath::PlatformFallback
        }
    }

    async fn commit_platform_delegation_block(
        &self,
        origin_trade_no: &str,
        req: &ApiCollectEntity,
        exec_to_addr: &str,
        required_energy: u64,
        required_bandwidth: u64,
        available_energy: i64,
        available_bandwidth: i64,
        energy_price: f64,
    ) -> Result<(), ServiceError> {
        let amount = Self::collect_delegation_energy_amount(required_energy).to_string();
        let resource_trade_no = match self
            .apply_platform_resource_delegation(
                &req.uid,
                origin_trade_no,
                &req.chain_code,
                &req.from_addr,
                &amount,
            )
            .await?
        {
            PlatformApplyOutcome::AcceptedWithResourceTradeNo(resource_trade_no) => {
                Some(resource_trade_no)
            }
            PlatformApplyOutcome::AcceptedWithOriginalTradeNo => None,
            PlatformApplyOutcome::Rejected => {
                return self
                    .commit_local_delegation_block(
                        origin_trade_no,
                        req,
                        &exec_to_addr,
                        required_energy,
                        available_energy,
                        energy_price,
                    )
                    .await;
            }
        };

        // 商户侧只记录“原单正在等待平台代理结果”这个事实。
        // 真正的平台代理订单由平台钱包接收并执行，不在商户钱包本地创建任务行。
        let rows = ApiCollectRepo::mark_resource_blocked(
            &self.ctx.api_transaction_pool()?,
            origin_trade_no,
            ApiResourceBlockReason::NeedPlatformDelegate,
            resource_trade_no.as_deref(),
            Some(ApiResourceDependencyType::PlatformDelegate),
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(
            origin_trade_no = %origin_trade_no,
            rows = %rows,
            required_energy = %required_energy,
            available_energy = %available_energy,
            required_bandwidth = %required_bandwidth,
            available_bandwidth = %available_bandwidth,
            resource_trade_no = ?resource_trade_no,
            source = "shadow_worker_v2",
            "TRON collect resource gate blocked"
        );

        Ok(())
    }

    async fn apply_platform_resource_delegation(
        &self,
        uid: &str,
        origin_trade_no: &str,
        chain_code: &str,
        receiver_address: &str,
        amount: &str,
    ) -> Result<PlatformApplyOutcome, ServiceError> {
        let adapter =
            ApiChainAdapterFactory::get_transaction_adapter_with_ctx(self.ctx, chain_code).await?;
        let resource = adapter.account_resource(receiver_address).await?;
        let amounts = energy_shortfall_to_apply_amounts(amount, resource.energy_price())?;

        let wallet = ApiWalletRepo::find_by_uid(&self.ctx.api_wallet_pool()?, uid)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;

        let app_id = wallet.as_ref().and_then(|w| w.app_id.as_deref()).unwrap_or(uid);
        let org_id = wallet.as_ref().and_then(|w| w.merchant_id.as_deref()).unwrap_or(uid);

        let req = ResourceApplyReq::new(
            origin_trade_no,
            app_id,
            org_id,
            Some(chain_code),
            amounts.native_token_amount,
            Some(amounts.resource_amount),
            ResourceType::Energy,
            receiver_address,
            TransType::Col,
        );

        tracing::info!(
            origin_trade_no = %origin_trade_no,
            req = ?req,
            source = "shadow_worker_v2",
            "Platform resource delegation apply request"
        );
        let backend_api = self.ctx.get_global_backend_api();
        let apply_started_at = std::time::Instant::now();
        let resp = backend_api.apply_resource_delegation(&req).await?;
        let apply_elapsed_ms = apply_started_at.elapsed().as_millis();

        if resp.is_success() {
            info!(
                origin_trade_no = %origin_trade_no,
                resource_trade_no = ?resp.dl_trade_no,
                apply_elapsed_ms,
                source = "shadow_worker_v2",
                "Platform resource delegation apply succeeded"
            );
            Ok(PlatformApplyOutcome::from_backend_response(true, resp.dl_trade_no))
        } else {
            warn!(
                origin_trade_no = %origin_trade_no,
                apply_elapsed_ms,
                source = "shadow_worker_v2",
                "Platform resource delegation apply rejected, will try alternative paths"
            );
            Ok(PlatformApplyOutcome::Rejected)
        }
    }

    async fn commit_local_delegation_block(
        &self,
        origin_trade_no: &str,
        req: &ApiCollectEntity,
        exec_to_addr: &str,
        required_energy: u64,
        available_energy: i64,
        energy_price: f64,
    ) -> Result<(), ServiceError> {
        // local delegation 的 owner 是出款地址，receiver 是当前待归集子地址。
        // 这里写下的是“本地代理 fallback 已成为当前依赖”的事实，
        // 不是说本地代理已经执行成功。
        //
        // 一旦 local delegation 到终态，collect 只会被放回旧主链入口，
        // 不会在资源链里继续承接主币/补币逻辑。
        let resource_trade_no = Self::collect_local_delegate_trade_no(origin_trade_no);
        let amount = Self::collect_delegation_energy_amount(required_energy).to_string();
        let native_amount = energy_shortfall_to_apply_amounts(&amount, energy_price)?
            .native_token_amount
            .to_string();
        let delegation = NewApiResourceDelegation::local_delegate(
            req.uid.clone(),
            resource_trade_no.clone(),
            req.trade_no.clone(),
            i64::from(req.trade_type),
            exec_to_addr.to_string(),
            req.from_addr.clone(),
            native_amount,
            amount,
        );
        ApiResourceDelegationRepo::upsert(&self.ctx.api_resource_pool()?, delegation)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
        let rows = ApiCollectRepo::mark_resource_blocked(
            &self.ctx.api_transaction_pool()?,
            origin_trade_no,
            ApiResourceBlockReason::NeedLocalDelegate,
            Some(&resource_trade_no),
            Some(ApiResourceDependencyType::LocalDelegate),
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(
            origin_trade_no = %origin_trade_no,
            rows = %rows,
            resource_trade_no = %resource_trade_no,
            owner_address = %exec_to_addr,
            receiver_address = %req.from_addr,
            source = "shadow_worker_v2",
            "TRON collect resource gate switched to local delegation fallback"
        );
        self.advancer.trigger_local_delegation_fast_path(&resource_trade_no);
        Ok(())
    }

    async fn release_collect_gate_after_local_delegation_success(
        &self,
        delegation: &ApiResourceDelegationEntity,
    ) -> Result<(), ServiceError> {
        if delegation.source != ApiResourceDelegationSource::Local
            || delegation.operation_type != ApiResourceDelegationOperationType::Delegate
        {
            return Ok(());
        }
        let Some(origin_trade_no) = delegation.origin_trade_no.as_deref() else {
            return Ok(());
        };
        self.release_collect_gate_after_local_delegation(
            origin_trade_no,
            ApiResourceGateResult::LocalDelegationSuccess,
        )
        .await
    }

    async fn release_collect_gate_after_local_delegation_failure(
        &self,
        resource_trade_no: &str,
    ) -> Result<(), ServiceError> {
        let delegation = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if delegation.source != ApiResourceDelegationSource::Local {
            return Ok(());
        }
        if delegation.operation_type != ApiResourceDelegationOperationType::Delegate {
            return Ok(());
        }
        if delegation.result_status != Some(ApiResourceDelegationResultStatus::Fail)
            || delegation.finished_at.is_none()
        {
            return Ok(());
        }
        let Some(origin_trade_no) = delegation.origin_trade_no.as_deref() else {
            return Ok(());
        };
        self.release_collect_gate_after_local_delegation(
            origin_trade_no,
            ApiResourceGateResult::LocalDelegationFailedBypass,
        )
        .await
    }

    async fn release_collect_gate_after_local_delegation(
        &self,
        origin_trade_no: &str,
        gate_result: ApiResourceGateResult,
    ) -> Result<(), ServiceError> {
        // local delegation 到达终态后，collect 不再卡在资源 gate。
        // 后面是否还会因为主币不足、服务费不足而停下，交回原有 BuildTx/fee 流程判断。
        let affected = ApiCollectRepo::mark_resource_released(
            &self.ctx.api_transaction_pool()?,
            origin_trade_no,
            gate_result,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if affected == 0 {
            info!(
                origin_trade_no = %origin_trade_no,
                ?gate_result,
                source = "shadow_worker_v2",
                "Collect gate already released after local delegation"
            );
        }
        self.advancer.try_advance(origin_trade_no).await;
        Ok(())
    }

    /// 执行 Recover Command - 外层wrapper，确保所有错误都被捕获
    async fn process_recover(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "shadow_worker_v2", "Processing Recover command");

        // 使用内层函数来捕获所有错误
        if let Err(err) = self.process_recover_inner(&trade_no).await {
            error!(trade_no = %trade_no, error = %err, source = "shadow_worker_v2", "Recover inner failed, handling error");
            if Self::is_recovered_tx_terminal_chain_failure(&err) {
                info!(
                    trade_no = %trade_no,
                    error = %err,
                    source = "shadow_worker_v2",
                    "Recovered tx has confirmed on-chain failure; marking collect failed"
                );
                self.handle_collect_tx_failed(&trade_no, err).await?;
                return Ok(());
            }
            // 注意：recover 失败不写失败事实
            // recover 失败 = 不知道链上发生了什么，而不是"交易失败"
            // 因此不调用 handle_collect_tx_failed，让 Scanner 在下一次扫描时重新尝试
        }

        Ok(())
    }

    /// Recover 内部实现，可能返回错误
    async fn process_recover_inner(&self, trade_no: &str) -> Result<(), ServiceError> {
        // Linked signed competitions stay observable after a local business failure.
        let candidate = self.get_collect_entity(trade_no).await?;
        match self.reconcile_collect_replacement(&candidate).await? {
            replacement_flow::ReplacementProgress::Normal => {}
            _ => return Ok(()),
        }
        // ====== phase 1: 锁内 · 并发裁决 ======
        // ⚠️ 锁内禁止任何网络调用、sleep、await RPC
        let req = {
            // 获取地址锁，保护地址级并发
            // 注意：initial_req 仅用于定位地址锁，不参与裁决
            // 所有裁决必须基于锁内的 fresh_req
            let initial_req = self.get_collect_entity(trade_no).await?;
            let _addr_guard = self.address_locks.acquire(&initial_req.from_addr).await?;
            info!(trade_no = %trade_no, source = "shadow_worker_v2", "Acquired address lock");

            // 🔒 必须锁内重新读取，确保基于最新状态做决策
            // ⚠️ 只读"裁决字段"，不做任何业务推断
            let fresh_req = self.get_collect_entity(trade_no).await?;

            // 事实校验：Recover 只能处理 tx_hash 存在且 transaction_time 为空的交易
            // ⚠️ 这里是并发裁决的关键，确保只有一个task能通过
            if fresh_req.tx_hash.is_none() || fresh_req.transaction_time.is_some() {
                info!(trade_no = %trade_no, source = "shadow_worker_v2", "tx_hash empty or transaction_time exists, skipping Recover");
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
                    source = "shadow_worker_v2",
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
                let elapsed = Self::broadcast_uncertain_elapsed_secs(&req, now).unwrap_or_default();
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
                    source = "shadow_worker_v2",
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
                "shadow_worker_v2",
                crate::infrastructure::chain_rpc_guard::ChainRpcFailureLog::new()
                    .trade_no(trade_no)
                    .tx_hash(req.tx_hash.as_deref().unwrap_or_default())
                    .chain_code(&req.chain_code)
                    .source("shadow_worker_v2")
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

    /// Receipt failure is not a chain failure. Independently verify an old
    /// signed EVM nonce gap; the repository rechecks durable owners atomically.
    async fn try_report_query_uncertain_nonce_gap(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<bool, ServiceError> {
        if !shared_mechanism::is_evm_chain(&req.chain_code)
            || req.nonce <= 0
            || !req.raw_tx.as_deref().is_some_and(|raw| !raw.trim().is_empty())
            || !req.tx_hash.as_deref().is_some_and(|hash| !hash.trim().is_empty())
            || !shared_mechanism::is_broadcast_uncertain_timed_out(
                req.broadcast_uncertain_since_at.or(req.last_broadcast_at),
                Self::EVM_UNCERTAIN_TIMEOUT_SECS,
                Utc::now(),
            )
        {
            return Ok(false);
        }
        let observed = tokio::time::timeout(
            Duration::from_secs(30),
            ApiTransDomain::nonce(self.ctx, &req.from_addr, &req.chain_code),
        )
        .await;
        let Ok(Ok(chain_nonce)) = observed else {
            warn!(trade_no = %req.trade_no, "Independent nonce observation unavailable; preserving query-uncertain collect");
            return Ok(false);
        };
        let Ok(chain_next) = i64::try_from(chain_nonce) else {
            return Ok(false);
        };
        let rows = ApiCollectRepo::fail_verified_nonce_gap(
            &self.ctx.api_transaction_pool()?,
            &req.trade_no,
            chain_next,
            req.nonce as i64,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if rows > 0 {
            warn!(trade_no = %req.trade_no, chain_nonce, order_nonce = req.nonce,
                "Query-uncertain collect has a verified unsigned nonce gap; reporting failure and retaining signed transaction");
            self.advancer.try_advance(&req.trade_no).await;
        }
        Ok(rows > 0)
    }

    async fn apply_recover_observation(
        &self,
        req: &ApiCollectEntity,
        recover_result: Result<RecoveredTxObservation, ServiceError>,
    ) -> Result<(), ServiceError> {
        let trade_no = req.trade_no.as_str();
        match self.reconcile_collect_replacement(req).await? {
            replacement_flow::ReplacementProgress::Stop
            | replacement_flow::ReplacementProgress::Wait => return Ok(()),
            _ => {}
        }
        match recover_result {
            Ok(RecoveredTxObservation::QueryUncertain) => {
                if self.try_report_query_uncertain_nonce_gap(req).await? {
                    return Ok(());
                }
                warn!(trade_no = %trade_no, "Receipt observation failed; no verified nonce gap, preserve original order");
                return Ok(());
            }
            Ok(RecoveredTxObservation::Confirmed(tx_resp)) => {
                info!(trade_no = %trade_no, tx_hash = %tx_resp.tx_hash, source = "shadow_worker_v2", "Transaction recover successful");

                // ====== phase 3: 锁内 · 提交不可逆事实 ======
                {
                    // 重新获取地址锁，保护地址级并发
                    let _addr_guard = self.address_locks.acquire(&req.from_addr).await?;
                    info!(trade_no = %trade_no, source = "shadow_worker_v2", "Reacquired address lock for fact commit");

                    // 🔒 必须锁内重新读取，确保基于最新状态做决策
                    let fresh_req = self.get_collect_entity(trade_no).await?;

                    let existing_tx_hash =
                        shared_mechanism::normalized_tx_hash(fresh_req.tx_hash.as_deref());

                    // 事实校验：如果链上确认已经落库，补齐缺失的 tx_hash 后直接推进后续阶段
                    if fresh_req.transaction_time.is_some() {
                        let mut should_nudge = true;

                        if existing_tx_hash.is_none() {
                            let rows_affected = ApiCollectRepo::backfill_tx_hash_if_missing(
                                &self.ctx.api_transaction_pool()?,
                                &fresh_req.trade_no,
                                &tx_resp.tx_hash,
                                "shadow_worker_v2",
                            )
                            .await
                            .map_err(|e| ServiceError::Database(e.into()))?;

                            info!(
                                trade_no = %fresh_req.trade_no,
                                tx_hash = %tx_resp.tx_hash,
                                rows_affected = %rows_affected,
                                source = "shadow_worker_v2",
                                "Recovered tx hash backfilled after concurrent missing-hash read"
                            );

                            if rows_affected > 0 {
                                self.advancer.try_advance(&fresh_req.trade_no).await;
                                should_nudge = false;
                            }
                        }

                        info!(trade_no = %trade_no, source = "shadow_worker_v2", "tx_hash empty or transaction_time exists, skipping Recover fact commit");
                        if should_nudge {
                            info!(
                                trade_no = %trade_no,
                                source = "shadow_worker_v2",
                                "Recover facts already present; nudging advancer for downstream stages"
                            );
                            self.advancer.try_advance(&fresh_req.trade_no).await;
                        }
                        return Ok(());
                    }

                    // 🔒 事实保护：检查 tx_hash 一致性，防止事实被覆盖
                    shared_mechanism::validate_recovered_tx_hash(
                        &fresh_req.trade_no,
                        existing_tx_hash,
                        &tx_resp.tx_hash,
                        "shadow_worker_v2",
                    )?;

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
                    let resource_consume = if let Some(consumer) = tx_resp.consumer {
                        consumer.energy_used.to_string()
                    } else {
                        "0".to_string()
                    };

                    let rows_affected =
                        ApiCollectRepo::confirm_onchain_transaction_fact_with_recover(
                            &self.ctx.api_transaction_pool()?,
                            &fresh_req.trade_no,
                            &tx_resp.tx_hash,
                            &transaction_time,
                            &transaction_time,
                            &tx_resp.fee,
                            &resource_consume,
                        )
                        .await
                        .map_err(|e| ServiceError::Database(e.into()))?;

                    // 显式处理幂等情况：恢复已被其他并发执行
                    if rows_affected == 0 {
                        info!(
                            trade_no = %fresh_req.trade_no,
                            tx_hash = %tx_resp.tx_hash,
                            source = "shadow_worker_v2",
                            "confirm_onchain_transaction_fact_with_recover skipped: recover already executed (idempotent hit)"
                        );
                    } else {
                        // 直接调用 try_advance 进行点对点唤醒
                        self.advancer.try_advance(&fresh_req.trade_no).await;
                    }
                }
            }
            Ok(RecoveredTxObservation::Missing) => {
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
                            reason_code = "recover_expired_raw",
                            source = "shadow_worker_v2",
                            "Detected expired tron raw_tx during recover; invalidating stale tx facts"
                        );
                        info!(
                            trade_no = %req.trade_no,
                            source = "shadow_worker_v2",
                            "Using rebuild-only invalidation path for expired raw_tx (will NOT set need_service_fee)"
                        );
                        let rows = ApiCollectRepo::invalidate_raw_tx_for_rebuild(
                            &self.ctx.api_transaction_pool()?,
                            &req.trade_no,
                            None,
                        )
                        .await
                        .map_err(|e| ServiceError::Database(e.into()))?;
                        if rows > 0 {
                            self.advancer.try_advance(&req.trade_no).await;
                        }
                        return Ok(());
                    }
                }

                info!(trade_no = %trade_no, source = "shadow_worker_v2", "Transaction recover result is uncertain");
                if shared_mechanism::is_tron_chain(&req.chain_code) {
                    self.advancer.schedule_recover_retry(trade_no);
                }
                if !shared_mechanism::tracks_broadcast_uncertain_state(&req.chain_code) {
                    // 查链不确定（含链上查不到 hash）后，立即尝试推进一次；
                    // 若满足广播条件会直接进入 Broadcast 重试，避免纯等待下一轮定时扫描。
                    self.advancer.try_advance(trade_no).await;
                    return Ok(());
                }

                let now = Utc::now();
                let rows_affected = ApiCollectRepo::mark_broadcast_uncertain_attempt(
                    &self.ctx.api_transaction_pool()?,
                    trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                let refreshed = self.get_collect_entity(trade_no).await?;
                info!(
                    trade_no = %refreshed.trade_no,
                    tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                    nonce = refreshed.nonce,
                    rows_affected = %rows_affected,
                    retry_count = refreshed.broadcast_uncertain_retry_count,
                    uncertain_since_at = ?refreshed.broadcast_uncertain_since_at,
                    reconciled_at = ?refreshed.broadcast_uncertain_reconciled_at,
                    rebroadcast_count = refreshed.broadcast_uncertain_rebroadcast_count,
                    source = "shadow_worker_v2",
                    "Broadcast uncertain state recorded"
                );

                let elapsed_secs =
                    Self::broadcast_uncertain_elapsed_secs(&refreshed, now).unwrap_or_default();
                let timed_out = Self::should_auto_fail_broadcast_uncertain(&refreshed, now);
                if !timed_out {
                    return Ok(());
                }

                if !shared_mechanism::is_evm_chain(&refreshed.chain_code)
                    && !shared_mechanism::is_sol_chain(&refreshed.chain_code)
                {
                    warn!(
                        trade_no = %refreshed.trade_no,
                        tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                        uncertain_duration_sec = elapsed_secs,
                        chain_code = %refreshed.chain_code,
                        source = "shadow_worker_v2",
                        "Broadcast uncertain timeout reached for non-EVM chain; keep observing"
                    );
                    return Ok(());
                }

                if shared_mechanism::is_sol_chain(&refreshed.chain_code) {
                    warn!(
                        trade_no = %refreshed.trade_no,
                        tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                        uncertain_duration_sec = elapsed_secs,
                        source = "shadow_worker_v2",
                        "SOL uncertain timeout reached; auto fail order"
                    );

                    let rows_affected = ApiCollectRepo::update_api_collect_status_and_err(
                        &self.ctx.api_transaction_pool()?,
                        &refreshed.trade_no,
                        ApiCollectStatus::SendingTxFailed,
                        Self::EVM_UNCERTAIN_AUTO_FAIL_ERR_CODE,
                        "SOL broadcast uncertain timeout after 5m; confirmed result still not visible",
                    )
                    .await
                    .map_err(|db_err: wallet_database::Error| {
                        error!(trade_no = %refreshed.trade_no, error = %db_err, source = "shadow_worker_v2", "Failed to mark SOL uncertain timeout auto-fail");
                        ServiceError::Database(db_err.into())
                    })?;
                    if rows_affected > 0 {
                        self.advancer.try_advance(&refreshed.trade_no).await;
                    }
                    return Ok(());
                }

                let local_nonce_for_log = refreshed.nonce as u64;
                let chain_nonce =
                    ApiTransDomain::nonce(&self.ctx, &refreshed.from_addr, &refreshed.chain_code)
                        .await?;
                let can_rebroadcast_same_raw = refreshed.last_broadcast_at.is_none()
                    && refreshed.broadcast_uncertain_rebroadcast_count == 0;

                match Self::evm_uncertain_nonce_action(
                    chain_nonce,
                    local_nonce_for_log,
                    can_rebroadcast_same_raw,
                ) {
                    EvmUncertainNonceAction::PreserveFutureNonce => {
                        let chain_next = i64::try_from(chain_nonce).map_err(|_| {
                            ServiceError::System(SystemError::Internal(
                                "chain nonce exceeds i64".into(),
                            ))
                        })?;
                        let rows = ApiCollectRepo::fail_verified_nonce_gap(
                            &self.ctx.api_transaction_pool()?,
                            &refreshed.trade_no,
                            chain_next,
                            refreshed.nonce as i64,
                        )
                        .await
                        .map_err(|e| ServiceError::Database(e.into()))?;
                        if rows > 0 {
                            warn!(trade_no = %refreshed.trade_no, chain_nonce, order_nonce = refreshed.nonce,
                                "Verified unsigned nonce gap; reporting failure while retaining signed transaction");
                            self.advancer.try_advance(&refreshed.trade_no).await;
                            return Ok(());
                        }
                        warn!(
                            trade_no = %refreshed.trade_no,
                            tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                            chain_nonce = %chain_nonce,
                            order_nonce = %local_nonce_for_log,
                            uncertain_duration_sec = elapsed_secs,
                            decision = "preserve_future_nonce",
                            source = "shadow_worker_v2",
                            "EVM uncertain transaction has a future nonce; keep original transaction"
                        );
                    }
                    EvmUncertainNonceAction::RebroadcastSameRaw => {
                        warn!(
                            trade_no = %refreshed.trade_no,
                            tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                            chain_nonce = %chain_nonce,
                            order_nonce = %local_nonce_for_log,
                            uncertain_duration_sec = elapsed_secs,
                            decision = "rebroadcast_same_raw",
                            source = "shadow_worker_v2",
                            "EVM uncertain transaction is the next nonce; retrying the identical raw transaction"
                        );
                        let rows = ApiCollectRepo::arm_uncertain_same_raw_rebroadcast_once(
                            &self.ctx.api_transaction_pool()?,
                            &refreshed.trade_no,
                        )
                        .await
                        .map_err(|e| ServiceError::Database(e.into()))?;
                        if rows > 0 {
                            self.advancer.try_advance(&refreshed.trade_no).await;
                        } else {
                            info!(
                                trade_no = %refreshed.trade_no,
                                tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                                decision = "same_raw_rebroadcast_cas_lost",
                                source = "shadow_worker_v2",
                                "EVM identical-raw rebroadcast was already armed or transaction state advanced"
                            );
                        }
                    }
                    EvmUncertainNonceAction::PreserveConsumedNonce => {
                        if let Ok(nonce) = i32::try_from(refreshed.nonce)
                            && nonce >= 0
                            && chain_nonce > local_nonce_for_log
                        {
                            let proof = async {
                                let adapter =
                                    ApiChainAdapterFactory::get_transaction_adapter_with_ctx(
                                        self.ctx,
                                        &refreshed.chain_code,
                                    )
                                    .await?;
                                adapter
                                    .finalized_nonce_conflict(
                                        &refreshed.chain_code,
                                        &refreshed.from_addr,
                                        nonce,
                                        refreshed.tx_hash.as_deref().unwrap_or_default(),
                                    )
                                    .await
                            }
                            .await;
                            match proof {
                                Ok(Some(evidence)) => {
                                    let rows = ApiCollectRepo::record_finalized_nonce_conflict(
                                        &self.ctx.api_transaction_pool()?,
                                        &refreshed.trade_no,
                                        refreshed.tx_hash.as_deref().unwrap_or_default(),
                                        &evidence,
                                    )
                                    .await?;
                                    info!(
                                        observability_event = "collect_finalized_nonce_conflict",
                                        trade_no = %refreshed.trade_no,
                                        from_addr = %refreshed.from_addr,
                                        tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                                        conflicting_tx_hash = %evidence.conflicting_tx_hash,
                                        block_number = evidence.block_number,
                                        nonce, rows,
                                        "Finalized nonce conflict persistence completed"
                                    );
                                    if rows > 0 {
                                        self.advancer.try_advance(&refreshed.trade_no).await;
                                    }
                                    return Ok(());
                                }
                                Ok(None) => {}
                                Err(err) => warn!(
                                    observability_event = "collect_nonce_conflict_probe_unavailable",
                                    trade_no = %refreshed.trade_no,
                                    from_addr = %refreshed.from_addr,
                                    tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                                    nonce, error = %err,
                                    "Finalized nonce conflict probe unavailable; preserve original transaction"
                                ),
                            }
                        }
                        warn!(
                            trade_no = %refreshed.trade_no,
                            tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                            chain_nonce = %chain_nonce,
                            order_nonce = %local_nonce_for_log,
                            uncertain_duration_sec = elapsed_secs,
                            observability_event = "collect_nonce_conflict_unresolved",
                            from_addr = %refreshed.from_addr,
                            decision = "preserve_consumed_nonce",
                            source = "shadow_worker_v2",
                            "EVM uncertain nonce is no longer safely rebroadcastable; keep original transaction for recovery"
                        );
                    }
                }
                return Ok(());
            }
            Err(err) if shared_mechanism::is_tron_missing_confirmed_and_pending_error(&err) => {
                match Self::tron_missing_pools_recovery_action(&req) {
                    TronMissingPoolsRecoveryAction::ObserveWithPointToPointRetry => {
                        let recover_key = shadow_rpc_policy::recover_dispatch_key(trade_no);
                        let retry_delay =
                            shadow_rpc_policy::recover_dispatch_cooldown(&recover_key);
                        info!(
                            observability_event = "tron_collect_missing_pools_recover_retry_scheduled",
                            trade_no = %trade_no,
                            tx_hash = %req.tx_hash.as_deref().unwrap_or_default(),
                            retry_delay_ms = retry_delay.as_millis(),
                            source = "shadow_worker_v2",
                            "Tron tx missing from confirmed and pending pools; scheduling point-to-point observation"
                        );
                        self.advancer.schedule_recover_retry(trade_no);
                        return Ok(());
                    }
                    TronMissingPoolsRecoveryAction::Rebroadcast => {}
                }

                warn!(
                    trade_no = %trade_no,
                    tx_hash = %req.tx_hash.as_deref().unwrap_or_default(),
                    source = "shadow_worker_v2",
                    "Tron tx missing from confirmed and pending pools beyond timeout; rebroadcasting"
                );
                let rows = ApiCollectRepo::invalidate_raw_tx_for_rebroadcast(
                    &self.ctx.api_transaction_pool()?,
                    &req.trade_no,
                    None,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                if rows > 0 {
                    self.advancer.try_advance(&req.trade_no).await;
                }
            }
            Err(err) => return Err(err),
        }

        Ok(())
    }

    /// 执行 BuildTx Command - 外层wrapper，确保所有错误都被捕获
    async fn process_build_tx(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "shadow_worker_v2", "Processing BuildTx command");

        // 使用内层函数来捕获所有错误
        if let Err(err) = self.process_build_tx_inner(&trade_no).await {
            self.fee_reservations.release(&trade_no);
            error!(trade_no = %trade_no, error = %err, source = "shadow_worker_v2", "BuildTx inner failed, handling error");
            self.handle_collect_tx_failed(&trade_no, err).await?;
        }

        Ok(())
    }

    /// BuildTx 内部实现，可能返回错误
    async fn process_build_tx_inner(&self, trade_no: &str) -> Result<(), ServiceError> {
        self.build_collect_candidate(trade_no, None).await
    }

    pub(crate) async fn build_collect_candidate(
        &self,
        trade_no: &str,
        private_key_override: Option<wallet_chain_interact::types::ChainPrivateKey>,
    ) -> Result<(), ServiceError> {
        // 1. 从数据库中获取归集交易信息
        let mut req = self.get_collect_entity(trade_no).await?;

        // 2. 事实校验：BuildTx 只能处理 raw_tx 为空的交易
        if req.raw_tx.is_some() {
            if req.need_service_fee == Some(true) {
                error!(
                    trade_no = %trade_no,
                    source = "shadow_worker_v2",
                    "Invariant violated: raw_tx exists while need_service_fee is true"
                );
                return Err(ServiceError::System(SystemError::Internal(
                    "Invariant violated: raw_tx exists while need_service_fee is true".to_string(),
                )));
            }
            info!(trade_no = %trade_no, source = "shadow_worker_v2", "raw_tx already exists, skipping BuildTx");
            return Ok(());
        }

        // 3. 事实校验：如果已有 tx_hash 且 transaction_time 为空，跳过 BuildTx
        // Recover 逻辑已移至独立的 Recover Command，由 Scanner 触发
        if req.tx_hash.is_some() && req.transaction_time.is_none() {
            info!(trade_no = %trade_no, source = "shadow_worker_v2", "Found existing tx_hash without transaction_time, skipping BuildTx (recover will be handled by Scanner)");
            return Ok(());
        }

        // 4. 先占位建单槽位，防止同一 trade_no 在构建期间被重复推进
        // building_at 只作为短期 in-flight 保护，不参与最终事实判断。
        let build_slot_rows =
            ApiCollectRepo::update_building_at(&self.ctx.api_transaction_pool()?, trade_no).await?;
        if build_slot_rows == 0 {
            if self.reclaim_stale_build_slot(trade_no, req.building_at).await? {
                let reclaimed_build_slot_rows =
                    ApiCollectRepo::update_building_at(&self.ctx.api_transaction_pool()?, trade_no)
                        .await?;
                if reclaimed_build_slot_rows > 0 {
                    info!(
                        trade_no = %trade_no,
                        source = "shadow_worker_v2",
                        "Reclaimed stale build slot, continuing BuildTx"
                    );
                } else {
                    info!(
                        trade_no = %trade_no,
                        source = "shadow_worker_v2",
                        "Build slot still unavailable after stale reclaim attempt, skipping BuildTx"
                    );
                    return Ok(());
                }
            } else {
                info!(
                    trade_no = %trade_no,
                    source = "shadow_worker_v2",
                    "Build slot already claimed or recently updated, skipping BuildTx"
                );
                return Ok(());
            }
        }

        // 5. 解析执行地址 - 在执行期解析，支持重试
        let exec_to_addr = self.resolve_collect_to_addr(&req).await?;
        let latest_strategy_to = exec_to_addr.clone();
        let updated_to_addr = Self::apply_exec_to_addr(&mut req, &exec_to_addr);
        info!(
            trade_no = %trade_no,
            latest_strategy_to = %latest_strategy_to,
            persisted_exec_to_addr = %req.to_addr,
            updated_to_addr = %updated_to_addr,
            source = "shadow_worker_v2",
            "Resolved execution address for current build"
        );

        if updated_to_addr {
            ApiCollectRepo::update_api_collect_to_addr(
                &self.ctx.api_transaction_pool()?,
                &req.trade_no,
                &exec_to_addr,
            )
            .await?;
            info!(
                trade_no = %trade_no,
                latest_strategy_to = %latest_strategy_to,
                persisted_exec_to_addr = %req.to_addr,
                source = "shadow_worker_v2",
                "Updated persisted execution address in database"
            );
        }

        // Scope validation and latest-frontier reconciliation precede fee estimation.
        let scoped_nonce = self.scoped_collect_nonce_before_fee(&req).await?;
        // 6. 检查手续费
        //
        // ⚠️ IMPORTANT:
        // Fee insufficient is NOT a retryable failure.
        // It invalidates the current build facts and must go through invalidate_raw_tx.
        // Do NOT introduce any logic that only sets build_blocked_at.
        if !self.check_collect_amount(&req).await? {
            info!(
                trade_no = %trade_no,
                reason_code = "amount_check_failed",
                source = "shadow_worker_v2",
                "Collect amount insufficient, failing build directly"
            );
            return Err(ServiceError::Business(BusinessError::Chain(
                ChainError::InsufficientBalance(
                    InsufficientBalanceDetail::new()
                        .from_addr(req.from_addr.clone())
                        .to_addr(req.to_addr.clone())
                        .chain_code(req.chain_code.clone())
                        .token_addr(req.token_addr.to_string())
                        .value(req.value.clone())
                        .reason("collect amount is insufficient; balance is below requested value"),
                ),
            )));
        }

        if !self
            .check_fee_for_build(&req, scoped_nonce.as_ref().and_then(|(_, _, c)| c.as_ref()))
            .await?
        {
            info!(
                trade_no = %trade_no,
                reason_code = "fee_check_failed",
                source = "shadow_worker_v2",
                "Fee insufficient, invalidating current build attempt"
            );

            // 🔒 事实作废：如果 fee cycle 已经走完，只回滚可重建事实，不要把
            // need_service_fee 再次打回 true；否则会把已完成 fee cycle 的单子
            // 卡回“等待手续费结果”的死状态。
            let affected = self.invalidate_build_attempt_after_fee_check_failure(&req).await?;

            if affected == 0 {
                info!(
                    trade_no = %trade_no,
                    source = "shadow_worker_v2",
                    "Transaction already invalidated or no raw_tx to invalidate, skip"
                );
                self.clear_build_slot_after_claim(trade_no).await?;
            } else {
                // 直接调用 try_advance 进行点对点唤醒
                self.advancer.try_advance(&req.trade_no).await;
            }

            return Ok(());
        }
        info!(trade_no = %trade_no, source = "shadow_worker_v2", "Fee check passed");

        // 7. 检查交易摘要 - 仍然使用后端原始 digest 语义，不依赖当前执行地址
        if !self.check_digest(&req).await? {
            tracing::error!(trade_no=%trade_no, "collect_tx:send: 交易摘要验证失败");
            return Err(ServiceError::Business(
                ApiWalletError::Trans(TransError::TransactionDigestVerificationFailed).into(),
            ));
        }
        info!(trade_no = %trade_no, source = "shadow_worker_v2", "Transaction digest verification passed");

        // ====== phase 1: 锁内 · 快速检查 ======
        // ⚠️ 锁内禁止任何网络调用、sleep、await RPC
        {
            let _addr_guard = self.address_locks.acquire(&req.from_addr).await?;
            let fresh_req = self.get_collect_entity(trade_no).await?;
            if fresh_req.raw_tx.is_some() {
                self.clear_build_slot_after_claim(trade_no).await?;
                return Ok(());
            }
        }
        // Observation performs RPC outside the address lock and reserves nothing.
        // The shared counter is rechecked when signed facts commit in phase 3.
        let (nonce, chain_next, replacement_fee) = match scoped_nonce {
            Some(observation) => observation,
            None => self.observe_collect_nonce(&req).await?,
        };
        info!(trade_no = %trade_no, nonce, source = "shadow_worker_v2", "Observed nonce candidate");

        // ====== phase 2: 锁外 · 网络执行 ======
        // 通过Context获取Handles实例，然后获取私钥管理器
        let private_key = if let Some(key) = private_key_override {
            key
        } else {
            let handles = self.ctx.get_handles_arc().await?;
            handles
                .get_global_private_key_manager()
                .get_private_key(&req.from_addr, &req.chain_code)
                .await?
        };
        info!(trade_no = %trade_no, source = "shadow_worker_v2", "Retrieved private key from manager");

        // 生成转账请求 - 使用解析后的执行地址和获取到的nonce
        // Candidate is reserved only with signed facts in phase 3.
        let mut transfer_req = self.gen_transfer_req(&req, &exec_to_addr, nonce).await?;
        if let Some(context) = replacement_fee {
            transfer_req.base.metadata =
                Some(serde_json::json!({"collectReplacement":context}).to_string());
        }

        info!(trade_no = %trade_no, nonce = %nonce, source = "shadow_worker_v2", "Generated transfer request with nonce");

        // 构建交易。公共包装负责 guarded endpoint 并发、timeout、成功/失败记录。
        let Some((tx_hash, raw_tx, fee)) =
            crate::infrastructure::chain_rpc_guard::run_guarded_with_ctx_and_log(
                self.ctx,
                &req.chain_code,
                trade_no,
                "build",
                "shadow_worker_v2",
                crate::infrastructure::chain_rpc_guard::ChainRpcFailureLog::new()
                    .trade_no(trade_no)
                    .chain_code(&req.chain_code)
                    .source("shadow_worker_v2")
                    .op("build"),
                crate::domain::api_wallet::trans::ApiTransDomain::build_transfer_raw(
                    &self.ctx,
                    transfer_req,
                    Some(private_key),
                ),
            )
            .await?
        else {
            self.clear_build_slot_after_claim(trade_no).await?;
            return Ok(());
        };
        info!(trade_no = %trade_no, tx_hash = %tx_hash, fee = %fee, source = "shadow_worker_v2", "Built transfer raw transaction successfully");

        // The fee quote and raw build are separate RPC/oracle observations.
        // Recheck the signed EVM maximum before persisting broadcastable facts
        // so a later, higher maxFeePerGas re-enters the service-fee cycle
        // instead of failing at node admission.
        let final_fee_chain = ChainCode::try_from(req.chain_code.as_str())?;
        if crate::domain::chain::is_evm_chain(final_fee_chain) {
            let main_coin = ApiChainTransDomain::main_coin(self.ctx, &req.chain_code).await?;
            let latest_balance = self
                .query_balance(
                    &req.from_addr,
                    final_fee_chain,
                    AssetTokenKey::Native,
                    main_coin.decimals,
                )
                .await?;
            let latest_balance = conversion::decimal_from_str(&latest_balance)?;
            let signed_max_fee = conversion::decimal_from_str(&fee)?;

            if latest_balance < signed_max_fee {
                warn!(
                    trade_no = %trade_no,
                    from_addr = %req.from_addr,
                    balance = %latest_balance,
                    signed_max_fee = %signed_max_fee,
                    stage = "build.final_fee_check",
                    source = "shadow_worker_v2",
                    "Signed EVM maximum fee exceeds latest balance; reopening service fee cycle"
                );
                let affected = self.invalidate_build_attempt_after_fee_check_failure(&req).await?;
                self.fee_reservations.release(&req.trade_no);
                if affected > 0 {
                    self.advancer.try_advance(&req.trade_no).await;
                }
                return Ok(());
            }
        }

        // ====== phase 3: 锁内 · 提交不可逆事实 ======
        {
            // 重新获取地址锁，保护地址级并发
            let _addr_guard = self.address_locks.acquire(&req.from_addr).await?;
            info!(trade_no = %trade_no, source = "shadow_worker_v2", "Reacquired address lock for fact commit");

            // 🔒 必须锁内重新读取，确保基于最新状态做决策
            // ⚠️ 与 Broadcast / Recover 对齐，遵循"锁内 fresh read + 条件更新"铁律
            let fresh_req = self.get_collect_entity(trade_no).await?;

            // 事实校验：BuildTx 只能处理 raw_tx 为空的交易
            if fresh_req.raw_tx.is_some() {
                info!(trade_no = %trade_no, source = "shadow_worker_v2", "raw_tx already exists, skipping BuildTx fact commit");
                self.clear_build_slot_after_claim(trade_no).await?;
                return Ok(());
            }

            // 立即将tx_hash和raw_tx存储到数据库
            // 注意：使用序列化而非格式化，避免格式问题
            let raw_tx_str = wallet_utils::serde_func::serde_to_string(&raw_tx)?;
            let pool = self.ctx.api_transaction_pool()?;
            let rows_affected = if shared_mechanism::is_evm_chain(&req.chain_code) {
                ApiCollectRepo::reserve_signed_collect(
                    &pool,
                    &SignedCollectCandidate {
                        trade_no: &req.trade_no,
                        from_addr: &req.from_addr,
                        chain_code: &req.chain_code,
                        tx_hash: &tx_hash,
                        raw_tx: &raw_tx_str,
                        transaction_fee: &fee,
                        nonce: nonce as i64,
                        chain_next,
                    },
                )
                .await?
            } else {
                ApiCollectRepo::update_after_build(
                    &pool,
                    &fresh_req.trade_no,
                    &tx_hash,
                    &raw_tx_str,
                    &fee,
                    nonce as i64,
                )
                .await?
            };

            // 显式处理幂等情况：如果影响行数为0，表示raw_tx已存在或被并发写入
            if rows_affected == 0 {
                info!(trade_no = %trade_no, source = "shadow_worker_v2", "update_after_build skipped: raw_tx already exists (idempotent hit)");
                self.clear_build_slot_after_claim(trade_no).await?;
                return Ok(());
            }

            info!(trade_no = %trade_no, source = "shadow_worker_v2", "Updated tx_hash and raw_tx to database successfully");

            self.fee_reservations.release(&fresh_req.trade_no);

            // 直接调用 try_advance 进行点对点唤醒
            self.advancer.try_advance(&fresh_req.trade_no).await;
        }

        // BuildTx命令完成，不负责广播，由Broadcast命令处理
        Ok(())
    }

    /// 执行 Broadcast Command - 外层wrapper，确保所有错误都被捕获
    async fn process_broadcast(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "shadow_worker_v2", "Processing Broadcast command");

        // 使用内层函数来捕获所有错误
        if let Err(err) = self.process_broadcast_inner(&trade_no).await {
            error!(trade_no = %trade_no, error = %err, source = "shadow_worker_v2", "Broadcast inner failed, handling error");
            self.handle_collect_tx_failed(&trade_no, err).await?;
        }

        Ok(())
    }

    /// Broadcast 内部实现，可能返回错误
    async fn process_broadcast_inner(&self, trade_no: &str) -> Result<(), ServiceError> {
        // ====== phase 1: 锁内 · 快速检查 ======
        // ⚠️ 锁内禁止任何网络调用、sleep、await RPC
        let req = {
            // 先获取初始的 collect 实体，用于获取 from_addr
            let initial_req = self.get_collect_entity(trade_no).await?;

            // 获取地址锁，保护地址级并发
            let _addr_guard = self.address_locks.acquire(&initial_req.from_addr).await?;
            info!(trade_no = %trade_no, source = "shadow_worker_v2", "Acquired address lock for broadcast");

            // 🔒 必须锁内重新读取，确保基于最新状态做决策
            // ⚠️ 只读"裁决字段"，不做任何业务推断
            let fresh_req = self.get_collect_entity(trade_no).await?;

            // 事实校验：Broadcast 只能处理 raw_tx 存在且 transaction_time 为空的交易
            if fresh_req.raw_tx.is_none() || fresh_req.transaction_time.is_some() {
                info!(trade_no = %trade_no, source = "shadow_worker_v2", "raw_tx empty or transaction_time exists, skipping Broadcast");
                return Ok(());
            }

            // 事实校验：Broadcast 成功只应写入 last_broadcast_at，且必须是幂等的
            // ⚠️ IMPORTANT:
            // Broadcast success MUST only write last_broadcast_at
            // and MUST be idempotent (WHERE last_broadcast_at IS NULL)
            if fresh_req.last_broadcast_at.is_some() {
                info!(trade_no = %trade_no, source = "shadow_worker_v2", "last_broadcast_at already exists, skipping Broadcast");
                return Ok(());
            }

            if shared_mechanism::is_evm_chain(&fresh_req.chain_code)
                && fresh_req.broadcast_uncertain_since_at.is_some()
                && fresh_req.transaction_time.is_none()
                && fresh_req.tx_exec_receipt_uploaded_at.is_none()
                && fresh_req.err_code.is_none()
            {
                info!(
                    observability_event = "shadow_broadcast_skipped",
                    reason = "broadcast_uncertain_in_progress",
                    trade_no = %trade_no,
                    source = "shadow_worker_v2",
                    "Skip Broadcast: EVM uncertain state in progress; recover should proceed"
                );
                return Ok(());
            }

            fresh_req
        };
        // 🔓 锁在这里已经释放

        // 检查是否已有raw_tx和tx_hash
        if req.tx_hash.is_none() || req.raw_tx.is_none() || req.raw_tx.as_ref().unwrap().is_empty()
        {
            error!(trade_no = %trade_no, source = "shadow_worker_v2", "No raw_tx or tx_hash found");
            return Err(ServiceError::Business(
                crate::error::business::BusinessError::ApiWallet(
                    crate::error::business::api_wallet::ApiWalletError::Trans(
                        crate::error::business::api_wallet::trans::TransError::BuildWithdrawTransactionFailed("Missing transaction data".to_string()),
                    ),
                ),
            ));
        }

        match self.reconcile_collect_replacement(&req).await? {
            replacement_flow::ReplacementProgress::Stop => return Ok(()),
            replacement_flow::ReplacementProgress::ContinueNew => {
                return self.process_recover(trade_no.to_owned()).await;
            }
            _ => {}
        }

        // 反序列化raw_tx
        // 从数据库中获取的raw_tx是字符串格式，需要反序列化为RawTx类型
        let raw_tx = wallet_utils::serde_func::serde_from_str(req.raw_tx.as_deref().unwrap())?;
        info!(trade_no = %trade_no, tx_hash = %req.tx_hash.as_deref().unwrap(), source = "shadow_worker_v2", "Deserialized raw_tx successfully");

        // 广播交易。公共包装负责 guarded endpoint 并发、timeout、成功/失败记录。
        info!(trade_no = %trade_no, tx_hash = %req.tx_hash.as_deref().unwrap(), source = "shadow_worker_v2", "Starting to broadcast transaction");
        let guard_outcome =
            crate::infrastructure::chain_rpc_guard::run_guarded_with_ctx_and_log_classified(
                self.ctx,
                &req.chain_code,
                trade_no,
                "broadcast",
                "shadow_worker_v2",
                crate::infrastructure::chain_rpc_guard::ChainRpcFailureLog::new()
                    .trade_no(trade_no)
                    .tx_hash(req.tx_hash.as_deref().unwrap_or_default())
                    .chain_code(&req.chain_code)
                    .source("shadow_worker_v2")
                    .op("broadcast"),
                crate::domain::api_wallet::trans::ApiTransDomain::broadcast_transfer(
                    &self.ctx,
                    &req.chain_code,
                    raw_tx,
                    req.tx_hash.as_deref(),
                ),
            )
            .await?;

        if Self::collect_broadcast_guard_skip_requires_retry(&guard_outcome) {
            info!(
                observability_event = "tron_collect_broadcast_retry_scheduled",
                reason = "rpc_guard_skipped_before_execution",
                trade_no = %trade_no,
                tx_hash = %req.tx_hash.as_deref().unwrap_or_default(),
                source = "shadow_worker_v2",
                "Collect broadcast was not submitted; scheduling fast BroadcastTx retry"
            );
            self.advancer.schedule_broadcast_retry_after_guard_skip(trade_no);
            return Ok(());
        }

        let tx_resp = match guard_outcome {
            crate::infrastructure::chain_rpc_guard::GuardedCallOutcome::Executed(response) => {
                response
            }
            crate::infrastructure::chain_rpc_guard::GuardedCallOutcome::SkippedBeforeExecution => {
                unreachable!("pre-execution guard skip returned above")
            }
            crate::infrastructure::chain_rpc_guard::GuardedCallOutcome::ExecutionTimedOut => None,
        };

        match tx_resp {
            Some(tx) => {
                info!(trade_no = %trade_no, tx_hash = %tx.tx_hash, source = "shadow_worker_v2", "Transaction broadcast successful");

                // ====== phase 3: 锁内 · 提交不可逆事实 ======
                {
                    // 重新获取地址锁，保护地址级并发
                    let _addr_guard = self.address_locks.acquire(&req.from_addr).await?;
                    info!(trade_no = %trade_no, source = "shadow_worker_v2", "Reacquired address lock for broadcast fact commit");

                    // 🔒 必须锁内重新读取，确保基于最新状态做决策
                    // ⚠️ Phase 3 永远只相信"锁内刚读出来的实体"
                    // Phase 1 / Phase 2 的 req 只能当上下文，不是事实来源
                    let fresh_req = self.get_collect_entity(trade_no).await?;

                    // 🔒 事实保护：检查 tx_hash 一致性，防止 build 阶段事实被覆盖
                    // 确保 build 阶段确立的 tx_hash 事实在 broadcast 阶段不被改写
                    if let Some(existing) = &fresh_req.tx_hash {
                        if existing != &tx.tx_hash {
                            error!(
                                trade_no = %fresh_req.trade_no,
                                existing_tx_hash = %existing,
                                broadcast_tx_hash = %tx.tx_hash,
                                source = "shadow_worker_v2",
                                "tx_hash mismatch between build and broadcast - fact integrity violated"
                            );
                            return Err(ServiceError::System(
                                crate::error::system::SystemError::Internal(
                                    "Invariant broken - tx_hash mismatch between build and broadcast"
                                        .to_string(),
                                ),
                            ));
                        }
                    }

                    // 广播成功 = 一次不可分割的事实提交
                    let resource_consume = if let Some(consumer) = tx.consumer {
                        consumer.energy_used.to_string()
                    } else {
                        "0".to_string()
                    };

                    let rows_affected = ApiCollectRepo::mark_broadcast_executed(
                        &self.ctx.api_transaction_pool()?,
                        &fresh_req.trade_no,
                    )
                    .await
                    .map_err(|e| ServiceError::Database(e.into()))?;

                    // 显式处理幂等情况：广播已被其他并发/恢复执行
                    if rows_affected == 0 {
                        info!(
                            trade_no = %fresh_req.trade_no,
                            tx_hash = %tx.tx_hash,
                            source = "shadow_worker_v2",
                            "mark_broadcast_executed skipped: broadcast already executed (idempotent hit)"
                        );
                    } else {
                        // 直接调用 try_advance 进行点对点唤醒
                        self.advancer.try_advance(&fresh_req.trade_no).await;
                    }
                }

                Ok(())
            }
            None => {
                info!(trade_no = %trade_no, source = "shadow_worker_v2", "Transaction broadcast result is uncertain");

                if shared_mechanism::tracks_broadcast_uncertain_state(&req.chain_code) {
                    let had_uncertain_since = req.broadcast_uncertain_since_at.is_some();
                    let rows_affected = ApiCollectRepo::mark_broadcast_uncertain_attempt(
                        &self.ctx.api_transaction_pool()?,
                        trade_no,
                    )
                    .await
                    .map_err(|e| ServiceError::Database(e.into()))?;
                    let refreshed = self.get_collect_entity(trade_no).await?;
                    info!(
                        observability_event = "shadow_broadcast_uncertain_recorded",
                        reason = "broadcast_result_uncertain",
                        trade_no = %trade_no,
                        tx_hash = %req.tx_hash.as_deref().unwrap_or_default(),
                        nonce = req.nonce,
                        rows_affected = %rows_affected,
                        uncertain_since_at_present_before = had_uncertain_since,
                        retry_count = refreshed.broadcast_uncertain_retry_count,
                        source = "shadow_worker_v2",
                        "Broadcast uncertain state recorded"
                    );
                }
                Ok(())
            }
        }
    }

    // Confirm 不由 Shadow Worker 处理
    // 链上结果由 MQTT 注入，由 Domain 层落库
    // process_confirm 方法已被删除，因为它违反了职责边界

    /// 从数据库中获取归集交易信息
    pub(crate) async fn get_collect_entity(
        &self,
        trade_no: &str,
    ) -> Result<ApiCollectEntity, ServiceError> {
        let entity = ApiCollectRepo::get_api_collect_by_trade_no(
            &self.ctx.api_transaction_pool()?,
            trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        Ok(entity)
    }

    /// 解析执行地址
    async fn resolve_collect_to_addr(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<String, ServiceError> {
        info!(trade_no = %req.trade_no, source = "shadow_worker_v2", "Resolving collect to address");

        // 1. 根据from_addr + chain_code查询account
        let account = match wallet_database::repositories::api_wallet::account::ApiAccountRepo::find_one_by_address_chain_code(
            &req.from_addr,
            &req.chain_code,
            &self.ctx.api_wallet_pool()?,
        )
        .await
        {
            Ok(Some(account)) => account,
            Ok(None) => {
                error!(trade_no = %req.trade_no, from_addr = %req.from_addr, chain_code = %req.chain_code, source = "shadow_worker_v2", "Account not found");
                return Err(ServiceError::Business(
                    crate::error::business::BusinessError::ApiWallet(
                        crate::error::business::api_wallet::ApiWalletError::Account(
                            crate::error::business::api_wallet::account::AccountError::NotFound,
                        ),
                    ),
                ));
            }
            Err(err) => {
                error!(trade_no = %req.trade_no, error = %err, source = "shadow_worker_v2", "Failed to find account");
                return Err(ServiceError::Database(err.into()));
            }
        };

        // 2. 查询用户归集策略
        let ctx = self.ctx;
        let strategy =
            crate::domain::api_wallet::strategy::StrategyDomain::query_collect_strategy_with_ctx(
                ctx,
                &account.uid,
            )
            .await?;
        info!(trade_no = %req.trade_no, uid = %account.uid, source = "shadow_worker_v2", "Retrieved collect strategy");

        // 3. 根据chain_code查询链配置
        let chain_config = match strategy
            .chain_configs
            .into_iter()
            .find(|config| config.chain_code == req.chain_code)
        {
            Some(config) => config,
            None => {
                error!(trade_no = %req.trade_no, chain_code = %req.chain_code, source = "shadow_worker_v2", "Chain config not found");
                return Err(ServiceError::Business(
                    crate::error::business::BusinessError::ApiWallet(
                        crate::error::business::api_wallet::ApiWalletError::ChainConfigNotFound(
                            req.chain_code.clone(),
                        ),
                    ),
                ));
            }
        };

        Ok(if req.risk_addr == 2 {
            chain_config.risk_address.address
        } else {
            chain_config.normal_address.address
        })
    }

    /// 检查手续费是否允许继续执行
    ///
    /// 返回值语义：
    /// - Ok(true): 手续费充足，可以继续构建
    /// - Ok(false): 手续费不足，caller 必须作废当前 build 事实（invalidate_raw_tx）
    /// - Err(_): 基础设施错误
    ///
    /// ⚠️ 本方法不做任何状态/事实写入
    /// ⚠️ 不存在"等待 / 重试 / 标记"语义
    pub(crate) async fn check_fee(&self, req: &ApiCollectEntity) -> Result<bool, ServiceError> {
        self.check_fee_for_build(req, None).await
    }
    async fn check_fee_for_build(
        &self,
        req: &ApiCollectEntity,
        replacement: Option<
            &crate::domain::api_wallet::adapter::eth_tx::replacement::ReplacementFeeContext,
        >,
    ) -> Result<bool, ServiceError> {
        tracing::info!(trade_no=%req.trade_no, source = "shadow_worker_v2", "collect_tx:send: 开始检查手续费, 发送方={}, 接收方={}, 金额={}, 代币地址={:?}", 
            req.from_addr, req.to_addr, req.value, req.token_addr);
        let ctx = self.ctx;

        // 查询主币信息
        let chain_code: ChainCode = req.chain_code.as_str().try_into()?;
        let main_coin = ApiChainTransDomain::main_coin(ctx, &req.chain_code).await?;
        tracing::info!(trade_no=%req.trade_no, source = "shadow_worker_v2", "collect_tx:send: 主币信息: 币种={}, 小数位数={}", main_coin.symbol, main_coin.decimals);

        // 确定代币信息
        let (token_symbol, token_key, token_decimals) = if req.token_addr.is_contract() {
            let token_coin = ApiCoinDomain::get_coin_by_token_key_exact(
                self.ctx,
                &req.chain_code,
                req.token_addr.clone(),
            )
            .await?;
            tracing::info!(trade_no=%req.trade_no, source = "shadow_worker_v2", "collect_tx:send: 代币信息: 币种={}, 代币地址={:?}, 小数位数={}", 
                token_coin.symbol, token_coin.token_address, token_coin.decimals);
            (token_coin.symbol, token_coin.token_address, token_coin.decimals)
        } else {
            (main_coin.symbol.clone(), AssetTokenKey::Native, main_coin.decimals)
        };

        // 查询资产主币余额
        tracing::info!(trade_no=%req.trade_no, source = "shadow_worker_v2", "collect_tx:send: 查询主币余额");
        let balance_str = self
            .query_balance(&req.from_addr, chain_code, AssetTokenKey::Native, main_coin.decimals)
            .await?;
        let balance = conversion::decimal_from_str(&balance_str)?;
        tracing::info!(trade_no=%req.trade_no, source = "shadow_worker_v2", "collect_tx:send: 主币余额查询完成: {}", balance);

        // 估算手续费
        tracing::info!(trade_no=%req.trade_no, source = "shadow_worker_v2", "collect_tx:send: 估算手续费参数: 发送方={}, 接收方={}, 金额={}, 主币={}, 代币={}, 代币小数位数={}", 
            req.from_addr, req.to_addr, req.value, main_coin.symbol, token_symbol, token_decimals);
        let spend_all_native = Self::should_spend_all_native_collect(&req.chain_code, &token_key);
        let sender_rent_reserve =
            Self::sol_token_collect_sender_rent_reserve(&req.chain_code, &token_key)?;
        let estimate = if let Some(context) = replacement {
            let adapter =
                ApiChainAdapterFactory::get_transaction_adapter_with_ctx(self.ctx, &req.chain_code)
                    .await?;
            let mut params =
                ApiBaseTransferReq::new(&req.from_addr, &req.to_addr, &req.value, &req.chain_code);
            params.with_token(token_key.to_chain_token_option(), token_decimals, &token_symbol);
            params.metadata = Some(serde_json::json!({"collectReplacement":context}).to_string());
            super::estimate_evm_collect_fee(adapter.as_ref(), chain_code, params, &main_coin.symbol)
                .await
        } else {
            self.estimate_fee(
                &req.from_addr,
                &req.to_addr,
                &req.value,
                chain_code,
                &token_symbol,
                &main_coin.symbol,
                token_key.clone(),
                token_decimals,
                spend_all_native,
            )
            .await
        };
        let fee_str = match estimate {
            Ok(fee_str) => fee_str,
            Err(err) if replacement.is_some() => return Err(err),
            Err(err) if Self::is_insufficient_fee_balance_error(&err) => {
                tracing::warn!(
                    trade_no = %req.trade_no,
                    from_addr = %req.from_addr,
                    to_addr = %req.to_addr,
                    chain_code = %req.chain_code,
                    token_addr = %req.token_addr,
                    source = "shadow_worker_v2",
                    "Fee estimation reported insufficient fee balance; reopening service fee cycle"
                );
                return Ok(false);
            }
            Err(err) if Self::should_reopen_fee_cycle_for_solana_token_collect(&req, &err) => {
                if Self::should_terminal_fail_solana_token_collect_rent_shortage(&req, &err) {
                    tracing::warn!(
                        trade_no = %req.trade_no,
                        from_addr = %req.from_addr,
                        to_addr = %req.to_addr,
                        chain_code = %req.chain_code,
                        token_addr = %req.token_addr,
                        source = "shadow_worker_v2",
                        "Fee estimation reported Solana sender rent reserve shortage after completed fee cycle; failing build"
                    );
                    return Err(err);
                }

                tracing::warn!(
                    trade_no = %req.trade_no,
                    from_addr = %req.from_addr,
                    to_addr = %req.to_addr,
                    chain_code = %req.chain_code,
                    token_addr = %req.token_addr,
                    source = "shadow_worker_v2",
                    "Fee estimation reported Solana sender rent reserve shortage; reopening service fee cycle"
                );
                return Ok(false);
            }
            Err(err) => return Err(err),
        };
        let fee = conversion::decimal_from_str(&fee_str)?;
        tracing::info!(trade_no=%req.trade_no, source = "shadow_worker_v2", "collect_tx:send: 估算手续费完成: {}", fee_str);

        // 计算需要的总金额
        let need = if spend_all_native {
            tracing::info!(
                trade_no=%req.trade_no,
                source = "shadow_worker_v2",
                "collect_tx:send: native SOL spend_all, fee check uses fee only and final amount will sweep the remaining balance"
            );
            let value = conversion::decimal_from_str(&req.value)?;
            tracing::info!(
                trade_no=%req.trade_no,
                source = "shadow_worker_v2",
                spend_all_native = true,
                requested_value = %value,
                "collect_tx:send: native SOL spend_all build branch selected"
            );
            Self::collect_balance_need(fee, value, true)?
        } else if req.token_addr.is_contract() {
            // Solana token collect 需要保留 sender 的 rent-exempt reserve。
            tracing::info!(
                trade_no = %req.trade_no,
                source = "shadow_worker_v2",
                sender_rent_reserve = %sender_rent_reserve,
                "collect_tx:send: 代币交易，手续费检查需要额外保留 sender rent reserve"
            );
            fee + sender_rent_reserve
        } else {
            // 主币交易需要手续费+转账金额
            let value = conversion::decimal_from_str(&req.value)?;
            tracing::info!(trade_no=%req.trade_no, source = "shadow_worker_v2", "collect_tx:send: 主币交易，需要手续费+转账金额, 转账金额={}", value);
            Self::collect_balance_need(fee, value, false)?
        };

        tracing::info!(trade_no=%req.trade_no, source = "shadow_worker_v2", "collect_tx:send: 手续费检查结果 - 可用余额: {}, 需要金额: {}, 手续费: {}", balance, need, fee);

        if fee > Decimal::from(0) && balance < need {
            if replacement.is_some() {
                return Err(ServiceError::Business(BusinessError::Chain(
                    ChainError::InsufficientFeeBalance,
                )));
            }
            if Self::tron_token_collect_resource_gate_covers_native_fee(req) {
                tracing::info!(
                    trade_no = %req.trade_no,
                    from_addr = %req.from_addr,
                    to_addr = %req.to_addr,
                    balance = %balance,
                    need = %need,
                    fee = %fee,
                    resource_gate_released_at = ?req.resource_gate_released_at,
                    source = "shadow_worker_v2",
                    "collect_tx:send: TRON token collect resource gate released; bypassing native fee balance gate"
                );
                return Ok(true);
            }

            tracing::warn!(
                trade_no = %req.trade_no,
                from_addr = %req.from_addr,
                to_addr = %req.to_addr,
                chain_code = %req.chain_code,
                token_addr = %req.token_addr,
                balance = %balance,
                need = %need,
                fee = %fee,
                stage = "build.check_fee",
                source = "shadow_worker_v2",
                "Insufficient balance for build fee check"
            );

            // 计算需要补充的手续费
            // NOTE: fee_to_upload is calculated for Fee module consumption.
            // Shadow worker must not trigger fee upload.
            // 由 caller 进入后续的 recover / side-effect 流程处理手续费补单。
            Ok(false)
        } else {
            tracing::info!(trade_no=%req.trade_no, source = "shadow_worker_v2", "collect_tx:send: 手续费充足，继续交易");
            Ok(true)
        }
    }

    /// 检查可归集金额是否充足。
    ///
    /// 返回值语义：
    /// - Ok(true): 可归集金额充足
    /// - Ok(false): 可归集金额不足，caller 必须直接失败，不再进入 fee 逻辑
    /// - Err(_): 基础设施错误
    pub(crate) async fn check_collect_amount(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<bool, ServiceError> {
        let ctx = self.ctx;
        let chain_code: ChainCode = req.chain_code.as_str().try_into()?;
        let token_key = if req.token_addr.is_contract() {
            req.token_addr.clone()
        } else {
            AssetTokenKey::Native
        };

        let token_coin = if req.token_addr.is_contract() {
            ApiCoinDomain::get_coin_by_token_key_exact(
                self.ctx,
                &req.chain_code,
                req.token_addr.clone(),
            )
            .await?
        } else {
            ApiChainTransDomain::main_coin(ctx, &req.chain_code).await?
        };

        let balance_str =
            self.query_balance(&req.from_addr, chain_code, token_key, token_coin.decimals).await?;
        let value = conversion::decimal_from_str(&req.value)?;

        tracing::info!(
            trade_no = %req.trade_no,
            balance = %balance_str,
            requested_value = %value,
            token_addr = %req.token_addr,
            source = "shadow_worker_v2",
            "collect_tx:send: 归集金额检查完成"
        );

        Ok(!Self::is_collect_amount_shortage(&balance_str, &req.value)?)
    }

    async fn mark_collect_amount_insufficient(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<(), ServiceError> {
        let err = ServiceError::Business(BusinessError::Chain(ChainError::InsufficientBalance(
            InsufficientBalanceDetail::new()
                .from_addr(req.from_addr.clone())
                .to_addr(req.to_addr.clone())
                .chain_code(req.chain_code.clone())
                .token_addr(req.token_addr.to_string())
                .value(req.value.clone())
                .reason("collect amount is insufficient; balance is below requested value"),
        )));

        self.clear_build_slot_after_claim(&req.trade_no).await?;

        let rows_affected = ApiCollectRepo::update_api_collect_status_and_err(
            &self.ctx.api_transaction_pool()?,
            &req.trade_no,
            ApiCollectStatus::InsufficientBalance,
            ErrCode::BalanceInsufficient,
            &format!("{}", err),
        )
        .await
        .map_err(|db_err| {
            error!(
                trade_no = %req.trade_no,
                error = %db_err,
                source = "shadow_worker_v2",
                "Failed to mark collect as terminal failure due to insufficient balance"
            );
            ServiceError::Database(db_err.into())
        })?;

        info!(
            trade_no = %req.trade_no,
            rows_affected = %rows_affected,
            source = "shadow_worker_v2",
            "Marked collect as terminal failure due to insufficient balance"
        );
        self.advancer.try_advance(&req.trade_no).await;

        Ok(())
    }

    /// Fee 不足时的构建回退策略。
    ///
    /// - 首次手续费不足：继续打回 `need_service_fee = true`
    /// - EVM fee cycle 完成后不足：保留补费事实，180 秒内交给 scanner 重估
    /// - 等待到期仍不足或非 EVM：终止订单，不再补费
    pub async fn invalidate_build_attempt_after_fee_check_failure(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<u64, ServiceError> {
        if shared_mechanism::is_evm_chain(&req.chain_code) {
            let _guard = self.address_locks.acquire(&req.from_addr).await?;
            let fresh = self.get_collect_entity(&req.trade_no).await?;
            if fresh.raw_tx.is_some()
                || fresh.tx_hash.is_some()
                || fresh.last_broadcast_at.is_some()
                || fresh.transaction_time.is_some()
                || fresh.finished_at.is_some()
                || fresh.err_code.is_some()
                || fresh.need_service_fee == Some(true)
                || (fresh.service_fee_uploaded_at.is_some()
                    && fresh.tx_fee_res_ack_sent_at.is_none())
            {
                return Ok(0);
            }
            return self.invalidate_unbuilt_fee_attempt(&fresh).await;
        }
        self.invalidate_unbuilt_fee_attempt(req).await
    }

    // EVM callers hold the address lock through fresh-read and fact update.
    async fn invalidate_unbuilt_fee_attempt(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<u64, ServiceError> {
        if Self::service_fee_cycle_completed(req)
            && !Self::should_terminal_fail_fee_shortage_after_completed_fee_cycle(req)
        {
            ApiCollectRepo::invalidate_raw_tx_for_rebuild(
                &self.ctx.api_transaction_pool()?,
                &req.trade_no,
                Some(ApiCollectStatus::InsufficientBalance),
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
            info!(trade_no = %req.trade_no, tx_fee_res_ack_sent_at = ?req.tx_fee_res_ack_sent_at,
                source = "shadow_worker_v2",
                "Post-payment EVM fee shortage; awaiting scanner re-estimation without another payment");
            // Returning zero suppresses immediate fast-path builds; the existing
            // 30-second scan predicate retries and survives restarts/cancellation.
            return Ok(0);
        }
        if Self::should_terminal_fail_fee_shortage_after_completed_fee_cycle(req) {
            info!(
                trade_no = %req.trade_no,
                chain_code = %req.chain_code,
                service_fee_uploaded_at = ?req.service_fee_uploaded_at,
                tx_fee_res_ack_sent_at = ?req.tx_fee_res_ack_sent_at,
                source = "shadow_worker_v2",
                "Collect fee shortage after the single completed fee cycle; marking collect as terminal failure"
            );

            let rows_affected = ApiCollectRepo::update_api_collect_status_and_err(
                &self.ctx.api_transaction_pool()?,
                &req.trade_no,
                ApiCollectStatus::InsufficientBalance,
                ErrCode::BalanceInsufficient,
                "collect fee shortage after the single completed fee cycle",
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;

            info!(
                trade_no = %req.trade_no,
                rows_affected = %rows_affected,
                source = "shadow_worker_v2",
                "Marked collect as terminal failure after the single completed fee cycle"
            );
            return Ok(0);
        }

        info!(
            trade_no = %req.trade_no,
            service_fee_uploaded_at = ?req.service_fee_uploaded_at,
            tx_fee_res_ack_sent_at = ?req.tx_fee_res_ack_sent_at,
            source = "shadow_worker_v2",
            "No fee-cycle facts found; reopening service fee cycle after fee check failure"
        );

        ApiCollectRepo::invalidate_raw_tx_need_service_fee(
            &self.ctx.api_transaction_pool()?,
            &req.trade_no,
            Some(ApiCollectStatus::InsufficientBalance),
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))
    }

    pub(crate) async fn resolve_withdraw_from_addr(
        ctx: &'static crate::context::Context,
        pool: &ApiWalletDbPool,
        req: &ApiCollectEntity,
    ) -> Result<String, ServiceError> {
        tracing::info!(trade_no=%req.trade_no, "collect_tx:send: resolve_withdraw_from_addr: 开始解析提币地址");
        // 1. 根据from_addr + chain_code查询account
        let account = match ApiAccountRepo::find_one_by_address_chain_code(
            &req.from_addr,
            &req.chain_code,
            &pool,
        )
        .await?
        {
            Some(account) => account,
            None => {
                tracing::warn!(trade_no=%req.trade_no, "collect_tx:send: resolve_withdraw_from_addr: 提币账户不存在, from_addr={}, chain_code={}", req.from_addr, req.chain_code);
                return Err(ServiceError::Business(
                    crate::error::business::BusinessError::ApiWallet(
                        crate::error::business::api_wallet::ApiWalletError::Account(
                            crate::error::business::api_wallet::account::AccountError::NotFound,
                        ),
                    ),
                ));
            }
        };

        // 2. 根据account.wallet_address查询wallet
        let wallet = match ApiWalletRepo::find_by_address(&pool.clone(), &account.wallet_address)
            .await?
        {
            Some(wallet) => wallet,
            None => {
                tracing::warn!(trade_no=%req.trade_no, "collect_tx:send: resolve_withdraw_from_addr: 钱包不存在, wallet_address={}", account.wallet_address);
                return Err(ServiceError::Business(
                    crate::error::business::BusinessError::ApiWallet(
                        crate::error::business::api_wallet::ApiWalletError::Wallet(
                            crate::error::business::api_wallet::wallet::WalletError::NotFound
                                .into(),
                        ),
                    ),
                ));
            }
        };
        let Some(bind_address) = wallet.binding_address else {
            tracing::warn!(trade_no=%req.trade_no, "collect_tx:send: resolve_withdraw_from_addr: 钱包未绑定地址, wallet_address={}", account.wallet_address);
            return Err(ServiceError::Business(
                crate::error::business::BusinessError::ApiWallet(
                    crate::error::business::api_wallet::ApiWalletError::Wallet(
                        crate::error::business::api_wallet::wallet::WalletError::SubAccountWalletNotBoundWithdrawalWalletAddress
                            .into(),
                    ),
                ),
            ));
        };

        let Some(withdraw_wallet) =
            ApiWalletRepo::find_by_address(&pool.clone(), &bind_address).await?
        else {
            tracing::warn!(trade_no=%req.trade_no, "collect_tx:send: resolve_withdraw_from_addr: 出款钱包不存在, bind_address={}", bind_address);
            return Err(ServiceError::Business(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::ApiWalletError::Wallet(
                    crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
                ),
            )));
        };

        // 3. 查询用户提币策略
        let strategy =
            StrategyDomain::query_withdraw_strategy_with_ctx(ctx, &withdraw_wallet.uid).await?;
        tracing::info!(trade_no=%req.trade_no, "collect_tx:send: resolve_withdraw_from_addr: 获取提现策略成功, 包含 {} 条链配置", strategy.chain_configs.len());

        // 4. 根据chain_code查询链配置
        let chain_config = match strategy
            .chain_configs
            .into_iter()
            .find(|config| config.chain_code == req.chain_code)
        {
            Some(config) => config,
            None => {
                tracing::error!(trade_no=%req.trade_no, "collect_tx:send: resolve_withdraw_from_addr: 未找到对应的链配置, chain_code={}", req.chain_code);
                return Err(ServiceError::Business(
                    crate::error::business::BusinessError::ApiWallet(
                        crate::error::business::api_wallet::ApiWalletError::ChainConfigNotFound(
                            req.chain_code.clone(),
                        ),
                    ),
                ));
            }
        };

        // 5. 根据risk_addr决定normal/risk地址
        // risk_addr: 1 正常地址，2 风险地址
        let exec_to_addr = chain_config.normal_address.address;

        tracing::info!(trade_no=%req.trade_no, "collect_tx:send: resolve_withdraw_from_addr: 解析执行地址成功, exec_to_addr={}", exec_to_addr);
        Ok(exec_to_addr)
    }

    /// 查询余额
    async fn query_balance(
        &self,
        owner_address: &str,
        chain_code: ChainCode,
        token_key: AssetTokenKey,
        decimals: u8,
    ) -> Result<String, ServiceError> {
        tracing::info!(owner_address=%owner_address, chain_code=%chain_code.to_string(), token_address=%token_key.as_db_str(),
            source = "shadow_worker_v2", "collect_tx:send: 查询余额");

        let adapter = ApiChainAdapterFactory::get_transaction_adapter_with_ctx(
            self.ctx,
            &chain_code.to_string(),
        )
        .await?;
        let balance = adapter.balance_token_key(&owner_address, token_key.clone()).await?;
        let amount = unit::format_to_string(balance, decimals)?;

        tracing::info!(owner_address=%owner_address, chain_code=%chain_code.to_string(), token_address=%token_key.as_db_str(),
            source = "shadow_worker_v2", "collect_tx:send: 查询余额完成: {}", amount);
        Ok(amount)
    }

    pub(crate) async fn estimate_fee(
        &self,
        from: &str,
        to: &str,
        value: &str,
        chain_code: ChainCode,
        symbol: &str,
        main_symbol: &str,
        token_key: AssetTokenKey,
        decimals: u8,
        spend_all_native: bool,
    ) -> Result<String, ServiceError> {
        // TODO: 可优化速度
        let start_time = std::time::Instant::now();
        tracing::info!(from=%from, to=%to, value=%value, chain_code=%chain_code.to_string(), symbol=%symbol,
            main_symbol=%main_symbol, token_address=%token_key.as_db_str(),
            source = "shadow_worker_v2", "collect_tx:send: 估算交易手续费开始");

        let adapter_start = std::time::Instant::now();
        let adapter = ApiChainAdapterFactory::get_transaction_adapter_with_ctx(
            self.ctx,
            &chain_code.to_string(),
        )
        .await?;
        tracing::info!(chain_code=%chain_code.to_string(), duration_ms=%adapter_start.elapsed().as_millis(), source = "shadow_worker_v2", "collect_tx:send: 获取适配器完成");

        let params_start = std::time::Instant::now();
        let mut params = ApiBaseTransferReq::new(from, to, value, &chain_code.to_string());
        params.with_token(token_key.to_chain_token_option(), decimals, symbol);
        params.spend_all = spend_all_native;
        if spend_all_native {
            params.metadata = Some(COLLECT_IGNORE_SENDER_RENT_METADATA.to_string());
        }
        tracing::info!(chain_code=%chain_code.to_string(), duration_ms=%params_start.elapsed().as_millis(), source = "shadow_worker_v2", "collect_tx:send: 构建请求参数完成");

        let estimate_start = std::time::Instant::now();
        let amount = if crate::domain::chain::is_evm_chain(chain_code) {
            super::estimate_evm_collect_fee(adapter.as_ref(), chain_code, params, main_symbol)
                .await?
        } else {
            let fee = adapter.estimate_fee(params, main_symbol).await?;
            tracing::info!(chain_code=%chain_code.to_string(), duration_ms=%estimate_start.elapsed().as_millis(), source = "shadow_worker_v2", "collect_tx:send: 调用estimate_fee完成");

            let parse_start = std::time::Instant::now();
            let amount = match chain_code {
                ChainCode::Tron => {
                    let res: TronFeeDetails = wallet_utils::serde_func::serde_from_str(&fee)?;
                    res.estimate_fee.amount.to_string()
                }
                ChainCode::Bitcoin => todo!(),
                ChainCode::Solana => {
                    let res: CommonFeeDetails = wallet_utils::serde_func::serde_from_str(&fee)?;
                    res.estimate_fee.amount.to_string()
                }
                ChainCode::Ethereum
                | ChainCode::BnbSmartChain
                | ChainCode::Polygon
                | ChainCode::Avalanche
                | ChainCode::Fantom
                | ChainCode::Arbitrum => super::evm_propose_max_fee(&fee)?,
                ChainCode::Litecoin => todo!(),
                ChainCode::Dogcoin => todo!(),
                ChainCode::Sui => todo!(),
                ChainCode::Ton => todo!(),
                _ => {
                    return Err(ServiceError::Parameter(format!(
                        "unsupported collect chain: {chain_code}"
                    )));
                }
            };
            tracing::info!(chain_code=%chain_code.to_string(), duration_ms=%parse_start.elapsed().as_millis(), source = "shadow_worker_v2", "collect_tx:send: 解析手续费结果完成");
            amount
        };

        tracing::info!(from=%from, to=%to, chain_code=%chain_code.to_string(), total_duration_ms=%start_time.elapsed().as_millis(), source = "shadow_worker_v2", "collect_tx:send: 估算手续费完成: {}", amount);
        Ok(amount)
    }

    async fn estimate_tron_fee_details(
        &self,
        from: &str,
        to: &str,
        value: &str,
        symbol: &str,
        main_symbol: &str,
        token_key: AssetTokenKey,
        decimals: u8,
    ) -> Result<TronFeeDetails, ServiceError> {
        let adapter =
            ApiChainAdapterFactory::get_transaction_adapter_with_ctx(self.ctx, "tron").await?;
        let mut params = ApiBaseTransferReq::new(from, to, value, "tron");
        params.with_token(token_key.to_chain_token_option(), decimals, symbol);
        let fee = adapter.estimate_fee(params, main_symbol).await?;
        let details: TronFeeDetails = wallet_utils::serde_func::serde_from_str(&fee)?;
        Ok(details)
    }

    /// 获取归集配置
    async fn get_collect_config(
        &self,
        uid: &str,
        chain_code: &str,
    ) -> Result<ChainConfig, ServiceError> {
        tracing::info!(uid=%uid, chain_code=%chain_code, source = "shadow_worker_v2", "collect_tx:send: 查询归集策略");

        // 查询策略
        let ctx = self.ctx;
        let strategy = StrategyDomain::query_collect_strategy_with_ctx(ctx, uid).await?;

        tracing::info!(uid=%uid, source = "shadow_worker_v2", "collect_tx:send: 获取归集策略成功，包含 {} 条链配置", strategy.chain_configs.len());

        let Some(chain_config) =
            strategy.chain_configs.into_iter().find(|config| config.chain_code == chain_code)
        else {
            tracing::error!(uid=%uid, chain_code=%chain_code, source = "shadow_worker_v2", "collect_tx:send: 未找到对应的链配置");
            return Err(crate::error::business::BusinessError::ApiWallet(
                ApiWalletError::ChainConfigNotFound(chain_code.to_owned()),
            )
            .into());
        };

        tracing::info!(uid=%uid, chain_code=%chain_code, source = "shadow_worker_v2", "collect_tx:send: 找到链配置, normal_address={}", chain_config.normal_address.address);
        Ok(chain_config)
    }
    async fn check_digest(&self, req: &ApiCollectEntity) -> Result<bool, ServiceError> {
        info!(trade_no = %req.trade_no, source = "shadow_worker_v2", "Checking transaction digest");

        let sn = self.ctx.get_sn();
        let mut d = wallet_utils::conversion::decimal_from_str(req.value.as_str())?;
        d = d.normalize();
        // ⚠️ 这里必须用后端给的空字符串的to_addr，不能用查询策略解析的地址
        let raw_data = req.from_addr.clone() + "" + d.to_string().as_str() + sn;
        let digest = wallet_utils::bytes_to_base64(&wallet_utils::md5_vec(&raw_data));

        let is_valid = req.validate == digest;
        tracing::info!(trade_no=%req.trade_no, "collect_tx:send: 交易摘要验证完成, 结果: {}", is_valid);
        Ok(is_valid)
    }

    /// 生成转账请求
    ///
    /// The nonce candidate is observed in Phase 1 and committed after signing.
    /// gen_transfer_req MUST NOT:
    /// - compute nonce
    /// - fallback nonce
    /// - modify nonce semantics
    async fn gen_transfer_req(
        &self,
        req: &ApiCollectEntity,
        exec_to_addr: &str,
        nonce: u64, // 外部传入的nonce
    ) -> Result<crate::request::api_wallet::trans::ApiTransferReq, ServiceError> {
        tracing::info!(trade_no=%req.trade_no, "collect_tx:send: 开始生成转账请求, exec_to_addr={}, nonce={}", exec_to_addr, nonce);

        // 获取币种信息
        let coin = ApiCoinDomain::get_coin_by_token_key_exact(
            self.ctx,
            &req.chain_code,
            req.token_addr.clone().into(),
        )
        .await?;
        tracing::info!(trade_no=%req.trade_no, "collect_tx:send: 获取币种信息成功, symbol={}, token_address={:?}, decimals={}", 
            coin.symbol, coin.token_address, coin.decimals);

        // 创建基础转账请求 - 使用exec_to_addr而非req.to_addr
        let mut params =
            ApiBaseTransferReq::new(&req.from_addr, exec_to_addr, &req.value, &req.chain_code);
        let token_address = if coin.token_address.is_native() {
            None
        } else {
            let s = coin.token_address.as_db_str().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        params.with_token(token_address, coin.decimals, &coin.symbol);
        params.spend_all =
            Self::should_spend_all_native_collect(&req.chain_code, &coin.token_address);
        if params.spend_all {
            params.metadata = Some(COLLECT_IGNORE_SENDER_RENT_METADATA.to_string());
        }
        tracing::info!(trade_no=%req.trade_no, "collect_tx:send: 创建基础转账请求成功");

        // 获取钱包解锁态 token
        let unlock_token = ApiWalletDomain::get_wallet_unlock_token().await?;
        tracing::info!(trade_no=%req.trade_no, "collect_tx:send: 获取钱包解锁态成功");

        let transfer_req = ApiTransferReq { base: params, password: unlock_token, nonce };
        tracing::info!(trade_no=%req.trade_no, "collect_tx:send: 生成转账请求成功");
        Ok(transfer_req)
    }

    /// Observe an unreserved EVM nonce candidate. Failures and cancellations
    /// consume nothing; reserve_signed_collect rechecks the counter when signed
    /// facts commit. Persisted signed slots are retained when selecting unsigned holes.
    async fn get_nonce(
        &self,
        from_addr: &str,
        chain_code: &str,
    ) -> Result<(u64, i64), ServiceError> {
        info!(from_addr = %from_addr, chain_code = %chain_code, source = "shadow_worker_v2", "Getting nonce");
        let chain: ChainCode = chain_code.try_into()?;

        match chain {
            c if crate::domain::chain::is_evm_chain(c) => {
                let nonce_engine = get_nonce_engine_with_ctx(self.ctx)?;
                nonce_engine.observe_signed_nonce_candidate(from_addr, chain_code).await
            }
            _ => {
                // 非 EVM 链不参与 nonce 分配
                Ok((0, 0))
            }
        }
    }

    /// 交易恢复逻辑 - 处理已有tx_hash的交易
    ///
    /// ⚠️ IMPORTANT:
    /// - Recover logic MUST only be triggered by Scanner commands
    /// - This method should NOT be called directly by other components
    /// - On-chain confirmation fact is owned by Scanner / Shadow Recovery ONLY
    async fn recover_tx(
        &self,
        req: &ApiCollectEntity,
    ) -> Result<RecoveredTxObservation, ServiceError> {
        let tx_hash = req.tx_hash.as_ref().unwrap();
        info!(trade_no = %req.trade_no, tx_hash = %tx_hash, source = "shadow_worker_v2", "Processing recovered tx");

        match crate::domain::api_wallet::trans::ApiTransDomain::observe_recovered_tx_with_ctx(
            &self.ctx,
            &req.chain_code,
            &req.from_addr,
            tx_hash,
            req.nonce,
            &req.transaction_fee,
        )
        .await
        {
            Ok(RecoveredTxObservation::Confirmed(tx_resp)) => {
                info!(trade_no = %req.trade_no, tx_hash = %tx_hash, source = "shadow_worker_v2", "Recovered tx success");
                Ok(RecoveredTxObservation::Confirmed(tx_resp))
            }
            Ok(RecoveredTxObservation::Missing) => {
                info!(trade_no = %req.trade_no, tx_hash = %tx_hash, source = "shadow_worker_v2", "Recovered tx result is uncertain, will retry");
                Ok(RecoveredTxObservation::Missing)
            }
            Ok(RecoveredTxObservation::QueryUncertain) => {
                Ok(RecoveredTxObservation::QueryUncertain)
            }
            Err(err) => {
                error!(trade_no = %req.trade_no, tx_hash = %tx_hash, error = %err, source = "shadow_worker_v2", "Recovered tx failed");
                Err(err)
            }
        }
    }

    fn is_recovered_tx_terminal_chain_failure(err: &ServiceError) -> bool {
        crate::domain::api_wallet::trans::is_confirmed_recover_chain_failure(err)
    }

    pub(super) fn should_reopen_evm_fee_cycle(req: &ApiCollectEntity, err: &ServiceError) -> bool {
        (matches!(req.chain_code.as_str(), "base" | "scroll")
            || (shared_mechanism::is_evm_chain(&req.chain_code)
                && Self::service_fee_cycle_completed(req)))
            && req.raw_tx.is_none()
            && req.tx_hash.is_none()
            && req.last_broadcast_at.is_none()
            && Self::is_insufficient_fee_balance_error(err)
    }

    // Serialize with Build phase 3: a stale failure must never clear committed raw.
    pub(super) async fn reopen_evm_fee_cycle_if_unbuilt(
        &self,
        stale_req: &ApiCollectEntity,
        err: &ServiceError,
    ) -> Result<u64, ServiceError> {
        let _addr_guard = self.address_locks.acquire(&stale_req.from_addr).await?;
        let fresh_req = self.get_collect_entity(&stale_req.trade_no).await?;
        if fresh_req.transaction_time.is_some()
            || fresh_req.status.is_terminal()
            || fresh_req.finished_at.is_some()
            || fresh_req.err_code.is_some()
            || fresh_req.need_service_fee == Some(true)
            || (fresh_req.service_fee_uploaded_at.is_some()
                && fresh_req.tx_fee_res_ack_sent_at.is_none())
            || !Self::should_reopen_evm_fee_cycle(&fresh_req, err)
        {
            return Ok(0);
        }
        self.invalidate_unbuilt_fee_attempt(&fresh_req).await
    }

    /// 处理归集交易失败
    pub(super) async fn handle_collect_tx_failed(
        &self,
        trade_no: &str,
        err: ServiceError,
    ) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, error = %err, source = "shadow_worker_v2", "Handling collect tx failed");

        if err.is_transient_db_pool_timeout() {
            warn!(
                trade_no = %trade_no,
                error = %err,
                source = "shadow_worker_v2",
                "Skip marking collect tx failed: local DB pool timeout is retryable infrastructure noise"
            );
            return Ok(());
        }

        // 🔒 事实保护：检查是否已存在成功事实
        // 规则：一旦成功事实（transaction_time）成立，失败事实永远不能覆盖它
        // 这是事实系统的"单调性约束"
        let req = self.get_collect_entity(trade_no).await?;
        if req.transaction_time.is_some() {
            info!(
                trade_no = %trade_no,
                source = "shadow_worker_v2",
                "Skip mark failed: transaction already confirmed (monotonicity constraint)"
            );
            return Ok(());
        }

        if err.to_string().to_ascii_lowercase().contains("underpriced") {
            let rejected=wallet_database::repositories::api_wallet::collect_replacement::CollectReplacementRepo::fail_signed_quote(&self.ctx.api_transaction_pool()?,trade_no).await.map_err(|e|ServiceError::Database(e.into()))?;
            if rejected {
                self.advancer.try_advance(trade_no).await;
                return Ok(());
            }
        }
        if Self::is_insufficient_fee_balance_error(&err) {
            let abandoned=wallet_database::repositories::api_wallet::collect_replacement::CollectReplacementRepo::fail_unsigned_attempt(
                &self.ctx.api_transaction_pool()?,trade_no,&format!("replacement fee budget insufficient: {err}; no automatic top-up"),
            ).await.map_err(|e|ServiceError::Database(e.into()))?;
            if abandoned {
                self.advancer.try_advance(trade_no).await;
                return Ok(());
            }
        }

        // 🔒 事实保护：检查是否已被 invalidate_raw_tx 作废
        // 规则：一旦 build 事实被作废（need_service_fee = true），失败事实不能覆盖它
        // 这确保 invalidate_raw_tx 写入的错误上下文是"最终解释权"
        // NOTE:
        // need_service_fee = true represents a final build invalidation fact.
        // Failure here MUST NOT override it.
        if req.need_service_fee == Some(true) {
            info!(
                trade_no = %trade_no,
                source = "shadow_worker_v2",
                "Skip mark failed: build already invalidated (fact rollback already applied)"
            );
            return Ok(());
        }

        // The adapter may reject the final signed fee budget before returning raw.
        // Preserve the existing single-fee-cycle policy and never invalidate durable raw.
        if Self::should_reopen_evm_fee_cycle(&req, &err) {
            let affected = self.reopen_evm_fee_cycle_if_unbuilt(&req, &err).await?;
            self.fee_reservations.release(trade_no);
            if affected > 0 {
                self.advancer.try_advance(trade_no).await;
            }
            return Ok(());
        }

        if Self::should_open_fee_cycle_for_tron_resource_ready(&req, &err) {
            info!(
                trade_no = %trade_no,
                from_addr = %req.from_addr,
                chain_code = %req.chain_code,
                resource_gate_result = ?req.resource_gate_result,
                source = "shadow_worker_v2",
                "Detected TRON Energy-ready bandwidth fee shortage; opening service fee cycle"
            );

            let rows_affected = self.invalidate_build_attempt_after_fee_check_failure(&req).await?;

            if rows_affected > 0 {
                self.advancer.try_advance(&req.trade_no).await;
                return Ok(());
            }

            info!(
                trade_no = %trade_no,
                source = "shadow_worker_v2",
                "TRON ResourceReady fee shortage affected no rows because newer collect facts exist"
            );
        }

        if Self::is_solana_rent_exempt_reserve_balance_error(&req, &err) {
            if Self::service_fee_cycle_completed(&req) {
                info!(
                    trade_no = %trade_no,
                    error = %err,
                    service_fee_uploaded_at = ?req.service_fee_uploaded_at,
                    tx_fee_res_ack_sent_at = ?req.tx_fee_res_ack_sent_at,
                    source = "shadow_worker_v2",
                    "Detected Solana rent-exempt reserve shortage after completed fee cycle; marking collect as terminal failure"
                );

                self.clear_build_slot_after_claim(trade_no).await?;

                let rows_affected = ApiCollectRepo::update_api_collect_status_and_err(
                    &self.ctx.api_transaction_pool()?,
                    trade_no,
                    ApiCollectStatus::InsufficientBalance,
                    ErrCode::BalanceInsufficient,
                    &format!("{}", err),
                )
                .await
                .map_err(|db_err| {
                    error!(trade_no = %trade_no, error = %db_err, source = "shadow_worker_v2", "Failed to mark collect as terminal failure after completed fee cycle");
                    ServiceError::Database(db_err.into())
                })?;

                info!(
                    trade_no = %trade_no,
                    rows_affected = %rows_affected,
                    source = "shadow_worker_v2",
                    "Marked collect as terminal failure after completed fee cycle"
                );
                self.advancer.try_advance(&req.trade_no).await;
            } else {
                info!(
                    trade_no = %trade_no,
                    error = %err,
                    source = "shadow_worker_v2",
                    "Detected Solana rent-exempt reserve shortage; reopening service fee cycle"
                );

                let affected = self.invalidate_build_attempt_after_fee_check_failure(&req).await?;

                if affected == 0 {
                    info!(
                        trade_no = %trade_no,
                        source = "shadow_worker_v2",
                        "Transaction already invalidated or no raw_tx to invalidate, skip"
                    );
                    self.clear_build_slot_after_claim(trade_no).await?;
                } else {
                    self.advancer.try_advance(&req.trade_no).await;
                }
            }

            return Ok(());
        }

        if Self::is_collect_amount_insufficient_error(&err) {
            info!(
                trade_no = %trade_no,
                error = %err,
                source = "shadow_worker_v2",
                "Detected collect amount insufficient error; marking collect as terminal failure"
            );

            self.mark_collect_amount_insufficient(&req).await?;

            return Ok(());
        }

        if Self::should_fail_tron_platform_delegated_collect_after_build_resource_shortage(
            &req, &err,
        ) {
            info!(
                trade_no = %trade_no,
                error = %err,
                source = "shadow_worker_v2",
                "Detected TRON platform delegated collect resource shortage after BuildTx; marking collect as terminal failure"
            );

            self.clear_build_slot_after_claim(trade_no).await?;

            let rows_affected = ApiCollectRepo::update_api_collect_status_and_err(
                &self.ctx.api_transaction_pool()?,
                trade_no,
                ApiCollectStatus::SendingTxFailed,
                ErrCode::FeeInsufficient,
                &format!("{}", err),
            )
            .await
            .map_err(|db_err| {
                error!(trade_no = %trade_no, error = %db_err, source = "shadow_worker_v2", "Failed to mark collect as terminal failure after platform resource shortage");
                ServiceError::Database(db_err.into())
            })?;

            info!(
                trade_no = %trade_no,
                rows_affected = %rows_affected,
                source = "shadow_worker_v2",
                "Marked TRON platform delegated collect as terminal failure"
            );
            if rows_affected > 0 {
                self.advancer.try_advance(&req.trade_no).await;
            }

            return Ok(());
        }

        let build_rpc_error =
            crate::infrastructure::chain_rpc_guard::ChainRpcErrorContext::from_display(&err);
        if Self::should_retry_collect_build_service_unavailable(&req, &err) {
            info!(
                trade_no = %trade_no,
                error_code = ?build_rpc_error.error_code(),
                guarded_host = ?build_rpc_error.guarded_host(),
                transient_reason = ?build_rpc_error.transient_reason(),
                error = %build_rpc_error.message(),
                source = "shadow_worker_v2",
                "BuildTx service unavailable; releasing build slot for circuit-breaker retry"
            );

            self.clear_build_slot_after_claim(trade_no).await?;
            return Ok(());
        }

        // BuildTx 失败只需要释放 build slot，让 scanner 后续重试。
        // 这里不写失败事实，避免把可重试的构建失败误记成终态失败。
        if self.handle_build_tx_failure_without_raw_tx(trade_no, &req).await? {
            return Ok(());
        }

        let broadcast_failure_decision = broadcast_mechanism::classify_broadcast_failure(
            &err,
            broadcast_mechanism::BroadcastFailureFacts::broadcast_stage(
                &req.chain_code,
                req.raw_tx.is_some(),
                req.tx_hash.is_some(),
                req.last_broadcast_at.is_some(),
            ),
        );

        if let Some(rebuild_reason) = broadcast_failure_decision.raw_rebuild_reason() {
            let rows_affected = ApiCollectRepo::invalidate_raw_tx_for_rebuild(
                &self.ctx.api_transaction_pool()?,
                trade_no,
                None,
            )
            .await
            .map_err(|db_err: wallet_database::Error| {
                error!(trade_no = %trade_no, error = %db_err, source = "shadow_worker_v2", "{}", rebuild_reason.db_error_message());
                ServiceError::Database(db_err.into())
            })?;

            info!(
                trade_no = %trade_no,
                rows_affected = %rows_affected,
                error = %err,
                source = "shadow_worker_v2",
                "{}", rebuild_reason.log_message()
            );

            if rows_affected > 0 {
                self.advancer.try_advance(trade_no).await;
            }
            return Ok(());
        }

        let error_msg = format!("{}", err);
        if let Some(accepted_reason) = broadcast_failure_decision.accepted_reason() {
            if let Some(message) = accepted_reason.pre_self_heal_log_message() {
                info!(
                    trade_no = %trade_no,
                    source = "shadow_worker_v2",
                    "{}", message
                );
            }

            if accepted_reason.requires_nonce_self_heal() {
                nonce_recovery_mechanism::try_nonce_self_heal(
                    self.ctx,
                    trade_no,
                    &req.from_addr,
                    &req.chain_code,
                    &error_msg,
                    "shadow_worker_v2",
                )
                .await?;
            }

            let rows_affected =
                wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_broadcast_executed(
                    &self.ctx.api_transaction_pool()?,
                    trade_no,
                )
                .await
                .map_err(|db_err: wallet_database::Error| {
                    error!(trade_no = %trade_no, error = %db_err, source = "shadow_worker_v2", "{}", accepted_reason.db_error_message());
                    ServiceError::Database(db_err.into())
                })?;

            info!(
                trade_no = %trade_no,
                rows_affected = %rows_affected,
                error = %err,
                source = "shadow_worker_v2",
                "{}", accepted_reason.log_message()
            );

            self.advancer.try_advance(trade_no).await;
            return Ok(());
        }

        // 处理 nonce too low 错误
        if broadcast_failure_decision.should_fail_after_nonce_self_heal() {
            info!(trade_no = %trade_no, source = "shadow_worker_v2", "Detected nonce too low error, syncing nonce from chain");

            nonce_recovery_mechanism::try_nonce_self_heal(
                self.ctx,
                trade_no,
                &req.from_addr,
                &req.chain_code,
                &error_msg,
                "shadow_worker_v2",
            )
            .await?;

            let rows_affected = ApiCollectRepo::update_api_collect_status_and_err(
                &self.ctx.api_transaction_pool()?,
                trade_no,
                wallet_database::entities::api_collect::ApiCollectStatus::SendingTxFailed,
                ErrCode::SDKInternalError, // 使用通用错误码
                &error_msg,
            )
            .await
            .map_err(|db_err: wallet_database::Error| {
                error!(trade_no = %trade_no, error = %db_err, source = "shadow_worker_v2", "Failed to update status to failed");
                ServiceError::Database(db_err.into())
            })?;

            info!(trade_no = %trade_no, rows_affected = %rows_affected, source = "shadow_worker_v2", "Updated status to failed with nonce error");

            // 只有第一次写入失败事实才发送 Tick
            if rows_affected > 0 {
                // 直接调用 try_advance 进行点对点唤醒
                self.advancer.try_advance(&trade_no).await;
            }

            return Ok(());
        }

        // 更新数据库状态为失败
        match err.retry_policy() {
            wallet_utils::error::RetryPolicy::Never => {
                let err_code = if err.is_network_error() {
                    ErrCode::NetworkException
                } else {
                    ErrCode::SDKInternalError
                };

                let rows_affected = ApiCollectRepo::update_api_collect_status_and_err(
                    &self.ctx.api_transaction_pool()?,
                    trade_no,
                    wallet_database::entities::api_collect::ApiCollectStatus::SendingTxFailed,
                    err_code, // err_code - 通用失败码
                    &error_msg,
                )
                .await
                .map_err(|db_err: wallet_database::Error| {
                    error!(trade_no = %trade_no, error = %db_err, source = "shadow_worker_v2", "Failed to update status to failed");
                    ServiceError::Database(db_err.into())
                })?;
                info!(trade_no = %trade_no, rows_affected = %rows_affected, source = "shadow_worker_v2", "Updated status to failed");

                // 只有第一次写入失败事实才发送 Tick
                if rows_affected > 0 {
                    // 直接调用 try_advance 进行点对点唤醒
                    self.advancer.try_advance(&trade_no).await;
                }

                // 注意：Shadow Worker 是执行者，不是裁决者
                // 不设置 finished_at，因为链上事实尚未闭环
                // 只有 Scanner/Shadow Recovery 才能设置终态
            }
            wallet_utils::error::RetryPolicy::Delay => {
                tracing::info!(trade_no = %trade_no, error = %err, source = "shadow_worker_v2", "Collect tx failed, will retry later");
            }
        }

        Ok(())
    }

    async fn handle_build_tx_failure_without_raw_tx(
        &self,
        trade_no: &str,
        req: &ApiCollectEntity,
    ) -> Result<bool, ServiceError> {
        if req.raw_tx.is_some() {
            return Ok(false);
        }

        let rows_affected = ApiCollectRepo::clear_building_at(&self.ctx.api_transaction_pool()?, trade_no)
            .await
            .map_err(|e: wallet_database::Error| {
                error!(trade_no = %trade_no, error = %e, source = "shadow_worker_v2", "Failed to clear build slot for BuildTx failure");
                ServiceError::Database(e.into())
            })?;

        info!(
            trade_no = %trade_no,
            rows_affected = %rows_affected,
            source = "shadow_worker_v2",
            "BuildTx failure cleared build slot"
        );

        Ok(true)
    }

    async fn clear_build_slot_after_claim(&self, trade_no: &str) -> Result<(), ServiceError> {
        Self::clear_build_slot_after_claim_in_pool(&self.ctx.api_transaction_pool()?, trade_no)
            .await
    }

    async fn clear_build_slot_after_claim_in_pool(
        pool: &ApiTransactionDbPool,
        trade_no: &str,
    ) -> Result<(), ServiceError> {
        let rows_affected = ApiCollectRepo::clear_building_at(pool, trade_no)
            .await
            .map_err(|e: wallet_database::Error| {
                error!(trade_no = %trade_no, error = %e, source = "shadow_worker_v2", "Failed to clear build slot after BuildTx early exit");
                ServiceError::Database(e.into())
            })?;

        info!(
            trade_no = %trade_no,
            rows_affected = %rows_affected,
            source = "shadow_worker_v2",
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

        let rows_affected = ApiCollectRepo::clear_building_at(pool, trade_no)
            .await
            .map_err(|e: wallet_database::Error| {
                error!(trade_no = %trade_no, error = %e, source = "shadow_worker_v2", "Failed to clear stale build slot");
                ServiceError::Database(e.into())
            })?;

        info!(
            trade_no = %trade_no,
            rows_affected = %rows_affected,
            building_at = ?building_at,
            source = "shadow_worker_v2",
            "Cleared stale build slot before retrying BuildTx"
        );

        Ok(rows_affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EvmUncertainNonceAction, ResourceBudget, ResourceGateSnapshot, ResourceReservationDecision,
        ShadowCollectWorker, TronMissingPoolsRecoveryAction, WithdrawResourceReservationManager,
    };
    use crate::{
        context::api_wallet_backend::ApiWalletBackend,
        error::{service::ServiceError, system::SystemError},
        infrastructure::api_trans::collect::shadow::{ChainIntent, CollectIntent},
    };
    use async_trait::async_trait;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::{
        fs,
        path::PathBuf,
        str::FromStr,
        sync::{Arc, OnceLock},
    };
    use tempfile::tempdir;
    use tokio::sync::{OnceCell, mpsc};
    use wallet_chain_interact::{BillResourceConsume, tron::operations::RawTransactionParams};
    use wallet_database::{
        ApiWalletDbPool, SqliteContext,
        entities::{
            api_collect::{ApiCollectEntity, ApiCollectStatus, PendingCollectResourceBudget},
            api_resource_delegation::{
                ApiResourceDelegationOperationType, ApiResourceDelegationSource,
                NewApiResourceDelegation,
            },
            api_resource_gate::{
                ApiResourceBlockReason, ApiResourceDependencyType, ApiResourceGateResult,
            },
            api_resource_type::ApiResourceType,
            api_trade_type::ApiTradeType,
            asset_token_key::AssetTokenKey,
        },
        repositories::api_wallet::{
            collect::ApiCollectRepo, resource_delegation::ApiResourceDelegationRepo,
            tron_account_activation::ApiTronAccountActivationRepo,
        },
    };
    use wallet_transport_backend::{
        request::{
            DeviceDeleteReq, KeysInitReq,
            api_wallet::{
                address::ExpandAddressCompleteReq,
                swap::{ApiInitSwapReq, ApiInitSwapResponse},
                wallet::{AppIdImportReq, AppIdUidUsageReq, BindAppIdReq},
            },
        },
        response_vo::api_wallet::wallet::{
            AppIdUidUsageRes as ApiWalletAppIdUidUsageRes, KeysUidCheckRes,
            QueryUidBindInfoRes as QueryBindInfoRes,
            QueryWalletActivationInfoResp as ApiQueryActivationResp, UidStatus,
        },
    };

    #[test]
    fn evm_collect_fee_paths_use_propose_max_fee_only() {
        let response = serde_json::json!({
            "default": "propose",
            "data": [
                {
                    "type": "safe",
                    "estimateFee": balance_info("0.000002"),
                    "maxFee": balance_info("0.0000024"),
                    "feeSetting": ethereum_fee_setting()
                },
                {
                    "type": "propose",
                    "estimateFee": balance_info("0.000003"),
                    "maxFee": balance_info("0.00000698"),
                    "feeSetting": {
                        "gasLimit": 23_277,
                        "baseFee": "1",
                        "priorityFee": "1",
                        "maxFeePerGas": "300000000"
                    }
                },
                {
                    "type": "fast",
                    "estimateFee": balance_info("0.000004"),
                    "maxFee": balance_info("0.0000048"),
                    "feeSetting": ethereum_fee_setting()
                }
            ]
        });

        let amount = super::super::evm_propose_max_fee(&response.to_string())
            .expect("select propose max fee");

        assert_eq!(amount, "0.0000069831");
    }

    #[test]
    fn evm_collect_fee_paths_do_not_fallback_to_display_rounded_max_fee() {
        let response = serde_json::json!({
            "default": "propose",
            "data": [{
                "type": "propose",
                "estimateFee": balance_info("0.000003"),
                "maxFee": balance_info("0.00000698"),
                "feeSetting": {
                    "gasLimit": 23_277,
                    "baseFee": "1",
                    "priorityFee": "1",
                    "maxFeePerGas": "invalid"
                }
            }]
        });

        let err = super::super::evm_propose_max_fee(&response.to_string())
            .expect_err("invalid exact maximum fee must fail");

        assert!(err.to_string().contains("invalid"), "unexpected error: {err}");
    }

    fn balance_info(amount: &str) -> serde_json::Value {
        serde_json::json!({
            "amount": amount,
            "currency": "USD",
            "unitPrice": 1.0,
            "fiatValue": 0.0
        })
    }

    fn ethereum_fee_setting() -> serde_json::Value {
        serde_json::json!({
            "gasLimit": 50_000,
            "baseFee": "1",
            "priorityFee": "1",
            "maxFeePerGas": "3"
        })
    }

    #[test]
    fn collect_broadcast_guard_skip_does_not_become_uncertain() {
        use crate::infrastructure::chain_rpc_guard::GuardedCallOutcome;

        assert!(ShadowCollectWorker::collect_broadcast_guard_skip_requires_retry(
            &GuardedCallOutcome::<()>::SkippedBeforeExecution,
        ));
        assert!(!ShadowCollectWorker::collect_broadcast_guard_skip_requires_retry(
            &GuardedCallOutcome::<()>::ExecutionTimedOut,
        ));
        assert!(!ShadowCollectWorker::collect_broadcast_guard_skip_requires_retry(
            &GuardedCallOutcome::Executed(()),
        ));
    }

    #[derive(Default)]
    struct NoopApiWalletBackend;

    #[async_trait]
    impl ApiWalletBackend for NoopApiWalletBackend {
        async fn wallet_bind_appid(&self, _: BindAppIdReq) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn init_api_wallet(&self, _: AppIdImportReq) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn old_keys_init(&self, _: KeysInitReq) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn appid_import(&self, _: AppIdImportReq) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn appid_import_recharge_wallet(
            &self,
            _: wallet_transport_backend::request::api_wallet::wallet::AppIdImportRechargeWalletReq,
        ) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn keys_uid_check(&self, _: &str) -> Result<KeysUidCheckRes, ServiceError> {
            Ok(KeysUidCheckRes { uid: String::new(), status: UidStatus::NormalWallet })
        }
        async fn query_uid_bind_info(&self, _: &str) -> Result<QueryBindInfoRes, ServiceError> {
            Ok(QueryBindInfoRes {
                app_id: String::new(),
                org_id: String::new(),
                bind_status: false,
                sn: String::new(),
            })
        }
        async fn query_wallet_activation_info(
            &self,
            _: &str,
        ) -> Result<ApiQueryActivationResp, ServiceError> {
            Ok(ApiQueryActivationResp(vec![]))
        }
        async fn appid_uid_usage(
            &self,
            _: AppIdUidUsageReq,
        ) -> Result<ApiWalletAppIdUidUsageRes, ServiceError> {
            Ok(ApiWalletAppIdUidUsageRes { used: false })
        }
        async fn expand_address_complete(
            &self,
            _: ExpandAddressCompleteReq,
        ) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn appid_withdrawal_wallet_change(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn init_swap(&self, _: &ApiInitSwapReq) -> Result<ApiInitSwapResponse, ServiceError> {
            Ok(ApiInitSwapResponse { success: true, code: None, msg: None, data: None })
        }
        async fn device_delete(&self, _: &DeviceDeleteReq) -> Result<Option<()>, ServiceError> {
            Ok(None)
        }
    }

    async fn test_ctx() -> &'static crate::context::Context {
        let ctx = crate::context::init_context_with_api_wallet_backend(
            "wallet_api_shadow_test_sn",
            "unittest",
            {
                let mut dir = PathBuf::from(std::env::temp_dir());
                dir.push("wallet-api-shadow-worker-tests");
                fs::create_dir_all(&dir).expect("ensure temp dir");
                crate::dirs::Dirs::new(&dir.to_string_lossy()).expect("create dirs")
            },
            None,
            crate::config::Config::new(
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
            .expect("parse test config"),
            Arc::new(NoopApiWalletBackend::default()),
        )
        .await
        .expect("init test context");
        Box::leak(Box::new(ctx))
    }

    fn base_collect() -> ApiCollectEntity {
        ApiCollectEntity {
            id: 1,
            name: "collect".to_string(),
            uid: "uid".to_string(),
            from_addr: "from".to_string(),
            to_addr: "old-to".to_string(),
            value: "1.12".to_string(),
            validate: "digest".to_string(),
            chain_code: "sol".to_string(),
            token_addr: AssetTokenKey::Contract("token".to_string()),
            symbol: "USDC".to_string(),
            trade_no: "trade-no".to_string(),
            trade_type: 2,
            risk_addr: 1,
            status: ApiCollectStatus::Init,
            nonce: 0,
            tx_hash: None,
            transaction_fee: "0".to_string(),
            transaction_time: None,
            block_height: Some("0".to_string()),
            notes: Some(String::new()),
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: Some(String::new()),
            resource_check_at: None,
            resource_gate_released_at: None,
            resource_gate_result: None,
            resource_block_reason: None,
            resource_dependency_trade_no: None,
            resource_dependency_type: None,
            order_ack_sent_at: Some(Utc::now()),
            raw_tx: None,
            resource_consume: "0".to_string(),
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            result_ack_sent_at: None,
            result_ack_send_count: 0,
            tx_res_received_at: None,
            service_fee_order_received_at: None,
            service_fee_uploaded_at: None,
            need_service_fee: None,
            ever_needed_service_fee: false,
            tx_fee_res_ack_sent_at: None,
            tx_exec_receipt_uploaded_at: None,
            finished_at: None,
            created_at: Utc::now(),
            updated_at: Some(Utc::now()),
        }
    }

    #[test]
    fn collect_resource_gate_requires_order_ack_fact() {
        let mut collect = base_collect();

        collect.order_ack_sent_at = None;
        assert!(!ShadowCollectWorker::resource_gate_prerequisites_ready(&collect));

        collect.order_ack_sent_at = Some(Utc::now());
        assert!(ShadowCollectWorker::resource_gate_prerequisites_ready(&collect));
    }

    #[test]
    fn collect_build_service_unavailable_remains_retryable_regardless_of_updated_at() {
        let mut req = base_collect();
        req.updated_at = Some(Utc::now() - chrono::Duration::days(30));
        let err = ServiceError::System(SystemError::Internal(
            "Node response error: code=503, rpc=https://api.nileex.io/wallet/triggerconstantcontract"
                .to_string(),
        ));

        assert!(ShadowCollectWorker::should_retry_collect_build_service_unavailable(&req, &err));

        req.raw_tx = Some("{}".to_string());
        assert!(!ShadowCollectWorker::should_retry_collect_build_service_unavailable(&req, &err));
    }

    #[test]
    fn tron_missing_pools_recovery_action_observes_fresh_and_rebroadcasts_expired() {
        let mut req = base_collect();
        req.chain_code = "tron".to_string();
        req.last_broadcast_at = Some(Utc::now());

        assert_eq!(
            ShadowCollectWorker::tron_missing_pools_recovery_action(&req),
            TronMissingPoolsRecoveryAction::ObserveWithPointToPointRetry
        );

        req.last_broadcast_at = Some(
            Utc::now()
                - chrono::Duration::seconds(
                    ShadowCollectWorker::TRON_MISSING_CONFIRMED_AND_PENDING_REBROADCAST_TIMEOUT_SECS
                        + 1,
                ),
        );
        assert_eq!(
            ShadowCollectWorker::tron_missing_pools_recovery_action(&req),
            TronMissingPoolsRecoveryAction::Rebroadcast
        );
    }

    #[tokio::test]
    async fn clear_build_slot_after_claim_releases_building_at() {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();

        let collect_ctx = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db");
        let collect_pool = collect_ctx.into_transaction_db_pool().expect("transaction pool");
        let trade_no = "T_clear_build_slot_after_claim";
        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "from",
            "to",
            "1.12",
            "digest",
            "sol",
            None,
            "USDC",
            trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");

        let claimed = ApiCollectRepo::update_building_at(&collect_pool, trade_no)
            .await
            .expect("claim build slot");
        assert_eq!(claimed, 1);

        ShadowCollectWorker::clear_build_slot_after_claim_in_pool(&collect_pool, trade_no)
            .await
            .expect("clear build slot");

        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");
        assert!(persisted.building_at.is_none());
    }

    #[tokio::test]
    async fn reclaim_stale_build_slot_clears_and_allows_reclaim() {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();

        let collect_ctx = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db");
        let collect_pool = collect_ctx.into_transaction_db_pool().expect("transaction pool");
        let trade_no = "T_reclaim_stale_build_slot";
        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "from",
            "to",
            "1.12",
            "digest",
            "sol",
            None,
            "USDC",
            trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");
        sqlx::query(
            "UPDATE api_collect
             SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 building_at = datetime('now', '-31 seconds')
             WHERE trade_no = ?",
        )
        .bind(trade_no)
        .execute(collect_pool.as_ref())
        .await
        .expect("seed stale slot");

        let req = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");
        assert_eq!(
            req.building_at.map(|ts| Utc::now().signed_duration_since(ts).num_seconds() >= 30),
            Some(true)
        );

        let reclaimed = ShadowCollectWorker::reclaim_stale_build_slot_in_pool(
            &collect_pool,
            trade_no,
            req.building_at,
        )
        .await
        .expect("reclaim stale slot");
        assert!(reclaimed);

        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");
        assert!(persisted.building_at.is_none());

        let claimed = ApiCollectRepo::update_building_at(&collect_pool, trade_no)
            .await
            .expect("reclaim build slot");
        assert_eq!(claimed, 1);
    }

    #[tokio::test]
    async fn fresh_build_slot_is_not_reclaimed() {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();

        let collect_ctx = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db");
        let collect_pool = collect_ctx.into_transaction_db_pool().expect("transaction pool");
        let trade_no = "T_fresh_build_slot";
        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "from",
            "to",
            "1.12",
            "digest",
            "sol",
            None,
            "USDC",
            trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");
        sqlx::query(
            "UPDATE api_collect
             SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 building_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind(trade_no)
        .execute(collect_pool.as_ref())
        .await
        .expect("seed fresh slot");

        let req = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");
        let reclaimed = ShadowCollectWorker::reclaim_stale_build_slot_in_pool(
            &collect_pool,
            trade_no,
            req.building_at,
        )
        .await
        .expect("reclaim fresh slot");
        assert!(!reclaimed);

        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");
        assert!(persisted.building_at.is_some());

        let claimed = ApiCollectRepo::update_building_at(&collect_pool, trade_no)
            .await
            .expect("claim build slot");
        assert_eq!(claimed, 0);
    }

    #[test]
    fn collect_rebuild_refreshes_to_addr() {
        let mut req = base_collect();

        let changed = ShadowCollectWorker::apply_exec_to_addr(&mut req, "new-to");

        assert!(changed);
        assert_eq!(req.to_addr, "new-to");
    }

    #[test]
    fn collect_rebuild_keeps_to_addr_when_already_latest() {
        let mut req = base_collect();
        req.to_addr = "same-to".to_string();

        let changed = ShadowCollectWorker::apply_exec_to_addr(&mut req, "same-to");

        assert!(!changed);
        assert_eq!(req.to_addr, "same-to");
    }

    #[test]
    fn native_sol_collect_uses_spend_all() {
        assert!(ShadowCollectWorker::should_spend_all_native_collect(
            "sol",
            &AssetTokenKey::Native
        ));
        assert!(!ShadowCollectWorker::should_spend_all_native_collect(
            "eth",
            &AssetTokenKey::Native
        ));
        assert!(!ShadowCollectWorker::should_spend_all_native_collect(
            "sol",
            &AssetTokenKey::Contract("token".to_string())
        ));
    }

    #[test]
    fn native_sol_spend_all_fee_check_uses_fee_only() {
        let fee = Decimal::from_str("0.000005").expect("fee");
        let value = Decimal::from_str("0.01299088").expect("value");
        let need =
            ShadowCollectWorker::collect_balance_need(fee, value, true).expect("spend-all need");
        assert_eq!(need, fee);
    }

    #[test]
    fn non_spend_all_fee_check_keeps_requested_value() {
        let fee = Decimal::from_str("0.000005").expect("fee");
        let value = Decimal::from_str("0.01299088").expect("value");
        let need = ShadowCollectWorker::collect_balance_need(fee, value, false)
            .expect("non spend-all need");

        assert_eq!(need, fee + value);
    }

    #[test]
    fn collect_amount_shortage_detects_balance_below_requested_value() {
        assert!(
            ShadowCollectWorker::is_collect_amount_shortage("1.109998", "1.109999")
                .expect("compare")
        );
        assert!(
            !ShadowCollectWorker::is_collect_amount_shortage("1.109999", "1.109999")
                .expect("compare")
        );
    }

    #[test]
    fn resource_gate_delegate_trade_no_is_deterministic() {
        assert_eq!(
            ShadowCollectWorker::collect_local_delegate_trade_no("C_1"),
            "rsc_local_delegate_C_1"
        );
    }

    #[test]
    fn tron_collect_resource_gate_ignores_bandwidth_shortage() {
        let reservations = WithdrawResourceReservationManager::default();
        let required = ShadowCollectWorker::tron_collect_required_budget(14_650, 345);

        let bandwidth_only_shortage = reservations.reserve_if_available(
            "T_ENERGY_READY",
            "C_ENERGY_READY",
            required,
            ResourceBudget::new(73_442, 127),
        );
        assert!(matches!(bandwidth_only_shortage, ResourceReservationDecision::Reserved { .. }));

        let energy_shortage = reservations.reserve_if_available(
            "T_ENERGY_SHORT",
            "C_ENERGY_SHORT",
            required,
            ResourceBudget::new(14_649, 10_000),
        );
        assert!(matches!(energy_shortage, ResourceReservationDecision::NeedMore(_)));
    }

    #[test]
    fn collect_pre_delegation_fee_ignores_energy_burn_cost() {
        let fee = ShadowCollectWorker::collect_pre_delegation_fee_trx(345, 0, 0.001, 0.0);

        assert!((fee - 0.345).abs() < f64::EPSILON);
    }

    #[test]
    fn collect_pre_delegation_fee_charges_full_bandwidth_when_not_fully_covered() {
        let insufficient_bandwidth =
            ShadowCollectWorker::collect_pre_delegation_fee_trx(345, 286, 0.001, 0.12);
        let covered_bandwidth =
            ShadowCollectWorker::collect_pre_delegation_fee_trx(345, 500, 0.001, 0.12);

        assert!((insufficient_bandwidth - 0.465).abs() < f64::EPSILON);
        assert!((covered_bandwidth - 0.12).abs() < f64::EPSILON);
    }

    #[test]
    fn tron_collect_amount_is_prechecked_before_resource_gate() {
        let mut req = base_collect();
        req.chain_code = "tron".to_string();
        req.token_addr = AssetTokenKey::Native;
        req.raw_tx = None;
        req.transaction_time = None;
        req.finished_at = None;
        req.err_code = None;
        req.resource_gate_released_at = None;

        assert!(ShadowCollectWorker::should_precheck_collect_amount_before_resource_gate(&req));

        req.resource_gate_released_at = Some(Utc::now());
        assert!(!ShadowCollectWorker::should_precheck_collect_amount_before_resource_gate(&req));
    }

    #[test]
    fn tron_collect_inactive_resource_snapshot_requires_account_activation() {
        let inactive_snapshot = ResourceGateSnapshot {
            estimated_fee_trx: 0.0,
            required_energy: 500,
            required_bandwidth: 268,
            available_energy: 0,
            available_bandwidth: 0,
            energy_price: 0.0,
        };
        assert!(ShadowCollectWorker::resource_snapshot_needs_account_activation(
            &inactive_snapshot
        ));

        let active_resource_shortfall =
            ResourceGateSnapshot { energy_price: 400.0, ..inactive_snapshot };
        assert!(!ShadowCollectWorker::resource_snapshot_needs_account_activation(
            &active_resource_shortfall
        ));

        let resource_ready = ResourceGateSnapshot { required_energy: 0, ..inactive_snapshot };
        assert!(!ShadowCollectWorker::resource_snapshot_needs_account_activation(&resource_ready));
        assert_eq!(
            ShadowCollectWorker::collect_account_activation_trade_no("C_INACTIVE"),
            "act_C_INACTIVE"
        );
    }

    #[tokio::test]
    async fn account_activation_block_commit_is_idempotent_and_blocks_collect_build() {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();

        let collect_ctx = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db");
        let collect_pool = collect_ctx.into_transaction_db_pool().expect("transaction pool");
        let resource_ctx = SqliteContext::new(&dir_path, Some("api_resource.db"))
            .await
            .expect("init api_resource.db");
        let resource_pool = resource_ctx.into_api_resource_db_pool().expect("resource pool");

        let trade_no = "C_account_activation_block";
        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "inactive_from",
            "to",
            "1.12",
            "digest",
            "tron",
            None,
            "USDT",
            trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");
        let req = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");

        ShadowCollectWorker::commit_account_activation_block_in_pools(
            &collect_pool,
            &resource_pool,
            trade_no,
            &req,
            "withdraw_payer",
            100,
            0,
            0.0,
        )
        .await
        .expect("first account activation block commit");
        ShadowCollectWorker::commit_account_activation_block_in_pools(
            &collect_pool,
            &resource_pool,
            trade_no,
            &req,
            "withdraw_payer",
            100,
            0,
            0.0,
        )
        .await
        .expect("second account activation block commit");

        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");
        assert!(persisted.resource_gate_released_at.is_none());
        assert!(persisted.raw_tx.is_none());
        assert_eq!(
            persisted.resource_block_reason,
            Some(ApiResourceBlockReason::NeedAccountActivation)
        );
        assert_eq!(
            persisted.resource_dependency_type,
            Some(ApiResourceDependencyType::AccountActivation)
        );
        assert_eq!(
            persisted.resource_dependency_trade_no.as_deref(),
            Some("act_C_account_activation_block")
        );
        assert!(
            ApiCollectRepo::scan_can_build(&collect_pool, 10)
                .await
                .expect("scan can build")
                .is_empty()
        );

        let activation = ApiTronAccountActivationRepo::find_by_activation_trade_no(
            &resource_pool,
            "act_C_account_activation_block",
        )
        .await
        .expect("load activation")
        .expect("activation task");
        assert_eq!(activation.origin_trade_no, trade_no);
        assert_eq!(activation.payer_address, "withdraw_payer");
        assert_eq!(activation.target_address, "inactive_from");
        assert_eq!(activation.amount, "0.00001");
        assert!(activation.tx_hash.is_none());
        assert!(activation.account_verified_at.is_none());
    }

    #[tokio::test]
    async fn account_activation_verified_requeues_resource_gate_without_release() {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();

        let collect_ctx = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db");
        let collect_pool = collect_ctx.into_transaction_db_pool().expect("transaction pool");
        let resource_ctx = SqliteContext::new(&dir_path, Some("api_resource.db"))
            .await
            .expect("init api_resource.db");
        let resource_pool = resource_ctx.into_api_resource_db_pool().expect("resource pool");

        let trade_no = "C_account_activation_recheck";
        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "inactive_from",
            "to",
            "1.12",
            "digest",
            "tron",
            None,
            "USDT",
            trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");
        sqlx::query(
            "UPDATE api_collect
             SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE trade_no = ?",
        )
        .bind(trade_no)
        .execute(collect_pool.as_ref())
        .await
        .expect("mark order ack sent");
        let req = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");

        ShadowCollectWorker::commit_account_activation_block_in_pools(
            &collect_pool,
            &resource_pool,
            trade_no,
            &req,
            "withdraw_payer",
            100,
            0,
            0.0,
        )
        .await
        .expect("account activation block commit");

        let activation_trade_no =
            ShadowCollectWorker::collect_account_activation_trade_no(trade_no);
        let rows = ApiCollectRepo::clear_account_activation_block_for_reeval(
            &collect_pool,
            trade_no,
            &activation_trade_no,
        )
        .await
        .expect("clear account activation block");
        assert_eq!(rows, 1);

        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect after activation verified");
        assert!(persisted.resource_gate_released_at.is_none());
        assert!(persisted.resource_gate_result.is_none());
        assert!(persisted.resource_block_reason.is_none());
        assert!(persisted.resource_dependency_trade_no.is_none());
        assert!(persisted.resource_dependency_type.is_none());
        assert!(
            ApiCollectRepo::scan_can_build(&collect_pool, 10)
                .await
                .expect("scan can build")
                .is_empty()
        );

        let need_resource_gate = ApiCollectRepo::scan_need_resource_gate(&collect_pool, 10)
            .await
            .expect("scan need resource gate");
        assert_eq!(need_resource_gate.len(), 1);
        assert_eq!(need_resource_gate[0].trade_no, trade_no);
    }

    #[tokio::test]
    async fn release_resource_gate_is_idempotent() {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();

        let collect_ctx = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db");
        let collect_pool = collect_ctx.into_transaction_db_pool().expect("transaction pool");

        let trade_no = "C_release_once";
        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "from",
            "to",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");

        ApiCollectRepo::mark_resource_released(
            &collect_pool,
            trade_no,
            ApiResourceGateResult::ResourceReady,
        )
        .await
        .expect("first release");
        ApiCollectRepo::mark_resource_released(
            &collect_pool,
            trade_no,
            ApiResourceGateResult::ResourceReady,
        )
        .await
        .expect("second release");

        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");
        assert!(persisted.resource_gate_released_at.is_some());
        assert_eq!(persisted.resource_gate_result, Some(ApiResourceGateResult::ResourceReady));
    }

    #[tokio::test]
    async fn stale_resource_ready_invalidation_reopens_resource_gate() {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();

        let collect_ctx = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db");
        let collect_pool = collect_ctx.into_transaction_db_pool().expect("transaction pool");

        let trade_no = "C_stale_resource_ready";
        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "from",
            "to",
            "1.12",
            "digest",
            "tron",
            Some("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string()),
            "USDT",
            trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");
        sqlx::query(
            "UPDATE api_collect
             SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                 building_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE trade_no = ?",
        )
        .bind(trade_no)
        .execute(collect_pool.as_ref())
        .await
        .expect("mark order ack and build slot");

        ApiCollectRepo::mark_resource_released_with_estimate(
            &collect_pool,
            trade_no,
            ApiResourceGateResult::ResourceReady,
            r#"{"energy":14650,"bandwidth":0}"#,
        )
        .await
        .expect("mark resource ready");

        assert_eq!(
            ApiCollectRepo::scan_can_build(&collect_pool, 10).await.expect("scan can build").len(),
            1
        );

        let rows = ApiCollectRepo::invalidate_resource_ready_for_reeval(&collect_pool, trade_no)
            .await
            .expect("invalidate resource ready");
        assert_eq!(rows, 1);

        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");
        assert!(persisted.resource_gate_released_at.is_none());
        assert!(persisted.resource_gate_result.is_none());
        assert!(persisted.resource_block_reason.is_none());
        assert!(persisted.resource_dependency_trade_no.is_none());
        assert!(persisted.resource_dependency_type.is_none());
        assert_eq!(persisted.resource_consume, "0");
        assert!(persisted.building_at.is_none());
        assert!(
            ApiCollectRepo::scan_can_build(&collect_pool, 10)
                .await
                .expect("scan can build")
                .is_empty()
        );

        let need_resource_gate = ApiCollectRepo::scan_need_resource_gate(&collect_pool, 10)
            .await
            .expect("scan need resource gate");
        assert_eq!(need_resource_gate.len(), 1);
        assert_eq!(need_resource_gate[0].trade_no, trade_no);
    }

    #[tokio::test]
    async fn tron_resource_ready_bandwidth_fee_shortage_opens_fee_cycle_without_reopening_gate() {
        let ctx = test_ctx().await;
        let collect_pool = ctx.api_transaction_pool().expect("transaction pool");
        let (intent_tx, _intent_rx) = mpsc::channel(1);
        let worker = ShadowCollectWorker::new(
            ctx,
            Arc::new(crate::infrastructure::api_trans::AddressLockManager::new()),
            Arc::new(crate::infrastructure::api_trans::collect::shadow::ShadowAdvancer::new(
                ctx, intent_tx, None,
            )),
        );
        let trade_no = format!("C_energy_ready_fee_{}", uuid::Uuid::new_v4().simple());
        let resource_estimate = r#"{"energy":14650,"bandwidth":345}"#;

        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "from",
            "to",
            "5",
            "digest",
            "tron",
            Some("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string()),
            "USDT",
            &trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");
        ApiCollectRepo::mark_resource_released_with_estimate(
            &collect_pool,
            &trade_no,
            ApiResourceGateResult::ResourceReady,
            resource_estimate,
        )
        .await
        .expect("mark resource ready");
        let released = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, &trade_no)
            .await
            .expect("load released collect");

        let rows = worker
            .invalidate_build_attempt_after_fee_check_failure(&released)
            .await
            .expect("open service fee cycle");
        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, &trade_no)
            .await
            .expect("load collect after fee shortage");

        assert_eq!(rows, 1);
        assert_eq!(persisted.need_service_fee, Some(true));
        assert!(persisted.resource_gate_released_at.is_some());
        assert_eq!(persisted.resource_gate_result, Some(ApiResourceGateResult::ResourceReady));
        assert_eq!(persisted.resource_consume, resource_estimate);
    }

    #[tokio::test]
    async fn local_delegation_block_commit_is_idempotent() {
        let ctx = test_ctx().await;
        let collect_pool = ctx.api_transaction_pool().expect("transaction pool");
        let resource_pool = ctx.api_resource_pool().expect("resource pool");
        let (intent_tx, mut intent_rx) = mpsc::channel(4);
        let worker = ShadowCollectWorker::new(
            ctx,
            Arc::new(crate::infrastructure::api_trans::AddressLockManager::new()),
            Arc::new(crate::infrastructure::api_trans::collect::shadow::ShadowAdvancer::new(
                ctx,
                intent_tx.clone(),
                None,
            )),
        );

        let trade_no = format!("C_local_block_once_{}", uuid::Uuid::new_v4().simple());
        let resource_trade_no = format!("rsc_local_delegate_{trade_no}");
        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "from",
            "to",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            &trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");
        let req = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, &trade_no)
            .await
            .expect("load collect");

        worker
            .commit_local_delegation_block(&trade_no, &req, "withdraw_owner", 100, 20, 10.0)
            .await
            .expect("first local block commit");
        worker
            .commit_local_delegation_block(&trade_no, &req, "withdraw_owner", 100, 20, 10.0)
            .await
            .expect("second local block commit");

        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, &trade_no)
            .await
            .expect("load collect");
        assert_eq!(
            persisted.resource_dependency_trade_no.as_deref(),
            Some(resource_trade_no.as_str())
        );
        assert_eq!(
            persisted.resource_dependency_type,
            Some(ApiResourceDependencyType::LocalDelegate)
        );
        assert_eq!(
            persisted.resource_block_reason,
            Some(ApiResourceBlockReason::NeedLocalDelegate)
        );

        let delegation =
            ApiResourceDelegationRepo::get_by_resource_trade_no(&resource_pool, &resource_trade_no)
                .await
                .expect("load delegation");
        assert_eq!(delegation.source, ApiResourceDelegationSource::Local);
        assert_eq!(delegation.origin_trade_no.as_deref(), Some(trade_no.as_str()));
        assert_eq!(delegation.owner_address, "withdraw_owner");
        assert_eq!(delegation.receiver_address, "from");
        assert_eq!(delegation.native_amount, "10");

        let intent = intent_rx.try_recv().expect("local delegation fast path should be dispatched");
        assert!(matches!(
            intent,
            CollectIntent::Chain(ChainIntent::ExecuteLocalResourceDelegation(ref dispatched))
                if dispatched == &resource_trade_no
        ));
        assert_eq!(delegation.amount, "100");
    }

    #[tokio::test]
    async fn local_delegation_failure_releases_collect_gate() {
        let ctx = test_ctx().await;
        let collect_pool = ctx.api_transaction_pool().expect("transaction pool");
        let resource_pool = ctx.api_resource_pool().expect("resource pool");
        let (intent_tx, _intent_rx) = mpsc::channel(1);
        let worker = ShadowCollectWorker::new(
            ctx,
            Arc::new(crate::infrastructure::api_trans::AddressLockManager::new()),
            Arc::new(crate::infrastructure::api_trans::collect::shadow::ShadowAdvancer::new(
                ctx,
                intent_tx.clone(),
                None,
            )),
        );

        let trade_no = "C_local_release";
        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "from",
            "to",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");
        ApiCollectRepo::mark_resource_blocked(
            &collect_pool,
            trade_no,
            ApiResourceBlockReason::NeedLocalDelegate,
            Some("rsc_local_delegate_C_local_release"),
            Some(ApiResourceDependencyType::LocalDelegate),
        )
        .await
        .expect("block collect");
        ApiResourceDelegationRepo::upsert(
            &resource_pool,
            NewApiResourceDelegation::local_delegate(
                "uid",
                "rsc_local_delegate_C_local_release",
                trade_no,
                2,
                "withdraw_owner",
                "from",
                "10",
                "10",
            ),
        )
        .await
        .expect("insert local delegation");
        ApiResourceDelegationRepo::mark_failed_with_result_if_unfinished(
            &resource_pool,
            "rsc_local_delegate_C_local_release",
            None,
            "ERR_6008",
            "local delegate failed",
            "local_delegation_terminal_failure",
        )
        .await
        .expect("mark failed");

        let failed = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &resource_pool,
            "rsc_local_delegate_C_local_release",
        )
        .await
        .expect("load failed delegation");
        assert!(failed.chain_failed_at.is_none());
        assert!(failed.finished_at.is_some());

        worker
            .release_collect_gate_after_local_delegation_failure(
                "rsc_local_delegate_C_local_release",
            )
            .await
            .expect("release collect after local failure");

        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");
        assert!(persisted.resource_gate_released_at.is_some());
        assert_eq!(
            persisted.resource_gate_result,
            Some(ApiResourceGateResult::LocalDelegationFailedBypass)
        );
    }

    #[tokio::test]
    async fn stale_local_delegation_failure_does_not_release_collect_gate() {
        let ctx = test_ctx().await;
        let collect_pool = ctx.api_transaction_pool().expect("transaction pool");
        let resource_pool = ctx.api_resource_pool().expect("resource pool");
        let (intent_tx, _intent_rx) = mpsc::channel(1);
        let worker = ShadowCollectWorker::new(
            ctx,
            Arc::new(crate::infrastructure::api_trans::AddressLockManager::new()),
            Arc::new(crate::infrastructure::api_trans::collect::shadow::ShadowAdvancer::new(
                ctx, intent_tx, None,
            )),
        );
        let test_id = uuid::Uuid::new_v4().simple().to_string();
        let trade_no = format!("C_local_stale_failure_{test_id}");
        let resource_trade_no = format!("rsc_local_delegate_{trade_no}");
        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "from",
            "to",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            &trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");
        ApiCollectRepo::mark_resource_blocked(
            &collect_pool,
            &trade_no,
            ApiResourceBlockReason::NeedLocalDelegate,
            Some(&resource_trade_no),
            Some(ApiResourceDependencyType::LocalDelegate),
        )
        .await
        .expect("block collect");
        ApiResourceDelegationRepo::upsert(
            &resource_pool,
            NewApiResourceDelegation::local_delegate(
                "uid",
                &resource_trade_no,
                &trade_no,
                2,
                "withdraw_owner",
                "from",
                "10",
                "10",
            ),
        )
        .await
        .expect("insert local delegation");
        assert_eq!(
            ApiResourceDelegationRepo::claim_build_slot(&resource_pool, &resource_trade_no)
                .await
                .expect("claim build"),
            1
        );
        assert_eq!(
            ApiResourceDelegationRepo::mark_built(
                &resource_pool,
                &resource_trade_no,
                "built_hash",
                "built_raw",
            )
            .await
            .expect("mark built"),
            1
        );

        let committed = worker
            .mark_local_resource_delegation_failed(
                &resource_trade_no,
                &ServiceError::Parameter("late local failure".to_string()),
            )
            .await
            .expect("handle stale failure");
        assert!(!committed);

        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, &trade_no)
            .await
            .expect("load collect");
        assert!(persisted.resource_gate_released_at.is_none());
        assert_eq!(
            persisted.resource_block_reason,
            Some(ApiResourceBlockReason::NeedLocalDelegate)
        );
    }

    #[tokio::test]
    async fn platform_delegation_failure_does_not_mark_backend_result_received() {
        let ctx = test_ctx().await;
        let collect_pool = ctx.api_transaction_pool().expect("transaction pool");
        let resource_pool = ctx.api_resource_pool().expect("resource pool");
        let trade_no =
            format!("rsc_platform_failed_{}", Utc::now().timestamp_nanos_opt().unwrap_or_default());
        let (intent_tx, _intent_rx) = mpsc::channel(1);
        let worker = ShadowCollectWorker::new(
            ctx,
            Arc::new(crate::infrastructure::api_trans::AddressLockManager::new()),
            Arc::new(crate::infrastructure::api_trans::collect::shadow::ShadowAdvancer::new(
                ctx, intent_tx, None,
            )),
        );
        ApiResourceDelegationRepo::upsert(
            &resource_pool,
            NewApiResourceDelegation::platform_delegate_task(
                "uid",
                &trade_no,
                ApiTradeType::Collect,
                ApiResourceDelegationOperationType::Delegate,
                "tron",
                "withdraw_owner",
                "from",
                ApiResourceType::Energy,
                "10",
                "10",
            ),
        )
        .await
        .expect("insert platform delegation");
        ApiResourceDelegationRepo::mark_failed_if_unfinished(
            &resource_pool,
            &trade_no,
            "ERR_6008",
            "platform delegate failed locally",
        )
        .await
        .expect("mark failed");

        worker
            .release_collect_gate_after_local_delegation_failure(&trade_no)
            .await
            .expect("ignore platform failure");

        let task = ApiResourceDelegationRepo::get_by_resource_trade_no(&resource_pool, &trade_no)
            .await
            .expect("load platform task");
        assert!(task.finished_at.is_some(), "a confirmed local failure is terminal");
        assert!(task.result_ack_sent_at.is_none());

        let rows = ApiResourceDelegationRepo::scan_need_result_ack(&resource_pool, 100)
            .await
            .expect("scan result ack");
        assert!(!rows.iter().any(|row| row.resource_trade_no == trade_no));
    }

    #[tokio::test]
    async fn platform_delegate_retryable_error_releases_build_slot() {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.path().to_string_lossy().to_string();

        let resource_ctx = SqliteContext::new(&dir_path, Some("api_resource.db"))
            .await
            .expect("init api_resource.db");
        let resource_pool = resource_ctx.into_api_resource_db_pool().expect("resource pool");

        ApiResourceDelegationRepo::upsert(
            &resource_pool,
            NewApiResourceDelegation::platform_delegate_task(
                "uid",
                "rsc_platform_delegate_retry",
                ApiTradeType::Collect,
                ApiResourceDelegationOperationType::Delegate,
                "tron",
                "withdraw_owner",
                "from",
                ApiResourceType::Energy,
                "10",
                "10",
            ),
        )
        .await
        .expect("insert platform delegation");
        ApiResourceDelegationRepo::mark_task_ack_sent(
            &resource_pool,
            "rsc_platform_delegate_retry",
        )
        .await
        .expect("mark ack");
        ApiResourceDelegationRepo::claim_build_slot(&resource_pool, "rsc_platform_delegate_retry")
            .await
            .expect("claim build slot");

        ShadowCollectWorker::schedule_local_resource_delegation_rebuild_retry_in_pool(
            &resource_pool,
            "rsc_platform_delegate_retry",
            &ServiceError::Parameter("retryable test error".to_string()),
        )
        .await
        .expect("schedule retry");

        let task = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &resource_pool,
            "rsc_platform_delegate_retry",
        )
        .await
        .expect("load platform task");
        assert!(task.building_at.is_none());
        assert_eq!(task.tx_hash, None);
    }

    #[tokio::test]
    async fn local_delegation_success_releases_collect_gate() {
        let ctx = test_ctx().await;
        let collect_pool = ctx.api_transaction_pool().expect("transaction pool");
        let resource_pool = ctx.api_resource_pool().expect("resource pool");
        let (intent_tx, _intent_rx) = mpsc::channel(1);
        let worker = ShadowCollectWorker::new(
            ctx,
            Arc::new(crate::infrastructure::api_trans::AddressLockManager::new()),
            Arc::new(crate::infrastructure::api_trans::collect::shadow::ShadowAdvancer::new(
                ctx,
                intent_tx.clone(),
                None,
            )),
        );

        let trade_no = "C_local_success";
        ApiCollectRepo::upsert_api_collect(
            &collect_pool,
            "uid",
            "collect",
            "from",
            "to",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");
        let input = NewApiResourceDelegation::local_delegate(
            "uid",
            "rsc_local_delegate_C_local_success",
            trade_no,
            2,
            "withdraw_owner",
            "from",
            "10",
            "10",
        );
        ApiResourceDelegationRepo::upsert(&resource_pool, input)
            .await
            .expect("insert local delegation");
        let delegation = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &resource_pool,
            "rsc_local_delegate_C_local_success",
        )
        .await
        .expect("load delegation");

        worker
            .release_collect_gate_after_local_delegation_success(&delegation)
            .await
            .expect("release collect after local success");

        let persisted = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, trade_no)
            .await
            .expect("load collect");
        assert!(persisted.resource_gate_released_at.is_some());
        assert_eq!(
            persisted.resource_gate_result,
            Some(ApiResourceGateResult::LocalDelegationSuccess)
        );
    }

    #[test]
    fn resource_shortfall_saturates_at_zero() {
        assert_eq!(ShadowCollectWorker::resource_shortfall(100, 40), 60);
        assert_eq!(ShadowCollectWorker::resource_shortfall(100, 120), 0);
        assert_eq!(ShadowCollectWorker::resource_shortfall(100, -1), 100);
    }

    #[test]
    fn collect_delegation_amount_uses_order_required_energy() {
        let required_energy = 14_650;
        let same_address_previous_order_energy = 14_649;

        assert_eq!(
            ShadowCollectWorker::resource_shortfall(
                required_energy,
                same_address_previous_order_energy
            ),
            1
        );
        assert_eq!(
            ShadowCollectWorker::collect_delegation_energy_amount(required_energy),
            required_energy
        );
        assert_eq!(ShadowCollectWorker::collect_delegation_energy_amount(0), 1);
    }

    #[test]
    fn collect_resource_ready_budget_subtracts_pending_same_address_orders() {
        let available_energy = 20_000;
        let pending = PendingCollectResourceBudget { energy: 14_650, bandwidth: 0 };

        let effective = ShadowCollectWorker::effective_resource_after_pending_collects(
            available_energy,
            600,
            pending,
        );

        assert_eq!(effective.energy, 5_350);
        assert_eq!(effective.bandwidth, 600);
        let decision = WithdrawResourceReservationManager::default().reserve_if_available(
            "T_PENDING_ENERGY",
            "C_PENDING_ENERGY",
            ShadowCollectWorker::tron_collect_required_budget(14_650, 345),
            effective,
        );
        assert!(matches!(decision, ResourceReservationDecision::NeedMore(_)));
    }

    #[test]
    fn platform_delegated_collect_fee_shortage_fails_instead_of_redelegating() {
        let mut req = base_collect();
        req.chain_code = "tron".to_string();
        req.token_addr = AssetTokenKey::Contract("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string());
        req.resource_gate_released_at = Some(Utc::now());
        req.resource_gate_result = Some(ApiResourceGateResult::PlatformDelegateSuccess);
        req.raw_tx = None;

        let err = ServiceError::Business(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ));

        assert!(
            ShadowCollectWorker::should_fail_tron_platform_delegated_collect_after_build_resource_shortage(
                &req,
                &err
            )
        );

        req.resource_gate_result = Some(ApiResourceGateResult::ResourceReady);
        assert!(ShadowCollectWorker::should_open_fee_cycle_for_tron_resource_ready(&req, &err));
        assert!(
            !ShadowCollectWorker::should_fail_tron_platform_delegated_collect_after_build_resource_shortage(
                &req,
                &err
            )
        );
    }

    #[test]
    fn sol_native_collect_keeps_sender_rent_bypass_scope() {
        assert!(ShadowCollectWorker::should_spend_all_native_collect(
            "sol",
            &AssetTokenKey::Native
        ));
        assert!(!ShadowCollectWorker::should_spend_all_native_collect(
            "sol",
            &AssetTokenKey::Contract("token".to_string())
        ));
    }

    #[test]
    fn sol_token_collect_fee_need_includes_sender_rent_reserve() {
        let fee = Decimal::from_str("0.000015").expect("fee");
        let rent_reserve = ShadowCollectWorker::sol_token_collect_sender_rent_reserve(
            "sol",
            &AssetTokenKey::Contract("token".to_string()),
        )
        .expect("rent reserve");
        let need = fee + rent_reserve;

        assert!(rent_reserve > Decimal::ZERO);
        assert_eq!(need - fee, rent_reserve);
    }

    #[test]
    fn sol_token_collect_exact_log_balance_is_still_short_after_fee_plus_rent() {
        let balance = Decimal::from_str("0.00094588").expect("balance");
        let fee = Decimal::from_str("0.000015").expect("fee");
        let rent_reserve = ShadowCollectWorker::sol_token_collect_sender_rent_reserve(
            "sol",
            &AssetTokenKey::Contract("token".to_string()),
        )
        .expect("rent reserve");
        let need = fee + rent_reserve;

        assert_eq!(need, Decimal::from_str("0.00100588").expect("need"));
        assert!(balance < need);
    }

    #[test]
    fn tron_token_collect_resource_gate_bypasses_native_fee_balance_only_after_release() {
        let mut req = base_collect();
        req.chain_code = "tron".to_string();
        req.token_addr = AssetTokenKey::Contract("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string());

        assert!(!ShadowCollectWorker::tron_token_collect_resource_gate_covers_native_fee(&req));

        req.resource_gate_released_at = Some(Utc::now());
        req.resource_gate_result = Some(ApiResourceGateResult::PlatformDelegateSuccess);
        assert!(!ShadowCollectWorker::tron_token_collect_resource_gate_covers_native_fee(&req));

        req.resource_gate_result = Some(ApiResourceGateResult::FallbackAllowed);
        assert!(!ShadowCollectWorker::tron_token_collect_resource_gate_covers_native_fee(&req));

        req.resource_gate_result = Some(ApiResourceGateResult::ResourceDelegationFailedBypass);
        assert!(!ShadowCollectWorker::tron_token_collect_resource_gate_covers_native_fee(&req));

        req.resource_gate_result = Some(ApiResourceGateResult::LocalDelegationFailedBypass);
        assert!(!ShadowCollectWorker::tron_token_collect_resource_gate_covers_native_fee(&req));

        req.resource_gate_result = Some(ApiResourceGateResult::ResourceReady);
        assert!(!ShadowCollectWorker::tron_token_collect_resource_gate_covers_native_fee(&req));

        req.resource_gate_result = Some(ApiResourceGateResult::LocalDelegationSuccess);
        assert!(!ShadowCollectWorker::tron_token_collect_resource_gate_covers_native_fee(&req));

        req.resource_gate_result = Some(ApiResourceGateResult::ResourceDelegationSuccess);
        assert!(ShadowCollectWorker::tron_token_collect_resource_gate_covers_native_fee(&req));

        req.token_addr = AssetTokenKey::Native;
        assert!(!ShadowCollectWorker::tron_token_collect_resource_gate_covers_native_fee(&req));

        req.token_addr =
            AssetTokenKey::Contract("So11111111111111111111111111111111111111112".to_string());
        req.chain_code = "sol".to_string();
        assert!(!ShadowCollectWorker::tron_token_collect_resource_gate_covers_native_fee(&req));
    }

    #[test]
    fn sol_token_collect_rent_shortage_reopens_fee_cycle() {
        use crate::error::{
            business::{
                BusinessError,
                chain::{ChainError, InsufficientBalanceDetail},
            },
            service::ServiceError,
        };

        let req = base_collect();
        let err = ServiceError::Business(BusinessError::Chain(ChainError::InsufficientBalance(
            InsufficientBalanceDetail::new()
                .reason("sender balance must keep rent-exempt reserve after transfer"),
        )));

        assert!(ShadowCollectWorker::should_reopen_fee_cycle_for_solana_token_collect(&req, &err));
    }

    #[test]
    fn native_sol_collect_does_not_reopen_token_rent_fee_cycle() {
        use crate::error::{
            business::{
                BusinessError,
                chain::{ChainError, InsufficientBalanceDetail},
            },
            service::ServiceError,
        };

        let mut req = base_collect();
        req.token_addr = AssetTokenKey::Native;
        let err = ServiceError::Business(BusinessError::Chain(ChainError::InsufficientBalance(
            InsufficientBalanceDetail::new()
                .reason("sender balance must keep rent-exempt reserve after transfer"),
        )));

        assert!(!ShadowCollectWorker::should_reopen_fee_cycle_for_solana_token_collect(&req, &err));
    }

    #[test]
    fn sol_token_collect_rent_shortage_after_fee_cycle_terminates() {
        use crate::error::{
            business::{
                BusinessError,
                chain::{ChainError, InsufficientBalanceDetail},
            },
            service::ServiceError,
        };

        let mut req = base_collect();
        req.service_fee_uploaded_at = Some(Utc::now());
        req.tx_fee_res_ack_sent_at = Some(Utc::now());
        let err = ServiceError::Business(BusinessError::Chain(ChainError::InsufficientBalance(
            InsufficientBalanceDetail::new()
                .reason("sender balance must keep rent-exempt reserve after transfer"),
        )));

        assert!(ShadowCollectWorker::should_terminal_fail_solana_token_collect_rent_shortage(
            &req, &err
        ));
    }

    #[test]
    fn sol_token_collect_fee_shortage_after_fee_cycle_terminates() {
        let mut req = base_collect();
        req.service_fee_uploaded_at = Some(Utc::now());
        req.tx_fee_res_ack_sent_at = Some(Utc::now());

        assert!(ShadowCollectWorker::should_terminal_fail_fee_shortage_after_completed_fee_cycle(
            &req
        ));
    }

    #[test]
    fn evm_token_collect_fee_shortage_after_fee_cycle_terminates() {
        let mut req = base_collect();
        req.chain_code = "eth".to_string();
        req.service_fee_uploaded_at = Some(Utc::now());
        req.tx_fee_res_ack_sent_at = Some(Utc::now() - chrono::Duration::seconds(180));

        assert!(ShadowCollectWorker::should_terminal_fail_fee_shortage_after_completed_fee_cycle(
            &req
        ));
    }

    #[test]
    fn evm_native_collect_fee_shortage_after_fee_cycle_terminates() {
        let mut req = base_collect();
        req.chain_code = "eth".to_string();
        req.token_addr = AssetTokenKey::Native;
        req.service_fee_uploaded_at = Some(Utc::now());
        req.tx_fee_res_ack_sent_at = Some(Utc::now() - chrono::Duration::seconds(180));

        assert!(ShadowCollectWorker::should_terminal_fail_fee_shortage_after_completed_fee_cycle(
            &req
        ));
    }

    #[test]
    fn evm_fee_reestimate_deadline_uses_original_ack_time() {
        let mut req = base_collect();
        req.chain_code = "arbitrum".into();
        let ack = Utc::now();
        req.service_fee_uploaded_at = Some(ack);
        req.tx_fee_res_ack_sent_at = Some(ack);
        assert!(!ShadowCollectWorker::should_terminal_fail_fee_shortage_at(&req, ack));
        assert!(!ShadowCollectWorker::should_terminal_fail_fee_shortage_at(
            &req,
            ack + chrono::Duration::seconds(179)
        ));
        assert!(ShadowCollectWorker::should_terminal_fail_fee_shortage_at(
            &req,
            ack + chrono::Duration::seconds(180)
        ));
        req.tx_fee_res_ack_sent_at = None;
        assert!(!ShadowCollectWorker::should_terminal_fail_fee_shortage_at(
            &req,
            ack + chrono::Duration::seconds(181)
        ));
    }

    #[test]
    fn service_fee_report_without_fee_order_ack_is_not_completed() {
        let mut req = base_collect();
        req.service_fee_uploaded_at = Some(Utc::now());
        req.tx_fee_res_ack_sent_at = None;

        assert!(!ShadowCollectWorker::service_fee_cycle_completed(&req));

        req.tx_fee_res_ack_sent_at = Some(Utc::now());
        assert!(ShadowCollectWorker::service_fee_cycle_completed(&req));
    }

    #[test]
    fn tron_token_collect_fee_report_does_not_complete_fee_cycle() {
        let mut req = base_collect();
        req.chain_code = "tron".to_string();
        req.token_addr = AssetTokenKey::Contract("token".to_string());
        req.service_fee_uploaded_at = Some(Utc::now());

        assert!(!ShadowCollectWorker::should_terminal_fail_fee_shortage_after_completed_fee_cycle(
            &req
        ));
    }

    #[test]
    fn tron_token_collect_fee_shortage_after_fee_cycle_terminates() {
        let mut req = base_collect();
        req.chain_code = "tron".to_string();
        req.token_addr = AssetTokenKey::Contract("token".to_string());
        req.service_fee_uploaded_at = Some(Utc::now());
        req.tx_fee_res_ack_sent_at = Some(Utc::now());

        assert!(ShadowCollectWorker::should_terminal_fail_fee_shortage_after_completed_fee_cycle(
            &req
        ));
    }

    #[test]
    fn tron_native_collect_fee_shortage_after_fee_cycle_terminates() {
        let mut req = base_collect();
        req.chain_code = "tron".to_string();
        req.token_addr = AssetTokenKey::Native;
        req.service_fee_uploaded_at = Some(Utc::now());
        req.tx_fee_res_ack_sent_at = Some(Utc::now());

        assert!(ShadowCollectWorker::should_terminal_fail_fee_shortage_after_completed_fee_cycle(
            &req
        ));
    }

    #[test]
    fn sol_rent_exempt_reserve_balance_error_reopens_fee_cycle() {
        use crate::error::{
            business::{
                BusinessError,
                chain::{ChainError, InsufficientBalanceDetail},
            },
            service::ServiceError,
        };

        let req = base_collect();
        let err = ServiceError::Business(BusinessError::Chain(ChainError::InsufficientBalance(
            InsufficientBalanceDetail::new()
                .reason("sender balance must keep rent-exempt reserve after transfer"),
        )));

        assert!(ShadowCollectWorker::is_solana_rent_exempt_reserve_balance_error(&req, &err));
    }

    #[test]
    fn non_sol_chain_does_not_reopen_fee_cycle_for_rent_reserve_error() {
        use crate::error::{
            business::{
                BusinessError,
                chain::{ChainError, InsufficientBalanceDetail},
            },
            service::ServiceError,
        };

        let mut req = base_collect();
        req.chain_code = "eth".to_string();
        let err = ServiceError::Business(BusinessError::Chain(ChainError::InsufficientBalance(
            InsufficientBalanceDetail::new()
                .reason("sender balance must keep rent-exempt reserve after transfer"),
        )));

        assert!(!ShadowCollectWorker::is_solana_rent_exempt_reserve_balance_error(&req, &err));
    }

    #[test]
    fn broadcast_uncertain_timeout_helper_trips_after_five_minutes() {
        let mut req = base_collect();
        req.broadcast_uncertain_since_at = Some(Utc::now() - chrono::TimeDelta::minutes(5));

        assert!(ShadowCollectWorker::should_auto_fail_broadcast_uncertain(&req, Utc::now()));
    }

    #[test]
    fn evm_uncertain_future_nonce_preserves_original_transaction_identity() {
        assert_eq!(
            ShadowCollectWorker::evm_uncertain_nonce_action(28, 29, true),
            EvmUncertainNonceAction::PreserveFutureNonce
        );
    }

    #[test]
    fn evm_uncertain_next_nonce_rebroadcasts_identical_raw_once() {
        assert_eq!(
            ShadowCollectWorker::evm_uncertain_nonce_action(28, 28, true),
            EvmUncertainNonceAction::RebroadcastSameRaw
        );
        assert_eq!(
            ShadowCollectWorker::evm_uncertain_nonce_action(28, 28, false),
            EvmUncertainNonceAction::PreserveConsumedNonce
        );
    }

    #[test]
    fn evm_uncertain_consumed_nonce_is_not_rebroadcast() {
        assert_eq!(
            ShadowCollectWorker::evm_uncertain_nonce_action(30, 29, true),
            EvmUncertainNonceAction::PreserveConsumedNonce
        );
    }
}
#[test]
fn local_delegation_post_broadcast_confirm_delays_cover_short_window() {
    assert_eq!(
        ShadowCollectWorker::LOCAL_DELEGATION_POST_BROADCAST_CONFIRM_DELAYS_SECS,
        [3, 8, 15]
    );
}

#[cfg(test)]
#[path = "collect_worker/receipt_observation_tests.rs"]
mod receipt_observation_tests;

#[cfg(test)]
#[path = "collect_worker/routing_tests.rs"]
mod routing_tests;
