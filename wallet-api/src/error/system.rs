#[derive(Debug, thiserror::Error)]
pub enum SystemError {
    #[error("Database error: {0}")]
    Database(#[from] wallet_database::Error),
    #[error("Service error: {0}")]
    Service(String),
    #[error("Context not init")]
    ContextNotInit,
    #[error("Lock poison: {0}")]
    LockPoison(String),
    #[error("Channel send failed: {0}")]
    ChannelSendFailed(String),
    #[error("Frontend notify sender not set")]
    FrontendNotifySenderUnset,
    #[error("Message wrong")]
    MessageWrong,
    #[error("Mqtt client not init")]
    MqttClientNotInit,
    #[error("device not init")]
    DeviceNotInit,
}
// impl SystemError {
//     pub fn get_status_code(&self) -> u32 {
//         match self {
//             SystemError::Database(_) => 6300,
//             SystemError::Service(_) => 6300,
//             SystemError::ContextNotInit => 6300,
//             SystemError::LockPoison(_) => 6300,
//             SystemError::ChannelSendFailed(_) => 6300,
//             SystemError::FrontendNotifySenderUnset => 6300,
//             SystemError::MessageWrong => 6300,
//         }
//     }
// }
