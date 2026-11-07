use super::{
    ApiAssetsDao,
    catalog_labels_test_support::{fixture, stored},
};

#[tokio::test]
async fn catalog_labels_post_upgrade_catalog_deletion_preserves_raw_row_and_display_filtering() {
    let pool = fixture("catalog_labels_missing").await;
    let before = stored(&pool).await;
    sqlx::query("DELETE FROM api_coin").execute(pool.write_ref()).await.unwrap();
    let refresh =
        ApiAssetsDao::assets_with_wallet_address_by_address(pool.read_ref(), &["owner".into()])
            .await
            .unwrap();
    assert_eq!(refresh.len(), 1);
    assert_eq!(refresh[0].symbol, "");
    let display = ApiAssetsDao::list(pool.read_ref(), vec!["owner".into()], Some("eth".into()))
        .await
        .unwrap();
    assert!(display.is_empty());
    assert_eq!(stored(&pool).await, before);
}

#[tokio::test]
async fn catalog_labels_disabled_coin_remains_hidden_but_raw_refresh_uses_catalog() {
    let pool = fixture("catalog_labels_disabled").await;
    sqlx::query("UPDATE api_coin SET status=0").execute(pool.write_ref()).await.unwrap();
    let refresh =
        ApiAssetsDao::assets_with_wallet_address_by_address(pool.read_ref(), &["owner".into()])
            .await
            .unwrap();
    assert_eq!(refresh.len(), 1);
    assert_eq!(refresh[0].symbol, "LIVE");
    assert!(
        ApiAssetsDao::list(pool.read_ref(), vec!["owner".into()], Some("eth".into()))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        ApiAssetsDao::get_api_wallet_assets_v2(pool.read_ref(), "wallet", Some(3), None, false)
            .await
            .unwrap()
            .is_empty()
    );
    sqlx::raw_sql("UPDATE api_coin SET status=1; UPDATE api_assets SET status=0")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert!(
        ApiAssetsDao::list(pool.read_ref(), vec!["owner".into()], Some("eth".into()))
            .await
            .unwrap()
            .is_empty()
    );
}
