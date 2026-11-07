use chrono::{DateTime, Utc};
use reqwest::{
    Client, ClientBuilder,
    header::{AUTHORIZATION, CONTENT_TYPE, DATE, HeaderMap},
};

use super::{auth::AuthAPI as _, error::OssError, request::RequestBuilder};

/// OSS配置
#[derive(Clone)]
pub struct Oss {
    key_id: String,
    key_secret: String,
    endpoint: String,
    bucket: String,
    client: Client,
}

unsafe impl Send for Oss {}

unsafe impl Sync for Oss {}

impl std::fmt::Debug for Oss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Oss")
            .field("key_id", &"<redacted>")
            .field("key_secret", &"<redacted>")
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("client", &"<redacted>")
            .finish()
    }
}

pub trait OSSInfo {
    fn endpoint(&self) -> String;
    fn bucket(&self) -> String;
    fn key_id(&self) -> String;
    fn key_secret(&self) -> String;
}

pub trait Api {
    #[allow(dead_code)]
    fn key_urlencode<S: AsRef<str>>(&self, key: S) -> String {
        key.as_ref().split("/").map(|x| urlencoding::encode(x)).collect::<Vec<_>>().join("/")
    }

    fn format_key<S: AsRef<str>>(&self, key: S) -> String {
        let key = key.as_ref();
        if key.starts_with("/") { key.to_string() } else { format!("/{}", key) }
    }

    fn format_oss_resource_str<S: AsRef<str>>(&self, bucket: S, key: S) -> String;
}

impl OSSInfo for Oss {
    fn endpoint(&self) -> String {
        self.endpoint.clone()
    }
    fn bucket(&self) -> String {
        self.bucket.clone()
    }

    fn key_id(&self) -> String {
        self.key_id.clone()
    }

    fn key_secret(&self) -> String {
        self.key_secret.clone()
    }
}

impl Api for Oss {
    fn format_oss_resource_str<S: AsRef<str>>(&self, bucket: S, key: S) -> String {
        let bucket = bucket.as_ref();
        if bucket.is_empty() {
            format!("/{}", bucket)
        } else {
            format!("/{}{}", bucket, key.as_ref())
        }
    }
}

impl Oss {
    #[allow(dead_code)]
    pub fn from_env() -> Result<Self, OssError> {
        let key_id =
            std::env::var("OSS_KEY_ID").map_err(|_| OssError::MissingEnvVar("OSS_KEY_ID"))?;
        let key_secret = std::env::var("OSS_KEY_SECRET")
            .map_err(|_| OssError::MissingEnvVar("OSS_KEY_SECRET"))?;
        let endpoint =
            std::env::var("OSS_ENDPOINT").map_err(|_| OssError::MissingEnvVar("OSS_ENDPOINT"))?;
        let bucket =
            std::env::var("OSS_BUCKET").map_err(|_| OssError::MissingEnvVar("OSS_BUCKET"))?;
        Ok(Oss::new(key_id, key_secret, endpoint, bucket))
    }

    // #[cfg(feature = "debug-print")]
    // pub fn open_debug(&self) {
    //     std::env::set_var("RUST_LOG", "oss=debug");
    //     tracing_subscriber::fmt()
    //         .with_max_level(tracing::Level::DEBUG)
    //         .with_line_number(true)
    //         .init();
    // }
    #[cfg(not(feature = "debug-print"))]
    #[allow(dead_code)]
    pub fn open_debug(&self) {}
    pub fn new<S: Into<String>>(key_id: S, key_secret: S, endpoint: S, bucket: S) -> Self {
        let mut client_builder = ClientBuilder::new();
        if std::env::var_os("WALLET_TRANSPORT_NO_PROXY").is_some() {
            client_builder = client_builder.no_proxy();
        }
        let client = client_builder.build().expect("build oss client");
        Oss {
            key_id: key_id.into(),
            key_secret: key_secret.into(),
            endpoint: endpoint.into(),
            bucket: bucket.into(),
            client,
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn format_host<S: AsRef<str>>(&self, bucket: S, key: S, build: &RequestBuilder) -> String {
        let key = if key.as_ref().starts_with("/") {
            key.as_ref().to_string()
        } else {
            format!("/{}", key.as_ref())
        };
        if let Some(cdn) = &build.cdn {
            format!("{}{}", cdn, key,)
        } else if self.endpoint().starts_with("https") {
            format!(
                "https://{}.{}{}",
                bucket.as_ref(),
                self.endpoint().replacen("https://", "", 1),
                key,
            )
        } else {
            format!(
                "http://{}.{}{}",
                bucket.as_ref(),
                self.endpoint().replacen("http://", "", 1),
                key,
            )
        }
    }

    pub fn build_request<S: AsRef<str>>(
        &self,
        key: S,
        build: RequestBuilder,
    ) -> Result<(String, HeaderMap), OssError> {
        let mut build = build.clone();
        let host = self.format_host(self.bucket(), key.as_ref().to_string(), &build);
        let mut header = HeaderMap::new();
        let date = self.date();
        header.insert(
            DATE,
            date.parse().map_err(|e| OssError::Err(format!("invalid date header: {e}")))?,
        );
        build.headers.insert(DATE.to_string(), date);
        let key = key.as_ref();
        let authorization = self.oss_sign(key, &build)?;
        if let Some(content_type) = build.content_type {
            header.insert(
                CONTENT_TYPE,
                content_type
                    .parse()
                    .map_err(|e| OssError::Err(format!("invalid content-type header: {e}")))?,
            );
        }
        header.insert(
            AUTHORIZATION,
            authorization
                .parse()
                .map_err(|e| OssError::Err(format!("invalid authorization header: {e}")))?,
        );
        Ok((host, header))
    }
    pub fn date(&self) -> String {
        let now: DateTime<Utc> = Utc::now();
        now.format("%a, %d %b %Y %T GMT").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::Oss;
    use crate::oss_client::error::OssError;

    #[test]
    fn from_env_returns_error_when_missing_env() {
        let result = Oss::from_env();
        assert!(matches!(result, Err(OssError::MissingEnvVar(_))));
    }

    #[test]
    fn debug_redacts_access_keys() {
        let oss = Oss::new("id-123", "secret-456", "endpoint", "bucket");
        let debug = format!("{oss:?}");

        assert!(!debug.contains("id-123"));
        assert!(!debug.contains("secret-456"));
        assert!(debug.contains("<redacted>"));
    }
}
