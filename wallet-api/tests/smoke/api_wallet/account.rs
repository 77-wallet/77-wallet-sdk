use anyhow::Result;
use wallet_api::{
    request::api_wallet::account::{CreateApiAccountReq, CreateWithdrawalAccountReq},
    testkit::env::{get_manager, get_manager_with_config},
};
use wallet_database::entities::api_wallet::ApiWalletType;

const WALLET_PASSWORD: &str = "q1111111";
const REDACTED_PASSWORD: &str = "[REDACTED:password]";

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and fixed account indices"]
async fn create_api_account_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let wallet_address = "0x17f6a199862FD0ffb2d5C79f3DBBE37597162A24";
    let indices = vec![5, 6, 7, 8, 9];
    let name = "666";
    let is_default_name = true;
    let api_wallet_type = ApiWalletType::SubAccount;

    let req = CreateApiAccountReq::new(
        wallet_address,
        WALLET_PASSWORD,
        indices,
        name,
        is_default_name,
        api_wallet_type,
    );
    let res = wallet_manager.create_api_account(req).await;

    tracing::info!("create_api_account result: {res:?}");
    assert!(res.is_ok(), "create_api_account failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and withdrawal-account password"]
async fn create_withdrawal_account_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let wallet_address = "0x0d8B30ED6837b2EF0465Be9EE840700A589eaDB6";
    let index = Some(5);
    let name = "666";
    let is_default_name = true;

    let req = CreateWithdrawalAccountReq::new(
        wallet_address,
        REDACTED_PASSWORD,
        None,
        index,
        name,
        is_default_name,
    );
    let res = wallet_manager.create_withdrawal_account(req).await;

    tracing::info!("create_withdrawal_account result: {res:?}");
    assert!(res.is_ok(), "create_withdrawal_account failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and private-key password"]
async fn get_api_account_private_key_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let address = "1BUttKYoVhXZbAogpHmm2Mm7X8Xtrjn6XH";
    let chain_code = "btc";

    let res =
        wallet_manager.get_api_account_private_key(address, chain_code, REDACTED_PASSWORD).await;

    tracing::info!("get_api_account_private_key result: {res:?}");
    assert!(res.is_ok(), "get_api_account_private_key failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and fixed uid/index"]
async fn address_used_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let res = wallet_manager
        .address_used("tron", 1, "eb7a5f6ce1234b0d9de0d63750d6aa2c1661e89a3cc9c1beb23aad3bd324071c")
        .await;

    tracing::info!("address_used result: {res:?}");
    assert!(res.is_ok(), "address_used failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local account data, and fixed wallet address"]
async fn list_api_wallet_account_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let res = wallet_manager
        .list_api_wallet_account("0x5489c657Be2504D657f1F56AB04abfE3C77ceC34", None, None, 1, 10)
        .await?;

    tracing::info!("api account list: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and fixed account id"]
async fn physical_delete_api_account_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let res = wallet_manager
        .physical_delete_api_account(
            "0x0016299F654BF3FaAcCb02E2B4dbbB971a597304",
            1,
            WALLET_PASSWORD,
        )
        .await;

    tracing::info!("physical_delete_api_account result: {res:?}");
    assert!(res.is_ok(), "physical_delete_api_account failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and fixed derivation path"]
async fn list_api_wallet_derived_addresses_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let res = wallet_manager
        .list_api_wallet_derived_addresses(
            "0x17f6a199862FD0ffb2d5C79f3DBBE37597162A24",
            1,
            WALLET_PASSWORD,
            true,
        )
        .await?;

    tracing::info!("derived addresses: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured client4.toml, API-wallet backend, and fixed uid/address"]
async fn search_api_wallet_address_by_uid_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager_with_config("client4.toml").await?;

    let res = wallet_manager
        .search_api_wallet_address(
            "5bdb1b748bb617d6683f57565b1493cfa5f9e45f3086daf265ca2e0cd325c15e",
            "TW6h166qfNfibxgovAnVyDDMNV1BFXp5A5",
        )
        .await?;

    tracing::info!("address search result: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}
