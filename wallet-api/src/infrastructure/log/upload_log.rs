use crate::{
    error::service::ServiceError,
    infrastructure::log::{format::LogBasePath, offset_tracker::OffsetTracker},
    messaging::notify::diagnostic::DiagnosticEmitter,
};
use std::{io::SeekFrom, sync::Arc, time::Duration};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader},
    sync::{Mutex, broadcast},
    task::JoinHandle,
    time::interval,
};
use wallet_transport_backend::log_upload::LogUploadClient;

#[async_trait::async_trait]
pub(crate) trait LogUploader: Send + Sync {
    async fn upload(
        &self,
        sn: &str,
        filename: &str,
        data: Vec<u8>,
    ) -> Result<(), crate::error::system::SystemError>;
}

#[async_trait::async_trait]
impl LogUploader for LogUploadClient {
    async fn upload(
        &self,
        sn: &str,
        filename: &str,
        data: Vec<u8>,
    ) -> Result<(), crate::error::system::SystemError> {
        LogUploadClient::upload(self, sn, filename, data)
            .await
            .map_err(|e| crate::error::system::SystemError::Service(e.to_string()))
    }
}

#[derive(Debug)]
pub struct UploadLogHandle {
    shutdown_tx: broadcast::Sender<()>,
    handle: Mutex<Option<JoinHandle<Result<(), ServiceError>>>>,
}

impl UploadLogHandle {
    pub async fn new(
        base_path: LogBasePath,
        interval_sec: u64,
        client: Option<Arc<LogUploadClient>>,
        sn: String,
    ) -> Self {
        Self::start(base_path, interval_sec, client, sn, None).await
    }

    pub(crate) async fn new_with_diagnostics(
        base_path: LogBasePath,
        interval_sec: u64,
        client: Option<Arc<LogUploadClient>>,
        sn: String,
        diagnostic: DiagnosticEmitter,
    ) -> Self {
        Self::start(base_path, interval_sec, client, sn, Some(diagnostic)).await
    }

    async fn start(
        base_path: LogBasePath,
        interval_sec: u64,
        client: Option<Arc<LogUploadClient>>,
        sn: String,
        diagnostic: Option<DiagnosticEmitter>,
    ) -> Self {
        if let Some(emitter) = &diagnostic {
            emitter.emit(serde_json::json!({"component":"log_upload", "event":"configured", "enabled":client.is_some(), "interval_sec":interval_sec}));
        }
        let (shutdown_tx, _) = broadcast::channel(1);
        let handle = client.map(|client| {
            let mut worker = UploadLogConsumer::new(
                shutdown_tx.subscribe(),
                base_path,
                interval_sec,
                client,
                sn,
            );
            worker.diagnostic = diagnostic;
            tokio::spawn(async move { worker.run().await })
        });
        Self { shutdown_tx, handle: Mutex::new(handle) }
    }

    pub(crate) async fn close(&self) -> Result<(), ServiceError> {
        let _ = self.shutdown_tx.send(());
        if let Some(handle) = self.handle.lock().await.take() {
            handle.await.map_err(|_| {
                ServiceError::System(crate::error::system::SystemError::BackendEndpointNotFound)
            })??;
        }
        Ok(())
    }
}

struct UploadLogConsumer {
    shutdown_rx: broadcast::Receiver<()>,
    base_path: LogBasePath,
    interval_sec: u64,
    uploader: Arc<dyn LogUploader>,
    sn: String,
    diagnostic: Option<DiagnosticEmitter>,
}

impl UploadLogConsumer {
    fn new(
        shutdown_rx: broadcast::Receiver<()>,
        base_path: LogBasePath,
        interval_sec: u64,
        uploader: Arc<dyn LogUploader>,
        sn: String,
    ) -> Self {
        Self { shutdown_rx, base_path, interval_sec, uploader, sn, diagnostic: None }
    }

    fn notify(
        &self,
        event: &'static str,
        outcome: &'static str,
        bytes: Option<u64>,
        error: Option<&'static str>,
    ) {
        if let Some(emitter) = &self.diagnostic {
            emitter.emit(serde_json::json!({"component":"log_upload", "event":event,
                "outcome":outcome, "bytes":bytes, "error_category":error}));
        }
    }

    async fn run(&mut self) -> Result<(), ServiceError> {
        let mut interval = interval(Duration::from_secs(self.interval_sec));
        let mut tracker = OffsetTracker::new(self.base_path.offset_path()).await;
        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => break,
                _ = interval.tick() => {
                    self.notify("tick", "begin", None, None);
                    if let Err(e) = self.upload_pending(&mut tracker).await {
                        self.notify("cycle", "error", None, Some(upload_error_category(&e)));
                        tracing::error!("backend log upload failed: {}", e);
                    }
                }
            }
        }
        Ok(())
    }

    async fn upload_pending(
        &self,
        tracker: &mut OffsetTracker,
    ) -> Result<(), crate::error::system::SystemError> {
        // Read identity and content through the same descriptor, even if the path rotates.
        let file = File::open(self.base_path.log_path()).await?;
        let mut reader = BufReader::new(file);
        let mut first_line = String::new();
        let header_bytes = reader.read_line(&mut first_line).await? as u64;
        let uid = first_line.trim();
        let offset = if uid == tracker.get_uid() {
            tracker.get_offset().max(header_bytes)
        } else {
            header_bytes
        };
        reader.seek(SeekFrom::Start(offset)).await?;
        let mut data = Vec::new();
        let bytes_read = reader.read_to_end(&mut data).await?;
        if data.len() < 1024 {
            self.notify("skip_below_threshold", "ok", Some(bytes_read as u64), None);
            return Ok(());
        }
        let filename = format!("sdk:{}.txt", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"));
        self.notify("request", "begin", Some(bytes_read as u64), None);
        let started = std::time::Instant::now();
        let result = self.uploader.upload(&self.sn, &filename, data).await;
        if let Some(emitter) = &self.diagnostic {
            emitter.emit(serde_json::json!({"component":"log_upload", "event":"request", "outcome":if result.is_ok(){"ok"}else{"error"},
                "elapsed_ms":started.elapsed().as_secs_f64()*1000.0, "bytes":bytes_read,
                "error_category":result.as_ref().err().map(upload_error_category),
                "http_status":result.as_ref().err().and_then(upload_http_status),
                "failure_detail":result.as_ref().err().and_then(upload_transport_detail)}));
        }
        result?;
        // A rejected request must leave BOTH offset and identity unchanged for retry.
        tracker.set_uid(uid.to_owned());
        tracker.set_offset(offset + bytes_read as u64);
        tracker.save().await;
        tracing::info!(bytes = bytes_read, "backend log upload succeeded");
        Ok(())
    }
}

// Never forward server messages, URLs, local paths or credentials.
fn upload_error_category(error: &crate::error::system::SystemError) -> &'static str {
    use crate::error::system::SystemError;
    match error {
        SystemError::IoError(e) if e.kind() == std::io::ErrorKind::NotFound => {
            "local_file_not_found"
        }
        SystemError::IoError(_) => "local_io",
        SystemError::Service(e) if e == "log upload timed out" => "timeout",
        SystemError::Service(e) if e == "log upload transport failed" => "transport",
        SystemError::Service(e) if e.starts_with("log upload transport failed: ") => {
            if e.split_whitespace().any(|field| field == "kind=timeout") {
                "timeout"
            } else {
                "transport"
            }
        }
        SystemError::Service(e) if e.starts_with("log upload returned HTTP ") => "http_status",
        SystemError::Service(e) if e.starts_with("backend rejected log upload:") => {
            "backend_rejected"
        }
        SystemError::Service(e) if e == "invalid log upload acknowledgement" => {
            "invalid_acknowledgement"
        }
        _ => "other",
    }
}

// The service boundary retains errors as strings. Parse only our fixed grammar and
// allowlisted values, never forward arbitrary service/server text.
fn upload_transport_detail(error: &crate::error::system::SystemError) -> Option<serde_json::Value> {
    let crate::error::system::SystemError::Service(message) = error else { return None };
    let fields = message.strip_prefix("log upload transport failed: ")?;
    let mut detail = serde_json::Map::new();
    for field in fields.split_whitespace().take(8) {
        let Some((key, value)) = field.split_once('=') else { continue };
        let allowed = match key {
            "request" => matches!(value, "credentials" | "file"),
            "step" => matches!(value, "send" | "response_body"),
            "kind" => {
                matches!(value, "timeout" | "connect" | "body" | "decode" | "request" | "other")
            }
            "io_kind" => matches!(
                value,
                "connection_refused"
                    | "connection_reset"
                    | "connection_aborted"
                    | "broken_pipe"
                    | "timed_out"
                    | "not_connected"
                    | "unknown"
            ),
            "cause_hint" => matches!(value, "dns" | "tls_certificate" | "unknown"),
            _ => false,
        };
        if allowed {
            detail.insert(key.into(), value.into());
        }
    }
    if ["request", "step", "kind"].iter().all(|key| detail.contains_key(*key)) {
        Some(detail.into())
    } else {
        None
    }
}

fn upload_http_status(error: &crate::error::system::SystemError) -> Option<u16> {
    if let crate::error::system::SystemError::Service(message) = error {
        return message
            .strip_prefix("log upload returned HTTP ")?
            .parse::<u16>()
            .ok()
            .filter(|status| (100..=599).contains(status));
    }
    None
}

#[cfg(test)]
#[path = "upload_log_tests.rs"]
mod tests;
