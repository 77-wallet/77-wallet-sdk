use super::{
    ApiAssetsDao,
    catalog_labels_test_support::{fixture, stored},
};
use crate::entities::assets::AssetsId;

#[tokio::test]
async fn catalog_labels_list_and_id_read_current_catalog_without_mutating_asset() {
    let pool = fixture("catalog_labels_list").await;
    let before = stored(&pool).await;
    let rows = ApiAssetsDao::list(pool.read_ref(), vec!["owner".into()], Some("eth".into()))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "cached symbol must not hide current catalog identity");
    assert_eq!((rows[0].name.as_str(), rows[0].symbol.as_str()), ("Live Name", "LIVE"));
    assert_eq!(
        (rows[0].decimals, rows[0].balance.as_str(), rows[0].protocol.as_deref()),
        (6, "4", Some("asset-protocol"))
    );
    let row =
        ApiAssetsDao::assets_by_id(pool.read_ref(), &AssetsId::new("owner", "eth", "0xabc".into()))
            .await
            .unwrap()
            .unwrap();
    assert_eq!((row.name.as_str(), row.symbol.as_str()), ("Live Name", "LIVE"));
    assert_eq!(stored(&pool).await, before);
}

#[tokio::test]
async fn catalog_labels_chain_and_address_selectors_use_current_symbol() {
    let pool = fixture("catalog_labels_selectors").await;
    let rows = ApiAssetsDao::get_chain_assets_by_address_chain_code_symbol(
        pool.read_ref(),
        vec!["owner".into()],
        Some("eth".into()),
        Some("LIVE"),
        None,
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Live Name");
    let rows = ApiAssetsDao::get_api_assets_by_address(
        pool.read_ref(),
        vec!["owner".into()],
        Some("eth".into()),
        Some("LIVE"),
        Some("0xabc"),
        None,
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].symbol, "LIVE");
    assert_eq!(rows[0].decimals, 6);
}

#[tokio::test]
async fn catalog_labels_status_update_selects_current_catalog_symbol() {
    let pool = fixture("catalog_labels_status").await;
    let before = stored(&pool).await;
    ApiAssetsDao::update_status(pool.write_ref(), "eth", "LIVE", "0xabc".into(), 0).await.unwrap();
    let after = stored(&pool).await;
    assert_eq!(after.1, 0, "current symbol must select cached-label rows");
    assert_eq!((after.0, after.2, after.3), (before.0, before.2, before.3));
    ApiAssetsDao::update_status(pool.write_ref(), "eth", "LIVE", "0xabc".into(), 1).await.unwrap();
    assert_eq!(stored(&pool).await.1, 1);
}
