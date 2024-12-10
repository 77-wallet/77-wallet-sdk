#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("request error {0}")]
    ReqError(#[from] reqwest::Error),
    #[error("Received a non-success status: {0}")]
    NonSuccessStatus(reqwest::StatusCode),
}

impl HttpError {
    pub fn get_status_code(&self) -> u32 {
        match self {
            HttpError::ReqError(_) => 6061,
            HttpError::NonSuccessStatus(_) => 6061,
        }
    }
}
