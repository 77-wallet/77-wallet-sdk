use super::{
    ApiAssetsDao,
    catalog_labels_test_support::{fixture, stored},
};

#[tokio::test]
async fn catalog_labels_empty_catalog_strings_are_authoritative() {
    let pool = fixture("catalog_labels_empty").await;
    sqlx::query("UPDATE api_coin SET name='',symbol=''").execute(pool.write_ref()).await.unwrap();
    let rows = ApiAssetsDao::list(pool.read_ref(), vec!["owner".into()], Some("eth".into()))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].name.as_str(), rows[0].symbol.as_str()), ("", ""));
    assert_eq!(stored(&pool).await.0, "4");
}

#[tokio::test]
async fn catalog_labels_identity_joins_preserve_evm_legacy_and_non_evm_case_without_multiplication()
{
    let pool = fixture("catalog_labels_identity").await;
    sqlx::query("UPDATE api_coin SET token_address='0xAbC' WHERE chain_code='eth'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    let eth = ApiAssetsDao::list(pool.read_ref(), vec!["owner".into()], Some("eth".into()))
        .await
        .unwrap();
    assert_eq!(eth.len(), 1);
    assert_eq!(eth[0].symbol, "LIVE");
    sqlx::raw_sql("INSERT INTO api_chain(name,chain_code,protocols,main_symbol,status,created_at) VALUES('Solana','sol','[]','SOL',1,CURRENT_TIMESTAMP);
        INSERT INTO api_coin(name,symbol,chain_code,token_address,price,decimals,status,created_at) VALUES('Upper','UP','sol','AbC','1',9,1,CURRENT_TIMESTAMP),('Lower','LOW','sol','abc','1',9,1,CURRENT_TIMESTAMP);
        INSERT INTO api_assets(decimals,address,token_address,chain_code,status,balance,created_at,updated_at) VALUES(6,'owner','AbC','sol',1,'6','2026-01-01T00:00:00Z','2026-01-02T00:00:00Z'),(6,'owner','abc','sol',1,'7','2026-01-01T00:00:00Z','2026-01-02T00:00:00Z');")
        .execute(pool.write_ref()).await.unwrap();
    let mut sol = ApiAssetsDao::list(pool.read_ref(), vec!["owner".into()], Some("sol".into()))
        .await
        .unwrap();
    sol.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    assert_eq!(sol.len(), 2);
    assert_eq!((sol[0].symbol.as_str(), sol[0].balance.as_str()), ("LOW", "7"));
    assert_eq!((sol[1].symbol.as_str(), sol[1].balance.as_str()), ("UP", "6"));
    let view_count: i64 = sqlx::query_scalar("SELECT count(*) FROM api_assets_with_coin_labels")
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(view_count, 3);
}
