// collect_fee/shadow/scanner.rs
//
// ============================================================================
// Scanner 设计铁律（Final · 不可违背）
// ============================================================================
//
// 核心定位：
// Scanner = 事实读取器 + 推进意图生成器
// Scanner 只"读事实 → 决定能否推进"，绝不创造事实
//
// ---------------------------------------------------------------------------
// 1. Scanner 只允许读取【不可逆事实字段】
// ---------------------------------------------------------------------------
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
//     - need_service_fee
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
// ---------------------------------------------------------------------------
// 1.z 铁律 D：err_code = 失败冻结闸
// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
// 1.z.1 副作用分类与 err_code 冻结范围
// ---------------------------------------------------------------------------
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
// - UploadTxExecReceipt 用于补齐“已发起链上执行”的事实
// - 无论成功失败都需要执行，确保行为事实完整性
// - 不属于“推进”，属于“事实补齐”
// - 补事实行为不得引入新的推进事实字段写入

// ---------------------------------------------------------------------------
// 1.z.2 finished_at = 系统终态屏障
// ---------------------------------------------------------------------------
//
// finished_at 表示本系统对该交易的生命周期已结束
//
// 一旦 finished_at IS NOT NULL：
//
// - Scanner 必须完全沉默
// - 不再产生任何 Intent
// - try_advance 不再推进
// - Recover 只允许补事实，不允许推进
//
// finished_at 的写入是唯一收口行为，
// Scanner / try_advance / recover 都不得绕过

// ---------------------------------------------------------------------------
// 1.z.3 手续费不足 = 构建失败事实
// ---------------------------------------------------------------------------
//
// Fee.need_service_fee = true
// 表示构建阶段已确认的最终失败事实
//
// 等价于：
// - err_code 的一种构建期来源
// - 不可恢复
// - 不允许进入任何链推进路径

// ---------------------------------------------------------------------------
// 2. Scanner 不使用时间做决策
// ---------------------------------------------------------------------------
//
// - 禁止使用：
//   - now - xxx > duration
//   - xxx_at < yyy_at
//
// - 时间字段唯一用途：
//   - 作为"该事实是否已发生"的布尔信号
//     （IS NULL / IS NOT NULL）
//
// ---------------------------------------------------------------------------
// 3. Scanner 不判断"该不该做"，只判断"事实是否已满足"
// ---------------------------------------------------------------------------
//
// - Scanner 不包含业务意图
// - Scanner 不做价值判断
// - Scanner 只回答一个问题：
//   👉「在当前事实快照下，是否允许推进某一步？」
//
// ---------------------------------------------------------------------------
// 4. Scanner 的唯一职责
// ---------------------------------------------------------------------------
//
// 事实快照（ApiFeeEntity）
//        ↓
// 生成 FeeIntent
//
// - Scanner 不写 DB
// - Scanner 不发请求
// - Scanner 不修改事实
//
// ---------------------------------------------------------------------------
// 5. Scanner 方法命名铁律
// ---------------------------------------------------------------------------
//
// - 方法名必须是【事实条件的直接翻译】
// - 禁止使用：
//   - done / finished / completed / success / failed
//
// 正确示例：
// - can_build
// - need_tx_ack
// - need_tx_res_ack
//
// 错误示例：
// - is_build_done
// - should_broadcast
// - is_tx_success
//
// ---------------------------------------------------------------------------
// 6. Scanner 面对的记录类型
// ---------------------------------------------------------------------------
//
// - 可推进记录
// - 已终止记录（finished_at IS NOT NULL）
// - 失败冻结记录（err_code IS NOT NULL，finished_at IS NULL）
//
// 其中：
// - 后两类记录 Scanner 必须完全沉默
//
// ❌ 不存在第四态：
// - "再等等"
// - "观察中"
// - "可能会好"
//
// ============================================================================
// IMPORTANT:
// All ApiFeeRepo::scan_xxx SQL conditions MUST be equivalent
// to the corresponding predicate function in this file.
// This ensures that scanner, try_advance, and future components
// all use the same logic and do not diverge.
// ============================================================================

/// ============================================================================
/// 手续费（Service Fee）流程铁律（必须遵守）
/// ============================================================================
///
/// 【核心定位】
/// 手续费流程是「构建阶段的失败分支」，而不是一条独立的成功路径。
///
/// ⚠️ 重要区分：
///
/// 本文档中“手续费不足”指的是：
///
/// - Fee 交易自身在构建阶段发现余额不足
/// - 即：用于打手续费的地址本身也无足够余额
///
/// 这与 Collect 交易中的 need_service_fee 语义完全不同：
///
/// - Collect.need_service_fee：
///   表示“我需要别人给我打手续费”，属于可恢复分支
///
/// - Fee.build 失败：
///   表示“没人能再给我打钱”，属于最终失败
///
/// 一旦确认“手续费不足”，该交易在**业务语义上已经结束**，
/// 后续只允许做“结果上报型副作用”，禁止任何继续推进链上流程。
///
/// ---------------------------------------------------------------------------
/// 一、手续费不足的定义（事实，而非状态）
/// ---------------------------------------------------------------------------
/// 当且仅当满足以下事实条件时，视为手续费不足：
///
/// - need_service_fee = true
///
/// ⚠️ 注意：
/// - 本文档中「手续费不足」特指【手续费交易自身】的余额不足
/// - 手续费不足 ≠ 链上失败
/// - 手续费不足发生在【构建阶段】
/// - 与 tx_hash / transaction_time 无关
/// - 这是一个最终失败事实，无法通过外部干预恢复
///
/// ---------------------------------------------------------------------------
/// 二、手续费不足的处理铁律（不可破坏）
/// ---------------------------------------------------------------------------
///
/// 一旦确认手续费不足（手续费交易自身余额不足）：
///
/// 1. 该交易【不再进入广播阶段】
/// 2. 该交易【不会产生 tx_hash】
/// 3. 该交易【不会发生 transaction_time】
/// 4. 该交易【不会进入重试 / 打手续费流程】
///
/// 允许的唯一推进方向：
///
/// - 视为“构建失败的最终结果”
/// - 直接进入 tx_exec_receipt_upload
/// - 上报失败执行结果给后端
///
/// ---------------------------------------------------------------------------
/// 三、tx_exec_receipt_upload 在手续费场景下的语义
/// ---------------------------------------------------------------------------
///
/// 注意：tx_exec_receipt_upload 只适用于以下情况：
/// - 手续费交易已成功构建并尝试广播
/// - 广播后失败（而非构建阶段失败）
///
/// tx_exec_receipt_upload 在此场景下表示：
///
/// “我已发起过链上执行请求，
/// 但由于手续费不足或其他原因，实际执行失败。”
///
/// 因此：
/// - receipt 内容为失败结果
/// - 不要求 tx_hash
/// - 不依赖 transaction_time
///
/// ⚠️ 特别说明：
/// - 手续费交易在**构建阶段失败**（自身余额不足）
///   → 直接作为失败终态，不产生 receipt
/// - 手续费交易在**广播之后失败**
///   → 进入 tx_exec_receipt_upload，上报失败结果
///
/// ---------------------------------------------------------------------------
/// 四、Scanner 约束（非常重要）
/// ---------------------------------------------------------------------------
///
/// Scanner 必须遵守以下规则：
///
/// - 不得因为手续费不足而触发：
///   - scan_can_broadcast
///   - scan_need_tx_res_ack（成功 ACK）
///
/// - 只允许触发：
///   - scan_need_tx_exec_receipt_upload（失败回执）
///
/// - tx_exec_receipt_uploaded_at 写入后：
///   - 该交易流程视为结束
///   - 不得再被任何 Scanner predicate 命中
///
/// ---------------------------------------------------------------------------
/// 五、与归集流程的关系
/// ---------------------------------------------------------------------------
///
/// 手续费流程与归集流程遵循相同的事实驱动铁律，
/// 区别仅在于：
///
/// - 手续费流程步骤更少
/// - 不存在“补打手续费后继续推进”的路径
///
/// 一旦手续费不足：
/// - 归集流程：结束
/// - 手续费流程：结束
///
/// 不允许“修复后重试”的隐式语义。
///
/// ---------------------------------------------------------------------------
/// 六、架构原则总结（一句话）
/// ---------------------------------------------------------------------------
///
/// 手续费不足不是“需要补救的异常”，
/// 而是“可以被确认并上报的最终事实”。
///
/// 在手续费不足场景下：
/// tx_exec_receipt_upload 之前，必须先成功终止所有
/// 依赖该手续费的归集流程。
/// 若归集终止失败，不得写入 tx_exec_receipt_uploaded_at，
/// 否则将导致系统进入不可恢复状态。
///
/// ============================================================================
/// END
/// ============================================================================

/// ============================================================================
/// Scanner Recover Rule ===
/// ============================================================================
///
/// Recover is a Scanner-level advancement rule, not a Worker heuristic.
///
/// Predicate:
/// - tx_hash IS NOT NULL
/// - transaction_time IS NULL
///
/// Semantics:
/// - Indicates that on-chain final result MAY already exist,
///   but system fact is missing.
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
//
// ❌ WRONG EXAMPLE:
//
// if !check_fee() {
//     return Ok(());
// }
//
// This is NOT sufficient to replace build_blocked_at.
// The scanner will continue to emit BuildTx intents.
//
use std::fmt;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tracing::{error, trace, warn};
use wallet_database::{ApiTransactionDbPool, entities::api_fee::ApiFeeEntity};

use crate::{
    error::service::ServiceError,
    infrastructure::api_trans::{
        collect_fee::shadow::{FeeChainIntent, FeeSideEffectIntent},
        shadow_rpc_policy,
    },
};

use super::FeeIntent;

/// 导入 ApiFeeRepo
use wallet_database::repositories::api_wallet::fee::ApiFeeRepo;

use super::{
    predicate::evaluate_point,
    stage::{ADVANCEMENT_ORDER, AdvancementPoint},
};
use crate::infrastructure::api_trans::collect_fee::diagnose::{
    DiagnoseEvent, DiagnoseEventSender, DiagnoseMeta, DiagnoseSource, DiagnoseStage,
    maybe_log_stuck,
};

/// 终态 / 完成判断（Future Use）
/// ----------------------------------------------------------------------------

/// 检查交易是否已完成所有链事实
///
/// 事实条件：
/// - transaction_time IS NOT NULL
///
/// ⚠️ 注意：
/// - chain finished ≠ system finished
/// - 不得用于判断 Scanner 是否停止
/// - 仅表示链上结果已确定，不表示所有副作用已完成
/// Reserved for metrics / observability only
fn is_chain_finished(fee: &ApiFeeEntity) -> bool {
    fee.transaction_time.is_some()
}

fn allow_fee_recover_dispatch(recover_key: &str, last_broadcast_at: Option<DateTime<Utc>>) -> bool {
    shadow_rpc_policy::allow_broadcast_recover_dispatch(recover_key, last_broadcast_at)
}

#[cfg(test)]
mod tests {
    use super::allow_fee_recover_dispatch;
    use crate::infrastructure::api_trans::shadow_rpc_policy;
    use chrono::Utc;
    use std::time::Duration;

    #[test]
    fn fee_recover_dispatch_uses_fresh_broadcast_window() {
        let key = shadow_rpc_policy::recover_dispatch_key("CF_FRESH_BROADCAST");
        shadow_rpc_policy::reset_recover_dispatch(&key);

        assert!(allow_fee_recover_dispatch(&key, Some(Utc::now())));
        assert_eq!(shadow_rpc_policy::recover_dispatch_cooldown(&key), Duration::from_secs(3));

        shadow_rpc_policy::reset_recover_dispatch(&key);
    }
}

/// Shadow Scanner 配置
#[derive(Debug, Clone)]
pub struct ScannerConfig {
    /// 扫描间隔
    pub scan_interval: Duration,
    /// 每轮最大处理数量
    pub max_items_per_scan: usize,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        let scan_interval_secs =
            shadow_rpc_policy::read_u64_env("FEE_SHADOW_SCAN_INTERVAL_SECS", 15, 10, 120);
        let max_items_per_scan =
            shadow_rpc_policy::read_usize_env("FEE_SHADOW_MAX_ITEMS_PER_SCAN", 60, 20, 200);
        Self { scan_interval: Duration::from_secs(scan_interval_secs), max_items_per_scan }
    }
}

/// Shadow Scanner
///
///
/// 只生成推进意图，不直接执行状态推进
///
/// NOTE:
/// Scanner and try_advance may generate duplicate intents.
/// Deduplication and idempotency MUST be guaranteed by Dispatcher / Worker.
/// This is a design choice to ensure simplicity and reliability in the scanner itself.
#[derive(Clone)]
pub struct ShadowScanner {
    ctx: &'static crate::context::Context,
    /// Scanner配置
    pub config: ScannerConfig,
    intent_tx: tokio::sync::mpsc::Sender<FeeIntent>,
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
        intent_tx: tokio::sync::mpsc::Sender<FeeIntent>,
        diagnose_tx: Option<DiagnoseEventSender>,
    ) -> Self {
        Self { ctx, config, intent_tx, diagnose_tx }
    }

    /// 执行一轮扫描
    pub async fn scan_round(&self) {
        let start = Instant::now();
        trace!("Starting fee shadow scan round");

        // 执行扫描逻辑：基于事实驱动
        // 推荐顺序：
        // - 正向推进（Ack / Build / Broadcast）
        // - 事实补齐（Recover / Receipt）
        // - 结果确认（ResAck）
        // 1. 交易确认 ACK
        // 2. 构建交易
        // 3. 广播交易
        // 4. 恢复交易
        // 5. 上传交易执行回执
        // 6. 发送交易结果 ACK
        for (stage, result) in [
            ("need_tx_ack", self.scan_need_tx_ack().await),
            ("can_build", self.scan_can_build().await),
            ("can_broadcast", self.scan_can_broadcast().await),
            ("need_recover", self.scan_need_recover().await),
            ("need_tx_exec_receipt_upload", self.scan_need_tx_exec_receipt_upload().await),
            ("confirmed_need_tx_res_ack", self.scan_confirmed_need_tx_res_ack().await),
        ] {
            if let Err(error) = result {
                error!(stage, %error, "Fee shadow scan stage failed");
            }
        }

        trace!("Fee shadow scan round completed in {:?}", start.elapsed());
    }

    /// 扫描需要发送交易确认 ACK 的记录
    ///
    /// 事实条件：
    /// - tx_ack_sent_at IS NULL
    ///
    /// 对应动作：
    /// - 生成SendTxAck意图
    ///
    /// ⚠️ 只看推进事实，不看行为事实：
    /// - tx_ack_sent_at IS NULL：尚未发送交易确认（推进事实）
    ///
    /// SQL must be equivalent to need_tx_ack()
    async fn scan_need_tx_ack(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning tx ack not sent records");

        // 查询DB中需要发送交易确认 ACK 的记录
        let records = ApiFeeRepo::scan_need_tx_ack(&pool, self.config.max_items_per_scan).await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found tx ack not sent records");

        // 生成推进意图
        for record in records {
            trace!(trade_no = %record.trade_no, "Queue tx ack send");
            let intent = FeeIntent::SideEffect(FeeSideEffectIntent::SendTxAck(record.trade_no));
            self.dispatch_intent(intent);
        }
        Ok(())
    }

    /// 扫描“允许构建 raw_tx”的记录
    ///
    /// 事实条件（强顺序屏障）：
    /// - tx_ack_sent_at IS NOT NULL   // 订单确认已完成
    /// - raw_tx IS NULL
    ///
    /// ⚠️ 设计说明：
    /// BuildTx 必须显式依赖 TxAck 完成，
    /// 禁止移除 tx_ack_sent_at 条件，否则会破坏强顺序保证。
    ///
    /// ⚠️ Scanner 不关心：
    /// - 为什么不能构建
    /// - 之前是否构建失败
    /// - 是否超时
    ///
    /// SQL must be equivalent to can_build()
    async fn scan_can_build(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning can build records");

        // 查询DB中可构建的记录
        let records = ApiFeeRepo::scan_can_build(&pool, self.config.max_items_per_scan).await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found can build records");

        // 生成推进意图
        for record in records {
            let intent = FeeIntent::Chain(FeeChainIntent::BuildTx(record.trade_no));
            self.dispatch_intent(intent);
        }
        Ok(())
    }

    /// 扫描“允许广播”的记录
    ///
    /// 事实条件：
    /// - raw_tx IS NOT NULL
    /// - last_broadcast_at IS NULL
    /// - transaction_time IS NULL
    /// - finished_at IS NULL
    ///
    /// SQL must be equivalent to can_broadcast()
    async fn scan_can_broadcast(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning can broadcast records");

        // 查询DB中可广播的记录
        let records = ApiFeeRepo::scan_can_broadcast(&pool, self.config.max_items_per_scan).await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found can broadcast records");

        let records: Vec<_> = records
            .into_iter()
            .filter(|record| {
                crate::infrastructure::api_trans::collect_fee::shadow::predicate::evaluate_point(
                    AdvancementPoint::CanBroadcast,
                    record,
                )
                .can_advance
            })
            .collect();

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
            let intent = FeeIntent::Chain(FeeChainIntent::BroadcastTx(record.trade_no));
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

    /// 扫描已确认但未发送TxRes ACK的记录
    ///
    /// 事实条件（强顺序屏障）：
    /// - transaction_time IS NOT NULL
    /// - tx_res_ack_sent_at IS NULL
    /// - finished_at IS NULL
    ///
    /// ⚠️ 设计说明：
    /// TxResAck 的唯一前提是“链上结果已确定”。
    /// 禁止前置条件：
    /// - 不检查 last_broadcast_at
    /// - 不检查 tx_exec_receipt_uploaded_at
    ///
    /// 对应动作：
    /// - 生成SendTxResAck意图
    ///
    /// SQL must be equivalent to need_tx_res_ack()
    async fn scan_confirmed_need_tx_res_ack(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning confirmed need tx res ACK records");

        // 查询DB中已确认但未发送TxRes ACK的记录
        let records =
            ApiFeeRepo::scan_confirmed_need_tx_res_ack(&pool, self.config.max_items_per_scan)
                .await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found confirmed need tx res ACK records");

        // 生成推进意图
        for record in records {
            let intent = FeeIntent::SideEffect(FeeSideEffectIntent::SendTxResAck(record.trade_no));
            self.dispatch_intent(intent);
        }
        Ok(())
    }

    /// 扫描需要上传交易执行回执的记录
    ///
    /// 事实条件：
    /// - transaction_time IS NOT NULL OR err_code IS NOT NULL
    /// - tx_exec_receipt_uploaded_at IS NULL
    ///
    /// 对应动作：
    /// - 生成UploadTxExecReceipt意图
    ///
    /// SQL must be equivalent to need_tx_exec_receipt_upload()
    async fn scan_need_tx_exec_receipt_upload(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning need tx exec receipt upload records");

        // 查询DB中需要上传交易执行回执的记录
        let records =
            ApiFeeRepo::scan_need_tx_exec_receipt_upload(&pool, self.config.max_items_per_scan)
                .await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found need tx exec receipt upload records");

        let records: Vec<_> = records
            .into_iter()
            .filter(|record| {
                crate::infrastructure::api_trans::collect_fee::shadow::predicate::evaluate_point(
                    AdvancementPoint::NeedTxExecReceiptUpload,
                    record,
                )
                .can_advance
            })
            .collect();

        // 生成推进意图
        for record in records {
            trace!(trade_no = %record.trade_no, "Queue tx exec receipt upload");
            let intent =
                FeeIntent::SideEffect(FeeSideEffectIntent::UploadTxExecReceipt(record.trade_no));
            self.dispatch_intent(intent);
        }
        Ok(())
    }

    /// 扫描需要恢复交易的记录
    ///
    /// 事实条件：
    /// - tx_hash IS NOT NULL
    /// - transaction_time IS NULL
    /// - tx_exec_receipt_uploaded_at IS NULL
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
        let records = ApiFeeRepo::scan_need_recover(&pool, self.config.max_items_per_scan).await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found need recover records");

        let records: Vec<_> = records
            .into_iter()
            .filter(|record| {
                crate::infrastructure::api_trans::collect_fee::shadow::predicate::evaluate_point(
                    AdvancementPoint::NeedRecover,
                    record,
                )
                .can_advance
            })
            .collect();

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
            let recover_key = shadow_rpc_policy::recover_dispatch_key(&record.trade_no);
            if !allow_fee_recover_dispatch(&recover_key, record.last_broadcast_at) {
                trace!(
                    trade_no = %record.trade_no,
                    cooldown = ?shadow_rpc_policy::recover_dispatch_cooldown(&recover_key),
                    "recover_skip_because_cooldown: fee safety scan skipped"
                );
                continue;
            }
            let intent = FeeIntent::Chain(FeeChainIntent::RecoverTx(record.trade_no));
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
    fn dispatch_intent(&self, intent: FeeIntent) {
        trace!(?intent, "Generated fee intent");

        // 将意图发送给Dispatcher（非阻塞；避免卡住 scanner loop）
        match self.intent_tx.try_send(intent) {
            Ok(_) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(intent))
            | Err(tokio::sync::mpsc::error::TrySendError::Closed(intent)) => {
                let trade_no = match &intent {
                    FeeIntent::Chain(FeeChainIntent::BuildTx(trade_no))
                    | FeeIntent::Chain(FeeChainIntent::BroadcastTx(trade_no))
                    | FeeIntent::Chain(FeeChainIntent::RecoverTx(trade_no))
                    | FeeIntent::SideEffect(FeeSideEffectIntent::SendTxAck(trade_no))
                    | FeeIntent::SideEffect(FeeSideEffectIntent::SendTxResAck(trade_no))
                    | FeeIntent::SideEffect(FeeSideEffectIntent::UploadTxExecReceipt(trade_no)) => {
                        trade_no.clone()
                    }
                };

                warn!(trade_no = %trade_no, ?intent, "Failed to dispatch fee intent");

                if let Some(tx) = &self.diagnose_tx {
                    let meta = DiagnoseMeta::new(
                        trade_no,
                        DiagnoseSource::Advancer,
                        DiagnoseStage::Unknown,
                    );
                    let _ = tx.try_send(DiagnoseEvent::IntentDispatchFailed { meta });
                }
            }
        }
    }

    /// 尝试基于当前事实推进一个阶段
    ///
    /// 注意：try_advance 每次最多推进一个阶段
    /// 多阶段推进依赖后续 Tick 或定时扫描
    ///
    /// 参数：
    /// - trade_no: 手续费交易编号
    ///
    /// 行为：
    /// 1. 查询最新的DB状态
    /// 2. 基于事实状态，按照 AdvancementPoint 顺序检查可推进点
    /// 3. 找到第一个满足条件的推进点，生成对应意图
    /// 4. 发送意图并返回
    ///
    /// TODO: extract to ShadowAdvancer
    /// Scanner should remain pure fact -> intent generator.
    /// try_advance performs one-shot advancement with side effects.
    ///
    /// 技术债：
    /// - try_advance 当前放在 ShadowScanner impl 中，语义上不够清晰
    /// - 未来理想形态：
    ///   - ShadowScanner: scan facts -> intents (只读)
    ///   - ShadowAdvancer: one-shot advance based on facts (可写)
    /// - 建议在 predicate 完全统一后进行重构
    pub async fn try_advance(&self, trade_no: &str) {
        if let Err(error) = self.try_advance_result(trade_no).await {
            error!(trade_no = %trade_no, %error, "Fee try_advance failed");
        }
    }

    async fn try_advance_result(&self, trade_no: &str) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(trade_no = %trade_no, "Try advancing fee transaction");

        // 查询最新的DB状态
        let fee = ApiFeeRepo::get_api_fee_by_trade_no(&pool, trade_no).await?;

        // 架构级保险丝：冻结或已终止的记录不允许推进
        if fee.finished_at.is_some() {
            trace!(trade_no = %trade_no, "Advance skipped: frozen or finished");
            return Ok(());
        }

        // err_code 冻结：只允许 UploadTxExecReceipt
        if fee.err_code.is_some() {
            let eval = evaluate_point(AdvancementPoint::NeedTxExecReceiptUpload, &fee);
            if eval.can_advance {
                trace!(trade_no = %trade_no, "Need to upload tx exec receipt (err_code frozen state)");
                let intent = FeeIntent::SideEffect(FeeSideEffectIntent::UploadTxExecReceipt(
                    trade_no.to_string(),
                ));
                self.dispatch_intent(intent);
            }
            return Ok(());
        }

        // 按照 ADVANCEMENT_ORDER 顺序检查可推进点
        // 顺序与 scan_round 完全一致，确保行为一致性
        for point in ADVANCEMENT_ORDER {
            let eval = evaluate_point(*point, &fee);
            if !eval.can_advance {
                continue;
            }

            match point {
                AdvancementPoint::NeedTxAck => {
                    trace!(trade_no = %trade_no, "Need to send tx ACK");
                    let intent =
                        FeeIntent::SideEffect(FeeSideEffectIntent::SendTxAck(trade_no.to_string()));
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::CanBuild => {
                    trace!(trade_no = %trade_no, "Can build transaction");
                    let intent = FeeIntent::Chain(FeeChainIntent::BuildTx(trade_no.to_string()));
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::CanBroadcast => {
                    if let Some((host, remaining)) =
                        shadow_rpc_policy::breaker_open_for_chain_code(self.ctx, &fee.chain_code)
                            .await
                    {
                        trace!(
                            trade_no = %trade_no,
                            chain_code = %fee.chain_code,
                            host = %host,
                            remaining = ?remaining,
                            "try_advance_skip_because_breaker_open: fee broadcast skipped"
                        );
                        if shadow_rpc_policy::should_emit_breaker_warn(&format!(
                            "fee.try_advance.breaker:{}:{}",
                            fee.chain_code, host
                        )) {
                            warn!(
                                trade_no = %trade_no,
                                chain_code = %fee.chain_code,
                                host = %host,
                                remaining = ?remaining,
                                "try_advance_skip_because_breaker_open: fee broadcast skipped"
                            );
                        }
                        return Ok(());
                    }
                    trace!(trade_no = %trade_no, "Can broadcast transaction");
                    let intent =
                        FeeIntent::Chain(FeeChainIntent::BroadcastTx(trade_no.to_string()));
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::NeedRecover => {
                    if let Some((host, remaining)) =
                        shadow_rpc_policy::breaker_open_for_chain_code(self.ctx, &fee.chain_code)
                            .await
                    {
                        trace!(
                            trade_no = %trade_no,
                            chain_code = %fee.chain_code,
                            host = %host,
                            remaining = ?remaining,
                            "try_advance_skip_because_breaker_open: fee recover skipped"
                        );
                        if shadow_rpc_policy::should_emit_breaker_warn(&format!(
                            "fee.try_advance.breaker:{}:{}",
                            fee.chain_code, host
                        )) {
                            warn!(
                                trade_no = %trade_no,
                                chain_code = %fee.chain_code,
                                host = %host,
                                remaining = ?remaining,
                                "try_advance_skip_because_breaker_open: fee recover skipped"
                            );
                        }
                        return Ok(());
                    }
                    let recover_key = shadow_rpc_policy::recover_dispatch_key(trade_no);
                    if !allow_fee_recover_dispatch(&recover_key, fee.last_broadcast_at) {
                        trace!(
                            trade_no = %trade_no,
                            cooldown = ?shadow_rpc_policy::recover_dispatch_cooldown(&recover_key),
                            "recover_skip_because_cooldown: fee recover skipped"
                        );
                        return Ok(());
                    }
                    trace!(trade_no = %trade_no, "Need to recover transaction");
                    let intent = FeeIntent::Chain(FeeChainIntent::RecoverTx(trade_no.to_string()));
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::NeedTxExecReceiptUpload => {
                    trace!(trade_no = %trade_no, "Need to upload tx exec receipt");
                    let intent = FeeIntent::SideEffect(FeeSideEffectIntent::UploadTxExecReceipt(
                        trade_no.to_string(),
                    ));
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::NeedTxResAck => {
                    trace!(trade_no = %trade_no, "Need to send tx res ACK");
                    let intent = FeeIntent::SideEffect(FeeSideEffectIntent::SendTxResAck(
                        trade_no.to_string(),
                    ));
                    self.dispatch_intent(intent);
                    return Ok(());
                }
                AdvancementPoint::FullyBlocked => {}
            };
        }

        // 无可用推进点
        trace!(trade_no = %trade_no, "No advancement possible based on current facts");
        let _ = maybe_log_stuck(
            &fee,
            &self.diagnose_tx,
            DiagnoseSource::ManualAdvance,
            DiagnoseStage::Unknown,
        );
        Ok(())
    }
}
