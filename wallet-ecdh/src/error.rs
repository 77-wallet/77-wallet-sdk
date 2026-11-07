use k256::elliptic_curve;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EncryptionError {
    #[error("lock key")]
    LockPoisoned,
    #[error("invalid shared key")]
    InvalidSharedKey,
    #[error("invalid pub key")]
    InvalidPubKey,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid signing input")]
    InvalidSigningInput,
    #[error("encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("key derivation failed: {0}")]
    KeyDerivationFailed(String),
    #[error("invalid encryption key")]
    InvalidEncryptedData,
    #[error("k256 err: {0}")]
    SignatureError(#[from] k256::ecdsa::Error),
    #[error("elliptic curve err: {0}")]
    EllipticCurveError(#[from] elliptic_curve::Error),
    #[error("wallet utils err")]
    WalletUtilsError(#[from] wallet_utils::Error),
}
