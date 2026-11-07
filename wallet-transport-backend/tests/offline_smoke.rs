use std::collections::HashMap;

use serial_test::serial;
use wallet_ecdh::GLOBAL_KEY;
use wallet_transport_backend::{api::BackendApi, api_request::ApiBackendRequest};

fn make_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("clientId".to_string(), "offline-client".to_string());
    headers.insert("AW-SEC-ID".to_string(), "offline-aw".to_string());
    headers
}

fn make_cryptor() -> wallet_utils::cbc::AesCbcCryptor {
    wallet_utils::cbc::AesCbcCryptor::new("1234567890abcdef", "abcdef1234567890")
}

#[tokio::test]
async fn backend_api_new_works_without_network() -> Result<(), wallet_transport_backend::Error> {
    // Scenario: 仅构建客户端，不发起网络请求，离线必须稳定可跑。
    let mut api = BackendApi::new(
        Some("https://example.com".to_string()),
        Some(make_headers()),
        make_cryptor(),
    )?;
    assert_eq!(api.base_url, "https://example.com");

    api.replace_base_url("https://example.org");
    assert_eq!(api.base_url, "https://example.org");
    Ok(())
}

#[tokio::test]
async fn backend_api_new_rejects_invalid_headers() {
    // Scenario: 非法 header 应返回错误，不能 panic。
    let mut headers = make_headers();
    headers.insert("bad\nname".to_string(), "x".to_string());
    let err =
        BackendApi::new(Some("https://example.com".to_string()), Some(headers), make_cryptor())
            .expect_err("invalid header should fail");
    match err {
        wallet_transport_backend::Error::Backend(Some(msg)) => {
            assert!(msg.contains("invalid header name"))
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[serial]
#[test]
fn api_backend_request_builds_signed_payload_offline() -> Result<(), wallet_transport_backend::Error>
{
    // Scenario: 请求封装在离线环境可构建，并包含签名与密文字段。
    const TEST_PUB_KEY: &str = r#"-----BEGIN PUBLIC KEY-----
MFYwEAYHKoZIzj0CAQYFK4EEAAoDQgAEWDZNP0ClbeWJey9hBr2rsjSayQEBywnv
ZXi0RberQCAp+06fOjvr+jZI5qwYGglmMkGJw49tbni6qgm4QNV6WQ==
-----END PUBLIC KEY-----"#;
    GLOBAL_KEY.set_shared_secret(TEST_PUB_KEY)?;
    GLOBAL_KEY.set_sn("offline-sn");

    let req = ApiBackendRequest::new(serde_json::json!({"uid":"u1"}))?;
    assert_eq!(req.sn, "offline-sn");
    assert!(!req.sign.is_empty());
    assert!(!req.body.key.is_empty());
    assert!(!req.body.data.is_empty());
    Ok(())
}
