#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config not found: {0}")]
    NotFound(String),
    #[error(
        "default node config network mismatch: profile={profile}, node={node_name}, network={network}"
    )]
    DefaultNodeNetworkMismatch { profile: String, node_name: String, network: String },
    #[error("Keys not reset")]
    KeysNotReset,
}

impl ConfigError {
    pub(crate) fn get_status_code(&self) -> i64 {
        match self {
            ConfigError::NotFound(_) => 4300,
            ConfigError::DefaultNodeNetworkMismatch { .. } => 4302,
            ConfigError::KeysNotReset => 4301,
        }
    }
}
