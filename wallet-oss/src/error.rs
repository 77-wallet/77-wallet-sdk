use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("node response  {0}")]
    NodeResponseError(String),
    #[error("query result empty")]
    EmptyResult,
    // #[error("Utils error: {0}")]
    // Utils(#[from] wallet_utils::error::Error),
    #[error("Aliyun oss error: {0}")]
    AliyunOss(#[from] crate::oss_client::error::OssError),
}

// impl TransportError {
//     pub fn is_network_error(&self) -> bool {
//         match self {
//             TransportError::Utils(e) => e.is_network_error(),
//             _ => false,
//         }
//     }
// }
