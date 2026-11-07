//! Multipart transport for backend-owned log storage, independent of ECDH requests.
use reqwest::{Client, Request, StatusCode, Url, multipart, redirect::Policy};
use serde::Deserialize;
use std::{fmt, time::Duration};
use tokio::sync::Mutex;

const MAX_ACK_BYTES: usize = 64 * 1024;

#[derive(Clone, Deserialize)]
pub struct LogUploadConfig {
    pub url: String,
}

impl fmt::Debug for LogUploadConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogUploadConfig").field("url", &"[configured endpoint]").finish()
    }
}

/// Errors deliberately exclude response bodies, response data, and credentials.
#[derive(Debug, thiserror::Error)]
pub enum LogUploadError {
    #[error("invalid log upload configuration")]
    InvalidConfig,
    #[error("invalid log upload metadata")]
    InvalidMetadata,
    #[error("failed to build log upload request")]
    Build,
    #[error("log upload transport failed")]
    Transport,
    #[error("log upload timed out")]
    Timeout,
    #[error(
        "log upload transport failed: request={request} step={step} kind={kind} io_kind={io_kind} cause_hint={cause_hint}"
    )]
    RequestTransport {
        request: &'static str,
        step: &'static str,
        kind: &'static str,
        io_kind: &'static str,
        cause_hint: &'static str,
    },
    #[error("log upload returned HTTP {0}")]
    Http(u16),
    #[error("invalid log upload acknowledgement")]
    InvalidAcknowledgement,
    #[error("backend rejected log upload: code={code}, msg={message}")]
    Rejected { code: String, message: String },
}

pub struct LogUploadClient {
    client: Client,
    endpoint: Url,
    credential_endpoint: Url,
    credentials: Mutex<Option<UploadCredentials>>,
}

impl fmt::Debug for LogUploadClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogUploadClient")
            .field("endpoint", &"[configured endpoint]")
            .field("credential_endpoint", &"[configured endpoint]")
            .field("credentials", &"[redacted]")
            .finish()
    }
}

#[derive(Clone)]
struct UploadCredentials {
    key: String,
    secret: String,
}

impl LogUploadClient {
    pub fn new(config: LogUploadConfig) -> Result<Self, LogUploadError> {
        let endpoint = Url::parse(&config.url).map_err(|_| LogUploadError::InvalidConfig)?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(LogUploadError::InvalidConfig);
        }
        let mut credential_endpoint = endpoint.clone();
        credential_endpoint.set_path("/log/getKey");
        // A 307/308 redirect could otherwise forward authentication and log content.
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| LogUploadError::Build)?;
        Ok(Self { client, endpoint, credential_endpoint, credentials: Mutex::new(None) })
    }

    fn build_request(
        &self,
        credentials: &UploadCredentials,
        sn: &str,
        filename: &str,
        data: Vec<u8>,
    ) -> Result<Request, LogUploadError> {
        if sn.trim().is_empty()
            || filename.trim().is_empty()
            || credentials.key.trim().is_empty()
            || credentials.secret.trim().is_empty()
            || matches!(filename, "." | "..")
            || filename.chars().any(|c| c.is_control() || c == '/' || c == '\\')
        {
            return Err(LogUploadError::InvalidMetadata);
        }
        let file = multipart::Part::bytes(data)
            .file_name(filename.to_owned())
            .mime_str("text/plain")
            .map_err(|_| LogUploadError::Build)?;
        let form = multipart::Form::new()
            .text("key", credentials.key.clone())
            .text("secret", credentials.secret.clone())
            .text("sn", sn.to_owned())
            .part("file", file);
        self.client
            .post(self.endpoint.clone())
            .multipart(form)
            .build()
            .map_err(|_| LogUploadError::Build)
    }

    /// Returns success only for HTTP 2xx with an explicit `success: true` envelope.
    /// Refreshes authorization and retries once only when the backend returns business code 401.
    pub async fn upload(
        &self,
        sn: &str,
        filename: &str,
        data: Vec<u8>,
    ) -> Result<(), LogUploadError> {
        for attempt in 0..=1 {
            let credentials = self.get_credentials(sn, attempt == 1).await?;
            let request = self.build_request(&credentials, sn, filename, data.clone())?;
            let response = self.execute_bounded(request).await;
            if attempt == 0
                && matches!(
                    &response,
                    Err(LogUploadError::Rejected { code, .. }) if code == "401"
                )
            {
                continue;
            }
            return response;
        }
        unreachable!("upload attempts are bounded")
    }

    async fn get_credentials(
        &self,
        sn: &str,
        force_refresh: bool,
    ) -> Result<UploadCredentials, LogUploadError> {
        let mut cached = self.credentials.lock().await;
        if !force_refresh {
            if let Some(credentials) = cached.as_ref() {
                return Ok(credentials.clone());
            }
        } else {
            *cached = None;
        }
        let request = self
            .client
            .post(self.credential_endpoint.clone())
            .json(&serde_json::json!({ "sn": sn }))
            .build()
            .map_err(|_| LogUploadError::Build)?;
        let credentials = self.fetch_credentials(request).await?;
        *cached = Some(credentials.clone());
        Ok(credentials)
    }

    async fn fetch_credentials(
        &self,
        request: Request,
    ) -> Result<UploadCredentials, LogUploadError> {
        let (status, body) = self.execute_response(request, "credentials").await?;
        if !status.is_success() {
            return Err(LogUploadError::Http(status.as_u16()));
        }
        #[derive(Deserialize)]
        struct CredentialEnvelope {
            success: bool,
            data: Option<UploadCredentialsBody>,
        }
        #[derive(Deserialize)]
        struct UploadCredentialsBody {
            key: String,
            secret: String,
        }
        let envelope: CredentialEnvelope =
            serde_json::from_slice(&body).map_err(|_| LogUploadError::InvalidAcknowledgement)?;
        let data = envelope
            .data
            .filter(|_| envelope.success)
            .ok_or(LogUploadError::InvalidAcknowledgement)?;
        if data.key.trim().is_empty() || data.secret.trim().is_empty() {
            return Err(LogUploadError::InvalidAcknowledgement);
        }
        Ok(UploadCredentials { key: data.key, secret: data.secret })
    }

    async fn execute_bounded(&self, request: Request) -> Result<(), LogUploadError> {
        let (status, body) = self.execute_response(request, "file").await?;
        validate_acknowledgement(status, &body)
    }

    async fn execute_response(
        &self,
        request: Request,
        operation: &'static str,
    ) -> Result<(StatusCode, Vec<u8>), LogUploadError> {
        let mut response = self
            .client
            .execute(request)
            .await
            .map_err(|e| transport_error(e, operation, "send"))?;
        let status = response.status();
        let mut body = Vec::new();
        while let Some(chunk) =
            response.chunk().await.map_err(|e| transport_error(e, operation, "response_body"))?
        {
            if chunk.len() > MAX_ACK_BYTES - body.len() {
                return Err(LogUploadError::InvalidAcknowledgement);
            }
            body.extend_from_slice(&chunk);
        }
        Ok((status, body))
    }
}

fn transport_error(
    error: reqwest::Error,
    request: &'static str,
    step: &'static str,
) -> LogUploadError {
    let kind = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_body() {
        "body"
    } else if error.is_decode() {
        "decode"
    } else if error.is_request() {
        "request"
    } else {
        "other"
    };
    let (io_kind, cause_hint) = safe_source_details(&error);
    LogUploadError::RequestTransport { request, step, kind, io_kind, cause_hint }
}

// Only fixed categories leave this function. Source text is never retained or emitted.
// DNS/certificate hints are best effort, not an authoritative diagnosis.
fn safe_source_details(error: &(dyn std::error::Error + 'static)) -> (&'static str, &'static str) {
    let mut current = Some(error);
    let mut io_kind = "unknown";
    let mut hint = "unknown";
    for _ in 0..16 {
        let Some(source) = current else { break };
        if io_kind == "unknown" {
            if let Some(io) = source.downcast_ref::<std::io::Error>() {
                io_kind = match io.kind() {
                    std::io::ErrorKind::ConnectionRefused => "connection_refused",
                    std::io::ErrorKind::ConnectionReset => "connection_reset",
                    std::io::ErrorKind::ConnectionAborted => "connection_aborted",
                    std::io::ErrorKind::BrokenPipe => "broken_pipe",
                    std::io::ErrorKind::TimedOut => "timed_out",
                    std::io::ErrorKind::NotConnected => "not_connected",
                    _ => "unknown",
                };
            }
        }
        if hint == "unknown" {
            let text = source.to_string().to_ascii_lowercase();
            if [
                "invalid peer certificate",
                "certificate verify failed",
                "unknownissuer",
                "certificateexpired",
            ]
            .iter()
            .any(|s| text.contains(s))
            {
                hint = "tls_certificate";
            } else if [
                "dns error",
                "failed to lookup address",
                "name or service not known",
                "nodename nor servname",
                "getaddrinfo",
            ]
            .iter()
            .any(|s| text.contains(s))
            {
                hint = "dns";
            }
        }
        current = source.source();
    }
    (io_kind, hint)
}

fn validate_acknowledgement(status: StatusCode, body: &[u8]) -> Result<(), LogUploadError> {
    if !status.is_success() {
        return Err(LogUploadError::Http(status.as_u16()));
    }
    // Same success flag as BackendResponseOk. Upload needs no returned data, so it
    // neither decrypts `data` nor logs/propagates a potentially sensitive `msg`.
    #[derive(Deserialize)]
    struct Acknowledgement {
        success: bool,
        #[serde(default)]
        code: Option<serde_json::Value>,
        #[serde(default)]
        msg: Option<String>,
    }
    let ack: Acknowledgement =
        serde_json::from_slice(body).map_err(|_| LogUploadError::InvalidAcknowledgement)?;
    if ack.success {
        Ok(())
    } else {
        let code = ack.code.as_ref().map(format_ack_code).unwrap_or_else(|| "unknown".to_string());
        let message = ack
            .msg
            .as_deref()
            .map(|value| sanitize_ack_field(value, 512))
            .unwrap_or_else(|| "unknown".to_string());
        Err(LogUploadError::Rejected { code, message })
    }
}

fn format_ack_code(value: &serde_json::Value) -> String {
    let raw = match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        _ => "unknown".to_string(),
    };
    sanitize_ack_field(&raw, 128)
}

fn sanitize_ack_field(value: &str, max_chars: usize) -> String {
    let sanitized: String = value
        .chars()
        .take(max_chars)
        .map(|character| if character.is_control() { ' ' } else { character })
        .collect();
    if sanitized.trim().is_empty() { "unknown".to_string() } else { sanitized }
}

#[cfg(test)]
#[path = "log_upload_tests.rs"]
mod tests;
