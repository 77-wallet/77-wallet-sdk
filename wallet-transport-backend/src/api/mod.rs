use std::collections::HashMap;

use crate::response::BackendResponse;

pub mod address;
pub mod announcement;
pub mod app;
pub mod chain;
pub mod coin;
pub mod config;
pub mod device;
pub mod mqtt;
pub mod signed;
pub mod stake;
pub mod transaction;

#[derive(Debug, Clone)]
pub struct BackendApi {
    pub base_url: String,
    pub client: wallet_transport::client::HttpClient,
}

impl BackendApi {
    pub fn new(
        backend_url: Option<String>,
        headers_opt: Option<HashMap<String, String>>,
    ) -> Result<Self, crate::Error> {
        let url = backend_url.unwrap_or(crate::consts::BASE_URL.to_string());

        let timeout = Some(std::time::Duration::from_secs(15));
        Ok(Self {
            base_url: url.to_string(),
            client: wallet_transport::client::HttpClient::new(&url, headers_opt, timeout)?,
        })
    }

    pub fn replace_base_url(&mut self, base_url: &str) {
        self.base_url = base_url.to_string();
        self.client.replace_base_url(base_url);
    }

    pub async fn post_request<T, R>(&self, endpoint: &str, req: T) -> Result<R, crate::Error>
    where
        T: serde::Serialize + std::fmt::Debug,
        R: serde::de::DeserializeOwned + serde::Serialize,
    {
        let res = self
            .client
            .post(endpoint)
            .json(req)
            .send::<BackendResponse>()
            .await?;
        res.process::<R>()
    }

    // 发送一个字符串的请求.
    pub async fn post_req_str<T>(
        &self,
        endpoint: &str,
        body: &serde_json::Value,
    ) -> Result<T, crate::Error>
    where
        T: serde::de::DeserializeOwned + serde::Serialize,
    {
        let res = self
            .client
            .post(endpoint)
            .body(body.to_string())
            .send::<BackendResponse>()
            .await?;
        res.process::<T>()
    }
}
