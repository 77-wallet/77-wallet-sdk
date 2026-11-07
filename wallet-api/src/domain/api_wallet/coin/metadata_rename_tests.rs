use super::*;

#[tokio::test]
async fn api_coin_metadata_rename_incremental_response_updates_existing_labels_only() {
    // Arrange: a pre-existing hidden asset populated by on-chain auto-create.
    let dir = tempfile::tempdir().unwrap();
    let pool =
        wallet_database::SqliteContext::new(dir.path().to_str().unwrap(), Some("api_wallet.db"))
            .await
            .unwrap()
            .into_api_wallet_db_pool()
            .unwrap();
    sqlx::query("INSERT INTO api_coin(name,symbol,chain_code,token_address,price,decimals,status,created_at) VALUES('USDT0','USDT0','polygon','0xabc','1',6,1,'2026-01-01T00:00:00Z')")
        .execute(pool.write_ref()).await.unwrap();
    sqlx::query("INSERT INTO api_assets(decimals,address,token_address,chain_code,status,balance,created_at,updated_at) VALUES(6,'owner','0xabc','polygon',0,'9.956788','2026-01-01','2026-01-02')")
        .execute(pool.write_ref()).await.unwrap();
    let response = serde_json::json!({"id":"fixture","chainCode":"polygon","code":"usdt","name":"(PoS) Tether USD","contractAddress":"0xAbC","unit":6,"enable":true,"price":1,"createTime":1792564370580_i64,"updateTime":1792566622429_i64});
    // Act: apply the exact decoded response used by startup's incremental sync.
    ApiCoinDomain::apply_token_updates(&pool, vec![serde_json::from_value(response).unwrap()])
        .await
        .unwrap();
    // Assert: both metadata stores converge without exposing or changing holdings.
    let coin = ApiCoinRepo::coin_by_chain_token_key_opt("polygon", "0xabc".into(), &pool)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((coin.symbol.as_str(), coin.name.as_str()), ("USDT", "(PoS) Tether USD"));
    let asset: (String, String, String, i64, String) = sqlx::query_as(
        "SELECT symbol,name,balance,status,updated_at FROM api_assets_with_coin_labels WHERE address='owner'",
    )
    .fetch_one(pool.read_ref())
    .await
    .unwrap();
    assert_eq!(
        asset,
        ("USDT".into(), "(PoS) Tether USD".into(), "9.956788".into(), 0, "2026-01-02".into())
    );
}

#[tokio::test]
async fn api_native_catalog_rename_reuses_identity_and_preserves_hidden_balance() {
    // Arrange: native-token identity is unchanged; Base fixture does not enable new API chains.
    let dir = tempfile::tempdir().unwrap();
    let pool =
        wallet_database::SqliteContext::new(dir.path().to_str().unwrap(), Some("api_wallet.db"))
            .await
            .unwrap()
            .into_api_wallet_db_pool()
            .unwrap();
    sqlx::query("INSERT INTO api_coin(name,symbol,chain_code,token_address,price,decimals,status,created_at) VALUES('Old native','OLD','base','','1',18,1,'2026-01-01T00:00:00Z')")
        .execute(pool.write_ref()).await.unwrap();
    sqlx::query("INSERT INTO api_assets(decimals,address,token_address,chain_code,status,balance,created_at,updated_at) VALUES(18,'owner','','base',0,'1.25','2026-01-01','2026-01-02')")
        .execute(pool.write_ref()).await.unwrap();
    let response = serde_json::json!({"id":"fixture","chainCode":"base","code":"eth","name":"Ethereum","contractAddress":"","unit":18,"enable":true,"price":1,"createTime":1792564370580_i64,"updateTime":1792566622429_i64});
    // Act: startup catalog upsert followed by incremental refresh and replay.
    for _ in 0..2 {
        ApiCoinRepo::upsert_multi_coin(
            &pool,
            vec![coin_info_to_coin_data(serde_json::from_value(response.clone()).unwrap())],
        )
        .await
        .unwrap();
        ApiCoinDomain::apply_token_updates(
            &pool,
            vec![serde_json::from_value(response.clone()).unwrap()],
        )
        .await
        .unwrap();
    }
    // Assert: one identity, fresh labels, untouched holdings/visibility/time.
    let coins: Vec<(String, String)> =
        sqlx::query_as("SELECT symbol,name FROM api_coin WHERE chain_code='base'")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(coins, vec![("ETH".into(), "Ethereum".into())]);
    let asset: (String, String, String, i64, String) = sqlx::query_as(
        "SELECT symbol,name,balance,status,updated_at FROM api_assets_with_coin_labels WHERE address='owner'",
    )
    .fetch_one(pool.read_ref())
    .await
    .unwrap();
    assert_eq!(asset, ("ETH".into(), "Ethereum".into(), "1.25".into(), 0, "2026-01-02".into()));
}
