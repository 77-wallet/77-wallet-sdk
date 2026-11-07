#[derive(Debug, thiserror::Error)]
pub enum MultisigAccountError {
    #[error("Multisig Account already exists")]
    AlreadyExist,
    #[error("Multisig Account not found")]
    NotFound,
    #[error("cannot cancel account")]
    CannotCancel,
    #[error("payer is need")]
    PayerNeed,
    #[error("is cancel not allowed operation")]
    IsCancel,
    #[error("service fee not config")]
    ServiceFeeNotConfig,
    #[error("has uncompleted account")]
    HasUncompletedAccount,
    #[error("The address is not a platform address.")]
    NotPlatFormAddress,
    #[error("Only Initiator create tx")]
    OnlyInitiatorCreateTx,
    #[error("not pay")]
    NotPay,
    #[error("not onchain")]
    NotOnChain,
    // 在tron 创建多签账号时，重复创建
    #[error("address repeat")]
    AddressRepeat,
    #[error("not support multisg chain: {0}")]
    NotSupportChain(String),
}

impl MultisigAccountError {
    pub fn get_status_code(&self) -> i64 {
        match self {
            MultisigAccountError::AlreadyExist => 3600,
            MultisigAccountError::NotFound => 3601,
            MultisigAccountError::CannotCancel => 3602,
            MultisigAccountError::PayerNeed => 3603,
            MultisigAccountError::IsCancel => 3604,
            MultisigAccountError::ServiceFeeNotConfig => 3605,
            MultisigAccountError::HasUncompletedAccount => 3606,
            MultisigAccountError::NotPlatFormAddress => 3607,
            MultisigAccountError::OnlyInitiatorCreateTx => 3608,
            MultisigAccountError::NotPay => 3609,
            MultisigAccountError::NotOnChain => 3610,
            MultisigAccountError::AddressRepeat => 3611,
            MultisigAccountError::NotSupportChain(_) => 3612,
        }
    }
}
