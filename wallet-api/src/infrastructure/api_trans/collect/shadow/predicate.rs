use smallvec::SmallVec;
use wallet_database::entities::{
    api_collect::ApiCollectEntity, api_resource_gate::ApiResourceBlockReason,
};

use crate::domain::api_wallet::trans::shared_mechanism;

use super::stage::CollectStage;

/// 推进原因
#[derive(Debug, Clone, PartialEq)]
pub struct StageReason {
    pub code: &'static str,
    pub message: String,
}

/// 阶段评估结果
#[derive(Debug, Clone)]
pub struct StageEval {
    pub can_advance: bool,
    pub reasons: SmallVec<[StageReason; 4]>,
}

/// 评估指定阶段
pub fn evaluate_stage(stage: CollectStage, collect: &ApiCollectEntity) -> StageEval {
    match stage {
        CollectStage::NeedOrderAck => evaluate_need_order_ack(collect),
        CollectStage::NeedResourceGate => evaluate_need_resource_gate(collect),
        CollectStage::CanBuild => evaluate_can_build(collect),
        CollectStage::NeedTxFeeResAck => evaluate_need_tx_fee_res_ack(collect),
        CollectStage::CanBroadcast => evaluate_can_broadcast(collect),
        CollectStage::NeedRecover => evaluate_need_recover(collect),
        CollectStage::NeedTxExecReceiptUpload => evaluate_need_tx_exec_receipt_upload(collect),
        CollectStage::NeedResultAck => evaluate_need_result_ack(collect),
        CollectStage::NeedServiceFeeUpload => evaluate_need_service_fee_upload(collect),
        CollectStage::FullyBlocked => StageEval { can_advance: false, reasons: SmallVec::new() },
    }
}

fn collect_resource_gate_released_for_build(collect: &ApiCollectEntity) -> bool {
    !shared_mechanism::is_tron_chain(&collect.chain_code)
        || collect.resource_gate_released_at.is_some()
}

fn collect_resource_gate_released_for_service_fee_upload(collect: &ApiCollectEntity) -> bool {
    !shared_mechanism::is_tron_chain(&collect.chain_code)
        || collect.resource_gate_released_at.is_some()
}

fn evaluate_need_resource_gate(collect: &ApiCollectEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if collect.order_ack_sent_at.is_none() {
        reasons.push(StageReason {
            code: "order_ack_not_sent",
            message: "Order ACK not sent yet".to_string(),
        });
    }

    if !shared_mechanism::is_tron_chain(&collect.chain_code) {
        reasons.push(StageReason {
            code: "not_tron",
            message: "Resource gate is only required for TRON collect".to_string(),
        });
    }

    if collect.resource_gate_released_at.is_some() {
        reasons.push(StageReason {
            code: "resource_gate_released",
            message: "Resource gate already released".to_string(),
        });
    }

    if collect.resource_block_reason == Some(ApiResourceBlockReason::NeedPlatformDelegate) {
        reasons.push(StageReason {
            code: "waiting_platform_delegate_result",
            message: "Waiting for platform delegation result".to_string(),
        });
    }

    if collect.resource_block_reason == Some(ApiResourceBlockReason::NeedLocalDelegate)
        && collect.resource_dependency_trade_no.is_some()
    {
        reasons.push(StageReason {
            code: "waiting_local_delegate_task",
            message: "Waiting for local delegation task".to_string(),
        });
    }

    if collect.raw_tx.is_some() {
        reasons.push(StageReason {
            code: "raw_tx_exists",
            message: "Raw tx already exists".to_string(),
        });
    }

    if collect.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    if collect.transaction_time.is_some() {
        reasons.push(StageReason {
            code: "transaction_time_exists",
            message: "Transaction already committed".to_string(),
        });
    }

    if collect.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }

    let can_advance = collect.order_ack_sent_at.is_some()
        && shared_mechanism::is_tron_chain(&collect.chain_code)
        && collect.resource_gate_released_at.is_none()
        && (collect.resource_block_reason.is_none()
            || (collect.resource_block_reason == Some(ApiResourceBlockReason::NeedLocalDelegate)
                && collect.resource_dependency_trade_no.is_none()))
        && collect.raw_tx.is_none()
        && collect.err_code.is_none()
        && collect.transaction_time.is_none()
        && collect.finished_at.is_none();

    StageEval { can_advance, reasons }
}

/// 当前周期是否已经真正进入过“服务费上传”阶段。
///
/// 这里不能只看 `ever_needed_service_fee`，因为它会在重开 fee cycle 时保留历史事实。
/// 只有当前周期真的出现过 `service_fee_uploaded_at`，后续才应该继续依赖 TxFeeResAck。
fn fee_res_ack_required_for_progress(collect: &ApiCollectEntity) -> bool {
    collect.service_fee_uploaded_at.is_none() || collect.tx_fee_res_ack_sent_at.is_some()
}

/// 当前周期是否仍在等待手续费结果 ACK。
///
/// 这个判断必须同时满足：
/// - 本周期已经进入过服务费上传
/// - 历史上确实需要过服务费
/// - 当前还没有收到/发送手续费结果 ACK
fn waiting_for_fee_res_ack(collect: &ApiCollectEntity) -> bool {
    collect.service_fee_uploaded_at.is_some()
        && collect.ever_needed_service_fee == true
        && collect.tx_fee_res_ack_sent_at.is_none()
}

/// 评估 NeedOrderAck 阶段
/// 检查是否需要发送订单 ACK
///
/// 事实条件：
/// - order_ack_sent_at IS NULL
/// - finished_at IS NULL
/// - err_code IS NULL
///
/// ⚠️ 重要说明：
/// - Scanner 不得在 finished_at IS NOT NULL 的记录上产生任何动作
/// - 确保在已终态的记录上不会再尝试发送订单 ACK
/// - 一旦 err_code IS NOT NULL，不再产生任何推进意图
fn evaluate_need_order_ack(collect: &ApiCollectEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if collect.order_ack_sent_at.is_some() {
        reasons.push(StageReason {
            code: "order_ack_sent",
            message: "Order ACK already sent".to_string(),
        });
    }

    if collect.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }

    if collect.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance = collect.order_ack_sent_at.is_none()
        && collect.finished_at.is_none()
        && collect.err_code.is_none();

    StageEval { can_advance, reasons }
}

/// 评估 CanBuild 阶段
/// 检查是否可以构建交易
///
/// 事实条件（强顺序屏障）：
/// - order_ack_sent_at IS NOT NULL   // 订单确认已完成
/// - raw_tx IS NULL
/// - need_service_fee != true
/// - TRON 订单必须先有 resource_gate_released_at
/// - 如果曾经需要过服务费补充，则必须先完成 TxFeeResAck
/// - transaction_time IS NULL
/// - finished_at IS NULL
fn evaluate_can_build(collect: &ApiCollectEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if collect.order_ack_sent_at.is_none() {
        reasons.push(StageReason {
            code: "order_ack_not_sent",
            message: "Order ACK not sent yet".to_string(),
        });
    }

    if collect.raw_tx.is_some() {
        reasons.push(StageReason {
            code: "raw_tx_exists",
            message: "Raw tx already exists".to_string(),
        });
    }

    if collect.need_service_fee == Some(true) {
        reasons.push(StageReason {
            code: "need_service_fee",
            message: "Need service fee".to_string(),
        });
    }

    if !collect_resource_gate_released_for_build(collect) {
        reasons.push(StageReason {
            code: "resource_gate_not_released",
            message: "TRON resource gate has not been released yet".to_string(),
        });
    }

    if collect.service_fee_uploaded_at.is_some()
        && collect.ever_needed_service_fee == true
        && collect.tx_fee_res_ack_sent_at.is_none()
    {
        reasons.push(StageReason {
            code: "tx_fee_res_ack_not_sent",
            message: "Tx fee res ACK not sent yet".to_string(),
        });
    }

    if collect.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    if collect.transaction_time.is_some() {
        reasons.push(StageReason {
            code: "transaction_time_exists",
            message: "Transaction already committed".to_string(),
        });
    }

    if collect.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }

    let can_advance = collect.order_ack_sent_at.is_some()
        && collect.raw_tx.is_none()
        && collect.need_service_fee != Some(true)
        && collect_resource_gate_released_for_build(collect)
        && fee_res_ack_required_for_progress(collect)
        && collect.err_code.is_none()
        && collect.transaction_time.is_none()
        && collect.finished_at.is_none();

    StageEval { can_advance, reasons }
}

/// 评估 NeedTxFeeResAck 阶段
fn evaluate_need_tx_fee_res_ack(collect: &ApiCollectEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if collect.need_service_fee == Some(true) {
        reasons.push(StageReason {
            code: "need_service_fee",
            message: "Still need service fee".to_string(),
        });
    }

    if collect.service_fee_uploaded_at.is_none() {
        reasons.push(StageReason {
            code: "service_fee_not_uploaded",
            message: "Service fee has not been uploaded yet".to_string(),
        });
    }

    if collect.ever_needed_service_fee != true {
        reasons.push(StageReason {
            code: "never_needed_service_fee",
            message: "Never needed service fee".to_string(),
        });
    }

    if collect.tx_fee_res_ack_sent_at.is_some() {
        reasons.push(StageReason {
            code: "tx_fee_res_ack_sent",
            message: "Tx fee res ACK already sent".to_string(),
        });
    }

    if collect.last_broadcast_at.is_some() {
        reasons.push(StageReason {
            code: "already_broadcasted",
            message: "Already broadcasted".to_string(),
        });
    }

    if collect.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }

    if collect.transaction_time.is_some() {
        reasons.push(StageReason {
            code: "transaction_time_exists",
            message: "Transaction time already exists".to_string(),
        });
    }

    if collect.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance = collect.need_service_fee != Some(true)
        && waiting_for_fee_res_ack(collect)
        && collect.last_broadcast_at.is_none()
        && collect.finished_at.is_none()
        && collect.transaction_time.is_none()
        && collect.err_code.is_none();

    StageEval { can_advance, reasons }
}

/// 评估 CanBroadcast 阶段
/// 检查是否可以广播交易
///
/// 事实条件：
/// - raw_tx IS NOT NULL
/// - last_broadcast_at IS NULL
/// - transaction_time IS NULL
/// - finished_at IS NULL
/// - AND (
///     - service_fee_uploaded_at IS NULL
///     - OR tx_fee_res_ack_sent_at IS NOT NULL
///   )
///
/// ⚠️ 语义：
/// - 当前周期未进入服务费上传的交易：可直接广播
/// - 当前周期已经进入服务费上传的交易：必须先完成 TxFeeResAck，才能广播
fn evaluate_can_broadcast(collect: &ApiCollectEntity) -> StageEval {
    let mut reasons = SmallVec::new();
    let broadcast_uncertain_in_progress =
        shared_mechanism::tracks_broadcast_uncertain_state(&collect.chain_code)
            && collect.broadcast_uncertain_since_at.is_some();

    if collect.raw_tx.is_none() {
        reasons.push(StageReason {
            code: "raw_tx_not_exists",
            message: "Raw tx not exists".to_string(),
        });
    }

    if collect.last_broadcast_at.is_some() {
        reasons.push(StageReason {
            code: "already_broadcasted",
            message: "Already broadcasted".to_string(),
        });
    }

    if collect.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }

    if collect.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    if !fee_res_ack_required_for_progress(collect) {
        reasons.push(StageReason {
            code: "tx_fee_res_ack_not_sent",
            message: "Tx fee res ACK not sent yet".to_string(),
        });
    }

    if broadcast_uncertain_in_progress {
        reasons.push(StageReason {
            code: "broadcast_uncertain_in_progress",
            message: "Tx is in uncertain broadcast state; recover owns progression".to_string(),
        });
    }

    let can_advance = collect.raw_tx.is_some()
        && collect.last_broadcast_at.is_none()
        && collect.transaction_time.is_none()
        && collect.finished_at.is_none()
        && collect.err_code.is_none()
        && fee_res_ack_required_for_progress(collect)
        && !broadcast_uncertain_in_progress;
    // let can_advance = false;
    // tracing::info!(
    //     trade_no = %collect.trade_no,
    //     can_advance,
    //     reasons = ?reasons,
    //     source = "shadow_worker_v2",
    //     "Evaluate CanBroadcast stage, 故意不让广播"
    // );
    StageEval { can_advance, reasons }
}

/// 评估 NeedRecover 阶段
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
fn evaluate_need_recover(collect: &ApiCollectEntity) -> StageEval {
    let mut reasons = SmallVec::new();
    let broadcast_not_attempted =
        shared_mechanism::tracks_broadcast_uncertain_state(&collect.chain_code)
            && collect.raw_tx.is_some()
            && collect.last_broadcast_at.is_none()
            && collect.broadcast_uncertain_since_at.is_none();

    if collect.tx_hash.is_none() {
        reasons.push(StageReason {
            code: "tx_hash_not_exists",
            message: "Tx hash not exists".to_string(),
        });
    }

    if collect.transaction_time.is_some() {
        reasons.push(StageReason {
            code: "transaction_time_exists",
            message: "Transaction time already exists".to_string(),
        });
    }

    if collect.last_broadcast_at.is_some() {
        reasons.push(StageReason {
            code: "already_broadcasted",
            message: "Broadcast already visible".to_string(),
        });
    }

    if collect.tx_exec_receipt_uploaded_at.is_some() {
        reasons.push(StageReason {
            code: "receipt_uploaded",
            message: "TxExecReceipt already uploaded".to_string(),
        });
    }

    if collect.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }

    if collect.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    if broadcast_not_attempted {
        reasons.push(StageReason {
            code: "broadcast_not_attempted",
            message: "Raw tx exists but broadcast has not entered uncertain or executed yet"
                .to_string(),
        });
    }

    let can_advance = collect.tx_hash.is_some()
        && collect.transaction_time.is_none()
        && collect.tx_exec_receipt_uploaded_at.is_none()
        && collect.finished_at.is_none()
        && collect.err_code.is_none()
        && !broadcast_not_attempted;

    StageEval { can_advance, reasons }
}

/// 评估 NeedTxExecReceiptUpload 阶段
/// 检查是否需要上传交易执行回执
///
/// 事实条件：
/// - transaction_time IS NOT NULL
///   OR err_code IS NOT NULL
/// - tx_exec_receipt_uploaded_at IS NULL
/// - finished_at IS NULL
///
/// ⚠️ 重要约束：
/// - 只有链上结果已确认或明确失败时，才允许上传最终回执
/// - 广播可见但结果未确定时，不得上报最终成功 / 失败
/// - 生命周期收口（finished_at）只能由 Worker 在副作用完成后写入
/// - 此为 err_code 失败冻结态的唯一例外
/// - 但仍然受 finished_at 终态屏障约束
fn evaluate_need_tx_exec_receipt_upload(collect: &ApiCollectEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if collect.transaction_time.is_none() && collect.err_code.is_none() {
        reasons.push(StageReason {
            code: "pending_execution_fact",
            message: "Waiting for confirmed execution fact".to_string(),
        });
    }

    if collect.tx_exec_receipt_uploaded_at.is_some() {
        reasons.push(StageReason {
            code: "receipt_uploaded",
            message: "Receipt already uploaded".to_string(),
        });
    }

    if collect.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }

    let can_advance = collect.tx_exec_receipt_uploaded_at.is_none()
        && collect.finished_at.is_none()
        && (collect.err_code.is_some() || collect.transaction_time.is_some());

    StageEval { can_advance, reasons }
}

/// 评估 NeedResultAck 阶段
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
fn evaluate_need_result_ack(collect: &ApiCollectEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if collect.tx_res_received_at.is_none() {
        reasons.push(StageReason {
            code: "tx_res_not_received",
            message: "SER tx result push (AWM_ORDER_TRANS_RES) not received".to_string(),
        });
    }
    if collect.transaction_time.is_none() {
        reasons.push(StageReason {
            code: "transaction_time_not_exists",
            message: "Transaction time not exists".to_string(),
        });
    }

    if collect.result_ack_sent_at.is_some() {
        reasons.push(StageReason {
            code: "result_ack_sent",
            message: "Result ACK already sent".to_string(),
        });
    }

    if collect.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }

    if collect.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance = collect.tx_res_received_at.is_some()
        && collect.transaction_time.is_some()
        && collect.result_ack_sent_at.is_none()
        && collect.finished_at.is_none()
        && collect.err_code.is_none();

    StageEval { can_advance, reasons }
}

/// 评估 NeedServiceFeeUpload 阶段
/// 检查是否需要上传服务费
///
/// 事实条件：
/// - need_service_fee = true
/// - service_fee_uploaded_at IS NULL
/// - err_code IS NULL
/// - 非 TRON 链不依赖 resource gate；TRON 必须等待 resource gate 释放
///
fn evaluate_need_service_fee_upload(collect: &ApiCollectEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if collect.need_service_fee != Some(true) {
        reasons.push(StageReason {
            code: "no_need_service_fee",
            message: "No need service fee".to_string(),
        });
    }

    if collect.service_fee_uploaded_at.is_some() {
        reasons.push(StageReason {
            code: "service_fee_uploaded",
            message: "Service fee already uploaded".to_string(),
        });
    }

    if collect.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    if collect.need_service_fee == Some(true)
        && collect.service_fee_uploaded_at.is_none()
        && collect.err_code.is_none()
        && collect.finished_at.is_none()
        && collect_resource_gate_released_for_service_fee_upload(collect)
    {
        reasons.push(StageReason {
            code: "ready_for_service_fee_upload",
            message: "Need service fee upload".to_string(),
        });
    }

    let can_advance = collect.need_service_fee == Some(true)
        && collect.service_fee_uploaded_at.is_none()
        && collect.err_code.is_none()
        && collect.finished_at.is_none()
        && collect_resource_gate_released_for_service_fee_upload(collect);

    StageEval { can_advance, reasons }
}

/// 快速检查是否有任何推进点
pub fn has_any_advancement_point(collect: &ApiCollectEntity) -> bool {
    use super::stage::COLLECT_ADVANCEMENT_ORDER;

    COLLECT_ADVANCEMENT_ORDER.iter().any(|stage| evaluate_stage(*stage, collect).can_advance)
}

/// 快速检查是否可能卡住
pub fn is_potentially_blocked(collect: &ApiCollectEntity) -> bool {
    // 已完成或有错误的订单不视为卡住
    if collect.finished_at.is_some() || collect.err_code.is_some() {
        return false;
    }

    // 没有推进点的订单视为可能卡住
    !has_any_advancement_point(collect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use wallet_database::entities::{
        api_collect::ApiCollectStatus, asset_token_key::AssetTokenKey,
    };

    fn base_collect() -> ApiCollectEntity {
        ApiCollectEntity {
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
            trade_no: "C_TEST".to_string(),
            trade_type: 2,
            risk_addr: 0,
            status: ApiCollectStatus::Init,
            nonce: 0,
            tx_hash: Some("h".to_string()),
            transaction_fee: "0".to_string(),
            transaction_time: None,
            block_height: Some("0".to_string()),
            notes: Some("".to_string()),
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: Some("".to_string()),
            resource_check_at: None,
            resource_gate_released_at: Some(Utc::now()),
            resource_gate_result: None,
            resource_block_reason: None,
            resource_dependency_trade_no: None,
            resource_dependency_type: None,
            order_ack_sent_at: Some(Utc::now()),
            raw_tx: Some("{}".to_string()),
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
            tx_exec_receipt_uploaded_at: Some(Utc::now()),
            finished_at: None,
            created_at: Utc::now(),
            updated_at: Some(Utc::now()),
        }
    }

    #[test]
    fn can_build_rejects_committed_or_finished_orders() {
        let mut committed = base_collect();
        committed.raw_tx = None;
        committed.need_service_fee = Some(false);
        committed.transaction_time = Some(Utc::now());

        let mut finished = base_collect();
        finished.raw_tx = None;
        finished.need_service_fee = Some(false);
        finished.finished_at = Some(Utc::now());

        let committed_eval = evaluate_stage(CollectStage::CanBuild, &committed);
        let finished_eval = evaluate_stage(CollectStage::CanBuild, &finished);

        assert!(!committed_eval.can_advance);
        assert!(
            committed_eval.reasons.iter().any(|reason| reason.code == "transaction_time_exists")
        );
        assert!(!finished_eval.can_advance);
        assert!(finished_eval.reasons.iter().any(|reason| reason.code == "finished"));
    }

    #[test]
    fn can_build_blocks_tron_until_resource_gate_is_released() {
        let mut blocked = base_collect();
        blocked.raw_tx = None;
        blocked.need_service_fee = Some(false);
        blocked.chain_code = "tron".to_string();
        blocked.resource_gate_released_at = None;

        let blocked_eval = evaluate_stage(CollectStage::CanBuild, &blocked);
        assert!(!blocked_eval.can_advance);
        assert!(
            blocked_eval.reasons.iter().any(|reason| reason.code == "resource_gate_not_released")
        );

        let mut released = blocked.clone();
        released.resource_gate_released_at = Some(Utc::now());
        let released_eval = evaluate_stage(CollectStage::CanBuild, &released);
        assert!(released_eval.can_advance);
    }

    #[test]
    fn can_build_blocks_tron_when_service_fee_is_required() {
        let mut blocked = base_collect();
        blocked.raw_tx = None;
        blocked.need_service_fee = Some(true);
        blocked.chain_code = "tron".to_string();
        blocked.resource_gate_released_at = Some(Utc::now());

        let blocked_eval = evaluate_stage(CollectStage::CanBuild, &blocked);
        assert!(!blocked_eval.can_advance);
        assert!(blocked_eval.reasons.iter().any(|reason| reason.code == "need_service_fee"));
    }

    #[test]
    fn need_resource_gate_advances_for_unreleased_tron_only() {
        let mut blocked = base_collect();
        blocked.raw_tx = None;
        blocked.chain_code = "tron".to_string();
        blocked.resource_gate_released_at = None;

        let blocked_eval = evaluate_stage(CollectStage::NeedResourceGate, &blocked);
        assert!(blocked_eval.can_advance);

        let mut released = blocked.clone();
        released.resource_gate_released_at = Some(Utc::now());
        let released_eval = evaluate_stage(CollectStage::NeedResourceGate, &released);
        assert!(!released_eval.can_advance);
        assert!(released_eval.reasons.iter().any(|reason| reason.code == "resource_gate_released"));

        let mut non_tron = blocked.clone();
        non_tron.chain_code = "sol".to_string();
        let non_tron_eval = evaluate_stage(CollectStage::NeedResourceGate, &non_tron);
        assert!(!non_tron_eval.can_advance);
    }

    #[test]
    fn need_resource_gate_waits_when_resource_dependency_is_in_progress() {
        let mut waiting_platform = base_collect();
        waiting_platform.raw_tx = None;
        waiting_platform.chain_code = "tron".to_string();
        waiting_platform.resource_gate_released_at = None;
        waiting_platform.resource_block_reason = Some(ApiResourceBlockReason::NeedPlatformDelegate);
        waiting_platform.resource_dependency_trade_no = Some("DL_1".to_string());

        let platform_eval = evaluate_stage(CollectStage::NeedResourceGate, &waiting_platform);
        assert!(!platform_eval.can_advance);
        assert!(
            platform_eval
                .reasons
                .iter()
                .any(|reason| reason.code == "waiting_platform_delegate_result")
        );

        let mut waiting_local = waiting_platform.clone();
        waiting_local.resource_block_reason = Some(ApiResourceBlockReason::NeedLocalDelegate);
        waiting_local.resource_dependency_trade_no = Some("rsc_local_delegate_C".to_string());
        let local_eval = evaluate_stage(CollectStage::NeedResourceGate, &waiting_local);
        assert!(!local_eval.can_advance);
        assert!(
            local_eval.reasons.iter().any(|reason| reason.code == "waiting_local_delegate_task")
        );

        let mut needs_local_task = waiting_local.clone();
        needs_local_task.resource_dependency_trade_no = None;
        let retry_eval = evaluate_stage(CollectStage::NeedResourceGate, &needs_local_task);
        assert!(retry_eval.can_advance);
    }

    #[test]
    fn can_build_keeps_non_tron_resource_gate_optional() {
        let mut ready = base_collect();
        ready.raw_tx = None;
        ready.need_service_fee = Some(false);
        ready.chain_code = "sol".to_string();
        ready.resource_gate_released_at = None;

        let eval = evaluate_stage(CollectStage::CanBuild, &ready);
        assert!(eval.can_advance);
    }

    #[test]
    fn need_result_ack_requires_tx_res_received_at() {
        let mut c = base_collect();
        c.transaction_time = Some(Utc::now());
        c.tx_res_received_at = None;

        let eval = evaluate_stage(CollectStage::NeedResultAck, &c);
        assert!(!eval.can_advance);

        c.tx_res_received_at = Some(Utc::now());
        let eval2 = evaluate_stage(CollectStage::NeedResultAck, &c);
        assert!(eval2.can_advance);
    }

    #[test]
    fn need_tx_exec_receipt_upload_allows_transaction_time_without_last_broadcast() {
        let mut c = base_collect();
        c.tx_exec_receipt_uploaded_at = None;
        c.last_broadcast_at = None;
        c.transaction_time = Some(Utc::now());

        let eval = evaluate_stage(CollectStage::NeedTxExecReceiptUpload, &c);
        assert!(eval.can_advance);
    }

    #[test]
    fn need_recover_allows_broadcast_visible_pending_chain_result() {
        let mut c = base_collect();
        c.last_broadcast_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = None;
        c.transaction_time = None;

        let eval = evaluate_stage(CollectStage::NeedRecover, &c);
        assert!(eval.can_advance);
    }

    #[test]
    fn need_tx_exec_receipt_upload_rejects_broadcast_only_pending() {
        let mut c = base_collect();
        c.last_broadcast_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = None;
        c.transaction_time = None;
        c.err_code = None;

        let eval = evaluate_stage(CollectStage::NeedTxExecReceiptUpload, &c);
        assert!(!eval.can_advance);
        assert!(eval.reasons.iter().any(|r| r.code == "pending_execution_fact"));
    }

    #[test]
    fn need_service_fee_upload_advances_without_backend_order_fact() {
        let mut c = base_collect();
        c.chain_code = "tron".to_string();
        c.need_service_fee = Some(true);
        c.service_fee_order_received_at = None;
        c.service_fee_uploaded_at = None;
        c.resource_gate_released_at = Some(Utc::now());

        let eval = evaluate_stage(CollectStage::NeedServiceFeeUpload, &c);
        assert!(eval.can_advance);
        assert!(eval.reasons.iter().any(|r| r.code == "ready_for_service_fee_upload"));
    }

    #[test]
    fn need_service_fee_upload_still_advances_when_backend_order_received() {
        let mut c = base_collect();
        c.chain_code = "tron".to_string();
        c.need_service_fee = Some(true);
        c.service_fee_order_received_at = Some(Utc::now());
        c.service_fee_uploaded_at = None;
        c.resource_gate_released_at = Some(Utc::now());

        let eval = evaluate_stage(CollectStage::NeedServiceFeeUpload, &c);
        assert!(eval.can_advance);
        assert!(eval.reasons.iter().any(|r| r.code == "ready_for_service_fee_upload"));
    }

    #[test]
    fn need_service_fee_upload_allows_sol_without_resource_gate() {
        let mut c = base_collect();
        c.chain_code = "sol".to_string();
        c.need_service_fee = Some(true);
        c.service_fee_uploaded_at = None;
        c.resource_gate_released_at = None;

        let eval = evaluate_stage(CollectStage::NeedServiceFeeUpload, &c);
        assert!(eval.can_advance);
        assert!(eval.reasons.iter().any(|r| r.code == "ready_for_service_fee_upload"));
    }

    #[test]
    fn need_service_fee_upload_blocks_unreleased_tron_resource_gate() {
        let mut c = base_collect();
        c.chain_code = "tron".to_string();
        c.need_service_fee = Some(true);
        c.service_fee_uploaded_at = None;
        c.resource_gate_released_at = None;

        let eval = evaluate_stage(CollectStage::NeedServiceFeeUpload, &c);
        assert!(!eval.can_advance);
    }

    // --- NeedOrderAck ---

    #[test]
    fn need_order_ack_advances_when_nothing_sent() {
        let mut c = base_collect();
        c.order_ack_sent_at = None;
        c.finished_at = None;
        c.err_code = None;
        c.tx_exec_receipt_uploaded_at = None;

        let eval = evaluate_stage(CollectStage::NeedOrderAck, &c);
        assert!(eval.can_advance);
    }

    #[test]
    fn need_order_ack_blocked_when_already_acked() {
        let mut c = base_collect();
        // order_ack_sent_at is set by base_collect
        let eval = evaluate_stage(CollectStage::NeedOrderAck, &c);
        assert!(!eval.can_advance);
        assert!(eval.reasons.iter().any(|r| r.code == "order_ack_sent"));
    }

    #[test]
    fn need_order_ack_blocked_when_finished() {
        let mut c = base_collect();
        c.order_ack_sent_at = None;
        c.finished_at = Some(Utc::now());

        let eval = evaluate_stage(CollectStage::NeedOrderAck, &c);
        assert!(!eval.can_advance);
        assert!(eval.reasons.iter().any(|r| r.code == "finished"));
    }

    #[test]
    fn need_order_ack_blocked_when_err_code() {
        let mut c = base_collect();
        c.order_ack_sent_at = None;
        c.err_code = Some(wallet_database::entities::api_collect::ErrCode::UnknownError);

        let eval = evaluate_stage(CollectStage::NeedOrderAck, &c);
        assert!(!eval.can_advance);
        assert!(eval.reasons.iter().any(|r| r.code == "error"));
    }

    // --- CanBroadcast: broadcast uncertain ---

    #[test]
    fn can_broadcast_blocks_tracked_uncertain_state() {
        for chain in ["tron", "eth", "bnb", "sol"] {
            let mut c = base_collect();
            c.chain_code = chain.to_string();
            c.broadcast_uncertain_since_at = Some(Utc::now());
            c.tx_exec_receipt_uploaded_at = None;

            let eval = evaluate_stage(CollectStage::CanBroadcast, &c);
            assert!(!eval.can_advance, "{chain}: should block on uncertain state");
            assert!(
                eval.reasons.iter().any(|r| r.code == "broadcast_uncertain_in_progress"),
                "{chain}: expected reason code broadcast_uncertain_in_progress"
            );
        }
    }

    #[test]
    fn can_broadcast_untracked_chain_not_blocked_by_uncertain_field() {
        let mut c = base_collect();
        c.chain_code = "unknown".to_string();
        c.broadcast_uncertain_since_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = None;

        let eval = evaluate_stage(CollectStage::CanBroadcast, &c);
        assert!(eval.can_advance);
    }

    // --- NeedRecover: pre-broadcast guard ---

    #[test]
    fn need_recover_blocks_tracked_chain_before_broadcast_attempted() {
        let mut c = base_collect();
        c.chain_code = "tron".to_string();
        c.raw_tx = Some("{}".to_string());
        c.last_broadcast_at = None;
        c.broadcast_uncertain_since_at = None;
        c.tx_exec_receipt_uploaded_at = None;

        let eval = evaluate_stage(CollectStage::NeedRecover, &c);
        assert!(
            !eval.can_advance,
            "tracked chain with raw_tx but no broadcast yet should not recover"
        );
        assert!(eval.reasons.iter().any(|r| r.code == "broadcast_not_attempted"));
    }

    #[test]
    fn need_recover_allows_tracked_chain_after_broadcast_uncertain() {
        let mut c = base_collect();
        c.chain_code = "tron".to_string();
        c.raw_tx = Some("{}".to_string());
        c.broadcast_uncertain_since_at = Some(Utc::now());
        c.last_broadcast_at = None;
        c.tx_exec_receipt_uploaded_at = None;

        // uncertain_since_at means broadcast was attempted; recover may proceed
        let eval = evaluate_stage(CollectStage::NeedRecover, &c);
        assert!(eval.can_advance);
    }

    // --- Utility helpers ---

    #[test]
    fn has_any_advancement_point_true_when_order_ack_pending() {
        let mut c = base_collect();
        c.order_ack_sent_at = None;
        c.finished_at = None;
        c.err_code = None;
        c.tx_exec_receipt_uploaded_at = None;

        assert!(has_any_advancement_point(&c));
    }

    #[test]
    fn has_any_advancement_point_false_when_fully_finished() {
        let mut c = base_collect();
        c.finished_at = Some(Utc::now());
        c.order_ack_sent_at = Some(Utc::now());
        c.raw_tx = Some("{}".to_string());
        c.tx_exec_receipt_uploaded_at = Some(Utc::now());
        c.result_ack_sent_at = Some(Utc::now());
        c.transaction_time = Some(Utc::now());

        assert!(!has_any_advancement_point(&c));
    }

    #[test]
    fn is_potentially_blocked_false_when_finished() {
        let mut c = base_collect();
        c.finished_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = Some(Utc::now());

        assert!(!is_potentially_blocked(&c));
    }

    #[test]
    fn is_potentially_blocked_false_when_err_code() {
        let mut c = base_collect();
        c.err_code = Some(wallet_database::entities::api_collect::ErrCode::UnknownError);
        c.tx_exec_receipt_uploaded_at = Some(Utc::now());

        assert!(!is_potentially_blocked(&c));
    }

    #[test]
    fn is_potentially_blocked_true_when_no_advancement_point() {
        // Simulate a record that passed all stages but is not yet finished and has no more
        // advancement points (e.g., waiting for an external event).
        let mut c = base_collect();
        c.last_broadcast_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = Some(Utc::now());
        c.transaction_time = None;
        c.result_ack_sent_at = None;
        c.finished_at = None;
        c.err_code = None;
        // tx_res_received_at is None → NeedResultAck cannot advance
        c.tx_res_received_at = None;

        assert!(is_potentially_blocked(&c));
    }
}
