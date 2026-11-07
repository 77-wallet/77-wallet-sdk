// collect/shadow/scanner.rs
//
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
// - 行为中间态（attempted_at / retry_count / timeout）
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
//    - SendOrderAck / SendResultAck / SendTxFeeResAck
//    - UploadServiceFee
//
// 2. 【行为事实补齐副作用】（err_code 下允许）
//    - UploadTxExecReceipt
//
// 说明：
// - UploadTxExecReceipt 用于补齐“已发起链上执行”的事实
// - 无论成功失败都需要执行，确保行为事实完整性
// - 不属于“推进”，属于“事实补齐”

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
// 2. Scanner 不使用时间推断链上结果
// ----------------------------------------------------------------------------
//
// - 链上推进阶段禁止使用：
//   - now - xxx > duration
//   - xxx_at < yyy_at
//
// - 例外：订单执行租期由独立的 expiry scan 使用 created_at 明确判断；
//   该判断只允许终结从未生成 raw_tx / tx_hash / broadcast 事实的订单。
// - 其他时间字段唯一用途：
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
// 事实快照（ApiCollectEntity）
//        ↓
// 生成 CollectIntent
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
// - need_tx_fee_res_ack
// - need_result_ack
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
/// ApiCollect 事实模型与 Scanner 推进规则（最终版 · Rust 注释规范）
/// ============================================================================
///
/// 本文档用于**唯一权威地**定义 api_collect 表中各字段的事实语义、
/// 各阶段 Scanner 的判断条件，以及 Recover / MQTT / Broadcast 三种路径
/// 下**允许写入哪些字段**。
///
/// ⚠️ 设计目标：
/// - 消除“上链 / 确认 / 结果”等歧义词
/// - 明确区分【行为事实】与【链上结果事实】
/// - Scanner 只负责推进副作用，不制造链上事实
///
/// ============================================================================
/// 一、核心事实字段定义（最重要）
/// ============================================================================

/// last_broadcast_at
/// ---------------------------------------------------------------------------
/// 语义：
/// - 本系统已确认“至少发生过一次链上执行请求”
/// - 可能来自：
///   - SDK Broadcast 成功返回
///   - Recover 路径（由链上结果反推）
///
/// 性质：
/// - 【行为事实】
/// - 可补写（Recover 允许）
/// - 表示“我已尝试将交易发送到链上”
///
/// 性质：
/// - 【行为事实】
/// - 可重复（多次 broadcast 会刷新）
///
/// ⚠️ 严格约束：
/// - 不表示交易一定进入链
/// - 不表示链上已执行
/// - 不表示交易成功或失败
///
/// 允许写入者：
/// - Shadow Worker（Broadcast 成功后）
///

/// transaction_time
/// ---------------------------------------------------------------------------
/// 语义：
/// - 链上**执行结果已确定**的时间
/// - 结果可能是：成功 或 失败
///
/// 性质：
/// - 【链上结果事实】（不可逆）
/// - 只允许写入一次
///
/// ⚠️ 严格约束（铁律）：
/// - ❌ 不表示 broadcast 时间
/// - ❌ 不表示进入 mempool 的时间
/// - ❌ 不表示“可能成功”
/// - ✅ 只能在“已明确知道最终结果”时写入
///
/// 允许写入者：
/// - MQTT TxRes Handler（后端扫链后通知）
/// - Recover 路径（本地查 hash 得到最终结果后）
///

/// finished_at
/// ---------------------------------------------------------------------------
/// 语义：
/// - 本系统对该交易的生命周期已结束
/// - 不再推进任何 Scanner / Side-effect
///
/// 性质：
/// - 【系统终态事实】
///
/// ⚠️ 注意：
/// - finished_at ≠ 交易成功
/// - finished_at ≠ 交易失败
/// - 仅表示“我不再碰你了”
///
/// 写入时机：
/// - result_ack_sent_at 完成
/// - service_fee_uploaded_at（若需要）完成
/// - 不再存在任何未完成副作用
///
/// ⚠️ Scanner 不得在 finished_at IS NOT NULL 的记录上产生任何动作

/// err_code
/// ---------------------------------------------------------------------------
/// 语义：
/// - 是否发生过一次不可逆执行失败（build / broadcast）
///
/// 性质：
/// - 【终止型错误事实】
/// - NULL: 没有发生终止型错误
/// - NOT NULL: 发生过一次不可逆执行失败
///
/// ⚠️ 注意：
/// - err_code 不能有默认成功值，只能是 NULL 或具体错误码
/// - 0 这种“约定俗成的成功码”不是事实，是解释
/// - 一旦 err_code IS NOT NULL，该交易不得再进入执行路径
///
/// 写入时机：
/// - 执行路径发生不可恢复错误时
/// - 必须保证 building_at 或 last_broadcast_at 已经存在
///
/// ⚠️ Scanner 逻辑：
/// - err_code IS NOT NULL → 禁止：
///   - scan_can_build
///   - scan_can_broadcast
///   - scan_need_result_ack（成功）
///

/// ============================================================================
/// 二、已删除 / 合并的字段说明
/// ============================================================================

/// tx_res_received_at（已恢复）
/// ---------------------------------------------------------------------------
/// 原意：
/// - 记录 MQTT TxRes 到达时间
///
/// 恢复原因：
/// - MQTT TxRes（AWM_ORDER_TRANS_RES）到达 ≠ 链上结果已确定
/// - transaction_time 表示“链上已确认”，而 tx_res_received_at 表示“SER 推送已送达并被 SDK 持久化”
/// - TX_RES ACK 的语义必须锁死为“已收到并处理 SER 推送”，因此需要该事实作为强顺序屏障
///
/// 结论：
/// - ResultAck 发送必须同时满足：
///   - transaction_time IS NOT NULL（链事实）
///   - tx_res_received_at IS NOT NULL（SER 推送事实）
///
///
/// build_blocked_at（已删除）
/// ---------------------------------------------------------------------------
/// 原意：
/// - 构建流程在这里被阻断过一次
///
/// 删除原因：
/// - 语义错误：手续费不足不是暂时 blocked，而是构建失败的最终事实
/// - 与 need_service_fee 语义重复
/// - 可能被误用，绕过 need_service_fee 的判断
///
/// 结论：
/// - 使用 need_service_fee 作为唯一构建失败事实
///

/// ============================================================================
/// 三、Recover / MQTT / Broadcast 三条路径的职责边界
/// ============================================================================

/// Broadcast 路径（SDK 主动上链）
/// ---------------------------------------------------------------------------
/// - 成功返回 ≠ 链上成功
/// - 只写：last_broadcast_at
/// - 绝不写：transaction_time
///

/// MQTT TxRes 路径（后端扫链后通知 SDK）
/// ---------------------------------------------------------------------------
/// - 已包含最终结果（成功 / 失败）
/// - 写入：transaction_time / tx_hash / fee / resource
/// - 这是最常规、最可信的结果来源
///

/// Recover 路径（本地重启 / 丢失状态）
/// ---------------------------------------------------------------------------
/// - 通过 tx_hash 查询链上最终结果
/// - 目的：
///   1. 修复本地事实
///   2. 补发“我已上链”的通知给后端
/// - 若查到最终结果：允许写入 transaction_time
///
/// ⚠️ Recover 强规则：
/// - 若通过 tx_hash 查询到链上最终结果，
///   则可确定 broadcast 行为一定已经发生
/// - 即使本地未曾成功写入 last_broadcast_at，
///   Recover 也允许补写该字段
///
/// 目的：
/// - 修复不可逆事实缺失
/// - 保证后续 scan_need_tx_exec_receipt_upload 能正常推进
///

/// ============================================================================
/// 四、Scanner 各阶段的判断条件（事实 → 副作用）
/// ============================================================================

/// 1. scan_can_build
/// ---------------------------------------------------------------------------
/// raw_tx IS NULL
/// AND need_service_fee != true
///

/// 2. scan_can_broadcast
/// ---------------------------------------------------------------------------
/// raw_tx IS NOT NULL
/// AND last_broadcast_at IS NULL
/// AND finished_at IS NULL
/// AND (
///     service_fee_uploaded_at IS NULL
///     OR tx_fee_res_ack_sent_at IS NOT NULL
/// )
///
/// 语义：
/// - 当前周期未进入服务费上传的交易：可直接广播
/// - 当前周期已经进入服务费上传的交易：
///   必须先完成 TxFeeResAck，才能广播
///

/// 3. scan_need_tx_exec_receipt_upload
/// ---------------------------------------------------------------------------
/// 语义：通知后端“我已发起过链上执行请求”
///
/// 判断条件：
/// last_broadcast_at IS NOT NULL
/// AND tx_exec_receipt_uploaded_at IS NULL
///
/// ⚠️ 不关心 transaction_time
///

/// 4. scan_confirmed_need_result_ack
/// ---------------------------------------------------------------------------
/// 语义：将**链上最终结果**可靠告知后端
///
/// 判断条件：
/// transaction_time IS NOT NULL
/// AND result_ack_sent_at IS NULL
/// AND finished_at IS NULL
///
/// ⚠️ 禁止前置条件：
/// - 不检查 last_broadcast_at
/// - 不检查 tx_exec_receipt_uploaded_at
///
/// 原因：
/// - ResultAck 的唯一前提是“链上结果已确定”
/// - 行为事实缺失由 Recover 负责补齐

/// 5. scan_confirmed_need_service_fee_upload
/// ---------------------------------------------------------------------------
/// 语义：构建阶段发现手续费不足，告知后端
///
/// 判断条件：
/// need_service_fee = true
/// AND service_fee_uploaded_at IS NULL
///
/// ⚠️ 与链上执行 / MQTT / transaction_time 无关
///
/// 6. scan_confirmed_need_tx_fee_res_ack
/// ---------------------------------------------------------------------------
/// 语义：手续费问题已解决，需要发送结果确认 ACK
///
/// 判断条件：
/// need_service_fee != true
/// AND ever_needed_service_fee = true
/// AND tx_fee_res_ack_sent_at IS NULL
/// AND last_broadcast_at IS NULL
/// AND finished_at IS NULL
/// AND transaction_time IS NULL
///
/// ⚠️ 语义：
/// - 这是一个前置广播的条件性步骤
/// - 仅适用于当前周期已经进入过服务费上传的交易
/// - 一旦完成，允许进入广播阶段
/// - TxFeeResAck 的触发与 raw_tx 是否存在没有必然关系
/// - Ack 是"事实修复完成后的确认"，不是"构建完成后的副作用"
/// - 一旦链上结果已知（transaction_time IS NOT NULL），不得再发送 TxFeeResAck
///

/// ============================================================================
/// 五、最终原则总结（必须遵守）
/// ============================================================================
///
/// 1. last_broadcast_at = 行为事实（我发过）
/// 2. transaction_time = 链上结果事实（我知道结局了）
/// 3. Scanner 永远不制造链上事实，只消费事实
/// 4. 不允许存在两个字段表达"结果已确定"
///
/// ============================================================================  
/// 六、TxFeeResAck 规则
/// ============================================================================
///
/// - TxFeeResAck is a pre-broadcast conditional step.
/// - TxFeeResAck MUST NOT be sent if:
///   - transaction_time IS NOT NULL
///
/// Reason:
/// - Once chain result is known, any pre-broadcast acknowledgement
///   becomes meaningless and time-inconsistent.
///
/// 如果未来模型再次演进，必须先更新本注释，再允许写代码。
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
use std::fmt;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::Utc;
use tokio::sync::Semaphore;
use tracing::{error, info, trace, warn};
use wallet_database::{
    ApiResourceDbPool, ApiTransactionDbPool,
    entities::{
        api_collect::ApiCollectEntity,
        api_resource_delegation::{
            ApiResourceDelegationOperationType, ApiResourceDelegationSource,
        },
        api_trade_type::ApiTradeType,
    },
    repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo,
};

use crate::{
    domain::api_wallet::trans::shared_mechanism,
    error::service::ServiceError,
    infrastructure::api_trans::{
        collect::{
            diagnose::{DiagnoseEventSender, DiagnoseSource, DiagnoseStage, maybe_log_stuck},
            shadow::{
                ChainIntent, SideEffectIntent,
                stage::{COLLECT_ADVANCEMENT_ORDER, CollectStage},
            },
        },
        shadow_rpc_policy,
    },
};

use super::CollectIntent;

async fn dispatch_account_activation_execute_records(
    pool: &ApiResourceDbPool,
    limit: usize,
    intent_tx: &tokio::sync::mpsc::Sender<CollectIntent>,
) -> Result<(), ServiceError> {
    let records = wallet_database::repositories::api_wallet::tron_account_activation::ApiTronAccountActivationRepo::scan_can_execute(
        pool,
        limit,
    )
    .await?;

    for record in records {
        intent_tx
            .send(CollectIntent::Chain(ChainIntent::ExecuteAccountActivation(
                record.activation_trade_no,
            )))
            .await
            .map_err(|_| ServiceError::Parameter("collect intent channel closed".to_string()))?;
    }

    Ok(())
}

async fn dispatch_account_activation_verify_records(
    pool: &ApiResourceDbPool,
    limit: usize,
    intent_tx: &tokio::sync::mpsc::Sender<CollectIntent>,
) -> Result<(), ServiceError> {
    let records = wallet_database::repositories::api_wallet::tron_account_activation::ApiTronAccountActivationRepo::scan_can_verify(
        pool,
        limit,
    )
    .await?;

    for record in records {
        intent_tx
            .send(CollectIntent::Chain(ChainIntent::ExecuteAccountActivation(
                record.activation_trade_no,
            )))
            .await
            .map_err(|_| ServiceError::Parameter("collect intent channel closed".to_string()))?;
    }

    Ok(())
}

/// ============================================================================
///                            推进点枚举与共用 Predicate 函数
/// ============================================================================
///
/// 推进点枚举：统一 scan_round 和 try_advance 的顺序定义
/// - 顺序只定义一次，确保 scan_round 和 try_advance 使用相同的优先级
/// - 将来添加新阶段时，只需修改此枚举，不会遗漏任何一处
/// ============================================================================

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

/// 副作用类（Side Effect）predicate
/// ----------------------------------------------------------------------------

/// 检查是否需要发送结果 ACK
///
/// 事实条件：
/// - transaction_time IS NOT NULL
/// - result_ack_sent_at IS NULL
/// - finished_at IS NULL
/// - err_code IS NULL
///
/// ⚠️ 重要说明：
/// - ResultAck 仅用于“成功结果确认”
/// - 失败结果通过 err_code 事实本身表达，不再发送 ResultAck
/// - 一旦 err_code IS NOT NULL，不再产生 ResultAck 意图
fn need_result_ack(collect: &ApiCollectEntity) -> bool {
    collect.tx_res_received_at.is_some()
        && collect.transaction_time.is_some()
        && collect.result_ack_sent_at.is_none()
        && collect.finished_at.is_none()
        && collect.err_code.is_none()
}

/// 检查是否需要上传服务费
///
/// 事实条件：
/// - need_service_fee = true
/// - service_fee_uploaded_at IS NULL
/// - err_code IS NULL
/// - 非 TRON 链不依赖 resource gate；TRON 必须等待 resource gate 释放
///
/// ⚠️ 重要说明：
/// - UploadServiceFee 只在构建阶段的可恢复失败路径触发
/// - 一旦发生不可逆执行失败（err_code IS NOT NULL），不再允许上传服务费
fn need_service_fee_upload(collect: &ApiCollectEntity) -> bool {
    collect.need_service_fee == Some(true)
        && collect.service_fee_uploaded_at.is_none()
        && collect.err_code.is_none()
        && (!shared_mechanism::is_tron_chain(&collect.chain_code)
            || collect.resource_gate_released_at.is_some())
}

/// 检查是否需要发送手续费结果确认 ACK
///
/// 事实条件：
/// - need_service_fee != true
/// - ever_needed_service_fee = true
/// - tx_fee_res_ack_sent_at IS NULL
/// - last_broadcast_at IS NULL
/// - finished_at IS NULL
/// - transaction_time IS NULL
///
/// ⚠️ 注意：
/// - TxFeeResAck 的触发与 raw_tx 是否存在没有必然关系
/// - Ack 是"事实修复完成后的确认"，不是"构建完成后的副作用"
/// - 只要手续费问题已解决，就应该触发 Ack
/// - 一旦链上结果已知（transaction_time IS NOT NULL），不得再发送 TxFeeResAck
///
/// Reason:
/// - TxFeeResAck confirms resolution of a build-blocking fact,
///   not the existence or validity of raw_tx.
/// - raw_tx may or may not have been generated before the failure.
/// - Therefore raw_tx MUST NOT be part of the predicate.
fn need_tx_fee_res_ack(collect: &ApiCollectEntity) -> bool {
    collect.need_service_fee != Some(true)
        && collect.ever_needed_service_fee == true
        && collect.tx_fee_res_ack_sent_at.is_none()
        && collect.last_broadcast_at.is_none()
        && collect.finished_at.is_none()
        && collect.transaction_time.is_none()
        && collect.err_code.is_none()
}

/// 检查是否需要恢复交易
///
/// 事实条件：
/// - tx_hash IS NOT NULL
/// - transaction_time IS NULL
/// - tx_exec_receipt_uploaded_at IS NULL
/// - finished_at IS NULL
/// - err_code IS NULL
///
/// ⚠️ 重要说明：
/// - Recover 的目的是补全链上结果事实
/// - Broadcast 可见但结果未确认时，Recover 仍然负责补全链上结果事实
/// - 回执上传后禁止自动 Recover（避免与后端状态冲突）
/// - 只看不可逆事实是否缺失，不做时间推断
fn need_recover(collect: &ApiCollectEntity) -> bool {
    collect.tx_hash.is_some()
        && collect.transaction_time.is_none()
        && collect.tx_exec_receipt_uploaded_at.is_none()
        && collect.finished_at.is_none()
        && collect.err_code.is_none()
}

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
/// - ❌ 禁止用于 scan / try_advance predicate
/// - 仅允许被 Recover / Debug 使用
fn is_chain_finished(collect: &ApiCollectEntity) -> bool {
    collect.transaction_time.is_some()
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
            shadow_rpc_policy::read_u64_env("COLLECT_SHADOW_SCAN_INTERVAL_SECS", 30, 10, 120);
        let max_items_per_scan =
            shadow_rpc_policy::read_usize_env("COLLECT_SHADOW_MAX_ITEMS_PER_SCAN", 20, 10, 200);
        Self { scan_interval: Duration::from_secs(scan_interval_secs), max_items_per_scan }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::sync::mpsc;
    use wallet_database::{
        SqliteContext,
        entities::{
            api_collect::ApiCollectStatus,
            api_resource_delegation::{
                ApiResourceDelegationOperationType, ApiResourceDelegationResultStatus,
                ApiResourceDelegationSource, NewApiResourceDelegation,
            },
            api_resource_gate::ApiResourceGateResult,
            api_trade_type::ApiTradeType,
            api_tron_account_activation::NewApiTronAccountActivation,
        },
        repositories::api_wallet::{
            collect::ApiCollectRepo, resource_delegation::ApiResourceDelegationRepo,
            tron_account_activation::ApiTronAccountActivationRepo,
        },
    };

    async fn scanner_test_resource_pool(name: &str) -> wallet_database::ApiResourceDbPool {
        let dir = tempdir().expect("tempdir");
        let dir_path = dir.keep().join(name);
        std::fs::create_dir_all(&dir_path).expect("create scanner test dir");
        let resource_ctx = SqliteContext::new(&dir_path.to_string_lossy(), Some("api_resource.db"))
            .await
            .expect("init api_resource.db");
        resource_ctx.into_api_resource_db_pool().expect("resource pool")
    }

    #[tokio::test]
    async fn try_advance_prioritizes_collect_resource_result_ack_before_build() -> anyhow::Result<()>
    {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let pool = ctx.api_transaction_pool()?;
        let resource_pool = ctx.api_resource_pool()?;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let scanner = ShadowScanner::new(ctx, ScannerConfig::default(), intent_tx, None);

        ApiCollectRepo::upsert_api_collect(
            &pool,
            "uid_1",
            "collect",
            "from_addr",
            "to_addr",
            "1",
            "digest",
            "tron",
            None,
            "TRX",
            "C_pending_rsc_ack",
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE api_collect
            SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                resource_gate_released_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                resource_gate_result = ?
            WHERE trade_no = ?
            "#,
        )
        .bind(ApiResourceGateResult::PlatformDelegateSuccess.as_i64())
        .bind("C_pending_rsc_ack")
        .execute(pool.as_ref())
        .await?;

        ApiResourceDelegationRepo::upsert_original_order_result_fact(
            &resource_pool,
            NewApiResourceDelegation::platform_delegate(
                "uid_1",
                "C_pending_rsc_ack",
                "C_pending_rsc_ack",
                ApiTradeType::Collect as i64,
                "",
                "",
                "0",
            ),
            ApiResourceDelegationResultStatus::Success,
            None,
            Some(r#"{"tradeNo":"C_pending_rsc_ack","status":true}"#),
        )
        .await?;

        scanner.try_advance("C_pending_rsc_ack").await;

        let intent = intent_rx.try_recv().expect("resource ACK intent should be dispatched");
        assert!(matches!(
            intent,
            CollectIntent::SideEffect(SideEffectIntent::SendResourceResultAck(ref trade_no))
                if trade_no == "C_pending_rsc_ack"
        ));
        assert!(intent_rx.try_recv().is_err());

        Ok(())
    }

    #[tokio::test]
    async fn collect_scanner_dispatches_broadcasted_local_delegate_recover() -> anyhow::Result<()> {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let resource_pool = ctx.api_resource_pool()?;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let scanner = ShadowScanner::new(ctx, ScannerConfig::default(), intent_tx, None);
        let resource_trade_no = "rsc_local_delegate_C_recover_scan";

        ApiResourceDelegationRepo::upsert(
            &resource_pool,
            NewApiResourceDelegation::local_delegate(
                "uid_1",
                resource_trade_no,
                "C_recover_scan",
                ApiTradeType::Collect as i64,
                "owner_addr",
                "receiver_addr",
                "100",
                "1000",
            ),
        )
        .await?;
        assert_eq!(
            ApiResourceDelegationRepo::claim_build_slot(&resource_pool, resource_trade_no).await?,
            1
        );
        assert_eq!(
            ApiResourceDelegationRepo::mark_built(
                &resource_pool,
                resource_trade_no,
                "tx_hash_recover_scan",
                r#"{"raw":"tx"}"#,
            )
            .await?,
            1
        );
        assert_eq!(
            ApiResourceDelegationRepo::mark_broadcasted(
                &resource_pool,
                resource_trade_no,
                "tx_hash_recover_scan",
            )
            .await?,
            1
        );

        scanner.scan_can_resource_delegation_recover().await?;

        let mut dispatched = Vec::new();
        while let Ok(intent) = intent_rx.try_recv() {
            if let CollectIntent::Chain(ChainIntent::ExecuteLocalResourceDelegation(trade_no)) =
                intent
            {
                dispatched.push(trade_no);
            }
        }

        assert!(
            dispatched.iter().any(|trade_no| trade_no == resource_trade_no),
            "broadcasted local delegate should be dispatched for recover; got {dispatched:?}"
        );

        let executable =
            ApiResourceDelegationRepo::scan_can_execute_for_origin_type_source_and_operation(
                &resource_pool,
                ApiTradeType::Collect as i64,
                ApiResourceDelegationSource::Local,
                ApiResourceDelegationOperationType::Delegate,
                100,
            )
            .await?;
        assert!(
            !executable.iter().any(|row| row.resource_trade_no == resource_trade_no),
            "broadcasted local delegate should not be treated as pre-broadcast executable"
        );

        Ok(())
    }

    #[tokio::test]
    async fn scan_account_activation_emits_execute_and_verify_intents() -> anyhow::Result<()> {
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let resource_pool = scanner_test_resource_pool("collect_scan_account_activation").await;

        ApiTronAccountActivationRepo::upsert(
            &resource_pool,
            NewApiTronAccountActivation::collect(
                "uid_1",
                "C_activation_execute",
                "act_C_activation_execute",
                "T_payer",
                "T_inactive_execute",
            ),
        )
        .await?;
        ApiTronAccountActivationRepo::upsert(
            &resource_pool,
            NewApiTronAccountActivation::collect(
                "uid_1",
                "C_activation_verify",
                "act_C_activation_verify",
                "T_payer",
                "T_inactive_verify",
            ),
        )
        .await?;
        assert_eq!(
            ApiTronAccountActivationRepo::claim_build_slot(
                &resource_pool,
                "act_C_activation_verify",
            )
            .await?,
            1
        );
        assert_eq!(
            ApiTronAccountActivationRepo::mark_broadcast_success(
                &resource_pool,
                "act_C_activation_verify",
                "tx_hash_activation_verify",
                r#"{"raw":"tx"}"#,
            )
            .await?,
            1
        );

        dispatch_account_activation_execute_records(&resource_pool, 100, &intent_tx).await?;
        dispatch_account_activation_verify_records(&resource_pool, 100, &intent_tx).await?;

        let mut trade_nos = Vec::new();
        while let Ok(intent) = intent_rx.try_recv() {
            if let CollectIntent::Chain(ChainIntent::ExecuteAccountActivation(trade_no)) = intent {
                trade_nos.push(trade_no);
            }
        }
        trade_nos.sort();

        assert_eq!(
            trade_nos,
            vec!["act_C_activation_execute".to_string(), "act_C_activation_verify".to_string(),]
        );

        Ok(())
    }

    #[tokio::test]
    async fn scan_account_activation_skips_verified_tasks() -> anyhow::Result<()> {
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let resource_pool =
            scanner_test_resource_pool("collect_scan_account_activation_verified").await;

        ApiTronAccountActivationRepo::upsert(
            &resource_pool,
            NewApiTronAccountActivation::collect(
                "uid_1",
                "C_activation_verified",
                "act_C_activation_verified",
                "T_payer",
                "T_inactive_verified",
            ),
        )
        .await?;
        assert_eq!(
            ApiTronAccountActivationRepo::mark_account_verified(
                &resource_pool,
                "act_C_activation_verified",
            )
            .await?,
            1
        );

        dispatch_account_activation_execute_records(&resource_pool, 100, &intent_tx).await?;
        dispatch_account_activation_verify_records(&resource_pool, 100, &intent_tx).await?;

        while let Ok(intent) = intent_rx.try_recv() {
            assert!(
                !matches!(intent, CollectIntent::Chain(ChainIntent::ExecuteAccountActivation(_))),
                "verified activation task must not dispatch: {intent:?}"
            );
        }

        Ok(())
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
    intent_tx: tokio::sync::mpsc::Sender<CollectIntent>,
    diagnose_tx: Option<DiagnoseEventSender>,
    /// 扫描执行锁，防止 scan_round 并发执行
    scan_guard: Arc<Semaphore>,
}

impl fmt::Debug for ShadowScanner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShadowScanner").finish()
    }
}

impl ShadowScanner {
    const UNEXECUTED_ORDER_TTL_HOURS: i64 = 24;
    pub fn new(
        ctx: &'static crate::context::Context,
        config: ScannerConfig,
        intent_tx: tokio::sync::mpsc::Sender<CollectIntent>,
        diagnose_tx: Option<DiagnoseEventSender>,
    ) -> Self {
        Self { ctx, config, intent_tx, diagnose_tx, scan_guard: Arc::new(Semaphore::new(1)) }
    }

    pub fn with_diagnose_tx(mut self, diagnose_tx: DiagnoseEventSender) -> Self {
        self.diagnose_tx = Some(diagnose_tx);
        self
    }

    /// 执行一轮扫描
    pub async fn scan_round(&self) {
        // 尝试获取扫描执行锁，如果获取失败则直接返回，避免并发执行
        let permit = match self.scan_guard.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                trace!("Scan round skipped due to concurrent execution");
                return;
            }
        };

        let start = Instant::now();
        trace!("Starting collect shadow scan round");

        match self.scan_unexecuted_expired().await {
            Ok(expired_count) if expired_count > 0 => {
                trace!(expired_count, "Expiry intents dispatched; deferring normal advancement");
                drop(permit);
                return;
            }
            Ok(_) => {}
            Err(error) => {
                error!(stage = "unexecuted_expiry", %error, "Collect shadow scan stage failed");
            }
        }

        // 资源结果 ACK 是后端允许原单继续推进的门槛。
        // 如果先扫 BuildTx/UploadServiceFee，可能出现“先申请手续费、后 ACK 资源结果”的乱序。
        if let Err(error) = self.scan_need_resource_result_ack().await {
            error!(stage = "need_resource_result_ack", %error, "Collect shadow scan stage failed");
        }

        // 按推进顺序执行扫描，确保与推进顺序完全一致
        for stage in COLLECT_ADVANCEMENT_ORDER {
            if let Err(error) = self.scan_stage(*stage).await {
                error!(?stage, %error, "Collect shadow scan stage failed");
            }
        }
        for (stage, result) in [
            ("need_resource_task_ack", self.scan_need_resource_task_ack().await),
            ("can_resource_delegation_execute", self.scan_can_resource_delegation_execute().await),
            ("can_resource_delegation_recover", self.scan_can_resource_delegation_recover().await),
            ("can_account_activation_execute", self.scan_can_account_activation_execute().await),
            ("can_account_activation_verify", self.scan_can_account_activation_verify().await),
            (
                "need_resource_tx_exec_receipt_upload",
                self.scan_need_resource_tx_exec_receipt_upload().await,
            ),
        ] {
            if let Err(error) = result {
                error!(stage, %error, "Collect shadow scan stage failed");
            }
        }
        // collect shadow 只保留商户本地代理 fallback；平台代理由后端执行。

        trace!(elapsed = ?start.elapsed(), "Collect shadow scan round completed");

        // 许可证会在这里自动释放
        drop(permit);
    }

    async fn scan_unexecuted_expired(&self) -> Result<usize, ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        let cutoff = Utc::now() - chrono::Duration::hours(Self::UNEXECUTED_ORDER_TTL_HOURS);
        let records =
            wallet_database::repositories::api_wallet::collect::ApiCollectRepo::scan_unexecuted_created_before(
                &pool,
                cutoff,
                self.config.max_items_per_scan,
            )
            .await?;

        let record_count = records.len();
        for record in records {
            self.dispatch_intent(CollectIntent::Chain(ChainIntent::ExpireUnexecuted(
                record.trade_no,
            )))
            .await;
        }

        Ok(record_count)
    }

    /// 根据阶段执行扫描
    async fn scan_stage(&self, stage: CollectStage) -> Result<(), ServiceError> {
        match stage {
            CollectStage::NeedOrderAck => {
                self.scan_order_ack_not_sent().await?;
            }
            CollectStage::NeedResourceGate => {
                self.scan_need_resource_gate().await?;
            }
            CollectStage::CanBuild => {
                self.scan_can_build().await?;
            }
            CollectStage::NeedTxFeeResAck => {
                self.scan_confirmed_need_tx_fee_res_ack().await?;
            }
            CollectStage::CanBroadcast => {
                self.scan_can_broadcast().await?;
            }
            CollectStage::NeedRecover => {
                self.scan_need_recover().await?;
            }
            CollectStage::NeedTxExecReceiptUpload => {
                self.scan_need_tx_exec_receipt_upload().await?;
            }
            CollectStage::NeedResultAck => {
                self.scan_confirmed_need_result_ack().await?;
            }
            CollectStage::NeedServiceFeeUpload => {
                self.scan_confirmed_need_service_fee_upload().await?;
            }
            CollectStage::FullyBlocked => {
                // 完全阻塞的阶段不需要扫描
            }
        }
        Ok(())
    }

    async fn scan_need_resource_result_ack(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;

        trace!(
            max_items = %self.config.max_items_per_scan,
            "Scanning resource result ACK records"
        );

        let records = wallet_database::repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo::scan_need_result_ack_for_origin_type(
            &pool,
            wallet_database::entities::api_trade_type::ApiTradeType::Collect as i64,
            self.config.max_items_per_scan,
        ).await?;

        for record in records {
            let intent = CollectIntent::SideEffect(SideEffectIntent::SendResourceResultAck(
                record.resource_trade_no,
            ));
            self.dispatch_intent(intent).await;
        }

        Ok(())
    }

    async fn scan_need_resource_task_ack(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;

        trace!(
            max_items = %self.config.max_items_per_scan,
            "Scanning resource task ACK records"
        );

        let records = wallet_database::repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo::scan_need_task_ack_for_origin_type(
            &pool,
            wallet_database::entities::api_trade_type::ApiTradeType::Collect as i64,
            self.config.max_items_per_scan,
        ).await?;

        for record in records {
            let intent = CollectIntent::SideEffect(SideEffectIntent::SendResourceTaskAck(
                record.resource_trade_no,
            ));
            self.dispatch_intent(intent).await;
        }

        Ok(())
    }

    async fn scan_can_resource_delegation_execute(&self) -> Result<(), ServiceError> {
        self.scan_can_local_resource_delegate().await
    }

    async fn scan_can_resource_delegation_recover(&self) -> Result<(), ServiceError> {
        self.scan_can_local_resource_delegate_recover().await
    }

    async fn scan_can_local_resource_delegate(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning executable local delegate records");

        let records = wallet_database::repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo::scan_can_execute_for_origin_type_source_and_operation(
            &pool,
            ApiTradeType::Collect as i64,
            ApiResourceDelegationSource::Local,
            ApiResourceDelegationOperationType::Delegate,
            self.config.max_items_per_scan,
        ).await?;

        for record in records {
            let intent = CollectIntent::Chain(ChainIntent::ExecuteLocalResourceDelegation(
                record.resource_trade_no,
            ));
            self.dispatch_intent(intent).await;
        }

        Ok(())
    }

    async fn scan_can_local_resource_delegate_recover(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning recoverable local delegate records");

        let historical_failures =
            ApiResourceDelegationRepo::scan_unreconciled_terminal_broadcasts_for_recovery(
                &pool,
                ApiTradeType::Collect as i64,
                ApiResourceDelegationOperationType::Delegate,
                self.config.max_items_per_scan,
            )
            .await?;
        for record in historical_failures {
            let reopened = ApiResourceDelegationRepo::reopen_terminal_broadcast_for_reconciliation(
                &pool,
                &record.resource_trade_no,
            )
            .await?;
            if reopened > 0 {
                info!(
                    resource_trade_no = %record.resource_trade_no,
                    origin_trade_no = ?record.origin_trade_no,
                    previous_err_code = ?record.err_code,
                    "Reopened historical terminal local delegation for chain reconciliation"
                );
            }
        }

        let records =
            ApiResourceDelegationRepo::scan_can_recover_by_origin_type_source_and_operation(
                &pool,
                ApiTradeType::Collect as i64,
                ApiResourceDelegationSource::Local,
                ApiResourceDelegationOperationType::Delegate,
                self.config.max_items_per_scan,
            )
            .await?;

        for record in records {
            let intent = CollectIntent::Chain(ChainIntent::ExecuteLocalResourceDelegation(
                record.resource_trade_no,
            ));
            self.dispatch_intent(intent).await;
        }

        Ok(())
    }

    async fn scan_can_account_activation_execute(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning executable account activation records");

        dispatch_account_activation_execute_records(
            &pool,
            self.config.max_items_per_scan,
            &self.intent_tx,
        )
        .await
    }

    async fn scan_can_account_activation_verify(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning verifiable account activation records");

        dispatch_account_activation_verify_records(
            &pool,
            self.config.max_items_per_scan,
            &self.intent_tx,
        )
        .await
    }

    async fn scan_need_resource_tx_exec_receipt_upload(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_resource_pool()?;

        trace!(
            max_items = %self.config.max_items_per_scan,
            "Scanning resource tx exec receipt upload records"
        );

        let records = wallet_database::repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo::scan_need_tx_exec_receipt_upload_for_origin_type(
            &pool,
            wallet_database::entities::api_trade_type::ApiTradeType::Collect as i64,
            self.config.max_items_per_scan,
        ).await?;

        for record in records {
            let intent = CollectIntent::SideEffect(SideEffectIntent::UploadResourceTxExecReceipt(
                record.resource_trade_no,
            ));
            self.dispatch_intent(intent).await;
        }

        Ok(())
    }

    /// 扫描“允许构建 raw_tx”的交易
    async fn scan_need_resource_gate(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning resource gate records");

        let records = wallet_database::repositories::api_wallet::collect::ApiCollectRepo::scan_need_resource_gate(
            &pool,
            self.config.max_items_per_scan,
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
            let intent = CollectIntent::Chain(ChainIntent::EvalResourceGate(record.trade_no));
            self.dispatch_intent(intent).await;
        }

        if skipped > 0 {
            if let Some((host, remaining)) = first_skip {
                warn!(
                    skipped = skipped,
                    host = %host,
                    remaining = ?remaining,
                    "chain rpc circuit breaker open; skipped some resource gate intents"
                );
            } else {
                warn!(
                    skipped = skipped,
                    "chain rpc circuit breaker open; skipped some resource gate intents"
                );
            }
        }

        Ok(())
    }

    /// 扫描“允许构建 raw_tx”的交易
    ///
    /// 事实条件（强顺序屏障）：
    /// - order_ack_sent_at IS NOT NULL   // 订单确认已完成
    /// - raw_tx IS NULL
    /// - need_service_fee != true        // 不需要服务费补充
    /// - 如果曾经缺过手续费，则必须先完成 TxFeeResAck
    ///
    /// ⚠️ 设计说明：
    /// BuildTx 必须显式依赖 OrderAck 完成，
    /// 禁止移除 order_ack_sent_at 条件，否则会破坏强顺序保证。
    /// 如果曾经缺过手续费，则必须先发送 TxFeeResAck。
    ///
    /// ⚠️ 铁律：
    /// - BuildTx 必须严格发生在 OrderAck 之后
    /// - 任何试图移除 order_ack_sent_at 条件的修改
    ///   都是架构级破坏，必须被拒绝
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
        let records =
            wallet_database::repositories::api_wallet::collect::ApiCollectRepo::scan_can_build(
                &pool,
                self.config.max_items_per_scan,
            )
            .await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found can build records");

        // 生成推进意图
        for record in records {
            let intent = CollectIntent::Chain(ChainIntent::BuildTx(record.trade_no));
            self.dispatch_intent(intent).await;
        }

        Ok(())
    }

    /// 扫描“允许广播”的交易
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
        let records =
            wallet_database::repositories::api_wallet::collect::ApiCollectRepo::scan_can_broadcast(
                &pool,
                self.config.max_items_per_scan,
            )
            .await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found can broadcast records");

        let records: Vec<_> = records
            .into_iter()
            .filter(|record| {
                crate::infrastructure::api_trans::collect::shadow::predicate::evaluate_stage(
                    CollectStage::CanBroadcast,
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
            let intent = CollectIntent::Chain(ChainIntent::BroadcastTx(record.trade_no));
            self.dispatch_intent(intent).await;
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
    /// - result_ack_sent_at IS NULL
    /// - finished_at IS NULL
    ///
    /// ⚠️ 设计说明：
    /// ResultAck 的唯一前提是“链上结果已确定”。
    /// 禁止前置条件：
    /// - 不检查 last_broadcast_at
    /// - 不检查 tx_exec_receipt_uploaded_at
    ///
    /// 对应动作：
    /// - 生成SendResultAck意图
    ///
    /// SQL must be equivalent to need_result_ack()
    async fn scan_confirmed_need_result_ack(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning confirmed need result ACK records");

        // 查询DB中已确认但未发送TxRes ACK的记录
        let records = wallet_database::repositories::api_wallet::collect::ApiCollectRepo::scan_confirmed_need_result_ack(
            &pool,
            self.config.max_items_per_scan,
        ).await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found confirmed need result ACK records");

        // 生成推进意图
        for record in records {
            let intent =
                CollectIntent::SideEffect(SideEffectIntent::SendResultAck(record.trade_no));
            self.dispatch_intent(intent).await;
        }

        Ok(())
    }

    /// 扫描需要上传服务费的交易
    ///
    /// 事实条件：
    /// - need_service_fee = true
    /// - service_fee_uploaded_at IS NULL
    ///
    async fn scan_confirmed_need_service_fee_upload(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(
            max_items = %self.config.max_items_per_scan,
            "Scanning confirmed need service fee upload records"
        );

        let records = wallet_database::repositories::api_wallet::collect::ApiCollectRepo::scan_confirmed_need_service_fee_upload(
            &pool,
            self.config.max_items_per_scan,
        ).await?;

        let original_count = records.len();
        trace!(found = %original_count, "Found confirmed need service fee upload records");

        for record in records {
            let intent =
                CollectIntent::SideEffect(SideEffectIntent::UploadServiceFee(record.trade_no));
            self.dispatch_intent(intent).await;
        }

        Ok(())
    }

    /// 扫描需要发送手续费结果确认 ACK 的交易
    ///
    /// 事实条件：
    /// - need_service_fee != true
    /// - ever_needed_service_fee = true
    /// - tx_fee_res_ack_sent_at IS NULL
    /// - last_broadcast_at IS NULL
    /// - finished_at IS NULL
    /// - transaction_time IS NULL
    ///
    /// 对应动作：
    /// - 生成SendTxFeeResAck意图
    ///
    /// SQL must be equivalent to need_tx_fee_res_ack()
    async fn scan_confirmed_need_tx_fee_res_ack(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning need tx fee res ack records");

        // 查询DB中需要发送手续费结果确认 ACK 的记录
        let records = wallet_database::repositories::api_wallet::collect::ApiCollectRepo::scan_confirmed_need_tx_fee_res_ack(
            &pool,
            self.config.max_items_per_scan,
        ).await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found confirmed need tx fee res ack records");

        // 生成推进意图
        for record in records {
            let intent =
                CollectIntent::SideEffect(SideEffectIntent::SendTxFeeResAck(record.trade_no));
            self.dispatch_intent(intent).await;
        }

        Ok(())
    }

    /// 扫描需要上传交易执行回执的交易
    ///
    /// 事实条件：
    /// - transaction_time IS NOT NULL
    /// - err_code IS NOT NULL
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
        let records = wallet_database::repositories::api_wallet::collect::ApiCollectRepo::scan_need_tx_exec_receipt_upload(
            &pool,
            self.config.max_items_per_scan,
        ).await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found need tx exec receipt upload records");

        let records: Vec<_> = records
            .into_iter()
            .filter(|record| {
                crate::infrastructure::api_trans::collect::shadow::predicate::evaluate_stage(
                    CollectStage::NeedTxExecReceiptUpload,
                    record,
                )
                .can_advance
            })
            .collect();

        // 生成推进意图
        for record in records {
            trace!(trade_no = %record.trade_no, "Queue tx exec receipt upload");
            let intent =
                CollectIntent::SideEffect(SideEffectIntent::UploadTxExecReceipt(record.trade_no));
            self.dispatch_intent(intent).await;
        }

        Ok(())
    }

    /// 扫描需要发送订单确认 ACK 的交易
    ///
    /// 事实条件：
    /// - order_ack_sent_at IS NULL
    ///
    /// 对应动作：
    /// - 生成SendOrderAck意图
    ///
    /// ⚠️ 只看推进事实，不看行为事实：
    /// - order_ack_sent_at IS NULL：尚未发送订单确认（推进事实）
    ///
    /// ❌ 不依赖 attempted 行为中间态（仅基于推进事实判断）
    ///
    /// SQL must be equivalent to need_order_ack()
    async fn scan_order_ack_not_sent(&self) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(max_items = %self.config.max_items_per_scan, "Scanning order ack not sent records");

        // 查询DB中需要发送订单确认 ACK 的记录
        let records = wallet_database::repositories::api_wallet::collect::ApiCollectRepo::scan_need_order_ack(
            &pool,
            self.config.max_items_per_scan,
        ).await?;

        // 保存原始记录数
        let original_count = records.len();
        trace!(found = %original_count, "Found order ack not sent records");

        // 生成推进意图
        for record in records {
            trace!(trade_no = %record.trade_no, "Queue order ack send");
            let intent = CollectIntent::SideEffect(SideEffectIntent::SendOrderAck(record.trade_no));
            self.dispatch_intent(intent).await;
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
        let mut records =
            wallet_database::repositories::api_wallet::collect::ApiCollectRepo::scan_need_recover(
                &pool,
                self.config.max_items_per_scan,
            )
            .await?;

        let linked=wallet_database::repositories::api_wallet::collect_replacement::CollectReplacementRepo::pending_orders(&pool,self.config.max_items_per_scan).await?;
        for row in linked {
            if !records.iter().any(|r| r.trade_no == row.trade_no) {
                records.push(row);
            }
        }

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
            let recover_key = shadow_rpc_policy::recover_dispatch_key(&record.trade_no);
            if !shadow_rpc_policy::allow_broadcast_recover_dispatch(
                &recover_key,
                record.last_broadcast_at,
            ) {
                trace!(
                    trade_no = %record.trade_no,
                    cooldown = ?shadow_rpc_policy::recover_dispatch_cooldown(&recover_key),
                    "recover_skip_because_cooldown: collect safety scan skipped"
                );
                continue;
            }
            let intent = CollectIntent::Chain(ChainIntent::RecoverTx(record.trade_no));
            self.dispatch_intent(intent).await;
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
    async fn dispatch_intent(&self, intent: CollectIntent) {
        trace!(?intent, "Generated collect intent");

        // 将意图发送给Dispatcher
        if let Err(e) = self.intent_tx.send(intent).await {
            warn!("Failed to send collect intent: {}", e);
        }
    }

    /// 尝试基于当前事实推进一个阶段
    ///
    /// 注意：try_advance 每次最多推进一个阶段
    /// 多阶段推进依赖后续 Tick 或定时扫描
    ///
    /// 参数：
    /// - trade_no: 归集交易编号
    ///
    /// 行为：
    /// 1. 查询最新的DB状态
    /// 2. 基于事实状态，按照 ADVANCEMENT_ORDER 顺序检查可推进点
    /// 3. 找到第一个满足条件的推进点，生成对应意图
    /// 4. 发送意图并返回
    pub async fn try_advance(&self, trade_no: &str) {
        if let Err(error) = self.try_advance_result(trade_no).await {
            error!(trade_no = %trade_no, %error, "Collect try_advance failed");
        }
    }

    async fn try_advance_result(&self, trade_no: &str) -> Result<(), ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;

        trace!(trade_no = %trade_no, "Try advancing collect transaction");

        // 查询最新的DB状态
        let collect = wallet_database::repositories::api_wallet::collect::ApiCollectRepo::get_api_collect_by_trade_no(&pool, trade_no).await?;

        // 架构级保险丝：冻结或已终止的记录不允许推进
        if collect.finished_at.is_some() {
            trace!(trade_no = %trade_no, "Advance skipped: frozen or finished");
            return Ok(());
        }

        match self.pending_resource_result_ack_trade_no(trade_no).await {
            Ok(Some(resource_trade_no)) => {
                trace!(
                    trade_no = %trade_no,
                    resource_trade_no = %resource_trade_no,
                    "Resource result ACK is pending; advancing ACK before collect main chain"
                );
                self.dispatch_intent(CollectIntent::SideEffect(
                    SideEffectIntent::SendResourceResultAck(resource_trade_no),
                ))
                .await;
                return Ok(());
            }
            Ok(None) => {}
            Err(e) => {
                error!(
                    trade_no = %trade_no,
                    error = %e,
                    "Failed to check pending collect resource result ACK"
                );
                return Ok(());
            }
        }

        // err_code 冻结：只允许 UploadTxExecReceipt
        if collect.err_code.is_some() {
            let eval = crate::infrastructure::api_trans::collect::shadow::predicate::evaluate_stage(
                CollectStage::NeedTxExecReceiptUpload,
                &collect,
            );

            if eval.can_advance {
                trace!(trade_no = %trade_no, "Need to upload tx exec receipt (err_code frozen state)");
                let intent = CollectIntent::SideEffect(SideEffectIntent::UploadTxExecReceipt(
                    trade_no.to_string(),
                ));
                self.dispatch_intent(intent).await;
            }
            return Ok(());
        }

        // 按照 COLLECT_ADVANCEMENT_ORDER 顺序检查可推进点
        // 顺序与 scan_round 完全一致，确保行为一致性
        for stage in COLLECT_ADVANCEMENT_ORDER.iter() {
            let eval = crate::infrastructure::api_trans::collect::shadow::predicate::evaluate_stage(
                *stage, &collect,
            );

            if eval.can_advance {
                match stage {
                    CollectStage::NeedOrderAck => {
                        trace!(trade_no = %trade_no, "Need to send order ACK");
                        let intent = CollectIntent::SideEffect(SideEffectIntent::SendOrderAck(
                            trade_no.to_string(),
                        ));
                        self.dispatch_intent(intent).await;
                        return Ok(());
                    }
                    CollectStage::NeedResourceGate => {
                        trace!(trade_no = %trade_no, "Need to eval resource gate");
                        let intent = CollectIntent::Chain(ChainIntent::EvalResourceGate(
                            trade_no.to_string(),
                        ));
                        self.dispatch_intent(intent).await;
                        return Ok(());
                    }
                    CollectStage::CanBuild => {
                        trace!(trade_no = %trade_no, "Can build transaction");
                        let intent =
                            CollectIntent::Chain(ChainIntent::BuildTx(trade_no.to_string()));
                        self.dispatch_intent(intent).await;
                        return Ok(());
                    }
                    CollectStage::NeedTxFeeResAck => {
                        trace!(trade_no = %trade_no, "Need to send tx fee res ACK");
                        let intent = CollectIntent::SideEffect(SideEffectIntent::SendTxFeeResAck(
                            trade_no.to_string(),
                        ));
                        self.dispatch_intent(intent).await;
                        return Ok(());
                    }
                    CollectStage::CanBroadcast => {
                        if let Some((host, remaining)) =
                            shadow_rpc_policy::breaker_open_for_chain_code(
                                self.ctx,
                                &collect.chain_code,
                            )
                            .await
                        {
                            trace!(
                                trade_no = %trade_no,
                                chain_code = %collect.chain_code,
                                host = %host,
                                remaining = ?remaining,
                                "try_advance_skip_because_breaker_open: collect broadcast skipped"
                            );
                            if shadow_rpc_policy::should_emit_breaker_warn(&format!(
                                "collect.try_advance.breaker:{}:{}",
                                collect.chain_code, host
                            )) {
                                warn!(
                                    trade_no = %trade_no,
                                    chain_code = %collect.chain_code,
                                    host = %host,
                                    remaining = ?remaining,
                                    "try_advance_skip_because_breaker_open: collect broadcast skipped"
                                );
                            }
                            return Ok(());
                        }
                        trace!(trade_no = %trade_no, "Can broadcast transaction");
                        let intent =
                            CollectIntent::Chain(ChainIntent::BroadcastTx(trade_no.to_string()));
                        self.dispatch_intent(intent).await;
                        return Ok(());
                    }
                    CollectStage::NeedRecover => {
                        if let Some((host, remaining)) =
                            shadow_rpc_policy::breaker_open_for_chain_code(
                                self.ctx,
                                &collect.chain_code,
                            )
                            .await
                        {
                            trace!(
                                trade_no = %trade_no,
                                chain_code = %collect.chain_code,
                                host = %host,
                                remaining = ?remaining,
                                "try_advance_skip_because_breaker_open: collect recover skipped"
                            );
                            if shadow_rpc_policy::should_emit_breaker_warn(&format!(
                                "collect.try_advance.breaker:{}:{}",
                                collect.chain_code, host
                            )) {
                                warn!(
                                    trade_no = %trade_no,
                                    chain_code = %collect.chain_code,
                                    host = %host,
                                    remaining = ?remaining,
                                    "try_advance_skip_because_breaker_open: collect recover skipped"
                                );
                            }
                            return Ok(());
                        }
                        let recover_key = shadow_rpc_policy::recover_dispatch_key(trade_no);
                        if !shadow_rpc_policy::allow_broadcast_recover_dispatch(
                            &recover_key,
                            collect.last_broadcast_at,
                        ) {
                            trace!(
                                trade_no = %trade_no,
                                cooldown = ?shadow_rpc_policy::recover_dispatch_cooldown(&recover_key),
                                "recover_skip_because_cooldown: collect recover skipped"
                            );
                            return Ok(());
                        }
                        trace!(trade_no = %trade_no, "Need to recover transaction");
                        let intent =
                            CollectIntent::Chain(ChainIntent::RecoverTx(trade_no.to_string()));
                        self.dispatch_intent(intent).await;
                        return Ok(());
                    }
                    CollectStage::NeedTxExecReceiptUpload => {
                        trace!(trade_no = %trade_no, "Need to upload tx exec receipt");
                        let intent = CollectIntent::SideEffect(
                            SideEffectIntent::UploadTxExecReceipt(trade_no.to_string()),
                        );
                        self.dispatch_intent(intent).await;
                        return Ok(());
                    }
                    CollectStage::NeedResultAck => {
                        trace!(trade_no = %trade_no, "Need to send result ACK");
                        let intent = CollectIntent::SideEffect(SideEffectIntent::SendResultAck(
                            trade_no.to_string(),
                        ));
                        self.dispatch_intent(intent).await;
                        return Ok(());
                    }
                    CollectStage::NeedServiceFeeUpload => {
                        trace!(trade_no = %trade_no, "Need to upload service fee");
                        let intent = CollectIntent::SideEffect(SideEffectIntent::UploadServiceFee(
                            trade_no.to_string(),
                        ));
                        self.dispatch_intent(intent).await;
                        return Ok(());
                    }
                    CollectStage::FullyBlocked => {
                        continue;
                    }
                }
            }
        }

        // 无可用推进点
        trace!(trade_no = %trade_no, "No advancement possible based on current facts");

        // 检查是否可能卡住
        let _ = maybe_log_stuck(
            &collect,
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
            wallet_database::entities::api_trade_type::ApiTradeType::Collect as i64,
            origin_trade_no,
        )
        .await?
        .map(|row| row.resource_trade_no))
    }
}
