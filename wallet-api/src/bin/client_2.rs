use std::{env, path::PathBuf};
use tokio_stream::StreamExt as _;
use wallet_api::{notify::FrontendNotifyEvent, InitDeviceReq, WalletManager};
use wallet_utils::init_test_log;

// create wallet
#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = get_manager().await;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<FrontendNotifyEvent>();
    let mut rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);

    // 启动检查任务
    let _ = manager.init_data().await;

    // 设置通知前端的流
    manager.set_frontend_notify_sender(tx).await?;

    // 初始化设备并启动mqtt
    manager.init_device(device_req()).await;

    create_wallet(&manager, false).await;

    while let Some(_data) = rx.next().await {
        tracing::info!("data: {_data:?}");
    }
    Ok(())
}

async fn get_manager() -> WalletManager {
    init_test_log();
    let path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("test_data")
        .to_string_lossy()
        .to_string();
    WalletManager::new(
        "ccc8a1e6f53affba2d13880049a1f2bd8dc83fdfe4159a7acac7e85fc9974432",
        "IOS",
        &path,
        None,
    )
    .await
    .unwrap()
}

async fn create_wallet(manager: &WalletManager, create: bool) {
    if create {
        let phrase = "";
        let salt = "";
        manager
            .create_wallet(1, phrase, salt, "test", "账户", true, "123456", None)
            .await;
    }
}

fn device_req() -> InitDeviceReq {
    let device_type = "IOS";
    let sn = "ccc8a1e6f53affba2d13880049a1f2bd8dc83fdfe4159a7acac7e85fc9974432";

    InitDeviceReq {
        device_type: device_type.to_string(),
        sn: sn.to_string(),
        code: device_type.to_string(),
        system_ver: device_type.to_string(),
        iemi: Some(device_type.to_string()),
        meid: Some(device_type.to_string()),
        iccid: Some(device_type.to_string()),
        mem: Some(device_type.to_string()),
        app_id: None,
        package_id: None,
    }
}
