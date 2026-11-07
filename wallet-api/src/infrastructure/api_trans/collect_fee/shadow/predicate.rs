use smallvec::SmallVec;
use wallet_database::entities::api_fee::ApiFeeEntity;

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

pub fn evaluate_point(point: AdvancementPoint, fee: &ApiFeeEntity) -> StageEval {
    match point {
        AdvancementPoint::NeedTxAck => evaluate_need_tx_ack(fee),
        AdvancementPoint::CanBuild => evaluate_can_build(fee),
        AdvancementPoint::CanBroadcast => evaluate_can_broadcast(fee),
        AdvancementPoint::NeedRecover => evaluate_need_recover(fee),
        AdvancementPoint::NeedTxExecReceiptUpload => evaluate_need_tx_exec_receipt_upload(fee),
        AdvancementPoint::NeedTxResAck => evaluate_need_tx_res_ack(fee),
        AdvancementPoint::FullyBlocked => {
            StageEval { can_advance: false, reasons: SmallVec::new() }
        }
    }
}

fn evaluate_need_tx_ack(fee: &ApiFeeEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if fee.tx_ack_sent_at.is_some() {
        reasons
            .push(StageReason { code: "tx_ack_sent", message: "Tx ACK already sent".to_string() });
    }
    if fee.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if fee.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance =
        fee.tx_ack_sent_at.is_none() && fee.finished_at.is_none() && fee.err_code.is_none();

    StageEval { can_advance, reasons }
}

fn evaluate_can_build(fee: &ApiFeeEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if fee.tx_ack_sent_at.is_none() {
        reasons.push(StageReason {
            code: "tx_ack_not_sent",
            message: "Tx ACK not sent yet".to_string(),
        });
    }
    if fee.raw_tx.is_some() {
        reasons.push(StageReason {
            code: "raw_tx_exists",
            message: "Raw tx already exists".to_string(),
        });
    }
    if fee.transaction_time.is_some() {
        reasons.push(StageReason {
            code: "transaction_time_exists",
            message: "Transaction already committed".to_string(),
        });
    }
    if fee.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if fee.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance = fee.tx_ack_sent_at.is_some()
        && fee.raw_tx.is_none()
        && fee.transaction_time.is_none()
        && fee.finished_at.is_none()
        && fee.err_code.is_none();

    StageEval { can_advance, reasons }
}

fn evaluate_can_broadcast(fee: &ApiFeeEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if fee.tx_ack_sent_at.is_none() {
        reasons.push(StageReason {
            code: "tx_ack_not_sent",
            message: "Tx ACK not sent yet".to_string(),
        });
    }
    if fee.raw_tx.is_none() {
        reasons.push(StageReason {
            code: "raw_tx_not_exists",
            message: "Raw tx not exists".to_string(),
        });
    }
    if fee.last_broadcast_at.is_some() {
        reasons.push(StageReason {
            code: "already_broadcasted",
            message: "Already broadcasted".to_string(),
        });
    }
    if fee.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if fee.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }
    if shared_mechanism::tracks_broadcast_uncertain_state(&fee.chain_code)
        && fee.broadcast_uncertain_since_at.is_some()
    {
        reasons.push(StageReason {
            code: "broadcast_uncertain_in_progress",
            message: "Tx is in uncertain broadcast state; recover owns progression".to_string(),
        });
    }

    let can_advance = fee.tx_ack_sent_at.is_some()
        && fee.raw_tx.is_some()
        && fee.last_broadcast_at.is_none()
        && fee.transaction_time.is_none()
        && fee.finished_at.is_none()
        && fee.err_code.is_none()
        && (!shared_mechanism::tracks_broadcast_uncertain_state(&fee.chain_code)
            || fee.broadcast_uncertain_since_at.is_none());

    StageEval { can_advance, reasons }
}

fn evaluate_need_recover(fee: &ApiFeeEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if fee.tx_hash.is_none() {
        reasons.push(StageReason {
            code: "tx_hash_not_exists",
            message: "Tx hash not exists".to_string(),
        });
    }
    if fee.transaction_time.is_some() {
        reasons.push(StageReason {
            code: "transaction_time_exists",
            message: "Transaction time already exists".to_string(),
        });
    }
    if fee.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if fee.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }
    if fee.tx_exec_receipt_uploaded_at.is_some() {
        reasons.push(StageReason {
            code: "tx_exec_receipt_uploaded",
            message: "Tx exec receipt already uploaded".to_string(),
        });
    }
    if shared_mechanism::tracks_broadcast_uncertain_state(&fee.chain_code)
        && fee.raw_tx.is_some()
        && fee.last_broadcast_at.is_none()
        && fee.broadcast_uncertain_since_at.is_none()
    {
        reasons.push(StageReason {
            code: "broadcast_not_attempted",
            message: "Raw tx exists but broadcast has not entered uncertain or executed yet"
                .to_string(),
        });
    }

    let can_advance = fee.tx_hash.is_some()
        && fee.transaction_time.is_none()
        && fee.finished_at.is_none()
        && fee.err_code.is_none()
        && fee.tx_exec_receipt_uploaded_at.is_none();

    StageEval { can_advance, reasons }
}

fn evaluate_need_tx_exec_receipt_upload(fee: &ApiFeeEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if fee.tx_exec_receipt_uploaded_at.is_some() {
        reasons.push(StageReason {
            code: "receipt_uploaded",
            message: "Receipt already uploaded".to_string(),
        });
    }
    if fee.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if fee.err_code.is_none() && fee.transaction_time.is_none() {
        reasons.push(StageReason {
            code: "pending_execution_fact",
            message: "Waiting for confirmed execution fact".to_string(),
        });
    }

    let can_advance = fee.finished_at.is_none()
        && fee.tx_exec_receipt_uploaded_at.is_none()
        && (fee.err_code.is_some() || fee.transaction_time.is_some());

    StageEval { can_advance, reasons }
}

fn evaluate_need_tx_res_ack(fee: &ApiFeeEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if fee.tx_res_received_at.is_none() {
        reasons.push(StageReason {
            code: "tx_res_not_received",
            message: "SER tx result push (AWM_ORDER_TRANS_RES) not received".to_string(),
        });
    }
    if fee.transaction_time.is_none() {
        reasons.push(StageReason {
            code: "transaction_time_not_exists",
            message: "Transaction time not exists".to_string(),
        });
    }

    if fee.tx_res_ack_sent_at.is_some() {
        reasons.push(StageReason {
            code: "tx_res_ack_sent",
            message: "Tx res ACK already sent".to_string(),
        });
    }
    if fee.finished_at.is_some() {
        reasons
            .push(StageReason { code: "finished", message: "Order already finished".to_string() });
    }
    if fee.err_code.is_some() {
        reasons.push(StageReason { code: "error", message: "Order has error".to_string() });
    }

    let can_advance = fee.tx_res_received_at.is_some()
        && fee.transaction_time.is_some()
        && fee.tx_res_ack_sent_at.is_none()
        && fee.finished_at.is_none()
        && fee.err_code.is_none();

    StageEval { can_advance, reasons }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use wallet_database::entities::{api_fee::ApiFeeStatus, asset_token_key::AssetTokenKey};

    fn base_fee() -> ApiFeeEntity {
        ApiFeeEntity {
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
            trade_no: "F_TEST".to_string(),
            trade_type: 3,
            status: ApiFeeStatus::Init,
            nonce: 0,
            tx_hash: Some("h".to_string()),
            raw_tx: Some("{}".to_string()),
            resource_consume: "0".to_string(),
            transaction_fee: "0".to_string(),
            transaction_time: None,
            block_height: Some("0".to_string()),
            notes: Some("".to_string()),
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: Some("".to_string()),
            tx_ack_sent_at: Some(Utc::now()),
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_exec_receipt_uploaded_at: Some(Utc::now()),
            tx_res_ack_sent_at: None,
            tx_res_received_at: None,
            finished_at: None,
            created_at: Utc::now(),
            updated_at: Some(Utc::now()),
        }
    }

    #[test]
    fn can_build_rejects_committed_or_finished_rows() {
        let mut committed = base_fee();
        committed.raw_tx = None;
        committed.transaction_time = Some(Utc::now());

        let mut finished = base_fee();
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
    fn need_tx_res_ack_requires_tx_res_received_at() {
        let mut f = base_fee();
        f.transaction_time = Some(Utc::now());
        f.tx_res_received_at = None;

        let eval = evaluate_point(AdvancementPoint::NeedTxResAck, &f);
        assert!(!eval.can_advance);

        f.tx_res_received_at = Some(Utc::now());
        let eval2 = evaluate_point(AdvancementPoint::NeedTxResAck, &f);
        assert!(eval2.can_advance);
    }

    #[test]
    fn need_tx_exec_receipt_upload_allows_transaction_time_without_last_broadcast() {
        let mut f = base_fee();
        f.tx_exec_receipt_uploaded_at = None;
        f.last_broadcast_at = None;
        f.transaction_time = Some(Utc::now());

        let eval = evaluate_point(AdvancementPoint::NeedTxExecReceiptUpload, &f);
        assert!(eval.can_advance);
    }

    #[test]
    fn need_recover_allows_broadcast_visible_pending_chain_result() {
        let mut f = base_fee();
        f.last_broadcast_at = Some(Utc::now());
        f.tx_exec_receipt_uploaded_at = None;
        f.transaction_time = None;

        let eval = evaluate_point(AdvancementPoint::NeedRecover, &f);
        assert!(eval.can_advance);
    }

    #[test]
    fn need_recover_allows_evm_uncertain_after_broadcast() {
        let mut f = base_fee();
        f.last_broadcast_at = Some(Utc::now());
        f.broadcast_uncertain_since_at = Some(Utc::now());
        f.tx_exec_receipt_uploaded_at = None;
        f.transaction_time = None;

        let eval = evaluate_point(AdvancementPoint::NeedRecover, &f);
        assert!(eval.can_advance);
        assert!(!eval.reasons.iter().any(|r| r.code == "already_broadcasted"));
    }

    #[test]
    fn tron_broadcast_uncertain_blocks_broadcast_and_allows_recover() {
        let mut f = base_fee();
        f.chain_code = "tron".to_string();
        f.last_broadcast_at = None;
        f.broadcast_uncertain_since_at = Some(Utc::now());
        f.tx_exec_receipt_uploaded_at = None;
        f.transaction_time = None;

        let broadcast = evaluate_point(AdvancementPoint::CanBroadcast, &f);
        assert!(!broadcast.can_advance);
        assert!(broadcast.reasons.iter().any(|r| r.code == "broadcast_uncertain_in_progress"));

        let recover = evaluate_point(AdvancementPoint::NeedRecover, &f);
        assert!(recover.can_advance);
    }

    #[test]
    fn need_tx_exec_receipt_upload_rejects_broadcast_only_pending() {
        let mut f = base_fee();
        f.last_broadcast_at = Some(Utc::now());
        f.tx_exec_receipt_uploaded_at = None;
        f.transaction_time = None;
        f.err_code = None;

        let eval = evaluate_point(AdvancementPoint::NeedTxExecReceiptUpload, &f);
        assert!(!eval.can_advance);
        assert!(eval.reasons.iter().any(|r| r.code == "pending_execution_fact"));
    }
}
