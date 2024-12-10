#[derive(Debug, thiserror::Error)]
pub enum CoinError {
    #[error("coin not found: {0}")]
    NotFound(String),
    #[error("invalid contract address: {0}")]
    InvalidContractAddress(String),
}

impl CoinError {
    pub(crate) fn get_status_code(&self) -> u32 {
        match self {
            CoinError::NotFound(_) => 4001,
            CoinError::InvalidContractAddress(_) => 4002,
        }
    }
}
