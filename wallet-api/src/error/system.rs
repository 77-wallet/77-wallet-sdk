use crate::messaging::mqtt::message::BizType;

#[derive(Debug, thiserror::Error)]
pub enum SystemError {
    #[error("Internal error: {0}")]
    Internal(String),
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
    #[error("Message wrong [biz_type]: {0:?}, [body]: {1:?}")]
    MessageWrong(BizType, serde_json::Value),
    #[error("Mqtt client not init")]
    MqttClientNotInit,
    #[error("device not init")]
    DeviceNotInit,
    #[error("backend endpoint not found")]
    BackendEndpointNotFound,
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Wallet type not set")]
    WalletTypeNotSet,
    #[error("Semaphore closed")]
    SemaphoreClosed,
    #[error("System not ready")]
    SystemNotReady,
}
