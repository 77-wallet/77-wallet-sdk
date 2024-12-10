#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("Account already exists")]
    AlreadyExist,
    #[error("Account not found")]
    NotFound,
    #[error("Address_not_correct")]
    AddressNotCorrect,
    #[error("address repeat")]
    AddressRepeat,
    #[error("Cannot delete the last account")]
    CannotDeleteLastAccount,
}

impl AccountError {
    pub(crate) fn get_status_code(&self) -> u32 {
        match self {
            AccountError::AlreadyExist => 3200,
            AccountError::NotFound => 3201,
            AccountError::AddressNotCorrect => 3202,
            AccountError::AddressRepeat => 3203,
            AccountError::CannotDeleteLastAccount => 3204,
        }
    }
}
