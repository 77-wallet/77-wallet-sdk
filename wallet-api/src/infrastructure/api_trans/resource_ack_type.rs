use wallet_database::entities::{
    api_resource_delegation::{ApiResourceDelegationEntity, ApiResourceDelegationOperationType},
    api_trade_type::ApiTradeType,
};
use wallet_transport_backend::request::api_wallet::transaction::{TransAckType, TransType};

// Resource result ACK has two local meanings even though both come from the
// backend resource-result message:
// - Platform wallet resource tasks use COL_RSC_DL/WD_RSC_DL/COL_RSC_RC/WD_RSC_RC
//   with TX_RES.
// - Merchant wallet original-order projections use COL/WD with TX_RSC_RES.
// Keep these helpers split so call sites show which side of that boundary they
// are acknowledging.
pub(crate) fn is_original_order_resource_result_fact(
    resource_task: &ApiResourceDelegationEntity,
) -> bool {
    resource_task
        .origin_trade_no
        .as_deref()
        .map(|origin| origin == resource_task.resource_trade_no)
        .unwrap_or(false)
}

/// Maps a real platform resource task to the backend transaction type.
pub(crate) fn platform_resource_task_trans_type(
    resource_task: &ApiResourceDelegationEntity,
) -> TransType {
    match (resource_task.origin_trade_type, resource_task.operation_type) {
        (Some(x), ApiResourceDelegationOperationType::Delegate)
            if x == ApiTradeType::Collect as i64 =>
        {
            TransType::ColRscDl
        }
        (Some(x), ApiResourceDelegationOperationType::Delegate)
            if x == ApiTradeType::Withdraw as i64 =>
        {
            TransType::WdRscDl
        }
        (Some(x), ApiResourceDelegationOperationType::Undelegate)
            if x == ApiTradeType::Collect as i64 =>
        {
            TransType::ColRscRc
        }
        (Some(x), ApiResourceDelegationOperationType::Undelegate)
            if x == ApiTradeType::Withdraw as i64 =>
        {
            TransType::WdRscRc
        }
        (origin_trade_type, operation_type) => {
            tracing::warn!(
                ?origin_trade_type,
                ?operation_type,
                resource_trade_no = %resource_task.resource_trade_no,
                "Unknown resource task origin trade type, fallback to COL_RSC_DL ack type"
            );
            TransType::ColRscDl
        }
    }
}

/// Maps a merchant-side original-order resource result projection to the
/// original order transaction type.
pub(crate) fn merchant_original_resource_result_trans_type(
    resource_task: &ApiResourceDelegationEntity,
) -> TransType {
    match resource_task.origin_trade_type {
        Some(x) if x == ApiTradeType::Collect as i64 => TransType::Col,
        Some(x) if x == ApiTradeType::Withdraw as i64 => TransType::Wd,
        origin_trade_type => {
            tracing::warn!(
                ?origin_trade_type,
                operation_type = ?resource_task.operation_type,
                resource_trade_no = %resource_task.resource_trade_no,
                "Unknown original-order resource result trade type, fallback to COL ack type"
            );
            TransType::Col
        }
    }
}

pub(crate) fn platform_resource_result_ack_type() -> TransAckType {
    TransAckType::TxRes
}

pub(crate) fn merchant_original_resource_result_ack_type() -> TransAckType {
    TransAckType::TxRscRes
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use wallet_database::entities::{
        api_resource_delegation::{
            ApiResourceDelegationOperationType, ApiResourceDelegationSource,
            ApiResourceDelegationStatus,
        },
        api_resource_type::ApiResourceType,
    };

    fn base_resource_task(
        origin_trade_type: ApiTradeType,
        operation_type: ApiResourceDelegationOperationType,
    ) -> ApiResourceDelegationEntity {
        ApiResourceDelegationEntity {
            id: 1,
            uid: "u".to_string(),
            source: ApiResourceDelegationSource::Platform,
            operation_type,
            origin_trade_no: Some("ORIGIN".to_string()),
            origin_trade_type: Some(origin_trade_type as i64),
            resource_trade_no: "rsc_1".to_string(),
            chain_code: "tron".to_string(),
            owner_address: "owner".to_string(),
            receiver_address: "receiver".to_string(),
            delegation_mode: wallet_database::entities::api_resource_delegation::ApiResourceDelegationMode::WithdrawAddress,
            permission_id: None,
            resource_type: ApiResourceType::Energy,
            native_amount: "1".to_string(),
            amount: "100".to_string(),
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
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    #[test]
    fn resource_task_ack_type_uses_resource_operation_type() {
        let collect =
            base_resource_task(ApiTradeType::Collect, ApiResourceDelegationOperationType::Delegate);
        assert!(matches!(platform_resource_task_trans_type(&collect), TransType::ColRscDl));

        let withdraw = base_resource_task(
            ApiTradeType::Withdraw,
            ApiResourceDelegationOperationType::Delegate,
        );
        assert!(matches!(platform_resource_task_trans_type(&withdraw), TransType::WdRscDl));

        let collect_reclaim = base_resource_task(
            ApiTradeType::Collect,
            ApiResourceDelegationOperationType::Undelegate,
        );
        assert!(matches!(platform_resource_task_trans_type(&collect_reclaim), TransType::ColRscRc));

        let withdraw_reclaim = base_resource_task(
            ApiTradeType::Withdraw,
            ApiResourceDelegationOperationType::Undelegate,
        );
        assert!(matches!(platform_resource_task_trans_type(&withdraw_reclaim), TransType::WdRscRc));
    }

    #[test]
    fn resource_result_ack_type_uses_result_fact_family() {
        let platform_task =
            base_resource_task(ApiTradeType::Collect, ApiResourceDelegationOperationType::Delegate);
        assert!(!is_original_order_resource_result_fact(&platform_task));
        assert!(matches!(platform_resource_result_ack_type(), TransAckType::TxRes));

        let mut original_result =
            base_resource_task(ApiTradeType::Collect, ApiResourceDelegationOperationType::Delegate);
        original_result.resource_trade_no = "C_ORIGIN".to_string();
        original_result.origin_trade_no = Some("C_ORIGIN".to_string());
        assert!(is_original_order_resource_result_fact(&original_result));
        assert!(matches!(merchant_original_resource_result_ack_type(), TransAckType::TxRscRes));
    }

    #[test]
    fn original_order_result_ack_type_uses_origin_order_type() {
        let mut collect =
            base_resource_task(ApiTradeType::Collect, ApiResourceDelegationOperationType::Delegate);
        collect.resource_trade_no = "C_ORIGIN".to_string();
        collect.origin_trade_no = Some("C_ORIGIN".to_string());
        assert!(matches!(merchant_original_resource_result_trans_type(&collect), TransType::Col));

        let mut withdraw = base_resource_task(
            ApiTradeType::Withdraw,
            ApiResourceDelegationOperationType::Delegate,
        );
        withdraw.resource_trade_no = "W_ORIGIN".to_string();
        withdraw.origin_trade_no = Some("W_ORIGIN".to_string());
        assert!(matches!(merchant_original_resource_result_trans_type(&withdraw), TransType::Wd));
    }
}
