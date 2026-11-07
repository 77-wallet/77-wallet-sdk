use super::ApiAssetsRepo;
use crate::{
    entities::{api_assets::ApiCreateAssetsVo, assets::AssetsId},
    repositories::test_helper::setup_api_wallet_pool,
};

fn asset(address: &str) -> ApiCreateAssetsVo {
    ApiCreateAssetsVo::new(AssetsId::new(address, "eth", None.into()), "ETH", 18, None, 0)
        .with_balance("0")
}

#[tokio::test]
async fn insert_missing_assets_preserves_existing_disabled_state_and_balance_on_retry() {
    let pool = setup_api_wallet_pool("missing_default_assets").await;
    ApiAssetsRepo::upsert_assets(&pool, asset("existing")).await.unwrap();
    sqlx::query("UPDATE api_assets SET status=0,is_multisig=1,balance='123',updated_at='2026-01-01T00:00:00Z' WHERE address='existing'").execute(pool.write_ref()).await.unwrap();
    for _ in 0..2 {
        ApiAssetsRepo::insert_missing_assets_multi(
            &pool,
            vec![asset("existing"), asset("missing")],
        )
        .await
        .unwrap();
    }
    let existing: (i32, i32, String, String) = sqlx::query_as(
        "SELECT status,is_multisig,balance,updated_at FROM api_assets WHERE address='existing'",
    )
    .fetch_one(pool.read_ref())
    .await
    .unwrap();
    assert_eq!(existing, (0, 1, "123".into(), "2026-01-01T00:00:00Z".into()));
    let missing: (i32, i32, String) =
        sqlx::query_as("SELECT status,is_multisig,balance FROM api_assets WHERE address='missing'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(missing, (1, 0, "0".into()));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_assets")
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn insert_missing_assets_rejected_chunk_leaves_persisted_defaults_unchanged() {
    let pool = setup_api_wallet_pool("missing_default_assets_failure").await;
    ApiAssetsRepo::upsert_assets(&pool, asset("existing")).await.unwrap();
    sqlx::query("UPDATE api_assets SET status=0,balance='9' WHERE address='existing'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TRIGGER reject_default_asset BEFORE INSERT ON api_assets WHEN NEW.address='rejected' BEGIN SELECT RAISE(ABORT,'controlled asset insert failure'); END;")
        .execute(pool.write_ref()).await.unwrap();
    assert!(
        ApiAssetsRepo::insert_missing_assets_multi(
            &pool,
            vec![asset("existing"), asset("missing"), asset("rejected")]
        )
        .await
        .is_err()
    );
    let existing: (i32, String) =
        sqlx::query_as("SELECT status,balance FROM api_assets WHERE address='existing'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(existing, (0, "9".into()));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_assets")
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(count, 1);
    sqlx::query("DROP TRIGGER reject_default_asset").execute(pool.write_ref()).await.unwrap();
    ApiAssetsRepo::insert_missing_assets_multi(
        &pool,
        vec![asset("existing"), asset("missing"), asset("rejected")],
    )
    .await
    .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_assets")
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(count, 3);
}
