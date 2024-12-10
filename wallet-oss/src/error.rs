use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("node response  {0}")]
    NodeResponseError(String),
    #[error("query result empty")]
    EmptyResult,
    #[error("Aliyun oss error: {0}")]
    AliyunOss(#[from] crate::oss_client::error::OssError),
}
