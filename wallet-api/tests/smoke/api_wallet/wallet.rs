use anyhow::{Context, Result};
use wallet_api::{
    manager::WalletManager,
    testkit::env::{ApiWalletImportParams, TestParams, get_manager, get_manager_with_config},
};
use wallet_database::entities::api_wallet::ApiWalletType;

const WALLET_PASSWORD: &str = "q1111111";

#[tokio::test]
#[ignore = "requires configured client4.toml, API-wallet backend, and fresh subaccount salt"]
async fn create_subaccount_wallet_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, test_params) = get_manager_with_config("client4.toml").await?;
    wallet_manager.init_api_swap().await?;

    let language_code = 1;
    let phrase = &test_params.create_wallet_req.phrase;
    let salt = "r0000007";
    let wallet_name = "api_wallet";
    let api_wallet_type = ApiWalletType::SubAccount;
    let binding_address = None;
    let invite_code = None;

    let res = wallet_manager
        .create_api_wallet(
            language_code,
            phrase,
            salt,
            wallet_name,
            WALLET_PASSWORD,
            invite_code,
            api_wallet_type,
            binding_address,
        )
        .await;

    tracing::info!("create subaccount wallet result: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured client4.toml, API-wallet backend, and fresh withdrawal salt"]
async fn create_withdrawal_wallet_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, test_params) = get_manager_with_config("client4.toml").await?;
    wallet_manager.init_api_swap().await?;

    let language_code = 1;
    let phrase = &test_params.create_wallet_req.phrase;
    let wallet_name = "api_wallet";
    let api_wallet_type = ApiWalletType::Withdrawal;
    let invite_code = None;
    let salt = "w0000007";
    let binding_address = Some("0x5489c657Be2504D657f1F56AB04abfE3C77ceC34");

    let res = wallet_manager
        .create_api_wallet(
            language_code,
            phrase,
            salt,
            wallet_name,
            WALLET_PASSWORD,
            invite_code,
            api_wallet_type,
            binding_address,
        )
        .await;

    tracing::info!("create withdrawal wallet result: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured client1.toml and API-wallet backend"]
async fn import_platform_api_wallet_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let config_file = "client1.toml";
    let (wallet_manager, test_params) = get_manager_with_config(config_file).await?;
    wallet_manager.init_api_swap().await?;

    import_configured_api_wallets(&wallet_manager, &test_params, config_file).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured client4.toml and API-wallet backend"]
async fn import_merchant_api_wallet_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let config_file = "client4.toml";
    let (wallet_manager, test_params) = get_manager_with_config(config_file).await?;
    wallet_manager.init_api_swap().await?;

    import_configured_api_wallets(&wallet_manager, &test_params, config_file).await?;
    Ok(())
}

async fn import_configured_api_wallets(
    wallet_manager: &WalletManager,
    test_params: &TestParams,
    config_file: &str,
) -> Result<()> {
    let import_config = test_params
        .api_wallet_import
        .as_ref()
        .with_context(|| format!("missing api_wallet_import in {config_file}"))?;

    let sub_account = import_config
        .sub_account
        .as_ref()
        .with_context(|| format!("missing api_wallet_import.sub_account in {config_file}"))?;
    import_configured_api_wallet(
        wallet_manager,
        sub_account,
        ApiWalletType::SubAccount,
        "sub_account",
    )
    .await?;

    let withdrawal = import_config
        .withdrawal
        .as_ref()
        .with_context(|| format!("missing api_wallet_import.withdrawal in {config_file}"))?;
    import_configured_api_wallet(
        wallet_manager,
        withdrawal,
        ApiWalletType::Withdrawal,
        "withdrawal",
    )
    .await?;

    Ok(())
}

async fn import_configured_api_wallet(
    wallet_manager: &WalletManager,
    params: &ApiWalletImportParams,
    api_wallet_type: ApiWalletType,
    label: &str,
) -> Result<()> {
    wallet_manager
        .import_api_wallet(
            params.language_code,
            &params.phrase,
            &params.salt,
            &params.wallet_name,
            &params.wallet_password,
            params.invite_code.clone(),
            api_wallet_type,
            params.binding_address.as_deref(),
        )
        .await?;

    tracing::info!("import {label} api wallet succeeded");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend and fixed uid"]
async fn query_uid_bind_info_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let uid = "4080938dda41a016b8c153be34b558345259a4b4116d5a88e004507341164b78";
    let res = wallet_manager.query_uid_bind_info(uid).await?;

    tracing::info!("uid bind info: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend and fixed merchant binding data"]
async fn import_bind_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;
    let _ = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    let sn = &test_params.device_req.sn;
    let app_id = "f2a904c3c12e4481bbabb86977c200b3";
    let org_id = "6933cf7a7fec37621a3ffc95";
    let subaccount_uid = "8fa020e0049b10e467fd21ea81b45bf44b88eaec3db8f167173760fc63cf9c90";
    let withdrawal_uid = "f64db1f0796fa815016a067dceb9f912b77ec96ad79dd201534b82e905a1f29a";

    let res = wallet_manager.import_bind(sn, org_id, app_id, subaccount_uid, withdrawal_uid).await;

    tracing::info!("import bind result: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured client4.toml, API-wallet backend, and fixed merchant binding data"]
async fn scan_bind_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager_with_config("client4.toml").await?;
    wallet_manager.init_api_swap().await?;
    let _ = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    let app_id = "8276baee61e14956bf8ad036e4a5efb3";
    let org_id = "6a044edb3f923904b04aaf71";
    let subaccount_uid = "ef98e62f7057e2c6cee9314ee017875b283dccaaeeeabc9370f8afa7a3a5e186";
    let withdrawal_uid = "5bdb1b748bb617d6683f57565b1493cfa5f9e45f3086daf265ca2e0cd325c15e";

    let res = wallet_manager.scan_bind(app_id, org_id, subaccount_uid, withdrawal_uid).await;
    tracing::info!("scan bind raw result: {res:?}");
    let normalized: (i64, String) = match res {
        Ok(_) => (0, "success".to_string()),
        Err(e) => e.into(),
    };

    tracing::info!("scan bind normalized result: {normalized:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend and fixed wallet address"]
async fn query_wallet_activation_info_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;
    let _ = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    let wallet_address = "0x76c17D35200533Aa9cB326a1A07B75aFBc89fB02";
    let res = wallet_manager.query_wallet_activation_info(wallet_address).await?;

    tracing::info!("wallet activation info: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend and local wallet list data"]
async fn get_api_wallet_list_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    let _ = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    let res = wallet_manager.get_api_wallet_list().await?;

    tracing::info!("api wallet list: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend and fixed wallet address"]
async fn physical_delete_api_wallet_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    let _ = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    let res = wallet_manager
        .physical_delete_api_wallet("0x806b94a00D6a4e415739D54D476832Adf432f229")
        .await;

    tracing::info!("physical delete api wallet result: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and wallet password"]
async fn get_api_phrase_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    let _ = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    let res = wallet_manager
        .get_api_phrase("0x5489c657Be2504D657f1F56AB04abfE3C77ceC34", WALLET_PASSWORD)
        .await;

    tracing::info!("get api phrase result: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend and local wallet password"]
async fn set_passwd_cache_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let res = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    tracing::info!("set password cache result: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend and fixed withdrawal binding uids"]
async fn change_withdrawal_wallet_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;
    let _ = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    let recharge_uid = "703dc9ffe712d3ced169cee62c3c9c8118ce822bd00d49650e02df80ba0fcc30";
    let withdrawal_uid = "17931d2265113d34604598200350c0e5eba860af969768c91d5aee7f499c08c1";
    let res = wallet_manager.change_withdrawal_wallet(recharge_uid, withdrawal_uid).await?;

    tracing::info!("change withdrawal wallet result: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and fixed wallet address"]
async fn is_wallet_authorized_on_device_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let res = wallet_manager
        .is_wallet_authorized_on_device("0x7F90ff4374cDFEF97c7Fd546B5E038E06a528166")
        .await;

    tracing::info!("is wallet authorized on device result: {res:?}");
    Ok(())
}
