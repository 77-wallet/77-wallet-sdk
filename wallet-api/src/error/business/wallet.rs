#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    // #[error("Community error: {0}")]
    // Community(#[from] super::api::community::CommunityError),
    // #[error("Account error: {0}")]
    // Account(#[from] super::api::account::AccountError),
    #[error("Wallet already exists")]
    AlreadyExist,
    #[error("Wallet not exist")]
    NotFound,
    #[error("Password incorrect")]
    PasswordIncorrect,
    #[error("Password not set")]
    PasswordNotSet,
}

impl WalletError {
    pub(crate) fn get_status_code(&self) -> u32 {
        match self {
            WalletError::AlreadyExist => 3100,
            WalletError::NotFound => 3101,
            WalletError::PasswordIncorrect => 3102,
            WalletError::PasswordNotSet => 3103,
        }
    }
}
