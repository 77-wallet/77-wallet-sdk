use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
struct FakeUploader {
    reject: AtomicBool,
    calls: std::sync::Mutex<Vec<(String, String, Vec<u8>)>>,
}
#[async_trait::async_trait]
impl LogUploader for FakeUploader {
    async fn upload(
        &self,
        sn: &str,
        filename: &str,
        data: Vec<u8>,
    ) -> Result<(), crate::error::system::SystemError> {
        self.calls.lock().unwrap().push((sn.into(), filename.into(), data));
        if self.reject.load(Ordering::SeqCst) {
            Err(crate::error::system::SystemError::Service("synthetic rejection".into()))
        } else {
            Ok(())
        }
    }
}

fn consumer(path: LogBasePath, uploader: Arc<FakeUploader>) -> UploadLogConsumer {
    let (_, rx) = broadcast::channel(1);
    UploadLogConsumer::new(rx, path, 300, uploader, "device-sn".into())
}

#[tokio::test]
async fn failed_upload_preserves_checkpoint_and_retry_sends_same_pending_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let base = LogBasePath(dir.path().into());
    let pending = vec![b'x'; 2048];
    let mut content = b"file-uid\nalready-sent\n".to_vec();
    let start = content.len() as u64;
    content.extend_from_slice(&pending);
    tokio::fs::write(base.log_path(), &content).await.unwrap();
    let mut tracker = OffsetTracker::new(base.offset_path()).await;
    tracker.set_uid("file-uid".into());
    tracker.set_offset(start);
    tracker.save().await;
    let before = tokio::fs::read(base.offset_path()).await.unwrap();
    let fake = Arc::new(FakeUploader::default());
    fake.reject.store(true, Ordering::SeqCst);
    let worker = consumer(base.clone(), fake.clone());
    assert!(worker.upload_pending(&mut tracker).await.is_err());
    assert_eq!(tracker.get_offset(), start);
    assert_eq!(tracker.get_uid(), "file-uid");
    assert_eq!(tokio::fs::read(base.offset_path()).await.unwrap(), before);
    fake.reject.store(false, Ordering::SeqCst);
    worker.upload_pending(&mut tracker).await.unwrap();
    let loaded = OffsetTracker::new(base.offset_path()).await;
    assert_eq!(loaded.get_offset(), content.len() as u64);
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    for (sn, filename, bytes) in calls.iter() {
        assert_eq!(sn, "device-sn");
        assert!(filename.starts_with("sdk:"));
        assert_eq!(bytes, &pending);
    }
}

#[tokio::test]
async fn rotated_file_commits_new_identity_only_after_acknowledgement() {
    let dir = tempfile::tempdir().unwrap();
    let base = LogBasePath(dir.path().into());
    let pending = vec![b'y'; 1024];
    let mut content = b"new-uid\n".to_vec();
    content.extend_from_slice(&pending);
    tokio::fs::write(base.log_path(), &content).await.unwrap();
    let mut tracker = OffsetTracker::new(base.offset_path()).await;
    tracker.set_uid("old-uid".into());
    tracker.set_offset(9000);
    tracker.save().await;
    let fake = Arc::new(FakeUploader::default());
    fake.reject.store(true, Ordering::SeqCst);
    let worker = consumer(base.clone(), fake.clone());
    assert!(worker.upload_pending(&mut tracker).await.is_err());
    assert_eq!(tracker.get_uid(), "old-uid");
    assert_eq!(tracker.get_offset(), 9000);
    fake.reject.store(false, Ordering::SeqCst);
    worker.upload_pending(&mut tracker).await.unwrap();
    let loaded = OffsetTracker::new(base.offset_path()).await;
    assert_eq!(loaded.get_uid(), "new-uid");
    assert_eq!(loaded.get_offset(), content.len() as u64);
    assert_eq!(fake.calls.lock().unwrap()[1].2, pending);
}

#[tokio::test]
async fn less_than_one_kib_waits_without_upload_or_checkpoint_change() {
    let dir = tempfile::tempdir().unwrap();
    let base = LogBasePath(dir.path().into());
    tokio::fs::write(base.log_path(), b"uid\nsmall log").await.unwrap();
    let mut tracker = OffsetTracker::new(base.offset_path()).await;
    let fake = Arc::new(FakeUploader::default());
    consumer(base.clone(), fake.clone()).upload_pending(&mut tracker).await.unwrap();
    assert!(fake.calls.lock().unwrap().is_empty());
    assert_eq!(tracker.get_offset(), 0);
    assert_eq!(tracker.get_uid(), "");
    assert!(!base.offset_path().exists());
}

#[tokio::test]
async fn missing_config_starts_no_background_upload_task() {
    let dir = tempfile::tempdir().unwrap();
    let handle =
        UploadLogHandle::new(LogBasePath(dir.path().into()), 300, None, "device-sn".into()).await;
    assert!(handle.handle.lock().await.is_none());
    handle.close().await.unwrap();
}

#[tokio::test]
async fn diagnostic_upload_failure_then_success_keeps_checkpoint_and_omits_raw_errors() {
    let dir = tempfile::tempdir().unwrap();
    let base = LogBasePath(dir.path().into());
    tokio::fs::write(base.log_path(), [b"uid\n".as_slice(), &vec![b'x'; 2048]].concat())
        .await
        .unwrap();
    let mut tracker = OffsetTracker::new(base.offset_path()).await;
    let fake = Arc::new(FakeUploader::default());
    fake.reject.store(true, Ordering::SeqCst);
    let (emitter, mut rx) = crate::messaging::notify::diagnostic::DiagnosticEmitter::test_channel();
    let mut worker = consumer(base, fake.clone());
    worker.diagnostic = Some(emitter);
    assert!(worker.upload_pending(&mut tracker).await.is_err());
    assert_eq!(tracker.get_offset(), 0);
    fake.reject.store(false, Ordering::SeqCst);
    worker.upload_pending(&mut tracker).await.unwrap();
    assert_eq!(tracker.get_offset(), 2052);
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(serde_json::to_value(e).unwrap());
    }
    let text = serde_json::to_string(&events).unwrap();
    assert!(!text.contains("synthetic rejection"));
    assert!(events.iter().any(|e| e["data"]["message"]["outcome"] == "error"));
    assert!(events.iter().any(|e| e["data"]["message"]["outcome"] == "ok"));
    assert!(events.iter().all(|e| e["event"] == "DEBUG"));
}

#[test]
fn upload_error_diagnostics_use_categories_not_server_or_local_details() {
    use crate::error::system::SystemError;
    assert_eq!(
        upload_error_category(&SystemError::Service("log upload timed out".into())),
        "timeout"
    );
    assert_eq!(
        upload_error_category(&SystemError::Service("log upload transport failed".into())),
        "transport"
    );
    assert_eq!(
        upload_error_category(&SystemError::Service(
            "backend rejected log upload: code=401, msg=secret".into()
        )),
        "backend_rejected"
    );
    assert_eq!(
        upload_error_category(&SystemError::IoError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "secret/path"
        ))),
        "local_file_not_found"
    );
    assert_eq!(upload_error_category(&SystemError::Service("unknown secret".into())), "other");
    assert_eq!(
        upload_http_status(&SystemError::Service("log upload returned HTTP 403".into())),
        Some(403)
    );
    assert_eq!(
        upload_http_status(&SystemError::Service("log upload returned HTTP 403 secret".into())),
        None
    );
}

#[tokio::test]
async fn disabled_upload_reports_configuration_without_starting_a_worker() {
    let dir = tempfile::tempdir().unwrap();
    let (emitter, mut rx) = crate::messaging::notify::diagnostic::DiagnosticEmitter::test_channel();
    let handle = UploadLogHandle::new_with_diagnostics(
        LogBasePath(dir.path().into()),
        300,
        None,
        "test-device".into(),
        emitter,
    )
    .await;
    assert!(handle.handle.lock().await.is_none());
    let value = serde_json::to_value(rx.try_recv().unwrap()).unwrap();
    assert_eq!(value["data"]["message"]["event"], "configured");
    assert_eq!(value["data"]["message"]["enabled"], false);
    handle.close().await.unwrap();
}

#[tokio::test]
async fn worker_reports_missing_local_log_without_exposing_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let (stop, rx) = broadcast::channel(1);
    let (emitter, mut events) =
        crate::messaging::notify::diagnostic::DiagnosticEmitter::test_channel();
    let mut worker = UploadLogConsumer::new(
        rx,
        LogBasePath(dir.path().into()),
        300,
        Arc::new(FakeUploader::default()),
        "test-device".into(),
    );
    worker.diagnostic = Some(emitter);
    let task = tokio::spawn(async move { worker.run().await });
    let tick = tokio::time::timeout(Duration::from_secs(2), events.recv()).await.unwrap().unwrap();
    assert_eq!(serde_json::to_value(tick).unwrap()["data"]["message"]["event"], "tick");
    let failure =
        tokio::time::timeout(Duration::from_secs(2), events.recv()).await.unwrap().unwrap();
    let value = serde_json::to_value(failure).unwrap();
    assert_eq!(value["data"]["message"]["error_category"], "local_file_not_found");
    assert!(!value.to_string().contains(&dir.path().display().to_string()));
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}

#[test]
fn detailed_transport_failure_retains_the_transport_category() {
    let error=crate::error::system::SystemError::Service(
        "log upload transport failed: request=credentials step=send kind=connect io_kind=connection_refused cause_hint=unknown".into());
    assert_eq!(upload_error_category(&error), "transport");
}

#[test]
fn transport_details_allow_only_fixed_categories_and_omit_unknown_text() {
    use crate::error::system::SystemError;
    let error=SystemError::Service("log upload transport failed: request=file step=response_body kind=body io_kind=connection_reset cause_hint=unknown private=private-token".into());
    let detail = upload_transport_detail(&error).unwrap();
    assert_eq!(detail["request"], "file");
    assert_eq!(detail["step"], "response_body");
    assert_eq!(detail["io_kind"], "connection_reset");
    assert!(!detail.to_string().contains("private-token"));
    let malformed = SystemError::Service(
        "log upload transport failed: request=https://private.example step=send kind=connect"
            .into(),
    );
    assert!(upload_transport_detail(&malformed).is_none());
    let timeout=SystemError::Service("log upload transport failed: request=credentials step=send kind=timeout io_kind=unknown cause_hint=unknown".into());
    assert_eq!(upload_error_category(&timeout), "timeout");
    assert_eq!(upload_transport_detail(&timeout).unwrap()["request"], "credentials");
}
