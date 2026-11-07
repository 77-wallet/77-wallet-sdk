use super::{
    ApiAssetsDao,
    catalog_labels_test_support::{fixture, stored},
};
use crate::{
    entities::{api_coin::ApiCoinData, coin::CoinId},
    repositories::api_wallet::coin::ApiCoinRepo,
};

#[tokio::test]
async fn catalog_labels_catalog_batch_rename_never_writes_cached_asset_labels() {
    let pool = fixture("catalog_labels_no_fanout").await;
    let before = stored(&pool).await;
    sqlx::raw_sql("CREATE TRIGGER forbid_label_fanout BEFORE UPDATE OF name,symbol ON api_assets BEGIN SELECT RAISE(ABORT,'asset label writes are forbidden'); END;")
        .execute(pool.write_ref()).await.unwrap();
    let coin = ApiCoinData::new(
        Some("New Name".into()),
        "NEW",
        "eth",
        "0xabc".into(),
        Some("2".into()),
        None,
        18,
        1,
        0,
        1,
        chrono::Utc::now(),
        None,
    );
    ApiCoinRepo::upsert_multi_coin(&pool, vec![coin]).await.unwrap();
    assert_eq!(stored(&pool).await, before);
    let rows = ApiAssetsDao::list(pool.read_ref(), vec!["owner".into()], Some("eth".into()))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].name.as_str(), rows[0].symbol.as_str()), ("New Name", "NEW"));
}

#[tokio::test]
async fn catalog_labels_incremental_rename_never_writes_cached_asset_labels() {
    let pool = fixture("catalog_labels_incremental_no_fanout").await;
    let before = stored(&pool).await;
    sqlx::raw_sql("CREATE TRIGGER forbid_label_fanout BEFORE UPDATE OF name,symbol ON api_assets BEGIN SELECT RAISE(ABORT,'asset label writes are forbidden'); END;")
        .execute(pool.write_ref()).await.unwrap();
    let id =
        CoinId { symbol: "LIVE".into(), chain_code: "eth".into(), token_address: "0xabc".into() };
    ApiCoinRepo::update_price_unit(
        &id,
        "3",
        None,
        None,
        None,
        Some("NEW".into()),
        Some("New Name".into()),
        &pool,
    )
    .await
    .unwrap();
    assert_eq!(stored(&pool).await, before);
    let rows = ApiAssetsDao::list(pool.read_ref(), vec!["owner".into()], Some("eth".into()))
        .await
        .unwrap();
    assert_eq!((rows[0].name.as_str(), rows[0].symbol.as_str()), ("New Name", "NEW"));
}
