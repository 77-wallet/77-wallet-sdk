#![cfg(feature = "online-tests")]

use std::{collections::HashMap, sync::Once};
use wallet_ecdh::GLOBAL_KEY;
use wallet_transport_backend::api::BackendApi;

mod api_wallet;
mod wallet;

static INIT_LOG: Once = Once::new();

#[derive(serde::Deserialize)]
struct OnlineTestConfig {
    base_url: String,
    client_id: String,
    aw_sec_id: String,
    aes_key: String,
    aes_iv: String,
}

fn load_online_test_config() -> Result<OnlineTestConfig, wallet_transport_backend::Error> {
    if let Ok(base_url) = std::env::var("WALLET_BACKEND_TEST_BASE_URL") {
        let client_id = std::env::var("WALLET_BACKEND_TEST_CLIENT_ID").map_err(|_| {
            wallet_transport_backend::Error::Backend(Some(
                "missing WALLET_BACKEND_TEST_CLIENT_ID".to_string(),
            ))
        })?;
        let aw_sec_id = std::env::var("WALLET_BACKEND_TEST_AW_SEC_ID").map_err(|_| {
            wallet_transport_backend::Error::Backend(Some(
                "missing WALLET_BACKEND_TEST_AW_SEC_ID".to_string(),
            ))
        })?;
        let aes_key = std::env::var("WALLET_BACKEND_TEST_AES_KEY").map_err(|_| {
            wallet_transport_backend::Error::Backend(Some(
                "missing WALLET_BACKEND_TEST_AES_KEY".to_string(),
            ))
        })?;
        let aes_iv = std::env::var("WALLET_BACKEND_TEST_AES_IV").map_err(|_| {
            wallet_transport_backend::Error::Backend(Some(
                "missing WALLET_BACKEND_TEST_AES_IV".to_string(),
            ))
        })?;
        return Ok(OnlineTestConfig { base_url, client_id, aw_sec_id, aes_key, aes_iv });
    }

    let cfg_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("backend_test_config.toml");
    let content = std::fs::read_to_string(&cfg_path).map_err(|e| {
        wallet_transport_backend::Error::Backend(Some(format!(
            "missing online test config at {}: {e}",
            cfg_path.display()
        )))
    })?;
    toml::from_str(&content).map_err(|e| {
        wallet_transport_backend::Error::Backend(Some(format!(
            "invalid online test config at {}: {e}",
            cfg_path.display()
        )))
    })
}

pub fn init(sn: &str) -> Result<BackendApi, wallet_transport_backend::Error> {
    GLOBAL_KEY.set_sn(sn);
    INIT_LOG.call_once(wallet_utils::init_test_log);

    let config = load_online_test_config()?;
    let mut headers_opt = HashMap::new();
    headers_opt.insert("clientId".to_string(), config.client_id);
    headers_opt.insert("AW-SEC-ID".to_string(), config.aw_sec_id);

    let cryptor = wallet_utils::cbc::AesCbcCryptor::new(&config.aes_key, &config.aes_iv);
    BackendApi::new(Some(config.base_url), Some(headers_opt), cryptor)
}
