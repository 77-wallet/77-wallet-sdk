use super::{ApiAssetsRepo, tests::seed_active_chain_and_coin};
use crate::{
    entities::{api_assets::ApiCreateAssetsVo, assets::AssetsId},
    repositories::{api_wallet::coin::ApiCoinRepo, test_helper::setup_api_wallet_pool},
};

#[tokio::test]
async fn token_case_repeated_writes_keep_one_balance_snapshot() {
    let mixed = "0xc2132D05D31c914a87C6611C10748AEb04B58e8F";
    let lower = mixed.to_ascii_lowercase();
    let pool = setup_api_wallet_pool("token_case_repeated_writes").await;
    seed_active_chain_and_coin(&pool, "polygon", "USDT", Some(mixed.into())).await;
    for (token, balance) in [(mixed, "1"), (lower.as_str(), "2")] {
        let id = AssetsId::new("sender", "polygon", token.into());
        ApiAssetsRepo::upsert_assets(
            &pool,
            ApiCreateAssetsVo::new(id, "USDT", 6, None, 0).with_name("USDT").with_balance(balance),
        )
        .await
        .unwrap();
    }
    let id = AssetsId::new("sender", "polygon", lower.as_str().into());
    let found = ApiAssetsRepo::find_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(found.token_address.as_db_str(), lower);
    assert_eq!(found.balance, "1"); // metadata upserts must not overwrite synced balances
    let rows =
        ApiAssetsRepo::list(&pool, vec!["sender".into()], Some("polygon".into())).await.unwrap();
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn token_case_coin_join_reads_legacy_spelling() {
    let mixed = "0xc2132D05D31c914a87C6611C10748AEb04B58e8F";
    let lower = mixed.to_ascii_lowercase();
    let pool = setup_api_wallet_pool("token_case_coin_join").await;
    seed_active_chain_and_coin(&pool, "polygon", "USDT", Some(mixed.into())).await;
    sqlx::query("UPDATE api_coin SET token_address=? WHERE chain_code='polygon'")
        .bind(mixed)
        .execute(pool.write_ref())
        .await
        .unwrap();
    let id = AssetsId::new("sender", "polygon", lower.into());
    ApiAssetsRepo::upsert_assets(
        &pool,
        ApiCreateAssetsVo::new(id.clone(), "USDT", 6, None, 0)
            .with_name("USDT")
            .with_balance("3.956788"),
    )
    .await
    .unwrap();
    let rows =
        ApiAssetsRepo::list(&pool, vec!["sender".into()], Some("polygon".into())).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(ApiAssetsRepo::find_by_id(&pool, &id).await.unwrap().is_some());
}

#[tokio::test]
async fn token_case_mixed_request_updates_lowercase_row() {
    let lower = "0xc2132d05d31c914a87c6611c10748aeb04b58e8f";
    let mixed = "0xc2132D05D31c914a87C6611C10748AEb04B58e8F";
    let pool = setup_api_wallet_pool("token_case_update_lowercase").await;
    seed_active_chain_and_coin(&pool, "polygon", "USDT", Some(lower.into())).await;
    for address in ["sender", "other"] {
        let id = AssetsId::new(address, "polygon", lower.into());
        ApiAssetsRepo::upsert_assets(
            &pool,
            ApiCreateAssetsVo::new(id, "USDT", 6, None, 0)
                .with_name("USDT")
                .with_balance("3.956788"),
        )
        .await
        .unwrap();
    }
    ApiAssetsRepo::batch_update_balance(
        &pool,
        vec![("sender".into(), "polygon".into(), mixed.into(), "2.956788".into())],
    )
    .await
    .unwrap();
    let id = AssetsId::new("sender", "polygon", lower.into());
    assert_eq!(ApiAssetsRepo::find_by_id(&pool, &id).await.unwrap().unwrap().balance, "2.956788");
    let other = AssetsId::new("other", "polygon", lower.into());
    assert_eq!(
        ApiAssetsRepo::find_by_id(&pool, &other).await.unwrap().unwrap().balance,
        "3.956788"
    );
}

#[tokio::test]
async fn token_case_legacy_asset_lookup_and_balance_update() {
    let mixed = "0xc2132D05D31c914a87C6611C10748AEb04B58e8F";
    let lower = mixed.to_ascii_lowercase();
    let pool = setup_api_wallet_pool("token_case_legacy_asset").await;
    for chain in ["polygon", "tron"] {
        seed_active_chain_and_coin(&pool, chain, "USDT", Some(mixed.into())).await;
        let id = AssetsId::new("sender", chain, mixed.into());
        let asset = ApiCreateAssetsVo::new(id, "USDT", 6, None, 0)
            .with_name("USDT")
            .with_balance("3.956788");
        ApiAssetsRepo::upsert_assets(&pool, asset).await.unwrap();
    }
    // Simulate an unmigrated single legacy spelling; normal writes now canonicalize.
    sqlx::query("UPDATE api_coin SET token_address=? WHERE chain_code='polygon'")
        .bind(mixed)
        .execute(pool.write_ref())
        .await
        .unwrap();
    sqlx::query("UPDATE api_assets SET token_address=? WHERE chain_code='polygon'")
        .bind(mixed)
        .execute(pool.write_ref())
        .await
        .unwrap();
    let id = AssetsId::new("sender", "polygon", lower.clone().into());
    let found = ApiAssetsRepo::find_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(found.token_address.as_db_str(), mixed);
    let coin = ApiCoinRepo::coin_by_chain_token_key_opt("polygon", lower.clone().into(), &pool)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(coin.token_address.as_db_str(), mixed);
    for _ in 0..2 {
        ApiAssetsRepo::batch_update_balance(
            &pool,
            vec![("sender".into(), "polygon".into(), lower.clone().into(), "2.956788".into())],
        )
        .await
        .unwrap();
    }
    assert_eq!(ApiAssetsRepo::find_by_id(&pool, &id).await.unwrap().unwrap().balance, "2.956788");
    ApiAssetsRepo::update_balance(&pool, "sender", "polygon", lower.clone().into(), "1.956788")
        .await
        .unwrap();
    ApiAssetsRepo::update_balance(&pool, "sender", "tron", lower.clone().into(), "99")
        .await
        .unwrap();
    let found = ApiAssetsRepo::find_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(found.balance, "1.956788");
    let rows =
        ApiAssetsRepo::list(&pool, vec!["sender".into()], Some("polygon".into())).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].token_address.as_db_str(), mixed);
    let tron = AssetsId::new("sender", "tron", lower.into());
    assert!(ApiAssetsRepo::find_by_id(&pool, &tron).await.unwrap().is_none());
    let original = AssetsId::new("sender", "tron", mixed.into());
    assert_eq!(
        ApiAssetsRepo::find_by_id(&pool, &original).await.unwrap().unwrap().balance,
        "3.956788"
    );
}
