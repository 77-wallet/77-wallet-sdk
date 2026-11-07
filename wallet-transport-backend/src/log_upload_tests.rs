use super::*;
use http_body_util::BodyExt;

fn config() -> LogUploadConfig {
    LogUploadConfig { url: "https://logs.example.invalid/log/uploadFile".into() }
}

#[tokio::test]
async fn multipart_contains_auth_device_and_exact_file_content() {
    let client = LogUploadClient::new(config()).unwrap();
    let mut request = client
        .build_request(
            &UploadCredentials {
                key: "synthetic-upload-key".into(),
                secret: "synthetic-upload-secret".into(),
            },
            "test-device",
            "sdk-test.txt",
            b"line one\nline two\n".to_vec(),
        )
        .unwrap();
    assert_eq!(request.method(), reqwest::Method::POST);
    assert_eq!(request.url().as_str(), config().url);
    let content_type = request.headers()[reqwest::header::CONTENT_TYPE].to_str().unwrap();
    assert!(content_type.starts_with("multipart/form-data; boundary="));
    let boundary = content_type.split("boundary=").nth(1).unwrap().to_owned();
    let body = request.body_mut().take().unwrap().collect().await.unwrap().to_bytes();
    let body = std::str::from_utf8(&body).unwrap();
    for (field, value) in [
        ("key", "synthetic-upload-key"),
        ("secret", "synthetic-upload-secret"),
        ("sn", "test-device"),
    ] {
        assert!(body.contains(&format!("name=\"{field}\"\r\n\r\n{value}\r\n")));
    }
    assert!(body.contains("name=\"file\"; filename=\"sdk-test.txt\""));
    assert!(body.contains("line one\nline two\n\r\n"));
    assert!(body.ends_with(&format!("--{boundary}--\r\n")));
}

#[test]
fn invalid_config_and_metadata_are_rejected_before_network_io() {
    for url in [
        "",
        "file:///tmp/log",
        "https://user:password@example.invalid/upload",
        "https://example.invalid/upload?secret=bad",
        "https://example.invalid/upload#fragment",
    ] {
        let mut cfg = config();
        cfg.url = url.into();
        assert!(LogUploadClient::new(cfg).is_err());
    }
    let client = LogUploadClient::new(config()).unwrap();
    let credentials = UploadCredentials { key: "key".into(), secret: "secret".into() };
    assert!(client.build_request(&credentials, "", "log.txt", vec![1]).is_err());
    for filename in ["", "../log.txt", "a/b.txt", "a\\b.txt", "bad\r\n.txt"] {
        assert!(client.build_request(&credentials, "test-device", filename, vec![1]).is_err());
    }
}

#[test]
fn rejected_acknowledgement_includes_backend_code_and_message_only() {
    let body = br#"{
        "success": false,
        "code": 40101,
        "msg": "invalid upload credentials",
        "data": {"token": "must-not-appear"}
    }"#;

    let error = validate_acknowledgement(StatusCode::OK, body).unwrap_err();
    let rendered = error.to_string();
    assert_eq!(rendered, "backend rejected log upload: code=40101, msg=invalid upload credentials");
    assert!(!rendered.contains("must-not-appear"));
}

#[test]
fn config_debug_does_not_reveal_credentials() {
    let output = format!("{:?}", config());
    assert!(!output.contains("key"));
    assert!(!output.contains("secret"));
}

#[test]
fn acknowledgement_requires_http_and_business_success() {
    use reqwest::StatusCode;
    assert!(
        validate_acknowledgement(
            StatusCode::OK,
            br#"{"success":true,"code":"200","msg":"ok","data":null}"#
        )
        .is_ok()
    );
    for (status, body) in [
        (StatusCode::OK, br#"{"success":false,"msg":"rejected"}"#.as_slice()),
        (StatusCode::BAD_REQUEST, br#"{"success":true}"#.as_slice()),
        (StatusCode::FOUND, br#"{"success":true}"#.as_slice()),
        (StatusCode::NO_CONTENT, b"".as_slice()),
        (StatusCode::OK, br#"{"code":"200"}"#.as_slice()),
        (StatusCode::OK, b"<html>error</html>".as_slice()),
    ] {
        assert!(validate_acknowledgement(status, body).is_err());
    }
}

// Loopback-only fake backend: no external service or real credentials.
fn fake_backend(
    responses: Vec<Vec<u8>>,
) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/log/uploadFile", listener.local_addr().unwrap());
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = requests.clone();
    let handle = std::thread::spawn(move || {
        for response in responses {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "fake backend accept timed out"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(e) => panic!("fake backend accept failed: {e}"),
                }
            };
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            stream.set_write_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut request = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&request[..end]).unwrap().to_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap_or("0")
                        .trim()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            captured.lock().unwrap().push(request);
            let _ = stream.write_all(&response);
        }
    });
    (url, requests, handle)
}

fn ok(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes()
}

fn credentials(key: &str, secret: &str) -> Vec<u8> {
    ok(&format!(
        r#"{{"code":"200","msg":null,"data":{{"key":"{key}","secret":"{secret}"}},"success":true}}"#
    ))
}

#[tokio::test]
async fn upload_fetches_credentials_before_sending_file() {
    let (url, requests, server) =
        fake_backend(vec![credentials("first-key", "first-secret"), ok(r#"{"success":true}"#)]);
    let client = LogUploadClient::new(LogUploadConfig { url }).unwrap();

    client.upload("test-device", "log.txt", b"test log".to_vec()).await.unwrap();

    server.join().unwrap();
    let requests = requests.lock().unwrap();
    let auth = String::from_utf8_lossy(&requests[0]);
    let upload = String::from_utf8_lossy(&requests[1]);
    assert!(auth.starts_with("POST /log/getKey "));
    assert!(auth.contains(r#"{"sn":"test-device"}"#));
    assert!(upload.starts_with("POST /log/uploadFile "));
    assert!(upload.contains("first-key"));
    assert!(upload.contains("first-secret"));
}

#[tokio::test]
async fn later_uploads_reuse_credentials_held_in_memory() {
    let (url, requests, server) = fake_backend(vec![
        credentials("cached-key", "cached-secret"),
        ok(r#"{"success":true}"#),
        ok(r#"{"success":true}"#),
    ]);
    let client = LogUploadClient::new(LogUploadConfig { url }).unwrap();

    client.upload("test-device", "first.txt", b"first".to_vec()).await.unwrap();
    client.upload("test-device", "second.txt", b"second".to_vec()).await.unwrap();

    server.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(String::from_utf8_lossy(&requests[1]).starts_with("POST /log/uploadFile "));
    assert!(String::from_utf8_lossy(&requests[2]).starts_with("POST /log/uploadFile "));
}

#[tokio::test]
async fn business_401_refreshes_credentials_and_retries_upload_once() {
    let (url, requests, server) = fake_backend(vec![
        credentials("expired-key", "expired-secret"),
        ok(r#"{"success":false,"code":"401","msg":"expired"}"#),
        credentials("fresh-key", "fresh-secret"),
        ok(r#"{"success":true}"#),
    ]);
    let client = LogUploadClient::new(LogUploadConfig { url }).unwrap();

    client.upload("test-device", "log.txt", b"test log".to_vec()).await.unwrap();

    server.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(String::from_utf8_lossy(&requests[3]).contains("fresh-key"));
}

#[tokio::test]
async fn repeated_business_401_stops_after_one_refresh() {
    let (url, requests, server) = fake_backend(vec![
        credentials("expired-key", "expired-secret"),
        ok(r#"{"success":false,"code":"401","msg":"expired"}"#),
        credentials("still-invalid-key", "still-invalid-secret"),
        ok(r#"{"success":false,"code":"401","msg":"expired again"}"#),
    ]);
    let client = LogUploadClient::new(LogUploadConfig { url }).unwrap();

    let result = client.upload("test-device", "log.txt", b"test log".to_vec()).await;

    server.join().unwrap();
    assert!(matches!(result, Err(LogUploadError::Rejected { code, .. }) if code == "401"));
    assert_eq!(requests.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn non_401_rejection_does_not_refresh_or_retry() {
    let (url, requests, server) = fake_backend(vec![
        credentials("valid-key", "valid-secret"),
        ok(r#"{"success":false,"code":"403","msg":"rejected"}"#),
    ]);
    let client = LogUploadClient::new(LogUploadConfig { url }).unwrap();

    let result = client.upload("test-device", "log.txt", b"test log".to_vec()).await;

    server.join().unwrap();
    assert!(matches!(result, Err(LogUploadError::Rejected { code, .. }) if code == "403"));
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn upload_executes_against_fake_backend_and_refuses_redirects_and_oversized_ack() {
    let mut oversized =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
    for _ in 0..2 {
        oversized.extend_from_slice(b"8001\r\n");
        oversized.extend(vec![b'x'; 32769]);
        oversized.extend_from_slice(b"\r\n");
    }
    oversized.extend_from_slice(b"0\r\n\r\n");
    let cases = [
        (b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"success\":true}".to_vec(), 0),
        (b"HTTP/1.1 307 Temporary Redirect\r\nLocation: /other\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(), 307),
        (b"HTTP/1.1 308 Permanent Redirect\r\nLocation: /other\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(), 308),
        (oversized, 1),
    ];
    for (response, expected) in cases {
        let (url, _, server) = fake_backend(vec![credentials("key", "secret"), response]);
        let result = LogUploadClient::new(LogUploadConfig { url })
            .unwrap()
            .upload("test-device", "log.txt", b"test log".to_vec())
            .await;
        server.join().unwrap();
        match expected {
            0 => assert!(result.is_ok()),
            1 => assert!(matches!(result, Err(LogUploadError::InvalidAcknowledgement))),
            status => {
                assert!(matches!(result, Err(LogUploadError::Http(actual)) if actual == status))
            }
        }
    }
}

#[tokio::test]
async fn credential_transport_failure_is_identified_before_any_file_request() {
    let (url, requests, server) = fake_backend(vec![vec![]]);
    let error = LogUploadClient::new(LogUploadConfig { url })
        .unwrap()
        .upload("synthetic-device", "private-filename.txt", b"private-log-content".to_vec())
        .await
        .unwrap_err();
    server.join().unwrap();
    let diagnostic = error.to_string();
    assert!(
        diagnostic.contains("request=credentials"),
        "missing credential request classification"
    );
    assert!(diagnostic.contains("step=send"));
    assert_eq!(requests.lock().unwrap().len(), 1);
    for secret in ["private-filename", "private-log-content", "synthetic-device", "127.0.0.1"] {
        assert!(!diagnostic.contains(secret));
    }
}

#[tokio::test]
async fn file_transport_failure_is_distinct_from_credentials_and_response_body() {
    let (url, requests, server) =
        fake_backend(vec![credentials("private-key", "private-secret"), vec![]]);
    let error = LogUploadClient::new(LogUploadConfig { url })
        .unwrap()
        .upload("synthetic-device", "private-filename.txt", b"private-log-content".to_vec())
        .await
        .unwrap_err();
    server.join().unwrap();
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("request=file"));
    assert!(diagnostic.contains("step=send"));
    assert_eq!(requests.lock().unwrap().len(), 2);
    for secret in
        ["private-key", "private-secret", "private-filename", "private-log-content", "127.0.0.1"]
    {
        assert!(!diagnostic.contains(secret));
    }
}

#[tokio::test]
async fn response_body_disconnect_has_separate_safe_diagnostics() {
    let incomplete=b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{\"private-response\":".to_vec();
    let (url, _, server) =
        fake_backend(vec![credentials("private-key", "private-secret"), incomplete]);
    let error = LogUploadClient::new(LogUploadConfig { url })
        .unwrap()
        .upload("synthetic-device", "log.txt", b"private-log-content".to_vec())
        .await
        .unwrap_err();
    server.join().unwrap();
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("request=file"));
    assert!(diagnostic.contains("step=response_body"));
    assert!(!diagnostic.contains("private-response"));
}

#[test]
fn transport_source_diagnostics_never_return_raw_source_text() {
    for (kind, text, expected_io, expected_hint) in [
        (
            std::io::ErrorKind::ConnectionReset,
            "private-token and private-log",
            "connection_reset",
            "unknown",
        ),
        (std::io::ErrorKind::Other, "dns error at private-host", "unknown", "dns"),
        (
            std::io::ErrorKind::Other,
            "invalid peer certificate: UnknownIssuer private-host",
            "unknown",
            "tls_certificate",
        ),
    ] {
        let error = std::io::Error::new(kind, text);
        assert_eq!(safe_source_details(&error), (expected_io, expected_hint));
    }
}
