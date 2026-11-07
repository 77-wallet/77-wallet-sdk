use chrono::{DateTime, Duration, Utc};
use smallvec::SmallVec;
use wallet_database::entities::api_resource_delegation::{
    ApiResourceDelegationEntity, ApiResourceDelegationOperationType, ApiResourceDelegationSource,
    ApiResourceDelegationStatus,
};

use crate::infrastructure::api_trans::resource_chain_facts::{
    resource_chain_failed, resource_chain_success, resource_success_receipt_uploadable,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceDelegationStage {
    NeedTaskAck,
    CanExecute,
    NeedRecover,
    NeedTxExecReceiptUpload,
    NeedResultAck,
}

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

pub fn evaluate_stage(
    stage: ResourceDelegationStage,
    task: &ApiResourceDelegationEntity,
    now: DateTime<Utc>,
) -> StageEval {
    match stage {
        ResourceDelegationStage::NeedTaskAck => evaluate_need_task_ack(task, now),
        ResourceDelegationStage::CanExecute => evaluate_can_execute(task, now),
        ResourceDelegationStage::NeedRecover => evaluate_need_recover(task, now),
        ResourceDelegationStage::NeedTxExecReceiptUpload => evaluate_need_receipt_upload(task),
        ResourceDelegationStage::NeedResultAck => evaluate_need_result_ack(task, now),
    }
}

fn build_slot_available(task: &ApiResourceDelegationEntity, now: DateTime<Utc>) -> bool {
    task.building_at.map(|building_at| building_at <= now - Duration::minutes(5)).unwrap_or(true)
}

fn has_tx_hash(task: &ApiResourceDelegationEntity) -> bool {
    task.tx_hash.as_deref().map(|hash| !hash.trim().is_empty()).unwrap_or(false)
}

fn platform_source_requires_ack(task: &ApiResourceDelegationEntity) -> bool {
    task.source == ApiResourceDelegationSource::Platform
}

fn evaluate_need_task_ack(task: &ApiResourceDelegationEntity, _now: DateTime<Utc>) -> StageEval {
    let mut reasons = SmallVec::new();

    if task.task_ack_sent_at.is_some() {
        reasons.push(StageReason {
            code: "task_ack_sent",
            message: "Resource task ACK already sent".to_string(),
        });
    }
    let can_advance = task.task_ack_sent_at.is_none();
    StageEval { can_advance, reasons }
}

fn evaluate_can_execute(task: &ApiResourceDelegationEntity, now: DateTime<Utc>) -> StageEval {
    let mut reasons = SmallVec::new();

    if platform_source_requires_ack(task) && task.task_ack_sent_at.is_none() {
        reasons.push(StageReason {
            code: "task_ack_not_sent",
            message: "Platform resource task ACK not sent yet".to_string(),
        });
    }
    if task.finished_at.is_some() {
        reasons.push(StageReason {
            code: "result_received",
            message: "Backend resource result already received".to_string(),
        });
    }
    if task.err_code.as_deref().is_some_and(|err| !err.trim().is_empty()) {
        reasons.push(StageReason {
            code: "error",
            message: "Resource task has error fact".to_string(),
        });
    }
    if !build_slot_available(task, now) {
        reasons.push(StageReason {
            code: "build_slot_claimed",
            message: "Build slot is still within claim TTL".to_string(),
        });
    }
    if has_tx_hash(task) {
        reasons.push(StageReason {
            code: "tx_hash_exists",
            message: "Resource task already has tx hash".to_string(),
        });
    }

    let can_advance = (!platform_source_requires_ack(task) || task.task_ack_sent_at.is_some())
        && task.finished_at.is_none()
        && task.err_code.is_none()
        && build_slot_available(task, now)
        && !has_tx_hash(task);
    StageEval { can_advance, reasons }
}

fn evaluate_need_recover(task: &ApiResourceDelegationEntity, now: DateTime<Utc>) -> StageEval {
    let mut reasons = SmallVec::new();

    if platform_source_requires_ack(task) && task.task_ack_sent_at.is_none() {
        reasons.push(StageReason {
            code: "task_ack_not_sent",
            message: "Platform resource task ACK not sent yet".to_string(),
        });
    }
    if task.finished_at.is_some() {
        reasons.push(StageReason {
            code: "result_received",
            message: "Backend resource result already received".to_string(),
        });
    }
    if task.err_code.as_deref().is_some_and(|err| !err.trim().is_empty()) {
        reasons.push(StageReason {
            code: "error",
            message: "Resource task has error fact".to_string(),
        });
    }
    if !has_tx_hash(task) {
        reasons.push(StageReason {
            code: "tx_hash_not_exists",
            message: "Resource task tx hash not exists".to_string(),
        });
    }

    let can_advance = (!platform_source_requires_ack(task) || task.task_ack_sent_at.is_some())
        && task.finished_at.is_none()
        && task.err_code.is_none()
        && has_tx_hash(task);
    StageEval { can_advance, reasons }
}

fn evaluate_need_receipt_upload(task: &ApiResourceDelegationEntity) -> StageEval {
    let mut reasons = SmallVec::new();

    if task.tx_exec_receipt_uploaded_at.is_some() {
        reasons.push(StageReason {
            code: "receipt_uploaded",
            message: "Resource tx execution receipt already uploaded".to_string(),
        });
    }
    if !resource_success_receipt_uploadable(task) && !resource_chain_failed(task) {
        reasons.push(StageReason {
            code: "execution_result_unconfirmed",
            message: "Resource chain result is not confirmed".to_string(),
        });
    }

    let can_advance = task.tx_exec_receipt_uploaded_at.is_none()
        && (resource_success_receipt_uploadable(task) || resource_chain_failed(task));
    StageEval { can_advance, reasons }
}

fn evaluate_need_result_ack(task: &ApiResourceDelegationEntity, _now: DateTime<Utc>) -> StageEval {
    let mut reasons = SmallVec::new();

    if task.finished_at.is_none() {
        reasons.push(StageReason {
            code: "result_not_received",
            message: "Backend resource result not received".to_string(),
        });
    }
    if task.result_payload.is_none() {
        reasons.push(StageReason {
            code: "result_payload_missing",
            message: "Backend resource result payload missing".to_string(),
        });
    }
    if task.tx_res_received_at.is_none() {
        reasons.push(StageReason {
            code: "tx_res_not_received",
            message: "Resource TX_RES push not received".to_string(),
        });
    }
    if task.result_ack_sent_at.is_some() {
        reasons.push(StageReason {
            code: "result_ack_sent",
            message: "Resource result ACK already sent".to_string(),
        });
    }
    let can_advance = task.finished_at.is_some()
        && task.result_payload.is_some()
        && task.tx_res_received_at.is_some()
        && task.result_ack_sent_at.is_none();
    StageEval { can_advance, reasons }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wallet_database::entities::{
        api_resource_delegation::{ApiResourceDelegationMode, ApiResourceDelegationResultStatus},
        api_resource_type::ApiResourceType,
        api_trade_type::ApiTradeType,
    };

    fn task(now: DateTime<Utc>) -> ApiResourceDelegationEntity {
        ApiResourceDelegationEntity {
            id: 1,
            uid: "uid".to_string(),
            source: ApiResourceDelegationSource::Platform,
            operation_type: ApiResourceDelegationOperationType::Delegate,
            origin_trade_no: Some("W1".to_string()),
            origin_trade_type: Some(ApiTradeType::Withdraw as i64),
            resource_trade_no: "WD_RSC_1".to_string(),
            chain_code: "tron".to_string(),
            owner_address: "owner".to_string(),
            receiver_address: "receiver".to_string(),
            delegation_mode: ApiResourceDelegationMode::WithdrawAddress,
            permission_id: None,
            resource_type: ApiResourceType::Energy,
            native_amount: "199".to_string(),
            amount: "14650".to_string(),
            status: ApiResourceDelegationStatus::Pending,
            task_ack_sent_at: None,
            task_ack_send_count: 0,
            building_at: None,
            last_broadcast_at: None,
            tx_hash: None,
            raw_tx: None,
            chain_success_at: None,
            chain_failed_at: None,
            transaction_time: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_exec_receipt_uploaded_at: None,
            result_status: None,
            tx_res_received_at: None,
            finished_at: None,
            result_ack_sent_at: None,
            result_ack_send_count: 0,
            result_payload: None,
            fail_type: None,
            err_code: None,
            err_msg: None,
            created_at: now,
            updated_at: None,
        }
    }

    fn reason_codes(eval: &StageEval) -> Vec<&'static str> {
        eval.reasons.iter().map(|reason| reason.code).collect()
    }

    #[test]
    fn resource_delegate_predicate_task_ack_requires_unsent() {
        let now = Utc::now();
        let mut record = task(now);

        assert!(evaluate_stage(ResourceDelegationStage::NeedTaskAck, &record, now).can_advance);

        record.task_ack_sent_at = Some(now);
        let eval = evaluate_stage(ResourceDelegationStage::NeedTaskAck, &record, now);
        assert!(!eval.can_advance);
        assert!(reason_codes(&eval).contains(&"task_ack_sent"));
    }

    #[test]
    fn resource_delegate_predicate_execute_matches_platform_ack_and_build_slot() {
        let now = Utc::now();
        let mut record = task(now);

        let before_ack = evaluate_stage(ResourceDelegationStage::CanExecute, &record, now);
        assert!(!before_ack.can_advance);
        assert!(reason_codes(&before_ack).contains(&"task_ack_not_sent"));

        record.task_ack_sent_at = Some(now);
        assert!(evaluate_stage(ResourceDelegationStage::CanExecute, &record, now).can_advance);

        record.status = ApiResourceDelegationStatus::Success;
        assert!(evaluate_stage(ResourceDelegationStage::CanExecute, &record, now).can_advance);

        record.building_at = Some(now);
        let claimed = evaluate_stage(ResourceDelegationStage::CanExecute, &record, now);
        assert!(!claimed.can_advance);
        assert!(reason_codes(&claimed).contains(&"build_slot_claimed"));
    }

    #[test]
    fn resource_delegate_predicate_recover_requires_broadcast_hash() {
        let now = Utc::now();
        let mut record = task(now);
        record.task_ack_sent_at = Some(now);

        let no_hash = evaluate_stage(ResourceDelegationStage::NeedRecover, &record, now);
        assert!(!no_hash.can_advance);
        assert!(reason_codes(&no_hash).contains(&"tx_hash_not_exists"));

        record.tx_hash = Some("hash".to_string());
        assert!(evaluate_stage(ResourceDelegationStage::NeedRecover, &record, now).can_advance);
    }

    #[test]
    fn resource_delegate_predicate_receipt_requires_confirmed_or_failed_fact() {
        let now = Utc::now();
        let mut record = task(now);
        record.tx_hash = Some("hash".to_string());

        let pending =
            evaluate_stage(ResourceDelegationStage::NeedTxExecReceiptUpload, &record, now);
        assert!(!pending.can_advance);
        assert!(reason_codes(&pending).contains(&"execution_result_unconfirmed"));

        record.chain_success_at = Some(now);
        assert!(
            evaluate_stage(ResourceDelegationStage::NeedTxExecReceiptUpload, &record, now)
                .can_advance
        );

        record.tx_exec_receipt_uploaded_at = Some(now);
        let uploaded =
            evaluate_stage(ResourceDelegationStage::NeedTxExecReceiptUpload, &record, now);
        assert!(!uploaded.can_advance);
        assert!(reason_codes(&uploaded).contains(&"receipt_uploaded"));
    }

    #[test]
    fn resource_delegate_predicate_result_ack_requires_backend_result_facts() {
        let now = Utc::now();
        let mut record = task(now);

        let missing = evaluate_stage(ResourceDelegationStage::NeedResultAck, &record, now);
        assert!(!missing.can_advance);
        assert!(reason_codes(&missing).contains(&"result_not_received"));
        assert!(reason_codes(&missing).contains(&"tx_res_not_received"));

        record.result_status = Some(ApiResourceDelegationResultStatus::Success);
        record.finished_at = Some(now);
        record.tx_res_received_at = Some(now);
        record.result_payload = Some("{}".to_string());
        assert!(evaluate_stage(ResourceDelegationStage::NeedResultAck, &record, now).can_advance);

        record.result_ack_sent_at = Some(now);
        let acked = evaluate_stage(ResourceDelegationStage::NeedResultAck, &record, now);
        assert!(!acked.can_advance);
        assert!(reason_codes(&acked).contains(&"result_ack_sent"));
    }
}
