pub mod database;
pub use database::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("nonce candidate conflict: candidate={candidate}, local_last={local_last}")]
    NonceCandidateConflict { candidate: i64, local_last: i64 },
    #[error("Database error: {0}")]
    Database(#[from] database::DatabaseError),
    #[error("Utils error: {0}")]
    Utils(#[from] wallet_utils::Error),
    #[error("{0}")]
    Other(String),
    #[error("data Not Found: {0}")]
    NotFound(String),
    #[error("Invalid value: {0}")]
    InvalidValue(u8),
}

impl Error {
    pub fn is_network_error(&self) -> bool {
        match self {
            Error::Utils(e) => e.is_network_error(),
            _ => false,
        }
    }
}
