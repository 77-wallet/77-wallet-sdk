#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Keystore I/O failed: {0}")]
    KeystoreIo(#[from] std::io::Error),
    #[error("Keystore worker failed")]
    KeystoreWorker,
    #[error("Core error: `{0}`")]
    Core(#[from] wallet_core::Error),
    #[error("Crypto error: `{0}`")]
    Crypto(#[from] wallet_crypto::error::Error),
    #[error("The wallet key is not saved locally")]
    LocalNoWallet,
    #[error("Tree error: `{0}`")]
    Utils(#[from] wallet_utils::Error),
    #[error("Filename invalid")]
    FilenameInvalid,
    #[error("Types error: `{0}`")]
    Types(#[from] wallet_types::Error),
    #[error("Private key not found")]
    PrivateKeyNotFound,
    #[error("Missing index")]
    MissingIndex,
    #[error("Missing address")]
    MissingAddress,
    #[error("Missing chain code")]
    MissingChainCode,
    #[error("Missing derivation")]
    MissingDerivation,
    #[error("Unsupported file type")]
    UnsupportedFileType,
    #[error("Failed to downcast")]
    FailedToDowncast,
    #[error("Phrase or salt incorrect")]
    Parase,
    #[error("Invalid key format")]
    InvalidKeyFormat,
    #[error("Invalid escape sequence")]
    InvalidEscapeSequence,
    #[error("Metadata not found")]
    MetadataNotFound,
    #[error("Chain instance error: `{0}`")]
    ChainInstance(#[from] wallet_chain_instance::Error),
}

impl Error {
    pub fn is_network_error(&self) -> bool {
        match self {
            Error::Utils(e) => e.is_network_error(),
            Error::ChainInstance(e) => e.is_network_error(),
            _ => false,
        }
    }
}
