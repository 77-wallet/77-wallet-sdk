#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("Device not init")]
    Uninitialized,
}

impl DeviceError {
    pub(crate) fn get_status_code(&self) -> u32 {
        match self {
            DeviceError::Uninitialized => 3000,
        }
    }
}
