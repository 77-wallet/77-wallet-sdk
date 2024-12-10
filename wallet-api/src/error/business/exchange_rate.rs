#[derive(Debug, thiserror::Error)]
pub enum ExchangeRate {
    #[error("exchange not found")]
    NotFound,
}

impl ExchangeRate {
    pub(crate) fn get_status_code(&self) -> u32 {
        match self {
            ExchangeRate::NotFound => 4101,
        }
    }
}
