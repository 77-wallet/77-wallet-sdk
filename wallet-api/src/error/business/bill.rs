#[derive(Debug, thiserror::Error)]
pub enum BillError {
    #[error("Bill not found")]
    NotFound,
    #[error("exists uncomfirmation tx")]
    ExistsUnConfirmationTx,
}

impl BillError {
    pub(crate) fn get_status_code(&self) -> i64 {
        match self {
            BillError::NotFound => 3301,
            BillError::ExistsUnConfirmationTx => 3302,
        }
    }
}
