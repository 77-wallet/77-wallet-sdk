#[derive(Debug, thiserror::Error)]
pub enum StrategyError {
    #[error("Strategy not found")]
    NotFoundStrategy,
    #[error("Strategy status not matched")]
    StatusNotMatched,
}

impl StrategyError {
    pub fn get_status_code(&self) -> i64 {
        match self {
            StrategyError::NotFoundStrategy => 21600,
            StrategyError::StatusNotMatched => 21601,
        }
    }
}
