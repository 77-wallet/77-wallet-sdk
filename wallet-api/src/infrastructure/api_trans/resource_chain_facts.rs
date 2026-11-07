use wallet_database::entities::api_resource_delegation::{
    ApiResourceDelegationEntity, ApiResourceDelegationResultStatus,
};

fn has_text(value: Option<&str>) -> bool {
    value.map(|s| !s.trim().is_empty()).unwrap_or(false)
}

pub(crate) fn resource_chain_broadcasted(task: &ApiResourceDelegationEntity) -> bool {
    has_text(task.tx_hash.as_deref()) || task.last_broadcast_at.is_some()
}

pub(crate) fn resource_chain_success(task: &ApiResourceDelegationEntity) -> bool {
    task.chain_success_at.is_some() || task.transaction_time.is_some()
}

pub(crate) fn resource_chain_failed(task: &ApiResourceDelegationEntity) -> bool {
    task.chain_failed_at.is_some()
        || has_text(task.err_code.as_deref())
        || task.result_status == Some(ApiResourceDelegationResultStatus::Fail)
}

pub(crate) fn resource_success_receipt_uploadable(task: &ApiResourceDelegationEntity) -> bool {
    has_text(task.tx_hash.as_deref())
        && resource_chain_success(task)
        && !resource_chain_failed(task)
}

pub(crate) fn resource_failure_receipt_uploadable(task: &ApiResourceDelegationEntity) -> bool {
    resource_chain_failed(task)
}

pub(crate) fn resource_receipt_uploadable(task: &ApiResourceDelegationEntity) -> bool {
    resource_success_receipt_uploadable(task) || resource_failure_receipt_uploadable(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use wallet_database::entities::{
        api_resource_delegation::{
            ApiResourceDelegationMode, ApiResourceDelegationOperationType,
            ApiResourceDelegationSource, ApiResourceDelegationStatus,
        },
        api_resource_type::ApiResourceType,
        api_trade_type::ApiTradeType,
    };

    fn task() -> ApiResourceDelegationEntity {
        let now = Utc::now();
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
            native_amount: "0".to_string(),
            amount: "1000".to_string(),
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

    #[test]
    fn broadcast_only_resource_is_not_success_uploadable() {
        let mut task = task();
        task.tx_hash = Some("hash".to_string());
        task.last_broadcast_at = Some(Utc::now());

        assert!(resource_chain_broadcasted(&task));
        assert!(!resource_chain_success(&task));
        assert!(!resource_success_receipt_uploadable(&task));
        assert!(!resource_receipt_uploadable(&task));
    }

    #[test]
    fn confirmed_resource_is_success_uploadable() {
        let mut task = task();
        task.tx_hash = Some("hash".to_string());
        task.chain_success_at = Some(Utc::now());

        assert!(resource_chain_success(&task));
        assert!(resource_success_receipt_uploadable(&task));
        assert!(resource_receipt_uploadable(&task));
    }

    #[test]
    fn failure_fact_is_uploadable_without_success_fact() {
        let mut task = task();
        task.err_code = Some("CHAIN_ERROR".to_string());

        assert!(resource_chain_failed(&task));
        assert!(resource_failure_receipt_uploadable(&task));
        assert!(resource_receipt_uploadable(&task));
    }
}
