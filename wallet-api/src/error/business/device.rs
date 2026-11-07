#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("Device not init")]
    Uninitialized,
    #[error("Invite status not confirmed")]
    InviteStatusNotConfirmed,
}

impl DeviceError {
    pub(crate) fn get_status_code(&self) -> i64 {
        match self {
            DeviceError::Uninitialized => 3000,
            DeviceError::InviteStatusNotConfirmed => 3001,
        }
    }
}
