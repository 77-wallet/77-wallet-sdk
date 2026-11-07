use anyhow::Result;
use wallet_api::testkit::env::get_manager;
use wallet_transport_backend::request::api_wallet::strategy::{ChainConfig, IndexAndAddress};
use wallet_types::chain::chain::ChainCode;

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local test data, and fixed strategy uid"]
async fn update_collect_strategy_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let uid = "823fc91ad98c164d372de036c2e5eec22f47530e7c4ab1c893f653f59260b61f";
    let threshold = 1;
    let chain_config = vec![ChainConfig {
        chain_code: ChainCode::Tron.to_string(),
        chain_address_type: None,
        normal_address: IndexAndAddress {
            index: Some(0),
            address: "TW6h166qfNfibxgovAnVyDDMNV1BFXp5A5".to_string(),
        },
        risk_address: IndexAndAddress {
            index: Some(1),
            address: "THLja2cJJxjbn4cUZZq6BRX8QHK1sxFbT4".to_string(),
        },
    }];

    let res = wallet_manager.update_collect_strategy(uid, threshold, chain_config).await;

    tracing::info!("update_collect_strategy result: {res:?}");
    assert!(res.is_ok(), "update_collect_strategy failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local test data, and fixed strategy uid"]
async fn update_existing_collect_strategy_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let uid = "ef98e62f7057e2c6cee9314ee017875b283dccaaeeeabc9370f8afa7a3a5e186";
    let strategy = wallet_manager.get_collect_strategy(uid).await?;
    let threshold = 1;

    let res = wallet_manager.update_collect_strategy(uid, threshold, strategy.chain_configs).await;

    tracing::info!("update_existing_collect_strategy result: {res:?}");
    assert!(res.is_ok(), "update_existing_collect_strategy failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local test data, and fixed strategy uid"]
async fn get_collect_strategy_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let uid = "ef98e62f7057e2c6cee9314ee017875b283dccaaeeeabc9370f8afa7a3a5e186";
    let strategy = wallet_manager.get_collect_strategy(uid).await?;

    tracing::info!("collect strategy: {}", serde_json::to_string(&strategy).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local test data, and fixed strategy uid"]
async fn update_withdrawal_strategy_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let uid = "e813253c11240023729a033feaa4b271b5e9a2a7e03df0464438e1b3b1bf2fb2";
    let threshold = 1;
    let chain_config = vec![ChainConfig {
        chain_code: ChainCode::Tron.to_string(),
        chain_address_type: Some("TRON".to_string()),
        normal_address: IndexAndAddress {
            index: Some(0),
            address: "TLXdEp1kaVx4ePKpZmXqaU8hBnxsvYUoxf".to_string(),
        },
        risk_address: IndexAndAddress {
            index: Some(0),
            address: "TLXdEp1kaVx4ePKpZmXqaU8hBnxsvYUoxf".to_string(),
        },
    }];

    let res = wallet_manager.update_withdrawal_strategy(uid, threshold, chain_config).await;

    tracing::info!("update_withdrawal_strategy result: {res:?}");
    assert!(res.is_ok(), "update_withdrawal_strategy failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local test data, and fixed strategy uid"]
async fn get_withdrawal_strategy_live_smoke() -> Result<()> {
    wallet_utils::log::init_log_with_level(tracing::Level::INFO);
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let uid = "5bdb1b748bb617d6683f57565b1493cfa5f9e45f3086daf265ca2e0cd325c15e";
    let strategy = wallet_manager.get_withdrawal_strategy(uid).await?;

    tracing::info!("withdrawal strategy: {}", serde_json::to_string(&strategy).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local test data, and fixed strategy uid"]
async fn update_existing_withdrawal_strategy_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let uid = "5bdb1b748bb617d6683f57565b1493cfa5f9e45f3086daf265ca2e0cd325c15e";
    let strategy = wallet_manager.get_withdrawal_strategy(uid).await?;
    let threshold = 5;

    let res =
        wallet_manager.update_withdrawal_strategy(uid, threshold, strategy.chain_configs).await;

    tracing::info!("update_existing_withdrawal_strategy result: {res:?}");
    assert!(res.is_ok(), "update_existing_withdrawal_strategy failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend and API-wallet strategy config data"]
async fn query_api_wallet_configs_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let configs = wallet_manager.query_api_wallet_configs().await?;

    tracing::info!("api wallet configs: {}", serde_json::to_string(&configs).unwrap());
    Ok(())
}
