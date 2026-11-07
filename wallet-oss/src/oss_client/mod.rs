use oss::Oss;
use request::RequestBuilder;

use crate::OssConfig;

mod async_impl;
mod auth;
mod entity;
pub(crate) mod error;
mod macros;
mod metadata;
mod oss;
mod request;
mod url;
mod util;

#[derive(Debug, Clone)]
pub struct OssClient {
    oss: Oss,
    builder: RequestBuilder,
}

impl OssClient {
    pub fn new(config: &OssConfig) -> Self {
        let oss = Oss::new(
            &config.access_key_id,
            &config.access_key_secret,
            &config.endpoint,
            &config.bucket_name,
        );
        let builder = request::RequestBuilder::new().with_expire(60);
        Self { oss, builder }
    }

    pub async fn upload_local_file(
        &self,
        src_file_path: &str,
        dst_file_name: &str,
    ) -> Result<(), crate::TransportError> {
        let oss = &self.oss;
        let builder = self.builder.clone();
        // let oss_path = format!("/logs/{}", file_name);
        let oss_path = format!("logs/{}", dst_file_name);
        // let oss_path = format!("./");
        tracing::warn!("[upload_local_file] oss_path: {}, file_path: {}", oss_path, src_file_path);

        oss.put_object_from_file(oss_path, src_file_path.to_string(), builder).await?;
        tracing::warn!("upload_local_file success");
        Ok(())
    }

    pub async fn get_object(&self, file_name: &str) -> Result<Vec<u8>, crate::TransportError> {
        let oss = &self.oss;
        let builder = self.builder.clone();
        // .oss_header_put("Content-Length", &metadata.to_string());
        let oss_path = format!("/logs/{}", file_name);
        tracing::warn!("oss_path: {}", oss_path);
        let res = oss.get_object(oss_path, builder).await?;
        Ok(res)
    }

    pub async fn get_object_metadata(
        &self,
        file_name: &str,
    ) -> Result<metadata::ObjectMetadata, crate::TransportError> {
        let oss = &self.oss;
        let builder = self.builder.clone();
        // .oss_header_put("Content-Length", &metadata.to_string());
        let oss_path = format!("/logs/{}", file_name);
        tracing::warn!("oss_path: {}", oss_path);
        let res = oss.get_object_metadata(oss_path, builder).await?;
        Ok(res)
    }

    pub async fn upload_buffer(
        &self,
        buff: Vec<u8>,
        file_name: &str,
    ) -> Result<(), crate::TransportError> {
        let oss = &self.oss;
        let builder = self.builder.clone();
        let oss_path = format!("logs/{}", file_name);

        oss.pub_object_from_buffer(oss_path, buff, builder).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const ACCESS_KEY_ID: &str = "xxxx";
    pub const ACCESS_KEY_SECRET: &str = "xxxx";
    pub const BUCKET_NAME: &str = "xxxx";
    pub const ENDPOINT: &str = "xxxx";

    fn get_config() -> OssConfig {
        OssConfig {
            access_key_id: ACCESS_KEY_ID.to_string(),
            access_key_secret: ACCESS_KEY_SECRET.to_string(),
            endpoint: ENDPOINT.to_string(),
            bucket_name: BUCKET_NAME.to_string(),
        }
    }

    #[tokio::test]
    #[ignore = "requires real OSS endpoint and credentials"]
    async fn test_oss_client() {
        let oss_client = OssClient::new(&get_config());
        tracing::debug!("oss_client initialized");
        // let file_path = "./test.txt";
        let file_path = "./sdk:2025-03-27 09:35:47.txt";
        // let file_name = "test.txt";
        let file_name = "sdk:2025-03-27 09:35:47.txt";
        let result = oss_client.upload_local_file(file_path, file_name).await;
        tracing::debug!("result: {result:?}");
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires real OSS endpoint and credentials"]
    async fn test_get_object() {
        let oss_client = OssClient::new(&get_config());
        let _file_name = "test.txt";
        // let _file_name = "sdk:2024-10-07 10:36:00.txt";
        // let _file_name = "sdk:2025-02-21 07:47:16.txt";
        let _file_name = "sdk:2025-03-27 09:35:47.txt";
        let result = oss_client.get_object(_file_name).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires real OSS endpoint and credentials"]
    async fn test_get_object_metadata() {
        let oss_client = OssClient::new(&get_config());
        // let file_name = "test.txt";
        // let file_name = "sdk:2025-02-19 23:17:00.txt";
        let file_name = "sdk:2025-03-27 09:35:47.txt";
        let result = oss_client.get_object_metadata(file_name).await;
        assert!(result.is_ok());
    }
}
