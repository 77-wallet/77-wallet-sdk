// 3000 - 4499

pub mod account; // 3200 - 3299
pub mod announcement; // 3800 - 3899
pub mod api_wallet; // 4400 - 4499
pub mod assets; // 3400 - 3499
pub mod bill; // 3300 - 3399
pub mod chain; // 3500 - 3599
pub mod chain_node; // 4200 - 4299
pub mod coin; // 4000 - 4099
pub mod config; // 4300 - 4399
pub mod device; // 3000 - 3099
pub mod exchange_rate; // 4100 - 4199
pub mod multisig_account; // 3600 - 3699
pub mod multisig_queue; // 3700 - 3799
pub mod permission; // 4300 - 4399
pub mod stake; // 3900 - 3999
pub mod wallet; // 3100 - 3199

#[derive(Debug, thiserror::Error)]
pub enum BusinessError {
    #[error("Device error: {0}")]
    Device(#[from] device::DeviceError),
    #[error("Wallet error: {0}")]
    Wallet(#[from] wallet::WalletError),
    #[error("Account error: {0}")]
    Account(#[from] account::AccountError),
    #[error("Bill error: {0}")]
    Bill(#[from] bill::BillError),
    #[error("Assets error: {0}")]
    Assets(#[from] assets::AssetsError),
    #[error("Chain error: {0}")]
    Chain(#[from] chain::ChainError),
    #[error("Multisig Account error: {0}")]
    MultisigAccount(#[from] multisig_account::MultisigAccountError),
    #[error("Multisig Queue error: {0}")]
    MultisigQueue(#[from] multisig_queue::MultisigQueueError),
    #[error("Announcement error: {0}")]
    Announcement(#[from] announcement::AnnouncementError),
    #[error("stake error: {0}")]
    Stake(#[from] stake::StakeError),
    #[error("coin error: {0}")]
    Coin(#[from] coin::CoinError),
    #[error("exchange error: {0}")]
    ExchangeRate(#[from] exchange_rate::ExchangeRate),
    #[error("chain node: error: {0}")]
    ChainNode(#[from] chain_node::ChainNodeError),
    #[error("Config: error: {0}")]
    Config(#[from] config::ConfigError),
    #[error("chain node: error: {0}")]
    Permission(#[from] permission::PermissionError),
    #[error("api wallet error: {0}")]
    ApiWallet(#[from] api_wallet::ApiWalletError),
}

impl BusinessError {
    pub fn get_status_code(&self) -> i64 {
        match self {
            BusinessError::Device(msg) => msg.get_status_code(),
            BusinessError::Wallet(msg) => msg.get_status_code(),
            BusinessError::Account(msg) => msg.get_status_code(),
            BusinessError::Bill(msg) => msg.get_status_code(),
            BusinessError::Assets(msg) => msg.get_status_code(),
            BusinessError::Chain(msg) => msg.get_status_code(),
            BusinessError::MultisigAccount(msg) => msg.get_status_code(),
            BusinessError::MultisigQueue(msg) => msg.get_status_code(),
            BusinessError::Announcement(msg) => msg.get_status_code(),
            BusinessError::Stake(msg) => msg.get_status_code(),
            BusinessError::Coin(msg) => msg.get_status_code(),
            BusinessError::ExchangeRate(msg) => msg.get_status_code(),
            BusinessError::ChainNode(msg) => msg.get_status_code(),
            BusinessError::Config(msg) => msg.get_status_code(),
            BusinessError::Permission(msg) => msg.get_status_code(),
            BusinessError::ApiWallet(msg) => msg.get_status_code(),
        }
    }
}
