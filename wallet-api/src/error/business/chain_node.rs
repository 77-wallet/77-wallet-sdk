#[derive(Debug, thiserror::Error)]
pub enum ChainNodeError {
    #[error("chain not found")]
    ChainNotFound,
    #[error("node not found")]
    NodeNotFound,
    #[error("no available node for chain {0}")]
    NoAvailableNode(String),
}

impl ChainNodeError {
    pub(crate) fn get_status_code(&self) -> i64 {
        match self {
            ChainNodeError::ChainNotFound => 4201,
            ChainNodeError::NodeNotFound => 4202,
            ChainNodeError::NoAvailableNode(_) => 4203,
        }
    }
}
