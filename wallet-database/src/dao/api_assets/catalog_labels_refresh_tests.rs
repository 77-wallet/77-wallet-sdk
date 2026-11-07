use super::{
    ApiAssetsDao,
    catalog_labels_test_support::{fixture, stored},
};

#[tokio::test]
async fn catalog_labels_address_refresh_records_use_current_symbol_and_asset_precision() {
    let pool = fixture("catalog_labels_refresh_address").await;
    let before = stored(&pool).await;
    for keys in [vec!["owner".into()], vec!["owner:eth:0xabc".into()]] {
        let rows = ApiAssetsDao::assets_with_wallet_address_by_address(pool.read_ref(), &keys)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol, "LIVE");
        assert_eq!(rows[0].balance, "4");
        assert_eq!(rows[0].decimals, 6);
    }
    assert_eq!(stored(&pool).await, before);
}

#[tokio::test]
async fn catalog_labels_token_refresh_keyset_uses_current_symbol() {
    let pool = fixture("catalog_labels_refresh_token").await;
    let rows = ApiAssetsDao::assets_with_wallet_address_by_token(
        pool.read_ref(),
        &["LIVE:eth:0xabc".into()],
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].symbol, "LIVE");
    assert_eq!(rows[0].balance, "4");
    let old = ApiAssetsDao::assets_with_wallet_address_by_token(
        pool.read_ref(),
        &["OLD:eth:0xabc".into()],
    )
    .await
    .unwrap();
    assert!(old.is_empty());
}

#[tokio::test]
async fn catalog_labels_wallet_summary_groups_by_current_symbol_without_changing_amounts() {
    let pool = fixture("catalog_labels_summary").await;
    sqlx::raw_sql("INSERT INTO api_coin(name,symbol,chain_code,token_address,price,decimals,status,is_default,created_at)
        VALUES('Second Name','LIVE2','eth','0xdef','3',18,1,1,CURRENT_TIMESTAMP);
        INSERT INTO api_assets(decimals,address,token_address,chain_code,status,balance,created_at,updated_at)
        VALUES(6,'owner','0xdef','eth',1,'5','2026-01-01T00:00:00Z','2026-01-02T00:00:00Z');")
        .execute(pool.write_ref()).await.unwrap();
    let mut rows = ApiAssetsDao::get_api_wallet_assets_v2(
        pool.read_ref(),
        "wallet",
        Some(3),
        Some("eth"),
        false,
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 2, "stale cached symbols must not merge different current symbols");
    rows.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    assert_eq!(
        (
            rows[0].symbol.as_str(),
            rows[0].api_assets_name.as_str(),
            rows[0].total_coins_quantity,
            rows[0].total_account_amount
        ),
        ("LIVE", "Live Name", 4.0, Some(8.0))
    );
    assert_eq!(
        (
            rows[1].symbol.as_str(),
            rows[1].api_assets_name.as_str(),
            rows[1].total_coins_quantity,
            rows[1].total_account_amount
        ),
        ("LIVE2", "Second Name", 5.0, Some(15.0))
    );
    let cached: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pragma_table_info('api_assets') WHERE name IN ('name','symbol')",
    )
    .fetch_one(pool.read_ref())
    .await
    .unwrap();
    assert_eq!(cached, 0);
}
