use anyhow::Result;
use std::collections::HashMap;
use wallet_api::testkit::env::get_manager;

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local test data, and fixed wallet address"]
async fn get_api_chain_list_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let wallet_address = "0x5489c657Be2504D657f1F56AB04abfE3C77ceC34";
    let account_id = 6;
    let mut chain_list = HashMap::new();
    chain_list.insert("tron".to_string(), "".to_string());

    let chains = wallet_manager.get_api_chain_list(wallet_address, account_id, chain_list).await?;

    tracing::info!("api chain list: {}", serde_json::to_string(&chains).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend and API-wallet chain config data"]
async fn get_api_hot_chain_list_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let chains = wallet_manager.get_api_hot_chain_list().await?;

    tracing::info!("api hot chain list: {}", serde_json::to_string(&chains).unwrap());
    Ok(())
}
