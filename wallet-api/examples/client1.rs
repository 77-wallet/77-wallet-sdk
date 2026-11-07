#![allow(dead_code)]
#![allow(unused)]

use std::sync::Arc;

use tokio_stream::StreamExt as _;
#[cfg(any(feature = "integration-tests", test))]
use wallet_api::messaging::notify::FrontendNotifyEvent;
#[cfg(any(feature = "integration-tests", test))]
use wallet_api::testkit::env::{get_manager, get_manager_with_config};

#[cfg(not(any(feature = "integration-tests", test)))]
use wallet_api::messaging::notify::FrontendNotifyEvent;

#[cfg(not(any(feature = "integration-tests", test)))]
#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::warn!("client1 example requires feature `integration-tests`");
    let (_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<FrontendNotifyEvent>();
    let _ = _tx;
    let _ = _rx;
    Ok(())
}

// TFzMRRzQFhY9XFS37veoswLRuWLNtbyhiB
// 694a5988a284522b74515e4b	AwmCmdAddrExpand
// {"eventNo":"2003389549658427392","eventType":"3","data":{"type":"CHA_BATCH","chain":"tron","index":null,"uid":"9c9e3364495c32daa7e0b04a8c484ae4c96c2b5521c1c42f30144085bbbf7282","serialNo":"tron_9c9e3364495c32daa7e0b04a8c484ae4c96c2b5521c1c42f30144085bbbf7282","number":"50","batchId":"694a59873034a42d1d0f1c42"},"time":1766480264,"sign":"kh0oLoudImFzM+1n+dZ6ge64qv1qBRMw10qPVbDzi9dCBMh7UsxS3mvKTllLnXsIpzNuOgSvObFR3VMCSV054A==","secret":"/HwKnoG2Q0K5xMjnxf78lZO43ghx/pMmTmIE3xfSeuM="}
//	2	0	0	2025-12-23T08:57:44Z	2025-12-23T09:22:48Z	TransportBackend error: `encryption error: `invalid shared key``

#[cfg(any(feature = "integration-tests", test))]
#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // wallet_utils::log::init_log_with_level(tracing::Level::INFO);
    // wallet_api::WalletManager::init_log(Some("warn"))
    //     .await
    //     .unwrap();
    // Self::init_log(Some("error")).await?;
    let (wallet_manager, _test_params): (
        wallet_api::manager::WalletManager,
        wallet_api::testkit::env::TestParams,
    ) = get_manager_with_config("client1.toml").await?;
    let dirs = wallet_manager.ctx().get_global_dirs();
    let _ = wallet_api::xlog::init_log(Some("info"), &"app_code", &dirs, "sn").await;
    tracing::info!("init_api_swap");
    wallet_manager.init_api_swap().await?;
    let wallet_password = "";

    let _ = wallet_manager.set_passwd_cache(wallet_password).await;
    // wallet_api::WalletManager::init_log(Some("info"), "xxxx").await?;
    tracing::info!("set_frontend_notify_sender");
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<FrontendNotifyEvent>();
    let mut rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
    wallet_manager.set_frontend_notify_sender(tx).await?;
    // wallet_manager.init(test_params.device_req).await?;
    tracing::info!("set_invite_code");
    let res = wallet_manager.set_invite_code(Some("I1912683353004912640".to_string())).await?;
    let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
    tracing::info!("set_invite_code res: {res}");

    // let manager_c = std::sync::Arc::new(wallet_manager.clone());
    // import_withdrawal_api_wallet(
    //     manager_c,
    //     _test_params.create_wallet_req.language_code,
    //     &_test_params.create_wallet_req.phrase,
    //     "w0000002",
    //     &_test_params.create_wallet_req.wallet_name,
    //     wallet_password,
    //     None,
    //     wallet_database::entities::api_wallet::ApiWalletType::Withdrawal,
    //     None,
    // )
    // .await?;

    // // 创建钱包
    // let language_code = 1;
    // let phrase = &test_params.create_wallet_req.phrase;
    // let salt = "";
    // let wallet_name = "api_wallet";
    // let account_name = "ccccc";
    // let is_default_name = true;
    // let invite_code = None;
    // let api_wallet_type = wallet_database::entities::api_wallet::ApiWalletType::SubAccount;
    // let wallet = wallet_manager
    //     .create_api_wallet(
    //         language_code,
    //         phrase,
    //         salt,
    //         wallet_name,
    //         account_name,
    //         is_default_name,
    //         wallet_password,
    //         invite_code,
    //         api_wallet_type,
    //     )
    //     .await
    //     .result;
    // tracing::warn!("wallet ------------------------ 1: {wallet:#?}");

    // let order_list = wallet_manager.get_api_collect_order_list().await.result;
    // tracing::info!("order_list ------------------- 2: {order_list:#?}");

    // let uid = "eb7a5f6ce1234b0d9de0d63750d6aa2c1661e89a3cc9c1beb23aad3bd324071c";

    // let from = "TMao3zPmTqNJWg3ZvQtXQxyW1MuYevTMHt";
    // // let from = "TRLJd4avtuGfW5KZHzigxVxZfVdrwvkoJ5";
    // // let to = "TRLJd4avtuGfW5KZHzigxVxZfVdrwvkoJ5";
    // let to = "TBQSs8KG82iQnLUZj5nygJzSUwwhQJcxHF";
    // // let to = "TMao3zPmTqNJWg3ZvQtXQxyW1MuYevTMHt";
    // let value = "20";
    // let trade_no = "0x000000001";
    // let chain_code = "tron";
    // let symbol = "USDT";
    // let token_address = Some("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string());

    // let res1 = wallet_manager
    //     .api_collect_order(from, to, value, chain_code, token_address, symbol, trade_no, 1, uid)
    //     .await;
    // tracing::info!("api_withdrawal_order ------------------- 4: {res1:#?}");

    // let index = Some(7);
    // let index = None;
    // let address_allock_type = wallet_api::AddressAllockType::ChaBatch;
    // let chain_code = "tron";

    // let res1 = wallet_manager.expand_address(address_allock_type, chain_code, index, uid).await;
    // tracing::info!("expand_address ------------------- 5: {res1:#?}");

    // let wallet = wallet_manager.create_wallet(_test_params.create_wallet_req).await?;
    // tracing::warn!("wallet: {wallet:#?}");

    // subscribe(&wallet_manager).await;

    // let sync_res =
    //     wallet_manager.sync_assets_by_wallet(wallet.address.to_string(), None, vec![]).await;
    // tracing::info!("sync res: {sync_res:#?}");
    // let wallet = wallet.unwrap();
    // test_params.create_account_req.wallet_address = wallet.address.clone();

    // let config = wallet_manager.get_config().await;
    // tracing::info!("config result: {config:#?}");
    // let res = wallet_utils::serde_func::serde_to_string(&config)?;
    // tracing::info!("config result: {res}");
    // subscribe(&wallet_manager).await;

    // let manager_c = std::sync::Arc::new(wallet_manager.clone());
    // test_balance(manager_c).await;

    // if !wallet_manager.sync_api_chains().await?.is_empty() {
    //     wallet_manager.sync_api_wallet_chain_data().await?;
    // }
    // tokio::spawn(async move {
    //     tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    //     let res = manager_c
    //         .physical_delete_api_wallet("0x1b6c7a238E27590a06bD6f200DA4a8d1b5899d4C")
    //         .await;
    //     tracing::info!("physical_delete_api_wallet res: {res:#?}");
    // });
    loop {
        tokio::select! {
            msg = rx.next() => {
                let data = serde_json::to_string(&msg).unwrap();
                tracing::info!("前端收到数据: {data:?}");
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("ctrl_c");
                let _ = wallet_manager.close().await;
                break;
            }
        }
    }
    Ok(())
}

async fn import_withdrawal_api_wallet(
    wallet_manager: Arc<wallet_api::manager::WalletManager>,
    language_code: u8,
    phrase: &str,
    salt: &str,
    wallet_name: &str,
    wallet_password: &str,
    invite_code: Option<String>,
    api_wallet_type: wallet_database::entities::api_wallet::ApiWalletType,
    binding_address: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    wallet_manager
        .import_api_wallet(
            language_code,
            phrase,
            salt,
            wallet_name,
            wallet_password,
            invite_code,
            api_wallet_type,
            binding_address,
        )
        .await?;
    Ok(())
}

#[allow(dead_code)]
async fn test_balance(wallet_manager: Arc<wallet_api::manager::WalletManager>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            // // 测试获取资产列表(app点击账户后调用)
            // test_get_api_assets_list(wallet_manager.clone()).await;

            // // 测试获取资产详情
            // test_get_api_assets(wallet_manager.clone()).await;

            // // 测试获取链列表
            // test_get_api_chain_list(wallet_manager.clone()).await;

            // 测试获取钱包账户列表(app首页账户列表)
            // test_list_api_wallet_account(wallet_manager.clone()).await;

            // // 测试获取钱包账户资产详情
            // test_get_api_account_assets(wallet_manager.clone()).await;

            // 测试获取钱包总资产(app首页资产总值调用)
            test_get_api_wallet_assets(wallet_manager.clone()).await;
        }
    });
}

/// 测试获取钱包列表
#[allow(unused)]
#[cfg(any(feature = "integration-tests", test))]
async fn test_get_api_wallet_list(wallet_manager: Arc<wallet_api::manager::WalletManager>) {
    let res = wallet_manager.get_api_wallet_list().await;
    tracing::info!("get_api_wallet_list: {res:#?}");
}

/// 测试获取资产列表
#[allow(unused)]
async fn test_get_api_assets_list(wallet_manager: Arc<wallet_api::manager::WalletManager>) {
    let res = wallet_manager
        .get_api_assets_list(
            "0x7F90ff4374cDFEF97c7Fd546B5E038E06a528166",
            Some(1),
            None,
            None,
            false,
        )
        .await
        .unwrap();
    let res = serde_json::to_string(&res).unwrap();
    tracing::info!("get_assets_list: {res:#?}");
}

/// 测试获取资产详情
#[allow(unused)]
async fn test_get_api_assets(wallet_manager: Arc<wallet_api::manager::WalletManager>) {
    let res = wallet_manager
        .get_api_assets(
            "TLXdEp1kaVx4ePKpZmXqaU8hBnxsvYUoxf",
            // Some(1),
            None,
            "tron",
            Some("TNDSHKGBmgRx9mDYA9CnxPx55nu672yQw2".to_string()),
        )
        .await;

    tracing::info!("get_api_assets: {res:#?}");
}

/// 测试获取链列表
#[allow(unused)]
async fn test_get_api_chain_list(wallet_manager: Arc<wallet_api::manager::WalletManager>) {
    let mut chain_list = std::collections::HashMap::new();
    chain_list.insert("tron".to_string(), "TNDSHKGBmgRx9mDYA9CnxPx55nu672yQw2".to_string());
    let res = wallet_manager
        .get_api_chain_list(
            "0x0016299F654BF3FaAcCb02E2B4dbbB971a597304",
            // Some(1),
            1,
            chain_list,
        )
        .await;

    tracing::info!("get_api_chain_list: {res:#?}");
}

/// 测试获取钱包账户列表
#[allow(unused)]
async fn test_list_api_wallet_account(wallet_manager: Arc<wallet_api::manager::WalletManager>) {
    let balance_list = wallet_manager
        .list_api_wallet_account(
            "0x7F90ff4374cDFEF97c7Fd546B5E038E06a528166",
            // Some(1),
            None,
            // Some("tron".to_string()),
            Some("tron".to_string()),
            // None,
            0,
            20,
        )
        .await
        .unwrap();
    let balance_list = serde_json::to_string(&balance_list).unwrap();
    tracing::info!("list_api_wallet_account balance_list: {balance_list:#?}");
}

/// 测试获取钱包账户资产详情
#[allow(unused)]
async fn test_get_api_account_assets(wallet_manager: Arc<wallet_api::manager::WalletManager>) {
    let res = wallet_manager
        .get_api_account_assets(1, "0x7F90ff4374cDFEF97c7Fd546B5E038E06a528166", None)
        .await;
    tracing::info!("get_api_account_assets: {res:#?}");
}

/// 测试获取钱包总资产
#[allow(unused)]
async fn test_get_api_wallet_assets(wallet_manager: Arc<wallet_api::manager::WalletManager>) {
    let res = wallet_manager
        .get_api_wallet_assets(Some("0x806b94a00D6a4e415739D54D476832Adf432f229"), None, None)
        .await;

    // let res = wallet_api::domain::api_wallet::assets::ApiAssetsDomain::get_api_wallet_assets(
    //     Some("0x806b94a00D6a4e415739D54D476832Adf432f229"),
    //     None,
    //     None,
    // )
    // .await
    // .unwrap();
    tracing::info!("get_api_wallet_assets: {res:#?}");
}

#[allow(dead_code)]
async fn subscribe(wallet_manager: &wallet_api::manager::WalletManager) {
    let topics = vec![
        "wallet/token/eth/usdc".to_string(),
        "wallet/token/tron/trx".to_string(),
        "wallet/token/doge/doge".to_string(),
        "wallet/token/tron/sun".to_string(),
        "wallet/token/tron/win".to_string(),
        "wallet/token/eth/hkby".to_string(),
        "wallet/token/btc/btc".to_string(),
        "wallet/token/eth/eth".to_string(),
        "wallet/token/bnb/bnb".to_string(),
        "wallet/token/sol/sol".to_string(),
        "wallet/token/ltc/ltc".to_string(),
        "wallet/token/eth/link".to_string(),
        "wallet/token/ton/ton".to_string(),
        "wallet/token/sui/sui".to_string(),
        "wallet/token/eth/cake".to_string(),
        "wallet/token/sol/usdt".to_string(),
    ];
    {
        let _ = wallet_manager.mqtt_subscribe(topics, None).await;
    }
}
