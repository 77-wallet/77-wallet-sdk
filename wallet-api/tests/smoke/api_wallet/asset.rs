use anyhow::Result;
use std::collections::HashMap;
use wallet_api::{
    request::coin::AddCoinReq, response_vo::standard_wallet::chain::ChainList,
    testkit::env::get_manager,
};

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and asset sync service"]
async fn sync_api_assets_by_wallet_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let wallet_address = "0x7def9E4B7eF0D88bC77fc7C704E32AFdf505FF5D".to_string();
    let res = wallet_manager.sync_api_assets_by_wallet(wallet_address, None, vec![]).await;

    tracing::info!("sync_api_assets_by_wallet result: {res:?}");
    assert!(res.is_ok(), "sync_api_assets_by_wallet failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local asset data, and fixed account id"]
async fn get_api_assets_list_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let address = "0x234bb8664b5a38573Be7116C10c41cd5c7CbcCD9";
    let account_id = Some(1);

    let _ = wallet_manager.set_currency("USD").await;
    let res = wallet_manager.get_api_assets_list_(address, account_id, None).await?;

    tracing::info!("api assets list: {}", wallet_utils::serde_func::serde_to_string(&res)?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local asset data, and fixed account id"]
async fn get_account_chain_assets_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let address = "0x234bb8664b5a38573Be7116C10c41cd5c7CbcCD9";
    let account_id = Some(1);

    let _ = wallet_manager.set_currency("USD").await;
    let res = wallet_manager.get_api_assets_list(address, account_id, None, None, true).await?;

    tracing::info!("account chain assets: {}", wallet_utils::serde_func::serde_to_string(&res)?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local asset data, and fixed account id"]
async fn get_api_account_assets_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let address = "0x01a68baa7523f16D64AD63d8a82A40e838170b5b";
    let account_id = 1;

    let _ = wallet_manager.set_currency("USD").await;
    let res = wallet_manager.get_api_account_assets(account_id, address, None).await?;

    tracing::info!("api account assets: {}", wallet_utils::serde_func::serde_to_string(&res)?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local SOL asset data, and fixed account id"]
async fn get_api_asset_detail_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let address = "0x0016299F654BF3FaAcCb02E2B4dbbB971a597304";
    let account_id = Some(1);
    let chain_code = "sol";

    let _ = wallet_manager.set_currency("USD").await;
    let res = wallet_manager.get_api_assets(address, account_id, chain_code, None).await?;

    tracing::info!("api asset detail: {}", wallet_utils::serde_func::serde_to_string(&res)?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, Solana RPC, and fixed SOL address"]
async fn api_chain_balance_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let address = "5UnnGocxjSmy4pwAa4GEQ34yZL7zvfce7A4Q8JFKUqd".to_string();
    let chain_code = "sol".to_string();
    let token_address = "".to_string();

    let _ = wallet_manager.set_currency("USD").await;
    let res = wallet_manager.api_chain_balance(address, chain_code, token_address).await?;

    tracing::info!("api chain balance: {}", wallet_utils::serde_func::serde_to_string(&res)?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and fixed TRON token"]
async fn api_add_assets_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let address = "0x01a68baa7523f16D64AD63d8a82A40e838170b5b";
    let chain_code = "tron".to_string();
    let token_address = "TLa2f6VPqDgRE67v1736s7bJ8Ray5wYjU7".to_string();
    let account_id = 1;

    let _ = wallet_manager.set_currency("USD").await;
    let req = AddCoinReq {
        wallet_address: address.to_string(),
        account_id,
        chain_list: ChainList(HashMap::from([(chain_code, token_address)])),
    };
    let res = wallet_manager.api_add_assets(req).await?;

    tracing::info!("api add assets result: {}", wallet_utils::serde_func::serde_to_string(&res)?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and fixed wallet address"]
async fn get_api_wallet_assets_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let address = "0x5489c657Be2504D657f1F56AB04abfE3C77ceC34";

    let _ = wallet_manager.set_currency("USD").await;
    let res = wallet_manager.get_api_wallet_assets(Some(address), None, None).await?;

    tracing::info!("api wallet assets: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and fixed wallet address"]
async fn get_api_wallet_assets_v3_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let address = "0x1b6c7a238E27590a06bD6f200DA4a8d1b5899d4C";

    let _ = wallet_manager.set_currency("USD").await;
    let res = wallet_manager.get_api_wallet_assets_v3(address).await?;

    tracing::info!("api wallet assets v3: {res:?}");
    Ok(())
}
