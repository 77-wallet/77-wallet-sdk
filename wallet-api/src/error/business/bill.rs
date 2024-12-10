#[derive(Debug, thiserror::Error)]
pub enum BillError {
    #[error("Bill not found")]
    NotFound,
}

impl BillError {
    pub(crate) fn get_status_code(&self) -> u32 {
        match self {
            BillError::NotFound => 3301,
        }
    }
}
