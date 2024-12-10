pub mod database;
pub use database::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // #[error("Resource error: {0}")]
    // Resource(#[from] resource::Error),
    #[error("Database error: {0}")]
    Database(#[from] database::DatabaseError),
    // #[error("Entity error: {0}")]
    // Entity(#[from] wallet_entity::error::PayloadError),
    #[error("Utils error: {0}")]
    Utils(#[from] wallet_utils::Error),
    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn is_network_error(&self) -> bool {
        match self {
            Error::Utils(e) => e.is_network_error(),
            _ => false,
        }
    }
}
