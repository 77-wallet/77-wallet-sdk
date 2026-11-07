#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    // 无法移除已配置出款策略的账户
    #[error("An account configured with a withdrawal strategy cannot be removed")]
    ConfiguredWithdrawalStrategyAccountCantBeRemoved,
    #[error("Expand address not done yet")]
    ExpandAddressNotDoneYet,
    #[error("In the recovery address, can not expand")]
    CanNotExpand,
    #[error("Expand batch not found")]
    ExpandBatchNotFound,
    #[error("Account not found")]
    NotFound,
}

impl AccountError {
    pub(crate) fn get_status_code(&self) -> i64 {
        match self {
            AccountError::ConfiguredWithdrawalStrategyAccountCantBeRemoved => 20100,
            AccountError::ExpandAddressNotDoneYet => 20101,
            AccountError::CanNotExpand => 20102,
            AccountError::ExpandBatchNotFound => 20103,
            AccountError::NotFound => 20104,
        }
    }
}
