pub(crate) mod error;
pub mod oss_client;
pub use error::TransportError;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct OssConfig {
    pub access_key_id: String,
    pub access_key_secret: String,
    pub bucket_name: String,
    pub endpoint: String,
}
