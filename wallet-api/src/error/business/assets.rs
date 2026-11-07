#[derive(Debug, thiserror::Error)]
pub enum AssetsError {
    #[error("Assets not found")]
    NotFound,
    #[error("assets not found please add")]
    NotFoundAssets,
}

impl AssetsError {
    pub(crate) fn get_status_code(&self) -> i64 {
        match self {
            AssetsError::NotFound => 3401,
            AssetsError::NotFoundAssets => 3402,
        }
    }
}
