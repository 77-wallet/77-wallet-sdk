use anyhow::Result;
use wallet_api::testkit::env::get_manager;

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local test data, and fixed wallet address"]
async fn hot_coin_list_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;

    let wallet_address = "0x7F90ff4374cDFEF97c7Fd546B5E038E06a528166";
    let account_id = 1;
    let chain_code = None;
    let keyword = None;
    let page = 1;
    let page_size = 10;

    let res = wallet_manager
        .api_hot_coin_list(wallet_address, account_id, chain_code, keyword, page, page_size)
        .await;

    tracing::info!("api_hot_coin_list result: {res:?}");
    assert!(res.is_ok(), "api_hot_coin_list failed: {res:?}");
    Ok(())
}
