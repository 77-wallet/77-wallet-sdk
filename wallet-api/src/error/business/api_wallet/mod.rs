pub mod account;
pub mod strategy;
pub mod trans;
pub mod wallet;

#[derive(Debug, thiserror::Error)]
pub enum ApiWalletError {
    #[error("Api Wallet error: {0}")]
    Wallet(#[from] wallet::WalletError),
    #[error("Api Account error: {0}")]
    Account(#[from] account::AccountError),
    #[error("Api Strategy error: {0}")]
    Strategy(#[from] strategy::StrategyError),
    #[error("Api transaction error: {0}")]
    Trans(#[from] trans::TransError),

    #[error("Chain config not found: `{0}`")]
    ChainConfigNotFound(String),
    #[error("Api Account not exist")]
    NotFoundAccount,
    #[error("gas oracle not exist")]
    GasOracle,
    #[error("Password not cached")]
    PasswordNotCached,

    #[error("the order not exist")]
    OrderNotFound(String),
    #[error("Wallet not init")]
    WalletNotInit,
    #[error("key not initialized")]
    KeyInitialized,
    #[error("status not matched")]
    StatusNotMatched,
    #[error("data time parse err")]
    DataTimeParseError(String),
}

impl ApiWalletError {
    pub(crate) fn get_status_code(&self) -> i64 {
        match self {
            ApiWalletError::Wallet(msg) => msg.get_status_code(),
            ApiWalletError::Account(msg) => msg.get_status_code(),
            ApiWalletError::ChainConfigNotFound(_) => 20500,
            ApiWalletError::NotFoundAccount => 20600,
            ApiWalletError::GasOracle => 20700,
            ApiWalletError::PasswordNotCached => 20800,
            ApiWalletError::OrderNotFound(_) => 21100,
            ApiWalletError::WalletNotInit => 21200,
            ApiWalletError::KeyInitialized => 21300,
            ApiWalletError::StatusNotMatched => 21400,
            ApiWalletError::DataTimeParseError(_) => 21500,
            ApiWalletError::Strategy(msg) => msg.get_status_code(),
            ApiWalletError::Trans(msg) => 21700,
        }
    }
}
