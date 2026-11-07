#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("Account already exists")]
    AlreadyExist,
    #[error("Account not found:{0}")]
    NotFound(String),
    #[error("Address_not_correct")]
    AddressNotCorrect,
    #[error("address repeat")]
    AddressRepeat,
    #[error("Cannot delete the last account")]
    CannotDeleteLastAccount,
}

impl AccountError {
    pub(crate) fn get_status_code(&self) -> i64 {
        match self {
            AccountError::AlreadyExist => 3200,
            AccountError::NotFound(_) => 3201,
            AccountError::AddressNotCorrect => 3202,
            AccountError::AddressRepeat => 3203,
            AccountError::CannotDeleteLastAccount => 3204,
        }
    }
}
