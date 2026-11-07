use anyhow::Result;
use wallet_api::{
    Context,
    testkit::mqtt::{api_wallet_pool, core_pool},
};
use wallet_database::{
    dao::{
        assets::{AssetsDao, CreateAssetsVo},
        chain::ChainDao,
    },
    entities::{
        api_assets::ApiCreateAssetsVo,
        api_chain::{ApiChainCreateVo, NodeBindType},
        api_coin::ApiCoinData,
        assets::AssetsId,
        chain::ChainCreateVo,
    },
    repositories::api_wallet::{assets::ApiAssetsRepo, chain::ApiChainRepo, coin::ApiCoinRepo},
};

use super::fixtures::AcctChangeFixture;

pub(crate) async fn ensure_sol_chain_active(ctx: &'static Context) -> Result<()> {
    let pool = api_wallet_pool(ctx)?;
    ApiChainRepo::add(
        &pool,
        ApiChainCreateVo::new(
            "Solana",
            "sol",
            &[String::from("m/44'/501'/0'/0'")],
            NodeBindType::AutoBackend,
            "SOL",
        ),
    )
    .await?;
    Ok(())
}

pub(crate) async fn ensure_eth_chain_active(ctx: &'static Context) -> Result<()> {
    let pool = core_pool(ctx)?;
    let chain = ChainCreateVo::new(
        "Ethereum",
        "eth",
        &[String::from("eth")],
        NodeBindType::AutoBackend,
        "ETH",
    );
    ChainDao::upsert(pool.as_ref(), chain).await?;
    Ok(())
}

pub(crate) async fn seed_api_wallet_sol_usdc_asset(
    ctx: &'static Context,
    fixture: &AcctChangeFixture,
) -> Result<()> {
    let api_pool = api_wallet_pool(ctx)?;
    let now = wallet_utils::time::now();

    let coin = ApiCoinData::new(
        Some("USD Coin".to_string()),
        "USDC",
        "sol",
        Some(fixture.token.clone()).into(),
        Some("1".to_string()),
        None,
        6,
        1,
        1,
        1,
        now,
        Some(now),
    );
    ApiCoinRepo::upsert_multi_coin(&api_pool, vec![coin]).await?;

    let asset = ApiCreateAssetsVo::new(
        AssetsId::new(&fixture.address, &fixture.chain_code, Some(fixture.token.clone()).into()),
        "USDC",
        6,
        None,
        0,
    )
    .with_name("USD Coin");
    ApiAssetsRepo::upsert_assets_multi(&api_pool, vec![asset]).await?;

    Ok(())
}

pub(crate) async fn seed_normal_eth_usdt_asset(
    ctx: &'static Context,
    fixture: &AcctChangeFixture,
) -> Result<()> {
    let pool = core_pool(ctx)?;
    let asset = CreateAssetsVo::new(
        AssetsId::new(&fixture.address, &fixture.chain_code, Some(fixture.token.clone()).into()),
        "USDT",
        6,
        None,
        0,
    )
    .with_name("Tether USD");
    AssetsDao::upsert_assets(pool.as_ref(), asset).await?;
    assert!(
        AssetsDao::get_by_addr_token(
            pool.as_ref(),
            &fixture.chain_code,
            &fixture.token,
            &fixture.address
        )
        .await?
        .is_some()
    );

    Ok(())
}

pub(crate) async fn seed_normal_eth_native_asset(
    ctx: &'static Context,
    fixture: &AcctChangeFixture,
) -> Result<()> {
    let pool = core_pool(ctx)?;
    let asset = CreateAssetsVo::new(
        AssetsId::new(&fixture.address, &fixture.chain_code, Some(String::new()).into()),
        "ETH",
        18,
        None,
        0,
    )
    .with_name("Ethereum");
    AssetsDao::upsert_assets(pool.as_ref(), asset).await?;
    assert!(
        AssetsDao::get_by_addr_token(
            pool.as_ref(),
            &fixture.chain_code,
            &fixture.token,
            &fixture.address
        )
        .await?
        .is_some()
    );

    Ok(())
}

pub(crate) async fn assert_api_wallet_asset_symbol(
    ctx: &'static Context,
    fixture: &AcctChangeFixture,
    expected_symbol: &str,
) -> Result<()> {
    let api_pool = api_wallet_pool(ctx)?;
    let saved = ApiAssetsRepo::find_by_id(
        &api_pool,
        &AssetsId::new(&fixture.address, &fixture.chain_code, Some(fixture.token.clone()).into()),
    )
    .await?;

    assert!(saved.is_some());
    assert_eq!(saved.unwrap().symbol, expected_symbol);
    Ok(())
}

pub(crate) async fn assert_normal_wallet_asset_symbol(
    ctx: &'static Context,
    fixture: &AcctChangeFixture,
    expected_symbol: &str,
) -> Result<()> {
    let pool = core_pool(ctx)?;
    let saved = AssetsDao::get_by_addr_token(
        pool.as_ref(),
        &fixture.chain_code,
        &fixture.token,
        &fixture.address,
    )
    .await?;

    assert!(saved.is_some());
    assert_eq!(saved.unwrap().symbol, expected_symbol);
    Ok(())
}
