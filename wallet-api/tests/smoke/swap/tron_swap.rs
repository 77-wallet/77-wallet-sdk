use crate::get_manager;
use wallet_api::request::transaction::{QuoteReq, SwapTokenInfo};
use wallet_database::{
    CoreDbPool, SqliteContext,
    dao::assets::CreateAssetsVo,
    entities::{asset_token_key::AssetTokenKey, assets::AssetsId},
    repositories::assets::AssetsRepo,
};

const AGGREGATOR: &str = "TSPSEgjQS1gChQLXW8qiVQtwrPdu1E4iCn";
const RECIPIENT: &str = "TQACP632EQvyecJTG5wTvMuqy8a4f4TJVr";

async fn get_seeded_pool() -> CoreDbPool {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_data");
    let db_dir = root.join("db");
    let ctx = SqliteContext::new(&db_dir.to_string_lossy(), Some("data.db")).await.unwrap();
    let pool = CoreDbPool::new(ctx.get_pool().unwrap());

    let trx_id = AssetsId::new(RECIPIENT, "tron", AssetTokenKey::Native);
    let trx_asset = CreateAssetsVo::new(trx_id, "TRX", 6, None, 0).with_balance("1000000");
    AssetsRepo::upsert_assets(&pool, trx_asset).await.unwrap();

    let usdt_id = AssetsId::new(
        RECIPIENT,
        "tron",
        AssetTokenKey::from_raw(Some("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t")),
    );
    let usdt_asset = CreateAssetsVo::new(usdt_id, "USDT", 6, None, 0).with_balance("10000");
    AssetsRepo::upsert_assets(&pool, usdt_asset).await.unwrap();

    pool
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_quote_trx_to_usdt() {
    let wallet_manager = get_manager().await;
    let _pool = get_seeded_pool().await;

    let token_in =
        SwapTokenInfo { token_addr: "".to_string(), symbol: "trx".to_string(), decimals: 6 };

    let token_out = SwapTokenInfo {
        token_addr: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string(),
        symbol: "usdt".to_string(),
        decimals: 6,
    };

    let req = QuoteReq {
        aggregator_addr: AGGREGATOR.to_string(),
        recipient: RECIPIENT.to_string(),
        chain_code: "tron".to_string(),
        amount_in: "100".to_string(),
        token_in,
        token_out,
        dex_list: vec![2, 3],
        slippage: None,
        allow_partial_fill: false,
    };

    let result = wallet_manager.quote(req).await.unwrap();
    tracing::warn!("quote trx -> usdt = {}", serde_json::to_string(&result).unwrap());
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_quote_usdt_to_trx() {
    let wallet_manager = get_manager().await;
    let _pool = get_seeded_pool().await;

    let token_in = SwapTokenInfo {
        token_addr: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string(),
        symbol: "USDT".to_string(),
        decimals: 6,
    };

    let token_out =
        SwapTokenInfo { token_addr: "".to_string(), symbol: "trx".to_string(), decimals: 6 };

    let req = QuoteReq {
        aggregator_addr: AGGREGATOR.to_string(),
        recipient: RECIPIENT.to_string(),
        chain_code: "tron".to_string(),
        amount_in: "10".to_string(),
        token_in,
        token_out,
        dex_list: vec![2, 3],
        slippage: None,
        allow_partial_fill: false,
    };

    let result = wallet_manager.quote(req).await.unwrap();
    tracing::warn!("quote usdt -> trx = {}", serde_json::to_string(&result).unwrap());
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_quote_usdt_to_sun() {
    let wallet_manager = get_manager().await;
    let _pool = get_seeded_pool().await;

    let token_in = SwapTokenInfo {
        token_addr: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string(),
        symbol: "usdt".to_string(),
        decimals: 6,
    };

    let token_out = SwapTokenInfo {
        token_addr: "TSSMHYeV2uE9qYH95DqyoCuNCzEL1NvU3S".to_string(),
        symbol: "sun".to_string(),
        decimals: 18,
    };

    let req = QuoteReq {
        aggregator_addr: AGGREGATOR.to_string(),
        recipient: RECIPIENT.to_string(),
        chain_code: "tron".to_string(),
        amount_in: "1".to_string(),
        token_in,
        token_out,
        dex_list: vec![2, 3],
        slippage: None,
        allow_partial_fill: false,
    };

    let result = wallet_manager.quote(req).await.unwrap();
    tracing::warn!("quote usdt -> sun = {}", serde_json::to_string(&result).unwrap());
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_quote_trx_to_usdt_with_slippage() {
    let wallet_manager = get_manager().await;
    let _pool = get_seeded_pool().await;

    let token_in =
        SwapTokenInfo { token_addr: "".to_string(), symbol: "trx".to_string(), decimals: 6 };

    let token_out = SwapTokenInfo {
        token_addr: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string(),
        symbol: "usdt".to_string(),
        decimals: 6,
    };

    let req = QuoteReq {
        aggregator_addr: AGGREGATOR.to_string(),
        recipient: RECIPIENT.to_string(),
        chain_code: "tron".to_string(),
        amount_in: "100".to_string(),
        token_in,
        token_out,
        dex_list: vec![2, 3],
        slippage: Some(0.01),
        allow_partial_fill: false,
    };

    let result = wallet_manager.quote(req).await.unwrap();
    tracing::warn!("quote trx -> usdt with slippage = {}", serde_json::to_string(&result).unwrap());
}
