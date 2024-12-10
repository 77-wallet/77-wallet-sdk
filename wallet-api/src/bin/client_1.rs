use tokio_stream::StreamExt as _;
use wallet_api::{
    notify::FrontendNotifyEvent,
    test::env::{setup_test_environment, TestData, TestWalletEnv},
    InitDeviceReq,
};

// TFzMRRzQFhY9XFS37veoswLRuWLNtbyhiB

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // std::env::set_var("RUST_BACKTRACE", "1");
    wallet_utils::init_test_log();

    let phrase = Some("".to_string());

    let TestData {
        wallet_manager,
        wallet_env: env,
        ..
    } = setup_test_environment(None, None, false, phrase)
        .await
        .unwrap();
    // wallet_api::WalletManager::init_log(Some("warn"))
    //     .await
    //     .unwrap();
    // Self::init_log(Some("error")).await?;

    let TestWalletEnv {
        language_code,
        phrase,
        salt,
        wallet_name,
        password,
    } = env;

    let device_type = "ANDROID";

    let sn = "bdb6412a9cb4b12c48ebe1ef4e9f052b07af519b7485cd38a95f38d89df97cb8";

    // let client_id = "wenjing";

    // let mqtt_url = "ws://100.106.144.126:8083/mqtt";
    let req = InitDeviceReq {
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
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<FrontendNotifyEvent>();
    let mut rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);

    wallet_manager.set_frontend_notify_sender(tx).await?;

    let _res = wallet_manager.init_device(req).await;
    let init_res = wallet_manager.init_data().await;
    tracing::info!("init_data res: {init_res:?}");

    tracing::info!("init_device res: {_res:?}");

    tracing::info!("start create wallet");

    let account_name = "账户";
    // let start_time = std::time::Instant::now();

    // let _res = wallet_manager
    //     .create_wallet(
    //         language_code,
    //         &phrase,
    //         &salt,
    //         &wallet_name,
    //         account_name,
    //         true,
    //         &password,
    //         None,
    //     )
    //     .await
    //     .result
    //     .unwrap();
    // tracing::info!("create_wallet res: {_res:?}");
    // let elapsed_time = start_time.elapsed();
    // tracing::info!("create_wallet elapsed time: {:?}", elapsed_time);
    // wallet_manager
    //     .create_account(
    //         &_res.address,
    //         &password,
    //         None,
    //         None,
    //         None,
    //         account_name,
    //         true,
    //     )
    //     .await
    //     .result
    //     .unwrap();
    // tracing::info!("create_wallet res: {_res:?}");
    // let _c = wallet_manager.sync_assets(vec![], None, vec![]).await;

    wallet_manager
        .mqtt_subscribe(vec!["wallet/token/btc/btc".to_string()], None)
        .await;

    // tokio::spawn(async move {
    //     tokio::time::sleep(std::time::Duration::from_secs(60)).await;

    //     let unsubscribe = wallet_api::service::wallet::WalletService::new(repo)
    //         .reset()
    //         .await
    //         .unwrap();
    //     let unsubscribe = wallet_manager
    //         .upload_log_file()
    //         .await;
    //     let unsubscribe = wallet_manager
    //         .mqtt_unsubscribe(vec!["wallet/token/eth/eth".to_string()])
    //         .await;
    //     tracing::info!("unsubscribe: {unsubscribe:?}");
    //     let unsubscribe = wallet_manager
    //         .mqtt_unsubscribe(vec!["wallet/token/eth/eth".to_string()])
    //         .await;
    //     tracing::info!("unsubscribe: {unsubscribe:?}");
    // });

    while let Some(data) = rx.next().await {
        tracing::info!("data: {data:?}");
    }
    // loop {
    //     println!("sleep ");
    //     tokio::time::sleep(std::time::Duration::from_secs(10)).await
    // }
    Ok(())
}
