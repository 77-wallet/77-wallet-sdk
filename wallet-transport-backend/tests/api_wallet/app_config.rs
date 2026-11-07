use std::collections::HashMap;

use serde_json::Value;
use wallet_ecdh::GLOBAL_KEY;
use wallet_transport_backend::{
    api::BackendApi,
    request::api_wallet::{swap::ApiInitSwapReq, wallet::AppConfigReq},
};

// Configuration may contain credentials unrelated to warning thresholds. Print
// field structure and numeric/boolean values, not arbitrary string contents.
fn diagnostic_shape(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let name = key.to_ascii_lowercase();
                    let sensitive = ["secret", "password", "private", "credential", "sign", "key"]
                        .iter()
                        .any(|part| name.contains(part));
                    (
                        key.clone(),
                        if sensitive {
                            Value::String("<redacted>".into())
                        } else if matches!(
                            key.as_str(),
                            "chainCode" | "contractAddress" | "tokenCode" | "amount"
                        ) {
                            value.clone()
                        } else {
                            diagnostic_shape(value)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(diagnostic_shape).collect()),
        Value::String(_) => Value::String("<string>".into()),
        other => other.clone(),
    }
}

/// Explicit read-only test-backend probe, not part of the offline suite.
#[tokio::test]
#[serial_test::serial]
#[ignore = "requires test-backend access and WALLET_TEST_APP_ID / WALLET_TEST_ORG_ID"]
async fn probe_app_config_response_contract() -> Result<(), Box<dyn std::error::Error>> {
    let app_id = std::env::var("WALLET_TEST_APP_ID")?;
    let org_id = std::env::var("WALLET_TEST_ORG_ID")?;
    assert!(!app_id.trim().is_empty() && !org_id.trim().is_empty());
    let config = crate::load_online_test_config()?;
    let url = url::Url::parse(&config.base_url)?;
    // An explicit probe must not silently target production via local config.
    assert_eq!(url.host_str(), Some("test-api.awsservice.xyz"));
    let headers = HashMap::from([
        ("clientId".to_string(), config.client_id),
        ("AW-SEC-ID".to_string(), config.aw_sec_id),
    ]);
    let cryptor = wallet_utils::cbc::AesCbcCryptor::new(&config.aes_key, &config.aes_iv);
    // Do not use the generic online initializer: it enables verbose crypto logs.
    let backend = BackendApi::new(Some(config.base_url), Some(headers), cryptor)?;
    let sn = std::env::var("WALLET_TEST_SN").unwrap_or_else(|_| "666".into());
    GLOBAL_KEY.set_sn(&sn);
    let handshake = backend
        .init_swap(&ApiInitSwapReq { sn, client_pub_key: GLOBAL_KEY.secret_pub_key() })
        .await?;
    let data = handshake.data.ok_or("test-backend handshake returned no data")?;
    GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    let response = backend.query_app_config(&AppConfigReq { app_id, org_id }).await?;
    let response = serde_json::to_value(response)?;
    println!(
        "appConfig response structure (string values redacted):\n{}",
        serde_json::to_string_pretty(&diagnostic_shape(&response))?
    );
    assert!(response.is_object() || response.is_array(), "expected configuration object or list");
    Ok(())
}
