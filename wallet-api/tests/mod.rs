// Nested transfer/observer futures need deeper Send trait resolution.
#![recursion_limit = "256"]
#![cfg(feature = "integration-tests")]
use std::{path::PathBuf, sync::Once};
use tokio_stream::StreamExt;
use wallet_api::{dirs::Dirs, manager::WalletManager, messaging::notify::FrontendNotifyEvent};
use wallet_utils::init_test_log;

mod harness;
mod integration;
mod smoke;

static TEST_LOG_INIT: Once = Once::new();

pub async fn get_manager() -> WalletManager {
    TEST_LOG_INIT.call_once(|| {
        let _ = std::panic::catch_unwind(init_test_log);
    });
    unsafe {
        std::env::set_var("WALLET_TRANSPORT_NO_PROXY", "1");
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| std::env::current_dir().unwrap().to_string_lossy().into_owned());
    let path = PathBuf::from(manifest_dir).join("test_data").to_string_lossy().to_string();

    // let sender = Some();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<FrontendNotifyEvent>();
    let mut rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);

    tokio::spawn(async move {
        while let Some(_data) = rx.next().await {
            tracing::info!("data: {_data:?}");
        }
    });
    let dirs = Dirs::new(&path).unwrap();

    let config_text = wallet_api::testkit::env::get_config().unwrap_or_else(|_| {
        r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
aggregate_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
"#
        .to_string()
    });
    let config = wallet_api::config::Config::new(&config_text).unwrap();
    let manager =
        WalletManager::new("guangxiang", "ANDROID", Some(tx.clone()), config, dirs).await.unwrap();
    manager.set_frontend_notify_sender(tx).await.unwrap();

    manager
}
