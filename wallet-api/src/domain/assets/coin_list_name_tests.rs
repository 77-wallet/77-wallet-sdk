//! Offline component regressions for coin catalog names versus cached asset labels.
use super::*;

async fn fixture() -> (tempfile::TempDir, &'static Context) {
    let dir = tempfile::tempdir().unwrap();
    let config = crate::config::Config::new(r#"
app_code: "test"
crypto: { aes_key: "1234567890abcdef", aes_iv: "abcdef1234567890" }
backend_api: { dev_url: "http://127.0.0.1:9", test_url: "http://127.0.0.1:9", prod_url: "http://127.0.0.1:9" }
aggregate_api: { dev_url: "http://127.0.0.1:9", test_url: "http://127.0.0.1:9", prod_url: "http://127.0.0.1:9" }
oss: { access_key_id: "test", access_key_secret: "test", bucket_name: "test", endpoint: "test" }
"#).unwrap();
    let ctx = Box::leak(Box::new(
        Context::new_without_handles_for_test(
            crate::dirs::Dirs::new(dir.path().to_str().unwrap()).unwrap(),
            config,
        )
        .await
        .unwrap(),
    ));
    let pool = ctx.core_pool().unwrap();
    sqlx::query("INSERT INTO chain(name,chain_code,protocols,main_symbol,status,created_at) VALUES ('Sui','sui','[]','SUI',1,'2026-01-01T00:00:00Z')")
        .execute(pool.write_ref()).await.unwrap();
    for (symbol, token, name) in
        [("USDT", "token-usdt", "Tether USD"), ("USDC", "token-usdc", "USD Coin")]
    {
        sqlx::query("INSERT INTO coin(name,symbol,chain_code,token_address,price,decimals,is_default,status,created_at,updated_at) VALUES (?,?,'sui',?,'1',6,1,1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
            .bind(name).bind(symbol).bind(token).execute(pool.write_ref()).await.unwrap();
        sqlx::query("INSERT INTO assets(name,symbol,decimals,address,chain_code,token_address,status,is_multisig,balance,created_at,updated_at) VALUES ('sui',?,6,'owner','sui',?,1,0,'12.34','2026-01-01T00:00:00Z','2026-01-02T00:00:00Z')")
            .bind(symbol).bind(token).execute(pool.write_ref()).await.unwrap();
    }
    (dir, ctx)
}

#[tokio::test]
async fn coin_list_name_uses_current_catalog_and_preserves_stale_assets() {
    let (_dir, ctx) = fixture().await;
    let pool = ctx.core_pool().unwrap();
    let before: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT symbol,name,balance,updated_at FROM assets ORDER BY symbol")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();

    let list = AssetsDomain::new()
        .get_local_coin_list(ctx, &pool, vec!["owner".into()], None, Some("USD"), None)
        .await
        .unwrap();

    assert_eq!(list.len(), 2);
    for (symbol, name) in [("USDT", "Tether USD"), ("USDC", "USD Coin")] {
        let coin = list.iter().find(|c| c.symbol == symbol).unwrap();
        assert_eq!(coin.name.as_deref(), Some(name));
        assert_eq!(
            coin.chain_list.get("sui").unwrap(),
            if symbol == "USDT" { "token-usdt" } else { "token-usdc" }
        );
        assert!(coin.show_contract);
    }
    sqlx::query("UPDATE coin SET name='Catalog renamed USDT' WHERE symbol='USDT'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    let renamed = AssetsDomain::new()
        .get_local_coin_list(ctx, &pool, vec!["owner".into()], None, None, None)
        .await
        .unwrap();
    assert_eq!(
        renamed.iter().find(|c| c.symbol == "USDT").unwrap().name.as_deref(),
        Some("Catalog renamed USDT")
    );
    let after: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT symbol,name,balance,updated_at FROM assets ORDER BY symbol")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn coin_list_name_missing_catalog_returns_error_without_changing_assets() {
    let (_dir, ctx) = fixture().await;
    let pool = ctx.core_pool().unwrap();
    sqlx::query("DELETE FROM coin").execute(pool.write_ref()).await.unwrap();
    let result = AssetsDomain::new()
        .get_local_coin_list(ctx, &pool, vec!["owner".into()], None, None, None)
        .await;
    assert!(result.is_err());
    let names: Vec<String> =
        sqlx::query_scalar("SELECT name FROM assets").fetch_all(pool.read_ref()).await.unwrap();
    assert_eq!(names, vec!["sui", "sui"]);
}

#[test]
fn coin_list_name_sui_usdt_default_and_hot_merge_use_catalog_names() {
    let default = crate::default_data::coin::mainnet_default_coins_list()
        .unwrap()
        .coins
        .iter()
        .find(|c| c.chain_code == "sui" && c.symbol == "USDT")
        .unwrap()
        .clone();
    assert_eq!(default.name, "Tether USD");
    for name in ["Tether USD", "Catalog supplied name"] {
        let coins = ["sui", "eth"]
            .into_iter()
            .map(|chain| CoinEntity {
                name: name.into(),
                symbol: "USDT".into(),
                chain_code: chain.into(),
                token_address: "token-usdt".into(),
                price: "1".into(),
                protocol: None,
                decimals: 6,
                is_default: 1,
                is_popular: 1,
                status: 1,
                created_at: chrono::DateTime::default(),
                updated_at: None,
            })
            .collect();
        let merged = CoinDomain::merge_coin_to_list(coins, false).unwrap();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name.as_deref(), Some(name));
        assert_eq!(merged[0].chain_list.len(), 2);
    }
}
