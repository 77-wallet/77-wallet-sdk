use super::catalog_labels_test_support::fixture;
use crate::{
    entities::{api_assets::ApiCreateAssetsVo, assets::AssetsId},
    repositories::api_wallet::assets::ApiAssetsRepo,
};

fn asset(address: &str) -> ApiCreateAssetsVo {
    ApiCreateAssetsVo::new(
        AssetsId {
            address: address.into(),
            chain_code: "eth".into(),
            token_address: "0xabc".into(),
        },
        "LIVE",
        6,
        Some("asset-protocol".into()),
        0,
    )
    .with_name("Live Name")
    .with_balance("9")
}

#[tokio::test]
async fn catalog_labels_removed_schema_preserves_all_four_writer_contracts() {
    let pool = fixture("removed_writers").await;
    ApiAssetsRepo::upsert_assets(&pool, asset("new-single")).await.unwrap();
    ApiAssetsRepo::upsert_assets_multi(
        &pool,
        vec![asset("owner").with_status(0), asset("new-bulk")],
    )
    .await
    .unwrap();
    ApiAssetsRepo::insert_missing_assets_multi(&pool, vec![asset("owner"), asset("new-init")])
        .await
        .unwrap();
    let stored: (String, i64) =
        sqlx::query_as("SELECT balance,status FROM api_assets WHERE address='owner'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(
        stored,
        ("4".into(), 0),
        "normal upsert preserves balance; missing initialization preserves status"
    );
    ApiAssetsRepo::upsert_assets_multi_update_balance(
        &pool,
        vec![asset("owner"), asset("new-balance")],
    )
    .await
    .unwrap();
    let stored: (String, i64) =
        sqlx::query_as("SELECT balance,status FROM api_assets WHERE address='owner'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(stored, ("9".into(), 0), "balance refresh must not enable hidden asset");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM api_assets")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        5
    );
    let labels: (String, String) = sqlx::query_as("SELECT name,symbol FROM api_coin")
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(labels, ("Live Name".into(), "LIVE".into()));
}
