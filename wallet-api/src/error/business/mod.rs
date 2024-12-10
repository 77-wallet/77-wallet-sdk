pub mod account;
pub mod announcement;
pub mod assets;
pub mod bill;
pub mod chain;
pub mod coin;
pub mod device;
pub mod exchange_rate;
pub mod multisig_account;
pub mod multisig_queue;
pub mod stake;
pub mod wallet;

#[derive(Debug, thiserror::Error)]
pub enum BusinessError {
    #[error("Wallet error: {0}")]
    Device(#[from] device::DeviceError),
    #[error("Wallet error: {0}")]
    Wallet(#[from] wallet::WalletError),
    #[error("Account error: {0}")]
    Account(#[from] account::AccountError),
    #[error("Bill error: {0}")]
    Bill(#[from] bill::BillError),
    #[error("Bill error: {0}")]
    Assets(#[from] assets::AssetsError),
    #[error("Chain error: {0}")]
    Chain(#[from] chain::ChainError),
    #[error("Multisig Account error: {0}")]
    MultisigAccount(#[from] multisig_account::MultisigAccountError),
    #[error("Multisig Queue error: {0}")]
    MultisigQueue(#[from] multisig_queue::MultisigQueueError),
    #[error("Announcement error: {0}")]
    Announcement(#[from] announcement::AnnouncementError),
    #[error("stake error:{0}")]
    Stake(#[from] stake::StakeError),
    #[error("coin error:{0}")]
    Coin(#[from] coin::CoinError),
    #[error("exchange error:{0}")]
    ExchangeRate(#[from] exchange_rate::ExchangeRate),
}

impl BusinessError {
    pub fn get_status_code(&self) -> u32 {
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
        }
    }
}
