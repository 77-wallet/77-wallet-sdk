use serde_repr::Serialize_repr;
use wallet_database::entities::api_withdraw::{ApiWithdrawStatus, ErrCode, WithdrawFailureStage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize_repr)]
#[repr(i32)]
pub enum FailureReasonDisplay {
    /// 未知失败
    UnknownFailed = 0,
    /// 审核拒绝
    AuditRejected = 1,
    /// 签名失败
    SignFailed = 2,
    /// 广播失败
    BroadcastFailed = 3,
    /// 链上失败
    ChainFailed = 4,
    /// 余额/手续费不足
    ResourceFailed = 5,
    /// 地址格式错误
    AddressInvalid = 6,
    /// 节点异常
    NodeFailed = 7,
    /// 网络异常
    NetworkFailed = 8,
    /// SDK 内部错误
    SdkInternalFailed = 9,
}

impl FailureReasonDisplay {
    /// 从状态、错误码和失败阶段推断失败原因类型
    pub fn from_status_and_error(
        status: ApiWithdrawStatus,
        err_code: Option<ErrCode>,
        failure_stage: Option<WithdrawFailureStage>,
    ) -> Self {
        match status {
            ApiWithdrawStatus::AuditReject => Self::AuditRejected,
            ApiWithdrawStatus::SendingTxFailed | ApiWithdrawStatus::Failure => {
                Self::from_failure_facts(err_code, failure_stage, false)
            }
            _ => Self::UnknownFailed,
        }
    }

    /// 从失败事实推断展示分类。上报态 status 不作为事实来源，只能配合这些事实使用。
    pub fn from_failure_facts(
        err_code: Option<ErrCode>,
        failure_stage: Option<WithdrawFailureStage>,
        has_chain_failed: bool,
    ) -> Self {
        match failure_stage {
            Some(WithdrawFailureStage::Build) => Self::SignFailed,
            Some(WithdrawFailureStage::Broadcast) => Self::BroadcastFailed,
            Some(WithdrawFailureStage::Chain) => Self::ChainFailed,
            // TxResultAck 阶段失败会自动重试，不算真正失败
            Some(WithdrawFailureStage::TxResultAck) => Self::UnknownFailed,
            Some(WithdrawFailureStage::Unknown) | None => {
                if has_chain_failed {
                    Self::ChainFailed
                } else {
                    Self::from_err_code(err_code)
                }
            }
        }
    }

    fn from_err_code(err_code: Option<ErrCode>) -> Self {
        match err_code {
            Some(ErrCode::BalanceInsufficient | ErrCode::FeeInsufficient) => Self::ResourceFailed,
            Some(ErrCode::AddressFormatIncorrect) => Self::AddressInvalid,
            Some(ErrCode::NodeError) => Self::NodeFailed,
            Some(ErrCode::NetworkException) => Self::NetworkFailed,
            Some(ErrCode::TransactionOnChainException) => Self::ChainFailed,
            Some(ErrCode::SDKInternalError) => Self::SdkInternalFailed,
            Some(ErrCode::UnknownError) | None => Self::UnknownFailed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failure_reason_type_audit_rejected() {
        let reason =
            FailureReasonDisplay::from_status_and_error(ApiWithdrawStatus::AuditReject, None, None);
        assert_eq!(reason, FailureReasonDisplay::AuditRejected);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "1");
    }

    #[test]
    fn test_failure_reason_type_sign_failed() {
        let reason = FailureReasonDisplay::from_status_and_error(
            ApiWithdrawStatus::SendingTxFailed,
            None,
            Some(WithdrawFailureStage::Build),
        );
        assert_eq!(reason, FailureReasonDisplay::SignFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "2");
    }

    #[test]
    fn test_failure_reason_type_broadcast_failed() {
        let reason = FailureReasonDisplay::from_status_and_error(
            ApiWithdrawStatus::SendingTxFailed,
            None,
            Some(WithdrawFailureStage::Broadcast),
        );
        assert_eq!(reason, FailureReasonDisplay::BroadcastFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "3");
    }

    #[test]
    fn test_failure_reason_type_reported_broadcast_failed() {
        let reason = FailureReasonDisplay::from_failure_facts(
            None,
            Some(WithdrawFailureStage::Broadcast),
            false,
        );
        assert_eq!(reason, FailureReasonDisplay::BroadcastFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "3");
    }

    #[test]
    fn test_failure_reason_type_chain_failed() {
        let reason = FailureReasonDisplay::from_status_and_error(
            ApiWithdrawStatus::SendingTxFailed,
            None,
            Some(WithdrawFailureStage::Chain),
        );
        assert_eq!(reason, FailureReasonDisplay::ChainFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "4");
    }

    #[test]
    fn test_failure_reason_type_resource_failed() {
        let reason = FailureReasonDisplay::from_status_and_error(
            ApiWithdrawStatus::Failure,
            Some(ErrCode::BalanceInsufficient),
            None,
        );
        assert_eq!(reason, FailureReasonDisplay::ResourceFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "5");
    }

    #[test]
    fn test_failure_reason_type_reported_resource_failed() {
        let reason =
            FailureReasonDisplay::from_failure_facts(Some(ErrCode::FeeInsufficient), None, false);
        assert_eq!(reason, FailureReasonDisplay::ResourceFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "5");
    }

    #[test]
    fn test_failure_reason_type_address_invalid() {
        let reason = FailureReasonDisplay::from_failure_facts(
            Some(ErrCode::AddressFormatIncorrect),
            None,
            false,
        );
        assert_eq!(reason, FailureReasonDisplay::AddressInvalid);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "6");
    }

    #[test]
    fn test_failure_reason_type_node_failed() {
        let reason =
            FailureReasonDisplay::from_failure_facts(Some(ErrCode::NodeError), None, false);
        assert_eq!(reason, FailureReasonDisplay::NodeFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "7");
    }

    #[test]
    fn test_failure_reason_type_network_failed() {
        let reason =
            FailureReasonDisplay::from_failure_facts(Some(ErrCode::NetworkException), None, false);
        assert_eq!(reason, FailureReasonDisplay::NetworkFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "8");
    }

    #[test]
    fn test_failure_reason_type_sdk_internal_failed() {
        let reason =
            FailureReasonDisplay::from_failure_facts(Some(ErrCode::SDKInternalError), None, false);
        assert_eq!(reason, FailureReasonDisplay::SdkInternalFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "9");
    }

    #[test]
    fn test_failure_reason_type_chain_failed_from_fact() {
        let reason = FailureReasonDisplay::from_failure_facts(None, None, true);
        assert_eq!(reason, FailureReasonDisplay::ChainFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "4");
    }

    #[test]
    fn test_failure_reason_type_tx_result_ack_returns_unknown() {
        // TxResultAck 阶段失败会自动重试，返回 UnknownFailed
        let reason = FailureReasonDisplay::from_status_and_error(
            ApiWithdrawStatus::SendingTxFailed,
            None,
            Some(WithdrawFailureStage::TxResultAck),
        );
        assert_eq!(reason, FailureReasonDisplay::UnknownFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "0");
    }

    #[test]
    fn test_failure_reason_type_unknown_failed() {
        let reason =
            FailureReasonDisplay::from_status_and_error(ApiWithdrawStatus::Failure, None, None);
        assert_eq!(reason, FailureReasonDisplay::UnknownFailed);
        assert_eq!(serde_json::to_string(&reason).unwrap(), "0");
    }
}
