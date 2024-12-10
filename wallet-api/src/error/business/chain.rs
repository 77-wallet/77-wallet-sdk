#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("Chain not found: {0}")]
    NotFound(String),
    #[error("Insufficient balance")]
    InsufficientBalance,
    #[error("Insufficient balance for fees")]
    InsufficientFeeBalance,
    #[error("btc address type cannot be empty")]
    BitcoinAddressEmpty,
    #[error("address format incorrect")]
    AddressFormatIncorrect,
    #[error("address is Frozen")]
    AddressIsFrozen,
    #[error("amount less than min amount")]
    AmountLessThanMin,
    #[error("address not init on chain")]
    AddressNotInit,
    #[error("The chain does not support this operation")]
    NotSupportChain,
    #[error("get node token err pelase change node!")]
    NodeToken(String),
}

impl ChainError {
    pub(crate) fn get_status_code(&self) -> u32 {
        match self {
            ChainError::NotFound(_) => 3501,
            ChainError::InsufficientBalance => 3502,
            ChainError::InsufficientFeeBalance => 3503,
            ChainError::BitcoinAddressEmpty => 3504,
            ChainError::AddressFormatIncorrect => 3505,
            ChainError::AddressIsFrozen => 3506,
            ChainError::AmountLessThanMin => 3507,
            ChainError::AddressNotInit => 3508,
            ChainError::NotSupportChain => 3509,
            ChainError::NodeToken(_) => 3510,
        }
    }
}
