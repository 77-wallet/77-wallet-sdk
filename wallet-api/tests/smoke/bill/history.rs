use crate::get_manager;
use anyhow::Result;
use wallet_database::CoreDbPool;

#[tokio::test]
#[ignore = "smoke test requires seeded bill data"]
async fn bill_detail() -> Result<()> {
    let wallet_manager = get_manager().await;

    let hash = "22d88a9855cc19d46ff268f5d2906b474ef7eecd2b44728916f2bff66e27c95a";
    let owner = "TYskFdYh9zsx4XcVRtGY6KhdwgwinmEhSZ";
    let detail = wallet_manager.bill_detail(&hash, &owner).await?;

    tracing::info!("result {}", serde_json::to_string(&detail).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires seeded bill data"]
async fn bill_list_by_hashs() -> Result<()> {
    let owner = "UQAJr_aCqkWARCMkTHYkpKL9B-kYOFvXxvyDumUXsZ79ZnYY".to_string();
    let hashs = vec![
        "a86da9424486b91a4adb4aa11e4acbc0edf67bf1a716ed00029aeff09bd1d59f".to_string(),
        "977eafff958563dbc79030d74c8b31fea0668cac2b08139f4f594e0f434af2aa".to_string(),
    ];

    let wallet_manager = get_manager().await;

    let res = wallet_manager.list_by_hashs(owner, hashs).await?;
    tracing::info!("result {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires seeded bill data"]
async fn bill_lists() -> Result<()> {
    let wallet_manager = get_manager().await;

    let root_addr: Option<String> = None;
    let account_id: Option<u32> = Some(1);
    let addr = None;
    let chain_code: Option<String> = None;
    // let symbol = Some("TRX".to_string());
    let symbol = None;
    let is_multisig = None;
    let filter_min_value = Some(false);
    let start_time = None;
    let end_time = None;

    let transfer_type = vec![25];

    let page = 0;
    let page_size = 5;

    let detail = wallet_manager
        .bill_lists(
            root_addr,
            account_id,
            is_multisig,
            addr,
            chain_code,
            symbol,
            filter_min_value,
            start_time,
            end_time,
            transfer_type,
            page,
            page_size,
        )
        .await?;
    tracing::info!("{:#?}", detail);
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires seeded transaction result data"]
async fn query_bill_result() -> Result<()> {
    let wallet_manager = get_manager().await;

    let req = vec!["3".to_string()];

    let result = wallet_manager.query_tx_result(req).await?;

    tracing::info!("查询结果{:?}", result);
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires backend/chain bill sync availability"]
async fn test_sync_bill() -> Result<()> {
    let wallet_manager = get_manager().await;

    let chain_code = "tron".to_string();
    let address = "TMgiqqdyLCvnZtjzKpesqciGNaB99KwHQx".to_string();
    let _c = wallet_manager.sync_bill(chain_code, address).await?;
    tracing::warn!("同步结果{:?}", _c);
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires seeded wallet and backend bill sync availability"]
async fn test_sync_bill_by_address() -> Result<()> {
    let wallet_manager = get_manager().await;

    let wallet_address = "0x3d669d78532F763118561b55daa431956ede4155".to_string();
    let account_id = 1;
    let _c = wallet_manager.sync_bill_by_wallet_and_account(wallet_address, account_id).await?;
    tracing::warn!("同步结果{:?}", _c);
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires backend recent bill availability"]
async fn recent_bill() -> Result<()> {
    let wallet_manager = get_manager().await;

    let symbol = "TON".to_string();
    let addr = "UQBbNvHNr_Lcqe-Vq8xrLUONWdK-3Di4LbeuWNuLxnRfThq6".to_string();
    let chain_code = "ton".to_string();
    let page = 0;
    let page_size = 10;

    let detail = wallet_manager.recent_bill(symbol, addr, chain_code, page, page_size).await?;

    tracing::warn!("{:#?}", detail);
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires backend price availability"]
async fn coin_currency_price() -> Result<()> {
    let wallet_manager = get_manager().await;

    let symbol = "TRX".to_string();
    let chain_code = "tron".to_string();

    let res = wallet_manager.coin_currency_price(chain_code, symbol, None).await?;

    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test writes a sample bill into local test data"]
async fn test_create_bill() -> Result<()> {
    let wallet_manager = get_manager().await;

    let kind = wallet_database::entities::bill::BillKind::Transfer;
    // 第一次主币
    let params = wallet_database::entities::bill::NewBillEntity::new(
        "121xxx".to_string(),
        "from".to_string(),
        "to".to_string(),
        200.0,
        "eth".to_string(),
        "USDT".to_string(),
        false,
        kind,
        "hello".to_string(),
    )
    .with_transaction_fee("0.003");

    let core_pool = CoreDbPool::new(wallet_api::testkit::mqtt::core_pool(wallet_manager.ctx())?);
    wallet_api::domain::bill::BillDomain::create_bill(&core_pool, params).await.unwrap();
    Ok(())
}

#[tokio::test]
#[ignore = "manual calculation check without assertions"]
async fn test_create() -> Result<()> {
    let value = wallet_utils::unit::convert_to_u256("0.00330888", 18).unwrap();

    // let b_fee = wallet_utils::unit::u256_from_str("35054491035").unwrap();
    // let p_fee = wallet_utils::unit::u256_from_str("15597128697").unwrap();
    let m_fee = wallet_utils::unit::u256_from_str("65847105651").unwrap();

    let gas_limit = wallet_utils::unit::u256_from_str("23100").unwrap();

    let cost = gas_limit * m_fee;

    let aa = value + cost;
    println!("cost {},aa = {}", cost, aa);
    Ok(())
}
