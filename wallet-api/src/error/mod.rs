pub(crate) mod business;
pub mod service;
pub use service::ServiceError;
pub(crate) mod system;

// #[derive(Debug, thiserror::Error)]
// pub enum Error {
//     #[error("Keystore error: `{0}`")]
//     Core(#[from] wallet_core::error::Error),
//     #[error("Types error: `{0}`")]
//     Types(#[from] wallet_types::Error),
//     #[error("Keystore error: `{0}`")]
//     Keystore(#[from] wallet_keystore::error::Error),
//     #[error("Utils error: `{0}`")]
//     Utils(#[from] wallet_utils::error::Error),
//     #[error("TransportBackend error: `{0}`")]
//     TransportBackend(#[from] wallet_transport_backend::Error),
//     #[error("Chain instance error: `{0}`")]
//     ChainInstance(#[from] wallet_chain_instance::Error),
//     #[error("Chain interact error: `{0}`")]
//     ChainInteract1(#[from] wallet_chain_interact::Error),
//     // 内部错误
//     #[error("Server error: {0}")]
//     System(#[from] system::SystemError),
//     // 业务错误
//     #[error("Business error: {0}")]
//     Business(#[from] business::BusinessError),
//     // 鉴权错误
//     #[error("Unauthorized")]
//     UnAuthorize,
//     #[error("parameter error: {0}")]
//     Parameter(String),
// }

#[derive(Debug, thiserror::Error)]
pub enum Errors {
    #[error("parameter error: {0}")]
    Parameter(String),
    #[error("service error: {0}")]
    Service(#[from] service::ServiceError),
}
