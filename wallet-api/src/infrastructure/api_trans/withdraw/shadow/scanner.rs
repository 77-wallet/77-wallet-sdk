// withdraw/shadow/scanner.rs
//
// ============================================================================
// SHADOW SCANNER ARCHITECTURE LOCK
//
// 本文件实现 Withdraw Shadow 推进逻辑。
// Scanner 只负责：
//
// ✔ 读取不可逆事实
// ✔ 判断是否可推进
// ✔ 生成推进意图
//
// Scanner 永远不：
// ✘ 写 status
// ✘ 生成事实
// ✘ 执行副作用
//
// 本文件修改前必须理解：
// Worker 写事实顺序 = 系统真实状态机
// ============================================================================
// ============================================================================
// Scanner 设计铁律（Final · 不可违背）
// ============================================================================
//
// 核心定位：
// Scanner = 事实读取器 + 推进意图生成器
// Scanner 只"读事实 → 决定能否推进"，绝不创造事实
//
// ----------------------------------------------------------------------------
// 1. Scanner 只允许读取【不可逆事实字段】
// ----------------------------------------------------------------------------
//
// Scanner 禁止读取：
// - 行为中间态（重试计数 / timeout 等）
// - 推断性状态（waiting / paused / blocked）
// - 任意"时间先后关系"
//
// Scanner 允许读取的事实分三类：
//
// 1.1 【链上结果事实】（不可逆）
//     - transaction_time
//     - tx_hash / fee / resource（若存在）
//
//     特性：
//     - 只写一次
//     - 一旦存在，结果已确定（成功或失败）
//
// 1.2 【不可逆历史事实】
//     - ever_needed_service_fee
//
//     必须满足：
//     - 单向变化（false → true）
//     - 永不回滚、不可 Recover 修复
//     - 表达"历史上是否发生过某个否定性事实"
//
//     用途：
//     - 作为后续阶段的 gating 条件
//     - ❌ 不允许用于推断时间先后
//
// 1.3 【终止型错误事实】
//     - err_code
//
//     必须满足：
//     - 单向变化（NULL → NOT NULL）
//     - 永不回滚、不可 Recover 修复
//     - 表达"是否发生过一次不可逆执行失败"
//
// ----------------------------------------------------------------------------
// 1.z 铁律 D：err_code = 失败冻结闸
// ----------------------------------------------------------------------------
//
// err_code 表示一次不可逆的执行失败，记录进入【失败冻结态】
//
// 一旦 err_code IS NOT NULL：
//
// - Scanner 不再产生任何【执行型或结果型】推进意图
// - 不再触发任何执行型或补偿型操作
// - 不再进行 retry / recover / 结果型 ack / 结果型 upload
//
// 唯一允许的行为：
// - UploadTxExecReceipt（属于【行为事实补齐副作用】，不属于推进）
//
// 唯一允许的状态变更：
// - 由统一收口流程写入 finished_at
//
// Scanner 的职责到此结束

// ----------------------------------------------------------------------------
// 1.z.1 副作用分类与 err_code 冻结范围
// ----------------------------------------------------------------------------
//
// Scanner 生成的副作用分为两类：
//
// 1. 【执行型或结果型推进意图】（err_code 下冻结）
//    - BuildTx / BroadcastTx
//    - SendTxAck / SendTxResAck
//
// 2. 【行为事实补齐副作用】（err_code 下允许）
//    - UploadTxExecReceipt
//
// 说明：
// - UploadTxExecReceipt 用于补齐"已发起链上执行"的事实
// - 无论成功失败都需要执行，确保行为事实完整性
// - 不属于"推进"，属于"事实补齐"

// ----------------------------------------------------------------------------
// 1.z.2 err_code ≠ 可恢复失败
// ----------------------------------------------------------------------------
//
// err_code ≠ need_service_fee
//
// - need_service_fee：构建失败（可恢复）
// - err_code：执行失败（不可恢复）
//
// 两者语义严格区分，禁止互相推断

// ----------------------------------------------------------------------------
// 1.z.3 为什么 err_code 下不再执行结果型操作
// ----------------------------------------------------------------------------
//
// err_code 下不再上传 receipt / ack 的原因：
// - 上游已通过 err_code 感知失败
// - 重复副作用可能造成幂等混乱
// - 失败事实一旦成立，只允许"终态收口"，不再补过程
//
// ----------------------------------------------------------------------------
// 2. Scanner 不使用时间做决策
// ----------------------------------------------------------------------------
//
// - 禁止使用：
//   - now - xxx > duration
//   - xxx_at < yyy_at
//
// - 时间字段唯一用途：
//   - 作为"该事实是否已发生"的布尔信号
//     （IS NULL / IS NOT NULL）
//
// ----------------------------------------------------------------------------
// 3. Scanner 不判断"该不该做"，只判断"事实是否已满足"
// ----------------------------------------------------------------------------
//
// - Scanner 不包含业务意图
// - Scanner 不做价值判断
// - Scanner 只回答一个问题：
//   👉「在当前事实快照下，是否允许推进某一步？」
//
// ----------------------------------------------------------------------------
// 4. Scanner 的唯一职责
// ----------------------------------------------------------------------------
//
// 事实快照（ApiWithdrawEntity）
//        ↓
// 生成 WithdrawIntent
//
// - Scanner 不写 DB
// - Scanner 不发请求
// - Scanner 不修改事实
//
// ----------------------------------------------------------------------------
// 5. Scanner 方法命名铁律
// ----------------------------------------------------------------------------
//
// - 方法名必须是【事实条件的直接翻译】
// - 禁止使用：
//   - done / finished / completed / success / failed
//
// 正确示例：
// - can_build
// - can_broadcast
// - need_recover
//
// 错误示例：
// - is_build_done
// - should_broadcast
// - is_tx_success
//
// ----------------------------------------------------------------------------
// 6. Scanner 只处理两类记录
// ----------------------------------------------------------------------------
//
// - 能推进的记录
// - 已终止（finished_at IS NOT NULL）的记录
//
// ❌ 不存在第三态：
// - "再等等"
// - "观察中"
// - "可能会好"
//
// ============================================================================
// Build Failure 铁律补充
// ============================================================================
//
// - 不存在 blocked / paused / waiting build 状态
// - need_service_fee = 构建失败的最终事实（可恢复）
// - need_service_fee = true ⇒ 构建失败，禁止推进
// - ever_needed_service_fee 只记录"历史上失败过"
// - 清除 need_service_fee ≠ 抹除失败历史
//
// Scanner 只处理：
// - 可推进的记录
// - 或已终态记录
//
// ============================================================================
// ⚠️ 本注释为唯一权威模型定义
// 若模型演进，必须先更新本注释，再允许改代码
// ============================================================================

/// ============================================================================
/// Scanner Recover Rule ===
/// ============================================================================
///
/// Recover is a Scanner-level advancement rule, not a Worker heuristic.
///
/// Predicate:
/// - tx_hash IS NOT NULL
/// - transaction_time IS NULL
/// - finished_at IS NULL
/// - err_code IS NULL
///
/// Semantics:
/// - Indicates that on-chain final result is still missing from local facts
/// - Broadcast visibility alone does not close the recover loop
/// - Scanner MUST emit Recover intent.
/// - Scanner MUST NOT:
///   - query chain
///   - infer success / failure
///   - depend on timing fields
///
/// Properties:
/// - Monotonic: once transaction_time is filled, predicate becomes false forever
/// - Idempotent: emitting Recover multiple times is allowed
/// - Safety-net: guarantees eventual fact completion after crash / restart
/// ============================================================================
/// ============================================================================
/// Withdraw Audit Gate Rule ===
/// ============================================================================
///
/// Withdraw execution must go through manual audit.
///
/// Therefore:
///
/// BuildTx predicate must include:
/// - tx_ack_sent_at IS NOT NULL
/// - audit_passed_at IS NOT NULL
///
/// audit_passed_at is an EXECUTION PERMISSION FACT,
/// serving as a hard gate in the advancement chain.
///
/// Scanner MUST NOT:
/// - automatically bypass audit
/// - infer audit status through status field
/// - modify the strong order chain
/// ============================================================================
///
/// Withdraw Resource Gate Rule ===
/// ============================================================================
///
/// TRON withdraw build must also pass resource gate before BuildTx.
///
/// `EvalResourceGate` is the operation step.
/// `resource_ready` / `need_platform_delegate` are persisted result facts.
/// BuildTx may only proceed after `resource_gate_released_at`.
/// ============================================================================
use std::fmt;
use std::time::{Duration, Instant};

use tracing::{error, trace, warn};
use wallet_database::{
    ApiTransactionDbPool,
    entities::{api_trade_type::ApiTradeType, api_withdraw::ApiWithdrawEntity},
};

use super::{WithdrawChainIntent, WithdrawIntent, WithdrawSideEffectIntent};

use super::{
    predicate::evaluate_point,
    stage::{ADVANCEMENT_ORDER, AdvancementPoint},
};
use crate::{
    domain::api_wallet::trans::shared_mechanism,
    error::service::ServiceError,
    infrastructure::api_trans::{
        fast_path::{FastPathFlow, FastPathReason, FastPathTarget, dispatch_fast_path},
        shadow_rpc_policy,
        withdraw::diagnose::{
            DiagnoseEvent, DiagnoseEventSender, DiagnoseMeta, DiagnoseSource, DiagnoseStage,
            maybe_log_stuck,
        },
    },
};

/// ============================================================================
///                            共用 Predicate 函数
/// ============================================================================
///
/// 注意：所有 predicate 函数必须是纯函数，不得：
/// - 写 DB
/// - 发请求
/// - 依赖时间
/// - 依赖外部状态
/// ============================================================================

/// 链推进类（Chain Progress）predicate
/// ----------------------------------------------------------------------------

// ============================================================================
// STRONG ORDER GATE — BuildTx 不可逆事实屏障
// ============================================================================
//
// BuildTx 只能在以下事实全部满足时发生：
//
// [FACT REQUIRED]
// ✔ tx_ack_sent_at IS NOT NULL   — 后端确认已发送
// ✔ audit_passed_at IS NOT NULL  — 审计通过（强顺序屏障）
//
// [FACT MUST NOT EXIST]
// ✘ raw_tx IS NOT NULL           — 防止重复构建
// ✘ finished_at IS NOT NULL      — 已终态
// ✘ err_code IS NOT NULL         — 终止错误
//
// ⚠️ DO NOT REMOVE ANY CONDITION
// ⚠️ Scanner 与 DAO predicate 必须完全一致
// ============================================================================
// ============================================================================
// ⚠️ Scanner / DAO Predicate Symmetry Rule
//
// 本方法 predicate 必须与：
// ApiWithdrawRepo::scan_can_build 完全一致
//
// 修改任一侧时必须同步修改另一侧，否则会导致：
// - Phantom Task
// - Double Build
// - 永久卡死
//
// Scanner 是安全网，不是事实来源
// ============================================================================
fn can_build(withdraw: &ApiWithdrawEntity) -> bool {
    withdraw.tx_ack_sent_at.is_some()          // 顺序门 1: TxAck
        && withdraw.audit_passed_at.is_some()   // 顺序门 2: AuditPass
        && (!crate::domain::api_wallet::trans::shared_mechanism::is_tron_chain(
            &withdraw.chain_code,
        ) || withdraw.resource_gate_released_at.is_some())
        && withdraw.raw_tx.is_none()           // 执行条件: 未构建
        && withdraw.finished_at.is_none()       // 终止排除: 未结束
        && withdraw.err_code.is_none() // 终止排除: 无错误
}

fn need_fee_estimate_snapshot(withdraw: &ApiWithdrawEntity) -> bool {
    withdraw.trade_type == ApiTradeType::Withdraw
        && withdraw.fee_estimated_at.is_none()
        && withdraw.raw_tx.is_none()
        && withdraw.transaction_time.is_none()
        && withdraw.finished_at.is_none()
        && withdraw.err_code.is_none()
}

/// 检查是否可以广播交易
///
/// 事实条件：
/// - raw_tx IS NOT empty
/// - err_code IS NULL
fn can_broadcast(withdraw: &ApiWithdrawEntity) -> bool {
    withdraw.raw_tx.is_some()
        && withdraw.err_code.is_none()
        && withdraw.last_broadcast_at.is_none()
        && withdraw.finished_at.is_none()
}

/// 副作用类（Side Effect）predicate
/// ----------------------------------------------------------------------------

/// 检查是否需要发送交易 ACK
///
/// 事实条件：
/// - tx_ack_sent_at IS NULL (交易 ACK 事实缺失)
/// - finished_at IS NULL (未完成)
/// - err_code IS NULL (无失败事实)
///
/// ⚠️ 重要说明：
/// - TxAck 属于【行为事实补齐副作用】
/// - 确保交易已被可靠接收
/// - 符合 Scanner 铁律：只基于不可逆事实做判断
/// - 不依赖时间或行为推断
fn need_tx_ack(withdraw: &ApiWithdrawEntity) -> bool {
    withdraw.tx_ack_sent_at.is_none()
        && withdraw.finished_at.is_none()
        && withdraw.err_code.is_none()
}

/// 检查是否需要上传交易执行回执
///
/// 事实条件：
/// - tx_exec_receipt_uploaded_at IS NULL (事实未补齐)
/// - finished_at IS NULL (未完成)
/// - chain_success_at / transaction_time / chain_failed_at / err_code 之一已存在
///
/// ⚠️ 重要说明：
/// - UploadTxExecReceipt 属于【行为事实补齐副作用】
/// - 只在链上结果或失败事实已确认后执行
/// - 广播可见但结果未确认时，不应上报最终结果
/// - 符合 Scanner 铁律：只基于不可逆事实做判断
/// - 不依赖时间或行为推断
/// ⚠️ 特例说明：
/// - 本 predicate 在 err_code != NULL 时仍然允许
/// - 因为 UploadTxExecReceipt 属于【行为事实补齐副作用】
/// - 不属于推进，不受 err_code 冻结
/// - 即使没有 tx_hash（未广播），如果发生错误也需要上传回执
fn need_tx_exec_receipt_upload(withdraw: &ApiWithdrawEntity) -> bool {
    withdraw.tx_exec_receipt_uploaded_at.is_none()
        && withdraw.finished_at.is_none()
        && (withdraw.chain_success_at.is_some()
            || withdraw.transaction_time.is_some()
            || withdraw.chain_failed_at.is_some()
            || withdraw.err_code.is_some())
}

/// 检查是否需要发送结果 ACK
///
/// 事实条件：
/// - transaction_time IS NOT NULL (链上结果事实已确认)
/// - tx_res_ack_sent_at IS NULL (结果 ACK 事实缺失)
/// - err_code IS NULL (无失败事实)
///
/// ⚠️ 重要说明：
/// - TxResAck 仅用于"成功结果确认"
/// - 失败结果通过 err_code 事实本身表达，不再发送 TxResAck
/// - 符合 Scanner 铁律：只基于不可逆事实做判断
/// - 不依赖时间或行为推断
fn need_tx_res_ack(withdraw: &ApiWithdrawEntity) -> bool {
    withdraw.tx_res_received_at.is_some()
        && withdraw.transaction_time.is_some()
        && withdraw.tx_res_ack_sent_at.is_none()
        && withdraw.finished_at.is_none()
        && withdraw.err_code.is_none()
}

/// 检查是否需要恢复交易
///
/// 事实条件：
/// - tx_hash IS NOT empty (链执行行为已发生)
/// - transaction_time IS NULL (链上结果事实缺失)
/// - err_code IS NULL (无失败事实)
///
/// ⚠️ 重要说明：
/// - Recover 的目的是补全链上结果事实
/// - 只看不可逆事实是否缺失，不做时间推断
/// - 符合 Scanner 铁律：只基于不可逆事实做判断
fn need_recover(withdraw: &ApiWithdrawEntity) -> bool {
    let needs_resource_recovery = shared_mechanism::tron_withdraw_resource_needs_recovery(
        &withdraw.chain_code,
        Some(withdraw.token_addr.as_db_str()),
        &withdraw.resource_consume,
    );
    withdraw.tx_hash.is_some()
        && (withdraw.transaction_time.is_none() || needs_resource_recovery)
        && (withdraw.finished_at.is_none() || needs_resource_recovery)
        && withdraw.err_code.is_none()
}

fn withdraw_recover_dispatch_key(withdraw: &ApiWithdrawEntity) -> String {
    let is_resource_recovery = withdraw.transaction_time.is_some()
        && shared_mechanism::tron_withdraw_resource_needs_recovery(
            &withdraw.chain_code,
            Some(withdraw.token_addr.as_db_str()),
            &withdraw.resource_consume,
        );

    if is_resource_recovery {
        shadow_rpc_policy::resource_recover_dispatch_key(&withdraw.trade_no)
    } else {
        shadow_rpc_policy::recover_dispatch_key(&withdraw.trade_no)
    }
}

fn allow_withdraw_recover_dispatch(withdraw: &ApiWithdrawEntity, recover_key: &str) -> bool {
    if withdraw.transaction_time.is_none() {
        shadow_rpc_policy::allow_broadcast_recover_dispatch(recover_key, withdraw.last_broadcast_at)
    } else {
        shadow_rpc_policy::allow_recover_dispatch(recover_key)
    }
}

/// Shadow Scanner 配置
#[derive(Debug, Clone)]
pub struct ScannerConfig {
    /// 扫描间隔
    pub scan_interval: Duration,
    /// 每轮最大处理数量
    pub max_items_per_scan: usize,
    /// 每轮最大手续费估算链 RPC 意图数量
    pub max_fee_estimate_per_scan: usize,
    /// 每轮最大资源门控链 RPC 意图数量
    pub max_resource_gate_per_scan: usize,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        let scan_interval_secs =
            shadow_rpc_policy::read_u64_env("WITHDRAW_SHADOW_SCAN_INTERVAL_SECS", 5, 3, 120);
        let max_items_per_scan =
            shadow_rpc_policy::read_usize_env("WITHDRAW_SHADOW_MAX_ITEMS_PER_SCAN", 80, 20, 200);
        let max_fee_estimate_per_scan = shadow_rpc_policy::read_usize_env(
            "WITHDRAW_SHADOW_MAX_FEE_ESTIMATE_PER_SCAN",
            6,
            1,
            50,
        );
        let max_resource_gate_per_scan = shadow_rpc_policy::read_usize_env(
            "WITHDRAW_SHADOW_MAX_RESOURCE_GATE_PER_SCAN",
            50,
            1,
            50,
        );
        Self {
            scan_interval: Duration::from_secs(scan_interval_secs),
            max_items_per_scan,
            max_fee_estimate_per_scan,
            max_resource_gate_per_scan,
        }
    }
}

/// Shadow Scanner
///
///
/// 只生成推进意图，不直接执行状态推进
#[derive(Clone)]
pub struct ShadowScanner {
    ctx: &'static crate::context::Context,
    /// Scanner配置
    pub config: ScannerConfig,
    intent_tx: tokio::sync::mpsc::Sender<WithdrawIntent>,
    diagnose_tx: Option<DiagnoseEventSender>,
}

impl fmt::Debug for ShadowScanner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShadowScanner").finish()
    }
}

impl ShadowScanner {
    pub fn new(
        ctx: &'static crate::context::Context,
        config: ScannerConfig,
        intent_tx: tokio::sync::mpsc::Sender<WithdrawIntent>,
        diagnose_tx: Option<DiagnoseEventSender>,
    ) -> Self {
        Self { ctx, config, intent_tx, diagnose_tx }
    }

    /// 执行一轮扫描
    pub async fn scan_round(&self) {
        let start = Instant::now();
        trace!("Starting withdraw shadow scan round");

        // Resource result ACK is the backend-visible closure for a platform
        // delegation result. Prefer it before main-chain stages so a resource
        // result cannot be followed by BuildTx/Broadcast before TX_RSC_RES ACK.
        for (stage, result) in [
            ("need_resource_result_ack", self.scan_need_resource_result_ack().await),
            ("need_fee_estimate", self.scan_need_fee_estimate().await),
            ("need_tx_ack", self.scan_need_tx_ack().await),
            ("need_resource_gate", self.scan_need_resource_gate().await),
            ("need_platform_delegate_apply", self.scan_need_platform_delegate_apply().await),
            ("can_build", self.scan_can_build().await),
            ("can_broadcast", self.scan_can_broadcast().await),
            ("need_recover", self.scan_need_recover().await),
            ("need_tx_exec_receipt_upload", self.scan_need_tx_exec_receipt_upload().await),
            ("confirmed_need_tx_res_ack", self.scan_confirmed_need_tx_res_ack().await),
        ] {
            if let Err(error) = result {
                error!(stage, %error, "Withdraw shadow scan stage failed");
            }
        }

        // Backend owns platform delegation execution. Withdraw shadow only
        // owns the merchant original-order resource result ACK.

        trace!("Withdraw shadow scan round completed in {:?}", start.elapsed());
    }

    /// 扫描需要写入手续费预估快照的提币。
    ///
    /// 事实条件：
    /// - TRON 普通提币
    /// - fee_estimated_at IS NULL
    /// - 未构建、未上链、未终止、无错误
    ///
    /// 该 intent 只补审计展示快照，不推进 BuildTx/ResourceGate。
    async fn scan_need_fee_estimate(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        trace!(max_items = %self.config.max_fee_estimate_per_scan, "Scanning withdraw fee estimate records");

        let records = wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::scan_need_fee_estimate(
            &pool,
            self.config.max_fee_estimate_per_scan,
        ).await?;

        let mut skipped = 0usize;
        let mut first_skip: Option<(String, std::time::Duration)> = None;
        for record in records {
            if let Some((host, remaining)) =
                crate::infrastructure::chain_rpc_guard::breaker_open_for_chain_code_with_ctx(
                    self.ctx,
                    &record.chain_code,
                )
                .await
            {
                skipped += 1;
                if first_skip.is_none() {
                    first_skip = Some((host, remaining));
                }
                continue;
            }
            let intent = WithdrawIntent::Chain(WithdrawChainIntent::EstimateFee(record.trade_no));
            self.dispatch_intent(intent);
        }

        if skipped > 0 {
            if let Some((host, remaining)) = first_skip {
                warn!(
                    skipped = skipped,
                    host = %host,
                    remaining = ?remaining,
                    "chain rpc circuit breaker open; skipped withdraw fee estimate intents"
                );
            }
        }

        Ok(())
    }

    /// 扫描需要发送交易 ACK 的交易
    ///
    /// 事实条件：
    /// - tx_ack_sent_at IS NULL
    /// - finished_at IS NULL
    /// - err_code IS NULL
    ///
    /// SQL must be equivalent to need_tx_ack()
    async fn scan_need_tx_ack(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        trace!(max_items = %self.config.max_items_per_scan, "Scanning need tx ack records");

        // 查询DB中需要发送交易 ACK 的记录
        let records =
            wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::scan_need_tx_ack(
                &pool,
                self.config.max_items_per_scan,
            )
            .await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found need tx ack records");

        // 生成推进意图
        for record in records {
            let intent =
                WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxAck(record.trade_no));
            self.dispatch_intent(intent);
        }

        Ok(())
    }

    async fn scan_need_resource_gate(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        trace!(max_items = %self.config.max_resource_gate_per_scan, "Scanning withdraw resource gate records");

        let records = wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::scan_need_resource_gate(
            &pool,
            self.config.max_resource_gate_per_scan,
        ).await?;

        let mut skipped = 0usize;
        let mut first_skip: Option<(String, std::time::Duration)> = None;
        for record in records {
            if let Some((host, remaining)) =
                crate::infrastructure::chain_rpc_guard::breaker_open_for_chain_code_with_ctx(
                    self.ctx,
                    &record.chain_code,
                )
                .await
            {
                skipped += 1;
                if first_skip.is_none() {
                    first_skip = Some((host, remaining));
                }
                continue;
            }
            let intent =
                WithdrawIntent::Chain(WithdrawChainIntent::EvalResourceGate(record.trade_no));
            self.dispatch_intent(intent);
        }

        if skipped > 0 {
            if let Some((host, remaining)) = first_skip {
                warn!(
                    skipped = skipped,
                    host = %host,
                    remaining = ?remaining,
                    "chain rpc circuit breaker open; skipped withdraw resource gate intents"
                );
            }
        }

        Ok(())
    }

    async fn scan_need_resource_result_ack(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;
        trace!(max_items = %self.config.max_items_per_scan, "Scanning withdraw resource result ACK records");

        let records = wallet_database::repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo::scan_need_result_ack_for_origin_type(
            &pool,
            wallet_database::entities::api_trade_type::ApiTradeType::Withdraw as i64,
            self.config.max_items_per_scan,
        ).await?;

        for record in records {
            let intent = WithdrawIntent::SideEffect(
                WithdrawSideEffectIntent::SendResourceResultAck(record.resource_trade_no),
            );
            self.dispatch_intent(intent);
        }

        Ok(())
    }

    async fn scan_need_platform_delegate_apply(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        trace!(max_items = %self.config.max_resource_gate_per_scan, "Scanning withdraw platform delegation apply fact gaps");

        let records = wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::scan_need_platform_delegation_apply(
            &pool,
            self.config.max_resource_gate_per_scan,
        ).await?;

        for record in records {
            self.dispatch_intent(WithdrawIntent::Chain(
                WithdrawChainIntent::ApplyPlatformDelegate(record.trade_no),
            ));
        }
        Ok(())
    }

    async fn scan_need_resource_task_ack(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;
        trace!(max_items = %self.config.max_items_per_scan, "Scanning withdraw resource task ACK records");

        let records = wallet_database::repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo::scan_need_task_ack_for_origin_type(
            &pool,
            wallet_database::entities::api_trade_type::ApiTradeType::Withdraw as i64,
            self.config.max_items_per_scan,
        ).await?;

        for record in records {
            let intent = WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendResourceTaskAck(
                record.resource_trade_no,
            ));
            self.dispatch_intent(intent);
        }

        Ok(())
    }

    async fn scan_can_resource_delegation_execute(&self) -> Result<(), ServiceError> {
        trace!(max_items = %self.config.max_items_per_scan, "Scanning executable withdraw resource delegation records");

        self.scan_can_withdraw_platform_delegate().await;

        Ok(())
    }

    async fn scan_can_withdraw_platform_delegate(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;
        trace!(max_items = %self.config.max_items_per_scan, "Scanning executable withdraw platform delegate records");

        let records = wallet_database::repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo::scan_can_execute_for_origin_type_source_and_operation(
            &pool,
            wallet_database::entities::api_trade_type::ApiTradeType::Withdraw as i64,
            wallet_database::entities::api_resource_delegation::ApiResourceDelegationSource::Platform,
            wallet_database::entities::api_resource_delegation::ApiResourceDelegationOperationType::Delegate,
            self.config.max_items_per_scan,
        ).await?;

        for record in records {
            let intent = WithdrawIntent::Chain(WithdrawChainIntent::ExecuteResourceDelegation(
                record.resource_trade_no,
            ));
            self.dispatch_intent(intent);
        }

        Ok(())
    }

    async fn scan_need_resource_tx_exec_receipt_upload(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;
        trace!(max_items = %self.config.max_items_per_scan, "Scanning withdraw resource tx exec receipt upload records");

        let records = wallet_database::repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo::scan_need_tx_exec_receipt_upload_for_origin_type(
            &pool,
            wallet_database::entities::api_trade_type::ApiTradeType::Withdraw as i64,
            self.config.max_items_per_scan,
        ).await?;

        for record in records {
            let intent = WithdrawIntent::SideEffect(
                WithdrawSideEffectIntent::UploadResourceTxExecReceipt(record.resource_trade_no),
            );
            self.dispatch_intent(intent);
        }

        Ok(())
    }

    /// 扫描"允许构建 raw_tx"的交易
    ///
    /// 事实条件（强顺序屏障）：
    /// - raw_tx IS NULL
    /// - need_service_fee != true        // 不需要服务费补充
    ///
    /// SQL must be equivalent to can_build()
    async fn scan_can_build(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        trace!(max_items = %self.config.max_items_per_scan, "Scanning can build records");

        // 查询DB中可构建的记录
        let records =
            wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::scan_can_build(
                &pool,
                self.config.max_items_per_scan,
            )
            .await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found can build records");

        // 生成推进意图
        for record in records {
            let intent = WithdrawIntent::Chain(WithdrawChainIntent::BuildTx(record.trade_no));
            self.dispatch_intent(intent);
        }

        Ok(())
    }

    /// 扫描"允许广播"的交易
    ///
    /// 事实条件：
    /// - raw_tx IS NOT NULL
    /// - last_broadcast_at IS NULL
    /// - finished_at IS NULL
    ///
    /// SQL must be equivalent to can_broadcast()
    async fn scan_can_broadcast(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        trace!(max_items = %self.config.max_items_per_scan, "Scanning can broadcast records");

        // 查询DB中可广播的记录
        let records = wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::scan_can_broadcast(
            &pool,
            self.config.max_items_per_scan,
        ).await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found can broadcast records");

        let mut skipped = 0usize;
        let mut first_skip: Option<(String, std::time::Duration)> = None;

        // 生成推进意图
        for record in records {
            if let Some((host, remaining)) =
                crate::infrastructure::chain_rpc_guard::breaker_open_for_chain_code_with_ctx(
                    self.ctx,
                    &record.chain_code,
                )
                .await
            {
                skipped += 1;
                if first_skip.is_none() {
                    first_skip = Some((host, remaining));
                }
                continue;
            }
            let intent = WithdrawIntent::Chain(WithdrawChainIntent::BroadcastTx(record.trade_no));
            self.dispatch_intent(intent);
        }

        if skipped > 0 {
            if let Some((host, remaining)) = first_skip {
                warn!(
                    skipped = skipped,
                    host = %host,
                    remaining = ?remaining,
                    "chain rpc circuit breaker open; skipped some broadcast intents"
                );
            } else {
                warn!(
                    skipped = skipped,
                    "chain rpc circuit breaker open; skipped some broadcast intents"
                );
            }
        }

        Ok(())
    }

    /// 扫描需要发送结果确认 ACK 的交易
    ///
    /// 事实条件（强顺序屏障）：
    /// - transaction_time IS NOT NULL
    /// - tx_res_ack_sent_at IS NULL
    /// - finished_at IS NULL
    ///
    /// SQL must be equivalent to need_tx_res_ack()
    async fn scan_confirmed_need_tx_res_ack(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        trace!(max_items = %self.config.max_items_per_scan, "Scanning confirmed need tx res ACK records");

        // 查询DB中已确认但未发送TxRes ACK的记录
        let records = wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::scan_confirmed_need_tx_res_ack(
            &pool,
            self.config.max_items_per_scan,
        ).await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found confirmed need tx res ACK records");

        // 生成推进意图
        for record in records {
            let intent =
                WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxResAck(record.trade_no));
            self.dispatch_intent(intent);
        }

        Ok(())
    }

    /// 扫描需要上传交易执行回执的交易
    ///
    /// 事实条件：
    /// - tx_exec_receipt_uploaded_at IS NULL
    /// - finished_at IS NULL
    /// - scanner 仅对满足 need_tx_exec_receipt_upload() 的记录生成派发意图
    async fn scan_need_tx_exec_receipt_upload(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        trace!(max_items = %self.config.max_items_per_scan, "Scanning need tx exec receipt upload records");

        // 查询DB中需要上传交易执行回执的记录
        let records = wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::scan_need_tx_exec_receipt_upload(
            &pool,
            self.config.max_items_per_scan,
        ).await?;

        // 保存原始记录数
        let original_count = records.len();
        let mut dispatchable_count = 0usize;
        let mut skipped_count = 0usize;

        // 生成推进意图
        for record in records {
            if !need_tx_exec_receipt_upload(&record) {
                skipped_count += 1;
                trace!(
                    trade_no = %record.trade_no,
                    "Skipping tx exec receipt upload: execution result not confirmed"
                );
                continue;
            }
            dispatchable_count += 1;
            trace!(trade_no = %record.trade_no, "Attempting tx exec receipt upload");
            let intent = WithdrawIntent::SideEffect(WithdrawSideEffectIntent::UploadTxExecReceipt(
                record.trade_no,
            ));
            self.dispatch_intent(intent);
        }

        trace!(
            raw_found = %original_count,
            found = %dispatchable_count,
            skipped = %skipped_count,
            "Found need tx exec receipt upload records"
        );

        Ok(())
    }

    /// 扫描需要恢复交易的记录
    ///
    /// 事实条件：
    /// - tx_hash IS NOT NULL
    /// - transaction_time IS NULL
    /// - finished_at IS NULL
    /// - err_code IS NULL
    ///
    /// scan_need_recover is a safety-net scan.
    /// It MUST exist even if try_advance already handles point-to-point wakeup.
    ///
    /// SQL must be equivalent to need_recover()
    async fn scan_need_recover(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        trace!(max_items = %self.config.max_items_per_scan, "Scanning need recover records");

        // 查询DB中需要恢复的记录
        let records = wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::scan_need_recover(
            &pool,
            self.config.max_items_per_scan,
        ).await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found need recover records");

        let mut skipped = 0usize;
        let mut first_skip: Option<(String, std::time::Duration)> = None;

        // 生成推进意图
        for record in records {
            if let Some((host, remaining)) =
                crate::infrastructure::chain_rpc_guard::breaker_open_for_chain_code_with_ctx(
                    self.ctx,
                    &record.chain_code,
                )
                .await
            {
                skipped += 1;
                if first_skip.is_none() {
                    first_skip = Some((host, remaining));
                }
                continue;
            }
            let recover_key = withdraw_recover_dispatch_key(&record);
            if !allow_withdraw_recover_dispatch(&record, &recover_key) {
                trace!(
                    trade_no = %record.trade_no,
                    cooldown = ?shadow_rpc_policy::recover_dispatch_cooldown(&recover_key),
                    "recover_skip_because_cooldown: withdraw safety scan skipped"
                );
                continue;
            }
            let intent = WithdrawIntent::Chain(WithdrawChainIntent::RecoverTx(record.trade_no));
            self.dispatch_intent(intent);
        }

        if skipped > 0 {
            if let Some((host, remaining)) = first_skip {
                warn!(
                    skipped = skipped,
                    host = %host,
                    remaining = ?remaining,
                    "chain rpc circuit breaker open; skipped some recover intents"
                );
            } else {
                warn!(
                    skipped = skipped,
                    "chain rpc circuit breaker open; skipped some recover intents"
                );
            }
        }

        Ok(())
    }

    /// 分发推进意图
    fn dispatch_intent(&self, intent: WithdrawIntent) {
        trace!(?intent, "Generated withdraw intent");

        // 将意图发送给Dispatcher（非阻塞；避免卡住 scanner loop）
        match self.intent_tx.try_send(intent) {
            Ok(_) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(intent))
            | Err(tokio::sync::mpsc::error::TrySendError::Closed(intent)) => {
                let trade_no = match &intent {
                    WithdrawIntent::Chain(WithdrawChainIntent::EstimateFee(trade_no))
                    | WithdrawIntent::Chain(WithdrawChainIntent::EvalResourceGate(trade_no))
                    | WithdrawIntent::Chain(WithdrawChainIntent::ApplyPlatformDelegate(trade_no))
                    | WithdrawIntent::Chain(WithdrawChainIntent::BuildTx(trade_no))
                    | WithdrawIntent::Chain(WithdrawChainIntent::BroadcastTx(trade_no))
                    | WithdrawIntent::Chain(WithdrawChainIntent::RecoverTx(trade_no))
                    | WithdrawIntent::Chain(WithdrawChainIntent::ExecuteResourceDelegation(
                        trade_no,
                    ))
                    | WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxAck(trade_no))
                    | WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxResAck(
                        trade_no,
                    ))
                    | WithdrawIntent::SideEffect(WithdrawSideEffectIntent::UploadTxExecReceipt(
                        trade_no,
                    ))
                    | WithdrawIntent::SideEffect(
                        WithdrawSideEffectIntent::SendResourceResultAck(trade_no),
                    )
                    | WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendResourceTaskAck(
                        trade_no,
                    ))
                    | WithdrawIntent::SideEffect(
                        WithdrawSideEffectIntent::UploadResourceTxExecReceipt(trade_no),
                    ) => trade_no.clone(),
                };

                warn!(trade_no = %trade_no, ?intent, "Failed to dispatch withdraw intent");

                self.emit_intent_dispatch_failed(trade_no);
            }
        }
    }

    fn emit_intent_dispatch_failed(&self, trade_no: String) {
        if let Some(tx) = &self.diagnose_tx {
            let meta =
                DiagnoseMeta::new(trade_no, DiagnoseSource::Advancer, DiagnoseStage::Unknown);
            let _ = tx.try_send(DiagnoseEvent::IntentDispatchFailed { meta });
        }
    }

    /// 尝试基于当前事实推进一个阶段
    ///
    /// 注意：try_advance 每次最多推进一个阶段
    /// 多阶段推进依赖后续 Tick 或定时扫描
    ///
    /// 参数：
    /// - trade_no: 提币交易编号
    ///
    /// 行为：
    /// 1. 查询最新的DB状态
    /// 2. 基于事实状态，按照 ADVANCEMENT_ORDER 顺序检查可推进点
    /// 3. 找到第一个满足条件的推进点，生成对应意图
    /// 4. 发送意图并返回
    pub async fn try_advance(&self, trade_no: &str) {
        if let Err(error) = self.try_advance_result(trade_no).await {
            error!(trade_no = %trade_no, %error, "Withdraw try_advance failed");
        }
    }

    pub fn trigger_build_fast_path_after_resource_gate_release(&self, trade_no: &str) {
        let intent = WithdrawIntent::Chain(WithdrawChainIntent::BuildTx(trade_no.to_string()));
        let failed_trade_no = trade_no.to_string();
        dispatch_fast_path(
            FastPathFlow::Withdraw,
            FastPathReason::ResourceGateReleased,
            FastPathTarget::BuildTx,
            trade_no,
            || self.intent_tx.try_send(intent),
            || self.emit_intent_dispatch_failed(failed_trade_no),
        );
    }

    pub fn trigger_fee_estimate_fast_path_after_tx_ack(&self, trade_no: &str) {
        let intent = WithdrawIntent::Chain(WithdrawChainIntent::EstimateFee(trade_no.to_string()));
        let failed_trade_no = trade_no.to_string();
        dispatch_fast_path(
            FastPathFlow::Withdraw,
            FastPathReason::TxAckSent,
            FastPathTarget::EstimateFee,
            trade_no,
            || self.intent_tx.try_send(intent),
            || self.emit_intent_dispatch_failed(failed_trade_no),
        );
    }

    pub fn trigger_resource_gate_fast_path_after_fee_estimate(&self, trade_no: &str) {
        let intent =
            WithdrawIntent::Chain(WithdrawChainIntent::EvalResourceGate(trade_no.to_string()));
        let failed_trade_no = trade_no.to_string();
        dispatch_fast_path(
            FastPathFlow::Withdraw,
            FastPathReason::FeeEstimatePersisted,
            FastPathTarget::EvalResourceGate,
            trade_no,
            || self.intent_tx.try_send(intent),
            || self.emit_intent_dispatch_failed(failed_trade_no),
        );
    }

    async fn try_advance_result(&self, trade_no: &str) -> Result<(), ServiceError> {
        trace!(trade_no = %trade_no, "Try advancing withdraw transaction");
        let pool = self.ctx.api_transaction_pool()?;

        // 查询最新的DB状态
        let withdraw = wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            trade_no,
            wallet_database::entities::api_trade_type::ApiTradeType::Withdraw,
        ).await?;

        // ============================================================================
        // ARCHITECTURE VIOLATION DETECTION — 只报警不阻断
        // ============================================================================
        // 检测潜在的架构违规
        if withdraw.raw_tx.is_some() && withdraw.audit_passed_at.is_none() {
            error!(
                trade_no = %withdraw.trade_no,
                "ARCHITECTURE VIOLATION: raw_tx exists but audit_passed_at missing"
            );
        }

        if withdraw.raw_tx.is_some() && withdraw.tx_ack_sent_at.is_none() {
            error!(
                trade_no = %withdraw.trade_no,
                "ARCHITECTURE VIOLATION: raw_tx exists but tx_ack_sent_at missing"
            );
        }

        if withdraw.finished_at.is_some()
            && withdraw.transaction_time.is_none()
            && withdraw.err_code.is_none()
        {
            error!(
                trade_no = %withdraw.trade_no,
                "ARCHITECTURE VIOLATION: finished_at exists but transaction_time missing"
            );
        }
        // ============================================================================

        match self.pending_resource_result_ack_trade_no(trade_no).await {
            Ok(Some(resource_trade_no)) => {
                trace!(
                    trade_no = %trade_no,
                    resource_trade_no = %resource_trade_no,
                    "Resource result ACK is pending; advancing ACK before withdraw main chain"
                );
                self.dispatch_intent(WithdrawIntent::SideEffect(
                    WithdrawSideEffectIntent::SendResourceResultAck(resource_trade_no),
                ));
                return Ok(());
            }
            Ok(None) => {}
            Err(e) => {
                error!(
                    trade_no = %trade_no,
                    error = %e,
                    "Failed to check pending withdraw resource result ACK"
                );
                return Ok(());
            }
        }

        let tx_ack_pending = evaluate_point(AdvancementPoint::NeedTxAck, &withdraw).can_advance;
        if tx_ack_pending {
            trace!(trade_no = %trade_no, "Need to send tx ACK before other order work");
            self.dispatch_intent(WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxAck(
                trade_no.to_string(),
            )));
            return Ok(());
        }

        let fee_estimate_pending = need_fee_estimate_snapshot(&withdraw);
        if fee_estimate_pending {
            if let Some((host, remaining)) =
                shadow_rpc_policy::breaker_open_for_chain_code(self.ctx, &withdraw.chain_code).await
            {
                trace!(
                    trade_no = %trade_no,
                    chain_code = %withdraw.chain_code,
                    host = %host,
                    remaining = ?remaining,
                    "try_advance_skip_because_breaker_open: withdraw fee estimate skipped"
                );
                return Ok(());
            }

            trace!(trade_no = %trade_no, "Need to estimate withdraw fee snapshot");
            let intent =
                WithdrawIntent::Chain(WithdrawChainIntent::EstimateFee(trade_no.to_string()));
            self.dispatch_intent(intent);
        }

        if fee_estimate_pending {
            return Ok(());
        }

        // err_code 冻结：只允许 UploadTxExecReceipt
        if withdraw.err_code.is_some() {
            let eval = evaluate_point(AdvancementPoint::NeedTxExecReceiptUpload, &withdraw);
            if eval.can_advance {
                trace!(trade_no = %trade_no, "Need to upload tx exec receipt (err_code frozen state)");
                let intent = WithdrawIntent::SideEffect(
                    WithdrawSideEffectIntent::UploadTxExecReceipt(trade_no.to_string()),
                );
                self.dispatch_intent(intent);
            }
            return Ok(());
        }

        // 按照 ADVANCEMENT_ORDER 顺序检查可推进点
        // 顺序与 scan_round 完全一致，确保行为一致性
        for point in ADVANCEMENT_ORDER {
            let eval = evaluate_point(*point, &withdraw);
            if !eval.can_advance {
                continue;
            }

            match point {
                AdvancementPoint::NeedTxAck => {
                    trace!(trade_no = %trade_no, "Need to send tx ACK");
                    let intent = WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxAck(
                        trade_no.to_string(),
                    ));
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::NeedResourceGate => {
                    trace!(trade_no = %trade_no, "Need to evaluate resource gate");
                    let intent = WithdrawIntent::Chain(WithdrawChainIntent::EvalResourceGate(
                        trade_no.to_string(),
                    ));
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::NeedPlatformDelegateApply => {
                    trace!(trade_no = %trade_no, "Need to apply platform delegation");
                    self.dispatch_intent(WithdrawIntent::Chain(
                        WithdrawChainIntent::ApplyPlatformDelegate(trade_no.to_string()),
                    ));
                    return Ok(());
                }
                AdvancementPoint::CanBuild => {
                    trace!(trade_no = %trade_no, "Can build transaction");
                    let intent =
                        WithdrawIntent::Chain(WithdrawChainIntent::BuildTx(trade_no.to_string()));
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::CanBroadcast => {
                    if let Some((host, remaining)) = shadow_rpc_policy::breaker_open_for_chain_code(
                        self.ctx,
                        &withdraw.chain_code,
                    )
                    .await
                    {
                        trace!(
                            trade_no = %trade_no,
                            chain_code = %withdraw.chain_code,
                            host = %host,
                            remaining = ?remaining,
                            "try_advance_skip_because_breaker_open: withdraw broadcast skipped"
                        );
                        if shadow_rpc_policy::should_emit_breaker_warn(&format!(
                            "withdraw.try_advance.breaker:{}:{}",
                            withdraw.chain_code, host
                        )) {
                            warn!(
                                trade_no = %trade_no,
                                chain_code = %withdraw.chain_code,
                                host = %host,
                                remaining = ?remaining,
                                "try_advance_skip_because_breaker_open: withdraw broadcast skipped"
                            );
                        }
                        return Ok(());
                    }
                    trace!(trade_no = %trade_no, "Can broadcast transaction");
                    let intent = WithdrawIntent::Chain(WithdrawChainIntent::BroadcastTx(
                        trade_no.to_string(),
                    ));
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::NeedRecover => {
                    if let Some((host, remaining)) = shadow_rpc_policy::breaker_open_for_chain_code(
                        self.ctx,
                        &withdraw.chain_code,
                    )
                    .await
                    {
                        trace!(
                            trade_no = %trade_no,
                            chain_code = %withdraw.chain_code,
                            host = %host,
                            remaining = ?remaining,
                            "try_advance_skip_because_breaker_open: withdraw recover skipped"
                        );
                        if shadow_rpc_policy::should_emit_breaker_warn(&format!(
                            "withdraw.try_advance.breaker:{}:{}",
                            withdraw.chain_code, host
                        )) {
                            warn!(
                                trade_no = %trade_no,
                                chain_code = %withdraw.chain_code,
                                host = %host,
                                remaining = ?remaining,
                                "try_advance_skip_because_breaker_open: withdraw recover skipped"
                            );
                        }
                        return Ok(());
                    }
                    let recover_key = withdraw_recover_dispatch_key(&withdraw);
                    if !allow_withdraw_recover_dispatch(&withdraw, &recover_key) {
                        trace!(
                            trade_no = %trade_no,
                            cooldown = ?shadow_rpc_policy::recover_dispatch_cooldown(&recover_key),
                            "recover_skip_because_cooldown: withdraw recover skipped"
                        );
                        return Ok(());
                    }
                    trace!(trade_no = %trade_no, "Need to recover transaction");
                    let intent =
                        WithdrawIntent::Chain(WithdrawChainIntent::RecoverTx(trade_no.to_string()));
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::NeedTxExecReceiptUpload => {
                    trace!(trade_no = %trade_no, "Need to upload tx exec receipt");
                    let intent = WithdrawIntent::SideEffect(
                        WithdrawSideEffectIntent::UploadTxExecReceipt(trade_no.to_string()),
                    );
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::NeedTxResAck => {
                    trace!(trade_no = %trade_no, "Need to send tx res ACK");
                    let intent = WithdrawIntent::SideEffect(
                        WithdrawSideEffectIntent::SendTxResAck(trade_no.to_string()),
                    );
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::FullyBlocked => {}
            }
        }

        // 无可用推进点
        trace!(trade_no = %trade_no, "No advancement possible based on current facts");
        let _ = maybe_log_stuck(
            &withdraw,
            &self.diagnose_tx,
            DiagnoseSource::ManualAdvance,
            DiagnoseStage::Unknown,
        );
        Ok(())
    }

    async fn pending_resource_result_ack_trade_no(
        &self,
        origin_trade_no: &str,
    ) -> Result<Option<String>, crate::error::service::ServiceError> {
        let pool = self.ctx.api_resource_pool()?;
        Ok(wallet_database::repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo::find_pending_result_ack_by_origin(
            &pool,
            wallet_database::entities::api_trade_type::ApiTradeType::Withdraw as i64,
            origin_trade_no,
        )
        .await?
        .map(|row| row.resource_trade_no))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::TempDir;
    use tokio::sync::mpsc;
    use wallet_database::{
        entities::{
            api_resource_delegation::{
                ApiResourceDelegationResultStatus, NewApiResourceDelegation,
            },
            api_trade_type::ApiTradeType,
            api_withdraw::{ApiWithdrawEntity, ApiWithdrawStatus, ErrCode, WithdrawFailureStage},
            asset_token_key::AssetTokenKey,
        },
        repositories::api_wallet::{
            resource_delegation::ApiResourceDelegationRepo, withdraw::ApiWithdrawRepo,
        },
    };

    async fn test_ctx() -> &'static crate::context::Context {
        crate::testkit::context::api_trans_test_ctx().await
    }

    async fn isolated_test_ctx() -> anyhow::Result<(TempDir, &'static crate::context::Context)> {
        let temp_dir = tempfile::tempdir()?;
        let config = crate::config::Config::new(
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
"#,
        )?;
        let dirs = crate::dirs::Dirs::new(temp_dir.path().to_str().expect("UTF-8 temp path"))?;
        let ctx = Box::leak(Box::new(
            crate::context::Context::new_without_handles_for_test(dirs, config).await?,
        ));
        Ok((temp_dir, ctx))
    }

    fn base_withdraw(trade_no: &str) -> ApiWithdrawEntity {
        ApiWithdrawEntity {
            id: 1,
            name: "n".to_string(),
            uid: "u".to_string(),
            from_addr: "from".to_string(),
            to_addr: "to".to_string(),
            value: "0".to_string(),
            validate: "v".to_string(),
            chain_code: "tron".to_string(),
            token_addr: AssetTokenKey::Native,
            symbol: "s".to_string(),
            trade_no: trade_no.to_string(),
            trade_type: ApiTradeType::Withdraw,
            init_status: ApiWithdrawStatus::Init,
            status: ApiWithdrawStatus::Init,
            nonce: 0,
            tx_hash: Some("0xhash".to_string()),
            raw_tx: Some("{}".to_string()),
            resource_consume: "0".to_string(),
            transaction_fee: "0".to_string(),
            estimated_transaction_fee: None,
            estimated_resource_consume: None,
            fee_estimated_at: None,
            transaction_time: None,
            block_height: None,
            notes: None,
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: None,
            resource_check_at: None,
            resource_gate_released_at: None,
            resource_gate_result: None,
            resource_block_reason: None,
            resource_dependency_trade_no: None,
            resource_dependency_type: None,
            tx_ack_sent_at: Some(Utc::now()),
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_res_ack_sent_at: None,
            tx_res_received_at: None,
            tx_exec_receipt_uploaded_at: None,
            finished_at: None,
            audit_passed_at: Some(Utc::now()),
            audit_rejected_at: None,
            audit_reason: None,
            chain_success_at: None,
            chain_failed_at: None,
            failure_stage: Some(WithdrawFailureStage::Unknown),
            created_at: Utc::now(),
            updated_at: Some(Utc::now()),
            out_order_id: None,
            client_id: None,
            create_time: None,
        }
    }

    #[test]
    fn tx_exec_receipt_upload_is_blocked_before_chain_confirmation() {
        let mut withdraw = base_withdraw("W_PENDING");
        withdraw.last_broadcast_at = Some(Utc::now());

        assert!(!need_tx_exec_receipt_upload(&withdraw));
    }

    #[test]
    fn tx_exec_receipt_upload_allows_confirmed_success() {
        let mut withdraw = base_withdraw("W_SUCCESS");
        withdraw.transaction_time = Some(Utc::now());

        assert!(need_tx_exec_receipt_upload(&withdraw));
    }

    #[test]
    fn tx_exec_receipt_upload_allows_explicit_failure() {
        let mut withdraw = base_withdraw("W_FAIL");
        withdraw.err_code = Some(ErrCode::UnknownError);

        assert!(need_tx_exec_receipt_upload(&withdraw));
    }

    #[test]
    fn confirmed_tron_resource_recovery_uses_independent_cooldown_key() {
        let mut withdraw = base_withdraw("W_RESOURCE_RECOVERY_KEY");
        withdraw.token_addr = AssetTokenKey::Contract("TToken".to_string());
        withdraw.transaction_time = Some(Utc::now());
        withdraw.resource_consume = r#"{"bandwidth":0,"energy":0}"#.to_string();

        assert_eq!(
            withdraw_recover_dispatch_key(&withdraw),
            "recover-resource:W_RESOURCE_RECOVERY_KEY"
        );

        withdraw.transaction_time = None;
        assert_eq!(withdraw_recover_dispatch_key(&withdraw), "recover:W_RESOURCE_RECOVERY_KEY");
    }

    #[test]
    fn fresh_broadcast_recover_uses_fast_admission_then_old_broadcast_uses_generic_admission() {
        let mut fresh = base_withdraw("W_FRESH_BROADCAST_RECOVER");
        fresh.last_broadcast_at = Some(Utc::now());
        let fresh_key = withdraw_recover_dispatch_key(&fresh);

        assert!(allow_withdraw_recover_dispatch(&fresh, &fresh_key));
        assert_eq!(
            shadow_rpc_policy::recover_dispatch_cooldown(&fresh_key),
            std::time::Duration::from_secs(3)
        );

        let mut old = base_withdraw("W_OLD_BROADCAST_RECOVER");
        old.last_broadcast_at = Some(Utc::now() - chrono::Duration::seconds(121));
        let old_key = withdraw_recover_dispatch_key(&old);

        assert!(allow_withdraw_recover_dispatch(&old, &old_key));
        assert_eq!(
            shadow_rpc_policy::recover_dispatch_cooldown(&old_key),
            std::time::Duration::from_secs(60)
        );

        let mut confirmed = base_withdraw("W_CONFIRMED_BROADCAST_RECOVER");
        confirmed.last_broadcast_at = Some(Utc::now());
        confirmed.transaction_time = Some(Utc::now());
        let confirmed_key = withdraw_recover_dispatch_key(&confirmed);

        assert!(allow_withdraw_recover_dispatch(&confirmed, &confirmed_key));
        assert_eq!(
            shadow_rpc_policy::recover_dispatch_cooldown(&confirmed_key),
            std::time::Duration::from_secs(60)
        );

        let mut future = base_withdraw("W_FUTURE_BROADCAST_RECOVER");
        future.last_broadcast_at = Some(Utc::now() + chrono::Duration::seconds(1));
        let future_key = withdraw_recover_dispatch_key(&future);

        assert!(allow_withdraw_recover_dispatch(&future, &future_key));
        assert_eq!(
            shadow_rpc_policy::recover_dispatch_cooldown(&future_key),
            std::time::Duration::from_secs(60)
        );
    }

    #[tokio::test]
    async fn try_advance_prioritizes_withdraw_resource_result_ack_before_build()
    -> anyhow::Result<()> {
        let ctx = test_ctx().await;
        let pool = ctx.api_transaction_pool()?;
        let resource_pool = ctx.api_resource_pool()?;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let scanner = ShadowScanner::new(ctx, ScannerConfig::default(), intent_tx, None);

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_1",
            "withdraw",
            "from_addr",
            "to_addr",
            "1",
            "digest",
            "tron",
            AssetTokenKey::Native,
            "TRX",
            "W_pending_rsc_ack",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE api_withdraws
            SET tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                audit_passed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                resource_gate_released_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?
            "#,
        )
        .bind("W_pending_rsc_ack")
        .execute(pool.as_ref())
        .await?;

        ApiResourceDelegationRepo::upsert_original_order_result_fact(
            &resource_pool,
            NewApiResourceDelegation::platform_delegate(
                "uid_1",
                "W_pending_rsc_ack",
                "W_pending_rsc_ack",
                ApiTradeType::Withdraw as i64,
                "",
                "",
                "0",
            ),
            ApiResourceDelegationResultStatus::Success,
            None,
            Some(r#"{"tradeNo":"W_pending_rsc_ack","status":true}"#),
        )
        .await?;

        scanner.try_advance("W_pending_rsc_ack").await;

        let intent = intent_rx.try_recv().expect("resource ACK intent should be dispatched");
        assert!(matches!(
            intent,
            WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendResourceResultAck(ref trade_no))
                if trade_no == "W_pending_rsc_ack"
        ));
        assert!(intent_rx.try_recv().is_err());

        Ok(())
    }

    #[tokio::test]
    async fn withdraw_fee_estimate_snapshot_scan_dispatches_before_audit() -> anyhow::Result<()> {
        let (_temp_dir, ctx) = isolated_test_ctx().await?;
        let pool = ctx.api_transaction_pool()?;
        let trade_no = format!(
            "W_fee_estimate_before_audit_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let scanner = ShadowScanner::new(ctx, ScannerConfig::default(), intent_tx, None);

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_1",
            "withdraw",
            "from_addr",
            "to_addr",
            "1",
            "digest",
            "tron",
            AssetTokenKey::Native,
            "TRX",
            &trade_no,
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await?;

        scanner.scan_round().await;

        let intent = intent_rx.try_recv().expect("fee estimate intent should be dispatched");
        assert!(matches!(
            intent,
            WithdrawIntent::Chain(WithdrawChainIntent::EstimateFee(ref dispatched_trade_no))
                if dispatched_trade_no == &trade_no
        ));

        Ok(())
    }

    #[tokio::test]
    async fn withdraw_try_advance_dispatches_ack_before_fee_estimate() -> anyhow::Result<()> {
        let ctx = test_ctx().await;
        let pool = ctx.api_transaction_pool()?;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let scanner = ShadowScanner::new(ctx, ScannerConfig::default(), intent_tx, None);

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_1",
            "withdraw",
            "from_addr",
            "to_addr",
            "1",
            "digest",
            "bnb",
            AssetTokenKey::Native,
            "BNB",
            "W_bnb_fee_estimate_try_advance",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await?;

        scanner.try_advance("W_bnb_fee_estimate_try_advance").await;

        let intent = intent_rx.try_recv().expect("tx ACK intent should be dispatched");
        assert!(matches!(
            intent,
            WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxAck(ref trade_no))
                if trade_no == "W_bnb_fee_estimate_try_advance"
        ));

        assert!(intent_rx.try_recv().is_err());

        Ok(())
    }

    #[tokio::test]
    async fn withdraw_resource_gate_fast_path_dispatches_build_intent() {
        let ctx = test_ctx().await;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let scanner = ShadowScanner::new(ctx, ScannerConfig::default(), intent_tx, None);

        scanner.trigger_build_fast_path_after_resource_gate_release("W_resource_released");

        let intent = intent_rx.try_recv().expect("BuildTx intent should be dispatched");
        assert!(matches!(
            intent,
            WithdrawIntent::Chain(WithdrawChainIntent::BuildTx(ref trade_no))
                if trade_no == "W_resource_released"
        ));
        assert!(intent_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn withdraw_fee_estimate_fast_path_dispatches_resource_gate_intent() {
        let ctx = test_ctx().await;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let scanner = ShadowScanner::new(ctx, ScannerConfig::default(), intent_tx, None);

        scanner.trigger_resource_gate_fast_path_after_fee_estimate("W_fee_estimated");

        let intent = intent_rx.try_recv().expect("resource gate intent should be dispatched");
        assert!(matches!(
            intent,
            WithdrawIntent::Chain(WithdrawChainIntent::EvalResourceGate(ref trade_no))
                if trade_no == "W_fee_estimated"
        ));
        assert!(intent_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn withdraw_tx_ack_fast_path_dispatches_fee_estimate_intent() {
        let ctx = test_ctx().await;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let scanner = ShadowScanner::new(ctx, ScannerConfig::default(), intent_tx, None);

        scanner.trigger_fee_estimate_fast_path_after_tx_ack("W_tx_ack_sent");

        let intent = intent_rx.try_recv().expect("fee estimate intent should be dispatched");
        assert!(matches!(
            intent,
            WithdrawIntent::Chain(WithdrawChainIntent::EstimateFee(ref trade_no))
                if trade_no == "W_tx_ack_sent"
        ));
        assert!(intent_rx.try_recv().is_err());
    }
}
