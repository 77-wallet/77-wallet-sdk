use anyhow::Result;
use wallet_api::{
    batch_transfer::{BatchTransferConfig, collect_target_addresses, run_batch_transfer},
    request::api_wallet::{trans::ApiBaseTransferReq, transfer::ApiTransferExReq},
    testkit::env::get_manager,
};

const WALLET_PASSWORD: &str = "q1111111";

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and TRON chain state"]
async fn api_transfer_tron_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;
    let _ = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    let from = "TW6h166qfNfibxgovAnVyDDMNV1BFXp5A5";
    let to = "TUDrRQ6zvwXhW3ScTxwGv8nwicLShVVWoF";
    let value = "1";
    let chain_code = "tron";

    let mut base = ApiBaseTransferReq::new(from, to, value, chain_code);
    base.with_token(None, 6, "TRX");
    let req = ApiTransferExReq {
        base,
        password: WALLET_PASSWORD.to_string(),
        fee_setting: "".to_string(),
        signer: None,
    };

    let res = wallet_manager.api_transfer(req).await;

    tracing::info!("api_transfer_tron result: {res:?}");
    assert!(res.is_ok(), "api_transfer_tron failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local test data, and fixed TRON address"]
async fn api_recent_bill_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let token = "";
    let addr = "TQJgSU6DvFvpMC1ExSJ1UVsznPqcH5v8G4";
    let chain_code = "tron";
    let page = 0;
    let page_size = 10;

    let res = wallet_manager.api_recent_bill(token, addr, chain_code, page, page_size).await;

    tracing::info!("api_recent_bill result: {res:?}");
    assert!(res.is_ok(), "api_recent_bill failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local bill data, and fixed wallet address"]
async fn api_bill_lists_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let page = 0;
    let page_size = 10;
    let bills = wallet_manager
        .api_bill_lists(
            Some("0x7Ee2D3e497910faE4b8223Df2575C874CE8f3026".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            None,
            page,
            page_size,
        )
        .await?;

    tracing::info!("api bill list: {}", serde_json::to_string(&bills).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and TRON chain state"]
async fn collect_to_api_subaccounts_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let from = "TEMz9b6wMzJAc56JQJseWBKYqoMjYxXx91";
    let list = wallet_manager
        .list_api_wallet_account(
            "0x7F90ff4374cDFEF97c7Fd546B5E038E06a528166",
            None,
            Some("tron".to_string()),
            0,
            50,
        )
        .await?;
    let chain_code = "tron";
    let value = "3.7";
    let symbol = "TRX";

    for account in list.data {
        if let Some(chain) = account.chain.iter().find(|chain| chain.chain_code == chain_code) {
            let mut base = ApiBaseTransferReq::new(from, &chain.address, value, chain_code);
            base.with_token(None, 6, symbol);
            let req = ApiTransferExReq {
                base,
                password: WALLET_PASSWORD.to_string(),
                fee_setting: "".to_string(),
                signer: None,
            };
            let res = wallet_manager.api_transfer(req).await;
            tracing::info!("collect_to_api_subaccounts result: {res:?}");
        }
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and TRON chain state"]
async fn api_transfer_to_subaccounts_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    const MAX_IN_FLIGHT: usize = 3;
    const REQUEST_START_INTERVAL_MS: u64 = 300;

    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;
    let _ = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    let chain_code = "tron";
    let value = "5";
    let symbol = "TRX";
    let from_address = "TW6h166qfNfibxgovAnVyDDMNV1BFXp5A5";
    let sub_wallet_addr = "0x5489c657Be2504D657f1F56AB04abfE3C77ceC34";
    let subaccounts = wallet_manager
        .list_api_wallet_account(sub_wallet_addr, None, Some(chain_code.to_string()), 0, 500)
        .await?;

    let transfer_targets = collect_target_addresses(subaccounts.data, chain_code)
        .into_iter()
        .filter(|addr| addr != from_address)
        .collect::<Vec<_>>();

    tracing::info!(
        "Found {} subaccounts, start controlled concurrent transfer (max_in_flight={}, start_interval_ms={})",
        transfer_targets.len(),
        MAX_IN_FLIGHT,
        REQUEST_START_INTERVAL_MS
    );

    let config = BatchTransferConfig {
        chain_code: chain_code.to_string(),
        from_address: from_address.to_string(),
        to_addresses: transfer_targets,
        value: value.to_string(),
        token_symbol: symbol.to_string(),
        token_decimals: 6,
        max_in_flight: MAX_IN_FLIGHT,
        start_interval_ms: REQUEST_START_INTERVAL_MS,
        password: WALLET_PASSWORD.to_string(),
        fee_setting: "".to_string(),
    };
    let summary = run_batch_transfer(wallet_manager, &config).await?;
    tracing::info!(
        "Transfer summary: total={}, success={}, failed={}",
        summary.total,
        summary.success,
        summary.failed
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and BNB chain state"]
async fn api_transfer_bnb_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;
    let _ = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    let chain_code = "bnb";
    let value = "0.0009";
    let symbol = "BNB";
    let from_address = "0x5A99406CE8D9F8B3527a38408582872144C8b890";
    let to_address = "0x37D9A67696956F67F1Bdd302A79460c1266b8F1F";

    tracing::info!("Transferring {} {} from {} to {}", value, symbol, from_address, to_address);

    let mut base = ApiBaseTransferReq::new(&from_address, &to_address, value, chain_code);
    base.with_token(None, 18, symbol);
    let req = ApiTransferExReq {
        base,
        password: WALLET_PASSWORD.to_string(),
        fee_setting: "".to_string(),
        signer: None,
    };

    let res = wallet_manager.api_transfer(req).await;

    tracing::info!("api_transfer_bnb result: {res:?}");
    assert!(res.is_ok(), "api_transfer_bnb failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local wallet data, and SOL chain state"]
async fn api_transfer_sol_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;
    let _ = wallet_manager.set_passwd_cache(WALLET_PASSWORD).await;

    let chain_code = "sol";
    let value = "0.000015000";
    let symbol = "SOL";
    let from_address = "7N42qhW6tDQwMzRnSpSRC6Ew9jV3XTQC8SR29ZseTaJU";
    let to_address = "72vgdLcQgdudUiGXudHNPhgCPNPCdxj2ijAGuXTQ5ppB";

    tracing::info!("Transferring {} {} from {} to {}", value, symbol, from_address, to_address);

    let mut base = ApiBaseTransferReq::new(&from_address, &to_address, value, chain_code);
    base.with_token(None, 9, symbol);
    let req = ApiTransferExReq {
        base,
        password: WALLET_PASSWORD.to_string(),
        fee_setting: "".to_string(),
        signer: None,
    };

    let res = wallet_manager.api_transfer(req).await;

    tracing::info!("api_transfer_sol result: {res:?}");
    assert!(res.is_ok(), "api_transfer_sol failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires local API collect order database state"]
async fn api_collect_order_stats_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let stats = wallet_manager.api_collect_order_stats().await?;

    tracing::info!(
        "Collect order stats: {}",
        wallet_utils::serde_func::serde_to_string(&stats).unwrap()
    );
    Ok(())
}
