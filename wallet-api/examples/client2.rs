#![allow(dead_code)]
#![allow(unused)]

use tokio_stream::StreamExt as _;
#[cfg(any(feature = "integration-tests", test))]
use wallet_api::testkit::env::TestParams;
#[cfg(any(feature = "integration-tests", test))]
use wallet_api::{
    dirs::Dirs, messaging::notify::FrontendNotifyEvent, testkit::env::get_manager, xlog,
};

#[cfg(not(any(feature = "integration-tests", test)))]
use wallet_api::{dirs::Dirs, messaging::notify::FrontendNotifyEvent, xlog};

#[cfg(not(any(feature = "integration-tests", test)))]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::warn!("client2 example requires feature `integration-tests`");
    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<FrontendNotifyEvent>();
    let _ = _tx;
    let _ = _rx;
    Ok(())
}

// create wallet
#[cfg(any(feature = "integration-tests", test))]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    wallet_utils::init_test_log();
    let (wallet_manager, test_params): (wallet_api::manager::WalletManager, TestParams) =
        get_manager().await.unwrap();
    let _c = wallet_manager.set_invite_code(None).await;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<FrontendNotifyEvent>();
    let mut rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);

    wallet_manager.set_frontend_notify_sender(tx).await?;
    wallet_manager.init(test_params.device_req).await?;

    // 创建钱包
    if true {
        let _wallet = wallet_manager.create_wallet(test_params.create_wallet_req).await.unwrap();
    }

    while let Some(_data) = rx.next().await {
        tracing::info!("data: {_data:?}");
    }
    Ok(())
}

async fn _log_report() {
    let client_id = "test_data";
    // 获取项目根目录
    let storage_dir =
        std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join(client_id);

    let dirs = Dirs::new(&storage_dir.to_string_lossy()).unwrap();
    xlog::init_log(None, "66a7577a2b2f3b0130375e6f", &dirs, "9528").await.unwrap();
}
