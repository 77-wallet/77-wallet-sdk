use smallvec::SmallVec;
use wallet_database::entities::{
    api_resource_gate::{ApiResourceBlockReason, ApiResourceDependencyType},
    api_withdraw::ApiWithdrawEntity,
};

use crate::domain::api_wallet::trans::shared_mechanism;

use super::stage::AdvancementPoint;

#[derive(Debug, Clone, PartialEq)]
pub struct StageReason {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct StageEval {
    pub can_advance: bool,
    pub reasons: SmallVec<[StageReason; 4]>,
}

fn withdraw_resource_gate_released_for_build(withdraw: &ApiWithdrawEntity) -> bool {
    !shared_mechanism::is_tron_chain(&withdraw.chain_code)
        || withdraw.resource_gate_released_at.is_some()
}

pub fn evaluate_point(point: AdvancementPoint, withdraw: &ApiWithdrawEntity) -> StageEval {
    match point {
        AdvancementPoint::NeedTxAck => evaluate_need_tx_ack(withdraw),
        AdvancementPoint::NeedResourceGate => evaluate_need_resource_gate(withdraw),
        AdvancementPoint::NeedPlatformDelegateApply => {
            evaluate_need_platform_delegate_apply(withdraw)
        }
        AdvancementPoint::CanBuild => evaluate_can_build(withdraw),
        AdvancementPoint::CanBroadcast => evaluate_can_broadcast(withdraw),
        AdvancementPoint::NeedRecover => evaluate_need_recover(withdraw),
        AdvancementPoint::NeedTxExecReceiptUpload => evaluate_need_tx_exec_receipt_upload(withdraw),
        AdvancementPoint::NeedTxResAck => evaluate_need_tx_res_ack(withdraw),
        AdvancementPoint::FullyBlocked => {
            StageEval { can_advance: false, reasons: SmallVec::new() }
        }
    }
}

fn evaluate_need_platform_delegate_apply(withdraw: &ApiWithdrawEntity) -> StageEval {
    let mut reasons = SmallVec::new();
    let platform_apply_fact_gap = withdraw.resource_block_reason
        == Some(ApiResourceBlockReason::NeedPlatformDelegate)
        && withdraw.resource_dependency_type == Some(ApiResourceDependencyType::PlatformDelegate)
        && withdraw.resource_dependency_trade_no.is_none();

    if !platform_apply_fact_gap {
        reasons.push(StageReason {
            code: "platform_delegate_apply_fact_gap_closed",
            message: "Platform delegation is not required or its apply response was accepted"
                .to_string(),
        });
    }
    if withdraw.resource_gate_result.is_some() {
        reasons.push(StageReason {
            code: "platform_delegate_result_received",
            message: "Platform delegation result already received".to_string(),
        });
    }
    if withdraw.resource_gate_released_at.is_some() {
        reasons.push(StageReason {
            code: "resource_gate_released",
            message: "Resource gate already released".to_string(),
        });
    }
    if withdraw.raw_tx.is_some() {
        reasons.push(StageReason {
            code: "raw_tx_exists",
            message: "Raw tx already exists".to_string(),
        });
    }
    if withdraw.transaction_time.is_some() {
        reasons.push(StageReason {
            code: "transaction_time_exists",
            message: "Transaction already committed".to_string(),
        });
    }
    if withdraw.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if withdraw.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance = platform_apply_fact_gap
        && withdraw.resource_gate_result.is_none()
        && withdraw.resource_gate_released_at.is_none()
        && withdraw.raw_tx.is_none()
        && withdraw.transaction_time.is_none()
        && withdraw.finished_at.is_none()
        && withdraw.err_code.is_none();

    StageEval { can_advance, reasons }
}

fn evaluate_need_resource_gate(withdraw: &ApiWithdrawEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if withdraw.tx_ack_sent_at.is_none() {
        reasons.push(StageReason {
            code: "tx_ack_not_sent",
            message: "Tx ACK not sent yet".to_string(),
        });
    }
    if withdraw.audit_passed_at.is_none() {
        reasons.push(StageReason {
            code: "audit_not_passed",
            message: "Audit not passed yet".to_string(),
        });
    }
    if !shared_mechanism::is_tron_chain(&withdraw.chain_code) {
        reasons.push(StageReason {
            code: "not_tron",
            message: "Resource gate is only required for TRON withdraw".to_string(),
        });
    }
    if withdraw.resource_gate_released_at.is_some() {
        reasons.push(StageReason {
            code: "resource_gate_released",
            message: "Resource gate already released".to_string(),
        });
    }
    if withdraw.resource_block_reason == Some(ApiResourceBlockReason::NeedPlatformDelegate) {
        reasons.push(StageReason {
            code: "waiting_platform_delegate_result",
            message: "Waiting for platform delegation result".to_string(),
        });
    }
    if withdraw.resource_block_reason == Some(ApiResourceBlockReason::NeedLocalDelegate)
        && withdraw.resource_dependency_trade_no.is_some()
    {
        reasons.push(StageReason {
            code: "waiting_local_delegate_task",
            message: "Waiting for local delegation task".to_string(),
        });
    }
    if withdraw.raw_tx.is_some() {
        reasons.push(StageReason {
            code: "raw_tx_exists",
            message: "Raw tx already exists".to_string(),
        });
    }
    if withdraw.transaction_time.is_some() {
        reasons.push(StageReason {
            code: "transaction_time_exists",
            message: "Transaction already committed".to_string(),
        });
    }
    if withdraw.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if withdraw.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance = withdraw.tx_ack_sent_at.is_some()
        && withdraw.audit_passed_at.is_some()
        && shared_mechanism::is_tron_chain(&withdraw.chain_code)
        && withdraw.resource_gate_released_at.is_none()
        && (withdraw.resource_block_reason.is_none()
            || (withdraw.resource_block_reason == Some(ApiResourceBlockReason::NeedLocalDelegate)
                && withdraw.resource_dependency_trade_no.is_none()))
        && withdraw.raw_tx.is_none()
        && withdraw.transaction_time.is_none()
        && withdraw.finished_at.is_none()
        && withdraw.err_code.is_none();

    StageEval { can_advance, reasons }
}

fn evaluate_need_tx_ack(withdraw: &ApiWithdrawEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if withdraw.tx_ack_sent_at.is_some() {
        reasons
            .push(StageReason { code: "tx_ack_sent", message: "Tx ACK already sent".to_string() });
    }
    if withdraw.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if withdraw.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance = withdraw.tx_ack_sent_at.is_none()
        && withdraw.finished_at.is_none()
        && withdraw.err_code.is_none();

    StageEval { can_advance, reasons }
}

fn evaluate_can_build(withdraw: &ApiWithdrawEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if withdraw.tx_ack_sent_at.is_none() {
        reasons.push(StageReason {
            code: "tx_ack_not_sent",
            message: "Tx ACK not sent yet".to_string(),
        });
    }
    if withdraw.audit_passed_at.is_none() {
        reasons.push(StageReason {
            code: "audit_not_passed",
            message: "Audit not passed yet".to_string(),
        });
    }
    if withdraw.raw_tx.is_some() {
        reasons.push(StageReason {
            code: "raw_tx_exists",
            message: "Raw tx already exists".to_string(),
        });
    }
    if !withdraw_resource_gate_released_for_build(withdraw) {
        reasons.push(StageReason {
            code: "resource_gate_not_released",
            message: "TRON resource gate has not been released yet".to_string(),
        });
    }
    if withdraw.transaction_time.is_some() {
        reasons.push(StageReason {
            code: "transaction_time_exists",
            message: "Transaction already committed".to_string(),
        });
    }
    if withdraw.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if withdraw.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance = withdraw.tx_ack_sent_at.is_some()
        && withdraw.audit_passed_at.is_some()
        && withdraw.raw_tx.is_none()
        && withdraw_resource_gate_released_for_build(withdraw)
        && withdraw.transaction_time.is_none()
        && withdraw.finished_at.is_none()
        && withdraw.err_code.is_none();

    StageEval { can_advance, reasons }
}

fn evaluate_can_broadcast(withdraw: &ApiWithdrawEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if withdraw.tx_ack_sent_at.is_none() {
        reasons.push(StageReason {
            code: "tx_ack_not_sent",
            message: "Tx ACK not sent yet".to_string(),
        });
    }
    if withdraw.raw_tx.is_none() {
        reasons.push(StageReason {
            code: "raw_tx_not_exists",
            message: "Raw tx not exists".to_string(),
        });
    }
    if withdraw.last_broadcast_at.is_some() {
        reasons.push(StageReason {
            code: "already_broadcasted",
            message: "Already broadcasted".to_string(),
        });
    }
    if withdraw.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if withdraw.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }
    if shared_mechanism::tracks_broadcast_uncertain_state(&withdraw.chain_code)
        && withdraw.broadcast_uncertain_since_at.is_some()
    {
        reasons.push(StageReason {
            code: "broadcast_uncertain_in_progress",
            message: "Tx is in uncertain broadcast state; recover owns progression".to_string(),
        });
    }

    let can_advance = withdraw.tx_ack_sent_at.is_some()
        && withdraw.raw_tx.is_some()
        && withdraw.last_broadcast_at.is_none()
        && (!shared_mechanism::tracks_broadcast_uncertain_state(&withdraw.chain_code)
            || withdraw.broadcast_uncertain_since_at.is_none())
        && withdraw.finished_at.is_none()
        && withdraw.err_code.is_none();

    StageEval { can_advance, reasons }
}

fn evaluate_need_recover(withdraw: &ApiWithdrawEntity) -> StageEval {
    let mut reasons = SmallVec::new();
    let needs_resource_recovery = shared_mechanism::tron_withdraw_resource_needs_recovery(
        &withdraw.chain_code,
        Some(withdraw.token_addr.as_db_str()),
        &withdraw.resource_consume,
    );

    if withdraw.tx_hash.is_none() {
        reasons.push(StageReason {
            code: "tx_hash_not_exists",
            message: "Tx hash not exists".to_string(),
        });
    }
    if withdraw.transaction_time.is_some() && !needs_resource_recovery {
        reasons.push(StageReason {
            code: "transaction_time_exists",
            message: "Transaction time already exists".to_string(),
        });
    }
    if withdraw.tx_exec_receipt_uploaded_at.is_some() && !needs_resource_recovery {
        reasons.push(StageReason {
            code: "tx_exec_receipt_uploaded",
            message: "Tx exec receipt already uploaded; auto recover disabled".to_string(),
        });
    }
    if shared_mechanism::tracks_broadcast_uncertain_state(&withdraw.chain_code)
        && withdraw.raw_tx.is_some()
        && withdraw.last_broadcast_at.is_none()
        && withdraw.broadcast_uncertain_since_at.is_none()
    {
        reasons.push(StageReason {
            code: "broadcast_not_attempted",
            message: "Raw tx exists but no uncertain marker; broadcast should proceed first"
                .to_string(),
        });
    }
    if withdraw.finished_at.is_some() && !needs_resource_recovery {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if withdraw.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance = withdraw.tx_hash.is_some()
        && ((withdraw.transaction_time.is_none()
            && withdraw.tx_exec_receipt_uploaded_at.is_none())
            || needs_resource_recovery)
        && !(shared_mechanism::tracks_broadcast_uncertain_state(&withdraw.chain_code)
            && withdraw.raw_tx.is_some()
            && withdraw.last_broadcast_at.is_none()
            && withdraw.broadcast_uncertain_since_at.is_none())
        && (withdraw.finished_at.is_none() || needs_resource_recovery)
        && withdraw.err_code.is_none();

    StageEval { can_advance, reasons }
}

fn evaluate_need_tx_exec_receipt_upload(withdraw: &ApiWithdrawEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if withdraw.tx_exec_receipt_uploaded_at.is_some() {
        reasons.push(StageReason {
            code: "receipt_uploaded",
            message: "Receipt already uploaded".to_string(),
        });
    }
    if withdraw.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    let has_confirmed_or_failed_result = withdraw.chain_success_at.is_some()
        || withdraw.transaction_time.is_some()
        || withdraw.chain_failed_at.is_some()
        || withdraw.err_code.is_some();
    if !has_confirmed_or_failed_result {
        reasons.push(StageReason {
            code: "execution_result_unconfirmed",
            message: if withdraw.last_broadcast_at.is_some() {
                "Broadcast visible, waiting for chain confirmation".to_string()
            } else {
                "Execution result not confirmed yet".to_string()
            },
        });
    }

    let can_advance = withdraw.finished_at.is_none()
        && withdraw.tx_exec_receipt_uploaded_at.is_none()
        && has_confirmed_or_failed_result;

    StageEval { can_advance, reasons }
}

fn evaluate_need_tx_res_ack(withdraw: &ApiWithdrawEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if withdraw.tx_res_received_at.is_none() {
        reasons.push(StageReason {
            code: "tx_res_not_received",
            message: "SER tx result push (AWM_ORDER_TRANS_RES) not received".to_string(),
        });
    }
    if withdraw.transaction_time.is_none() {
        reasons.push(StageReason {
            code: "tx_time_missing",
            message: "Transaction time missing".to_string(),
        });
    }
    if withdraw.tx_res_ack_sent_at.is_some() {
        reasons.push(StageReason {
            code: "tx_res_ack_sent",
            message: "Tx res ACK already sent".to_string(),
        });
    }
    if withdraw.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if withdraw.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance = withdraw.tx_res_received_at.is_some()
        && withdraw.transaction_time.is_some()
        && withdraw.tx_res_ack_sent_at.is_none()
        && withdraw.finished_at.is_none()
        && withdraw.err_code.is_none();

    StageEval { can_advance, reasons }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use wallet_database::entities::{
        api_resource_gate::ApiResourceGateResult,
        api_trade_type::ApiTradeType,
        api_withdraw::{ApiWithdrawStatus, WithdrawFailureStage},
        asset_token_key::AssetTokenKey,
    };

    fn base_withdraw() -> ApiWithdrawEntity {
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
            trade_no: "W_TEST".to_string(),
            trade_type: ApiTradeType::Withdraw,
            init_status: ApiWithdrawStatus::Init,
            status: ApiWithdrawStatus::Init,
            nonce: 0,
            tx_hash: Some("h".to_string()),
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
            tx_exec_receipt_uploaded_at: Some(Utc::now()),
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
    fn need_tx_res_ack_requires_tx_res_received_at() {
        let mut w = base_withdraw();
        w.transaction_time = Some(Utc::now());
        w.tx_res_received_at = None;

        let eval = evaluate_point(AdvancementPoint::NeedTxResAck, &w);
        assert!(!eval.can_advance);

        w.tx_res_received_at = Some(Utc::now());
        let eval2 = evaluate_point(AdvancementPoint::NeedTxResAck, &w);
        assert!(eval2.can_advance);
    }

    #[test]
    fn need_tx_exec_receipt_upload_allows_transaction_time_without_last_broadcast() {
        let mut w = base_withdraw();
        w.tx_exec_receipt_uploaded_at = None;
        w.last_broadcast_at = None;
        w.transaction_time = Some(Utc::now());

        let eval = evaluate_point(AdvancementPoint::NeedTxExecReceiptUpload, &w);
        assert!(eval.can_advance);
    }

    #[test]
    fn need_tx_exec_receipt_upload_rejects_broadcast_only_pending() {
        let mut w = base_withdraw();
        w.tx_exec_receipt_uploaded_at = None;
        w.last_broadcast_at = Some(Utc::now());

        let eval = evaluate_point(AdvancementPoint::NeedTxExecReceiptUpload, &w);
        assert!(!eval.can_advance);
        assert!(eval.reasons.iter().any(|r| r.code == "execution_result_unconfirmed"));
    }

    #[test]
    fn need_recover_allows_broadcasted_tx_without_transaction_time() {
        let mut w = base_withdraw();
        w.tx_exec_receipt_uploaded_at = None;
        w.last_broadcast_at = Some(Utc::now());

        let eval = evaluate_point(AdvancementPoint::NeedRecover, &w);
        assert!(eval.can_advance);
    }

    #[test]
    fn need_recover_allows_confirmed_tron_token_with_zero_resources() {
        let mut w = base_withdraw();
        w.chain_code = "tron".to_string();
        w.token_addr = AssetTokenKey::Contract("TToken".to_string());
        w.tx_hash = Some("tx-hash".to_string());
        w.last_broadcast_at = Some(Utc::now());
        w.transaction_time = Some(Utc::now());
        w.tx_exec_receipt_uploaded_at = Some(Utc::now());
        w.resource_consume = r#"{"bandwidth":0,"energy":0}"#.to_string();

        let eval = evaluate_point(AdvancementPoint::NeedRecover, &w);

        assert!(eval.can_advance);

        w.resource_consume = r#"{"bandwidth":0,"energy":13532}"#.to_string();
        let completed = evaluate_point(AdvancementPoint::NeedRecover, &w);
        assert!(!completed.can_advance);
    }

    #[test]
    fn tron_broadcast_uncertain_blocks_broadcast_and_allows_recover() {
        let mut w = base_withdraw();
        w.chain_code = "tron".to_string();
        w.tx_exec_receipt_uploaded_at = None;
        w.last_broadcast_at = None;
        w.broadcast_uncertain_since_at = Some(Utc::now());

        let broadcast = evaluate_point(AdvancementPoint::CanBroadcast, &w);
        assert!(!broadcast.can_advance);
        assert!(broadcast.reasons.iter().any(|r| r.code == "broadcast_uncertain_in_progress"));

        let recover = evaluate_point(AdvancementPoint::NeedRecover, &w);
        assert!(recover.can_advance);
    }

    #[test]
    fn can_build_rejects_committed_or_finished_rows() {
        let mut committed = base_withdraw();
        committed.raw_tx = None;
        committed.transaction_time = Some(Utc::now());

        let mut finished = base_withdraw();
        finished.raw_tx = None;
        finished.finished_at = Some(Utc::now());

        let committed_eval = evaluate_point(AdvancementPoint::CanBuild, &committed);
        let finished_eval = evaluate_point(AdvancementPoint::CanBuild, &finished);

        assert!(!committed_eval.can_advance);
        assert!(
            committed_eval.reasons.iter().any(|reason| reason.code == "transaction_time_exists")
        );
        assert!(!finished_eval.can_advance);
        assert!(finished_eval.reasons.iter().any(|reason| reason.code == "finished"));
    }

    #[test]
    fn withdraw_resource_gate_advances_for_unreleased_tron_only() {
        let mut tron = base_withdraw();
        tron.raw_tx = None;
        tron.tx_hash = None;
        tron.tx_exec_receipt_uploaded_at = None;

        let tron_eval = evaluate_point(AdvancementPoint::NeedResourceGate, &tron);
        assert!(tron_eval.can_advance);

        tron.resource_gate_released_at = Some(Utc::now());
        let released_eval = evaluate_point(AdvancementPoint::NeedResourceGate, &tron);
        assert!(!released_eval.can_advance);

        let mut sol = base_withdraw();
        sol.chain_code = "sol".to_string();
        sol.raw_tx = None;
        sol.tx_hash = None;
        sol.tx_exec_receipt_uploaded_at = None;
        let sol_eval = evaluate_point(AdvancementPoint::NeedResourceGate, &sol);
        assert!(!sol_eval.can_advance);
    }

    #[test]
    fn withdraw_resource_gate_waits_when_resource_dependency_is_in_progress() {
        let mut waiting_platform = base_withdraw();
        waiting_platform.raw_tx = None;
        waiting_platform.tx_hash = None;
        waiting_platform.tx_exec_receipt_uploaded_at = None;
        waiting_platform.resource_block_reason = Some(ApiResourceBlockReason::NeedPlatformDelegate);
        waiting_platform.resource_dependency_trade_no = Some("DL_W_1".to_string());

        let platform_eval = evaluate_point(AdvancementPoint::NeedResourceGate, &waiting_platform);
        assert!(!platform_eval.can_advance);
        assert!(
            platform_eval
                .reasons
                .iter()
                .any(|reason| reason.code == "waiting_platform_delegate_result")
        );

        let mut waiting_local = waiting_platform.clone();
        waiting_local.resource_block_reason = Some(ApiResourceBlockReason::NeedLocalDelegate);
        waiting_local.resource_dependency_trade_no = Some("rsc_local_delegate_W".to_string());
        let local_eval = evaluate_point(AdvancementPoint::NeedResourceGate, &waiting_local);
        assert!(!local_eval.can_advance);
        assert!(
            local_eval.reasons.iter().any(|reason| reason.code == "waiting_local_delegate_task")
        );

        let mut needs_local_task = waiting_local.clone();
        needs_local_task.resource_dependency_trade_no = None;
        let retry_eval = evaluate_point(AdvancementPoint::NeedResourceGate, &needs_local_task);
        assert!(retry_eval.can_advance);
    }

    #[test]
    fn platform_delegate_apply_advances_only_for_persisted_fact_gap() {
        let mut withdraw = base_withdraw();
        withdraw.resource_block_reason = Some(ApiResourceBlockReason::NeedPlatformDelegate);
        withdraw.resource_dependency_type = Some(ApiResourceDependencyType::PlatformDelegate);
        withdraw.resource_dependency_trade_no = None;
        withdraw.resource_gate_result = None;
        withdraw.resource_gate_released_at = None;
        withdraw.raw_tx = None;
        withdraw.transaction_time = None;
        withdraw.finished_at = None;
        withdraw.err_code = None;

        let pending = evaluate_point(AdvancementPoint::NeedPlatformDelegateApply, &withdraw);
        assert!(pending.can_advance);

        withdraw.resource_dependency_trade_no = Some(withdraw.trade_no.clone());
        let accepted = evaluate_point(AdvancementPoint::NeedPlatformDelegateApply, &withdraw);
        assert!(!accepted.can_advance);
        assert!(
            accepted
                .reasons
                .iter()
                .any(|reason| reason.code == "platform_delegate_apply_fact_gap_closed")
        );

        withdraw.resource_dependency_trade_no = None;
        withdraw.resource_gate_result = Some(ApiResourceGateResult::PlatformDelegateSuccess);
        let result_first = evaluate_point(AdvancementPoint::NeedPlatformDelegateApply, &withdraw);
        assert!(!result_first.can_advance);
        assert!(
            result_first
                .reasons
                .iter()
                .any(|reason| reason.code == "platform_delegate_result_received")
        );
    }

    #[test]
    fn tron_build_requires_resource_gate_release_even_when_other_facts_are_ready() {
        let mut withdraw = base_withdraw();
        withdraw.raw_tx = None;
        withdraw.resource_gate_released_at = None;

        let blocked = evaluate_point(AdvancementPoint::CanBuild, &withdraw);
        assert!(!blocked.can_advance);
        assert!(blocked.reasons.iter().any(|reason| reason.code == "resource_gate_not_released"));

        withdraw.resource_gate_released_at = Some(Utc::now());
        assert!(evaluate_point(AdvancementPoint::CanBuild, &withdraw).can_advance);
    }

    #[test]
    fn withdraw_resource_gate_blocks_tron_build_until_released() {
        let mut tron = base_withdraw();
        tron.raw_tx = None;
        tron.tx_hash = None;
        tron.tx_exec_receipt_uploaded_at = None;
        tron.resource_gate_released_at = None;

        let blocked_eval = evaluate_point(AdvancementPoint::CanBuild, &tron);
        assert!(!blocked_eval.can_advance);
        assert!(
            blocked_eval.reasons.iter().any(|reason| reason.code == "resource_gate_not_released")
        );

        tron.resource_gate_released_at = Some(Utc::now());
        let released_eval = evaluate_point(AdvancementPoint::CanBuild, &tron);
        assert!(released_eval.can_advance);
    }
}
