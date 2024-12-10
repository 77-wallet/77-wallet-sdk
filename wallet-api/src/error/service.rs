#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("Keystore error: `{0}`")]
    Core(#[from] wallet_core::error::Error),
    #[error("Types error: `{0}`")]
    Types(#[from] wallet_types::Error),
    #[error("Tree error: `{0}`")]
    Tree(#[from] wallet_tree::Error),
    #[error("Keystore error: `{0}`")]
    Keystore(#[from] wallet_keystore::error::Error),
    #[error("Utils error: `{0}`")]
    Utils(#[from] wallet_utils::error::Error),
    #[error("TransportBackend error: `{0}`")]
    TransportBackend(#[from] wallet_transport_backend::Error),
    #[error("Transport error: `{0}`")]
    Transport(#[from] wallet_transport::TransportError),
    #[error("Oss error: `{0}`")]
    Oss(#[from] wallet_oss::TransportError),
    #[error("Chain instance error: `{0}`")]
    ChainInstance(#[from] wallet_chain_instance::Error),
    #[error("Chain interact error: `{0}`")]
    ChainInteract(#[from] wallet_chain_interact::Error),
    #[error("Server error: {0}")]
    System(#[from] crate::error::system::SystemError),
    #[error("Server error: {0}")]
    Database(#[from] wallet_database::Error),
    // 业务错误
    #[error("Business error: {0}")]
    Business(#[from] super::business::BusinessError),
    #[error("parameter error: {0}")]
    Parameter(String),
}

impl ServiceError {
    pub fn is_network_error(&self) -> bool {
        match self {
            ServiceError::Keystore(err) => err.is_network_error(),
            ServiceError::Utils(err) => err.is_network_error(),
            ServiceError::TransportBackend(err) => err.is_network_error(),
            ServiceError::Transport(err) => err.is_network_error(),
            ServiceError::ChainInstance(err) => err.is_network_error(),
            ServiceError::ChainInteract(err) => err.is_network_error(),
            ServiceError::Database(err) => err.is_network_error(),
            _ => false,
        }
    }
}
