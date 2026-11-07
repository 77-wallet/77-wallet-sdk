use crate::{
    debug,
    oss_client::oss::{Api as _, OSSInfo as _},
};
use base64::{Engine, engine::general_purpose};
use hmac::{Hmac, Mac};
use reqwest::header::DATE;

use super::{error::OssError, oss::Oss, request::RequestBuilder};

pub trait AuthAPI {
    fn sign<S: AsRef<str>>(&self, object: S, build: &RequestBuilder) -> Result<String, OssError>;

    fn oss_sign<S: AsRef<str>>(
        &self,
        object: S,
        build: &RequestBuilder,
    ) -> Result<String, OssError>;
}

impl AuthAPI for Oss {
    fn sign<S: AsRef<str>>(&self, key: S, build: &RequestBuilder) -> Result<String, OssError> {
        let date = build.headers.get(&DATE.to_string()).ok_or(OssError::MissingHeader("Date"))?;
        let mut oss_headers =
            build.oss_headers.iter().map(|(k, v)| (k.to_lowercase(), v)).collect::<Vec<_>>();

        oss_headers.sort_by(|a, b| a.0.cmp(&b.0));

        let mut canonicalized_oss_headers =
            oss_headers.iter().map(|(k, v)| format!("{}:{}", k, v)).collect::<Vec<_>>().join("\n");

        if oss_headers.len() == 1 {
            canonicalized_oss_headers = format!("{}\n", canonicalized_oss_headers);
        }

        let mut canonicalized_resource =
            self.format_oss_resource_str(self.bucket().as_str(), key.as_ref());
        if !build.parameters.is_empty() {
            let mut params = build.parameters.iter().collect::<Vec<_>>();
            params.sort_by(|a, b| a.0.cmp(b.0));
            canonicalized_resource = format!(
                "{}?{}",
                canonicalized_resource,
                params
                    .into_iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("&")
            );
        }
        let verb = build.method.to_string();
        let content_md5 = build.content_md5.clone().unwrap_or_default();
        let content_type = build.content_type.clone().unwrap_or_default();
        let sign_str = format!(
            "{}\n{}\n{}\n{}\n{}{}",
            verb,
            content_md5,
            content_type,
            date,
            canonicalized_oss_headers,
            canonicalized_resource,
        );
        debug!("oss logsign_str: {}", sign_str);
        let mut hasher: Hmac<sha1::Sha1> = Hmac::new_from_slice(self.key_secret().as_bytes())
            .map_err(|e| OssError::Err(format!("hmac init failed: {e}")))?;
        hasher.update(sign_str.as_bytes());

        Ok(general_purpose::STANDARD.encode(hasher.finalize().into_bytes()))
    }

    fn oss_sign<S: AsRef<str>>(
        &self,
        object: S,
        build: &RequestBuilder,
    ) -> Result<String, OssError> {
        let sign_str_base64 = self.sign(object, build)?;
        Ok(format!("OSS {}:{}", self.key_id(), sign_str_base64))
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthAPI, OssError};
    use crate::oss_client::{oss::Oss, request::RequestBuilder};

    #[test]
    fn sign_returns_error_when_date_header_missing() {
        let oss = Oss::new("id", "secret", "oss-cn-hangzhou.aliyuncs.com", "bucket");
        let build = RequestBuilder::new();
        let result = oss.sign("logs/test.txt", &build);
        assert!(matches!(result, Err(OssError::MissingHeader("Date"))));
    }
}
