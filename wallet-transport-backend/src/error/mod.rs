use wallet_ecdh::error::EncryptionError;
use wallet_transport::errors::TransportError;
use wallet_utils::RetryableError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Transport error: `{0}`")]
    Transport(#[from] TransportError),
    #[error("Backend error: `{0:?}`")]
    Backend(Option<String>),
    #[error("Backend error: `{0:?}`")]
    ApiBackend(i64, Option<String>),
    #[error("Utils error: `{0}`")]
    Utils(#[from] wallet_utils::error::Error),
    #[error("backend service error")]
    BackendServiceError(#[from] BackendServiceError),
    #[error("encryption error: `{0}`")]
    EncryptionError(#[from] EncryptionError),
    #[error("acquire error: `{0}`")]
    AcquireError(#[from] tokio::sync::AcquireError),
    #[error("rate limited")]
    RateLimited,
    #[error("backpressure")]
    Backpressure,
    #[error("timeout")]
    Timeout,
}
impl Error {
    pub fn is_network_error(&self) -> bool {
        match self {
            Error::Transport(e) => e.is_network_error(),
            Error::Utils(e) => e.is_network_error(),
            _ => false,
        }
    }

    pub fn is_rate_limited(&self) -> bool {
        match self {
            crate::Error::ApiBackend(code, _) if *code == 429 => true,
            crate::Error::Transport(msg) => match msg {
                TransportError::NodeResponseError(err) if err.code == 429 => true,
                _ => false,
            },
            _ => false,
        }
    }
}

impl RetryableError for Error {
    fn is_network_error(&self) -> bool {
        self.is_network_error()
    }

    fn is_html_error(&self) -> bool {
        false
    }

    fn is_delay_retryable(&self) -> bool {
        match self {
            Error::Transport(e) => e.is_delay_retryable(),
            Error::ApiBackend(code, _) if *code == 429 => true,
            Error::RateLimited => true,
            Error::Utils(e) => e.is_delay_retryable(),
            _ => false,
        }
    }

    fn retry_policy(&self) -> wallet_utils::RetryPolicy {
        if self.is_delay_retryable() {
            wallet_utils::RetryPolicy::Delay
        } else {
            wallet_utils::RetryPolicy::Never
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BackendServiceError {
    #[error("not platform address")]
    NotPlatformAddress,
}
