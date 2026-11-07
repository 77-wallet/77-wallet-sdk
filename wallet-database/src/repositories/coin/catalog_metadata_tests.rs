use super::CoinRepo;
use crate::{
    dao::coin::CoinDao,
    entities::{asset_token_key::AssetTokenKey, coin::CoinData},
    repositories::test_helper::setup_core_pool,
};
use chrono::{DateTime, Utc};

fn timestamp(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}

fn catalog_coin(default: u8, updated: &str) -> CoinData {
    CoinData::new(
        Some("USDT".into()),
        "USDT",
        "polygon",
        AssetTokenKey::from_raw(Some("0xc2132d05d31c914a87c6611c10748aeb04b58e8f")),
        Some("1".into()),
        Some("ERC20".into()),
        6,
        default,
        0,
        1,
        false,
        timestamp("2026-10-01T14:40:23Z"),
        timestamp(updated),
    )
}

#[tokio::test]
async fn catalog_insert_preserves_distinct_server_timestamps() {
    let pool = setup_core_pool("catalog_insert_timestamps").await;
    let input = catalog_coin(0, "2026-10-08T11:41:04Z");
    let token = input.token_address.clone();
    CoinRepo::upsert_multi_coin(&pool, vec![input]).await.unwrap();
    let actual = CoinRepo::coin_by_chain_token_key("polygon", token, &pool).await.unwrap();
    assert_eq!(actual.created_at, timestamp("2026-10-01T14:40:23Z"));
    assert_eq!(actual.updated_at, Some(timestamp("2026-10-08T11:41:04Z")));
    assert_eq!(actual.is_default, 0);
}

#[tokio::test]
async fn catalog_conflict_updates_default_and_server_update_time() {
    let pool = setup_core_pool("catalog_conflict_timestamps").await;
    CoinRepo::upsert_multi_coin(&pool, vec![catalog_coin(0, "2026-10-02T00:00:00Z")])
        .await
        .unwrap();
    let input = catalog_coin(1, "2026-10-08T11:41:04Z");
    let token = input.token_address.clone();
    CoinRepo::upsert_multi_coin(&pool, vec![input]).await.unwrap();
    let actual = CoinRepo::coin_by_chain_token_key("polygon", token, &pool).await.unwrap();
    assert_eq!(actual.is_default, 1);
    assert_eq!(actual.status, 1);
    assert_eq!(actual.created_at, timestamp("2026-10-01T14:40:23Z"));
    assert_eq!(actual.updated_at, Some(timestamp("2026-10-08T11:41:04Z")));
}

#[tokio::test]
async fn catalog_rollback_preserves_default_and_timestamps() {
    let pool = setup_core_pool("catalog_rollback_timestamps").await;
    let input = catalog_coin(0, "2026-10-02T00:00:00Z");
    let token = input.token_address.clone();
    CoinRepo::upsert_multi_coin(&pool, vec![input]).await.unwrap();
    let before = CoinRepo::coin_by_chain_token_key("polygon", token.clone(), &pool).await.unwrap();
    let mut tx = pool.write_ref().begin().await.unwrap();
    CoinDao::upsert_multi_coin(tx.as_mut(), vec![catalog_coin(1, "2026-10-08T11:41:04Z")])
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    let after = CoinRepo::coin_by_chain_token_key("polygon", token, &pool).await.unwrap();
    assert_eq!(after.is_default, before.is_default);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.updated_at, before.updated_at);

    // A failed attempt must remain replayable; no partial timestamp acts as a cursor.
    CoinRepo::upsert_multi_coin(&pool, vec![catalog_coin(1, "2026-10-08T11:41:04Z")])
        .await
        .unwrap();
    let retried = CoinRepo::coin_by_chain_token_key(
        "polygon",
        catalog_coin(1, "2026-10-08T11:41:04Z").token_address,
        &pool,
    )
    .await
    .unwrap();
    assert_eq!(retried.is_default, 1);
    assert_eq!(retried.updated_at, Some(timestamp("2026-10-08T11:41:04Z")));
}

#[tokio::test]
async fn catalog_replay_repairs_legacy_timestamp_without_duplicate_rows() {
    let pool = setup_core_pool("catalog_legacy_replay").await;
    let legacy = catalog_coin(0, "2026-10-01T14:40:23Z");
    let token = legacy.token_address.clone();
    CoinRepo::upsert_multi_coin(&pool, vec![legacy]).await.unwrap();
    let before = CoinRepo::coin_by_chain_token_key("polygon", token.clone(), &pool).await.unwrap();
    assert_eq!(before.updated_at, Some(before.created_at));
    let count = CoinRepo::coin_count(&pool).await.unwrap();

    // Receiving the old-created row again repairs its metadata; repeated delivery is idempotent.
    for _ in 0..2 {
        CoinRepo::upsert_multi_coin(&pool, vec![catalog_coin(1, "2026-10-08T11:41:04Z")])
            .await
            .unwrap();
    }
    let actual = CoinRepo::coin_by_chain_token_key("polygon", token, &pool).await.unwrap();
    assert_eq!(CoinRepo::coin_count(&pool).await.unwrap(), count);
    assert_eq!(actual.created_at, before.created_at);
    assert_eq!(actual.updated_at, Some(timestamp("2026-10-08T11:41:04Z")));
    assert_eq!(actual.is_default, 1);
}

fn native_coin(symbol: &str) -> CoinData {
    CoinData::new(
        Some(symbol.into()),
        symbol,
        "ton",
        AssetTokenKey::Native,
        Some("7".into()),
        None,
        9,
        1,
        1,
        1,
        false,
        timestamp("2026-10-01T00:00:00Z"),
        timestamp("2026-10-26T00:00:00Z"),
    )
}

async fn seed_native_rename_assets(pool: &crate::CoreDbPool) {
    for (address, chain, token, status) in [
        ("visible", "ton", "", 1),
        ("hidden", "ton", "", 0),
        ("token", "ton", "jetton", 1),
        ("other", "eth", "", 1),
    ] {
        sqlx::query("INSERT INTO assets(name,symbol,decimals,address,token_address,chain_code,status,balance,created_at,updated_at) VALUES('Ton','TON',9,?,?,?,?, '1.23456789','2026-10-01','2026-10-02')")
            .bind(address).bind(token).bind(chain).bind(status)
            .execute(pool.write_ref()).await.unwrap();
    }
}

#[tokio::test]
async fn native_catalog_rename_preserves_holdings_and_replays_without_aliases() {
    let pool = setup_core_pool("native_catalog_rename").await;
    CoinRepo::upsert_multi_coin(&pool, vec![native_coin("TON")]).await.unwrap();
    seed_native_rename_assets(&pool).await;
    sqlx::query("INSERT INTO coin(name,symbol,chain_code,token_address,price,decimals,status,created_at) VALUES('contract','TON','ton','jetton','1',9,1,'2026-10-01'),('other chain','TON','eth','','1',9,1,'2026-10-01')")
        .execute(pool.write_ref()).await.unwrap();
    for _ in 0..2 {
        CoinRepo::upsert_multi_coin(&pool, vec![native_coin("GRAM")]).await.unwrap();
    }
    let coins: Vec<(String, String)> =
        sqlx::query_as("SELECT symbol,name FROM coin WHERE chain_code='ton' AND token_address=''")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(coins, vec![("GRAM".into(), "GRAM".into())]);
    let untouched: Vec<(String,String,String)> = sqlx::query_as("SELECT chain_code,token_address,symbol FROM coin WHERE chain_code='eth' OR token_address='jetton' ORDER BY chain_code")
        .fetch_all(pool.read_ref()).await.unwrap();
    assert_eq!(
        untouched,
        vec![
            ("eth".into(), "".into(), "TON".into()),
            ("ton".into(), "jetton".into(), "TON".into())
        ]
    );
    let assets: Vec<(String, String, String, String, i64, String)> = sqlx::query_as(
        "SELECT address,symbol,name,balance,status,updated_at FROM assets ORDER BY address",
    )
    .fetch_all(pool.read_ref())
    .await
    .unwrap();
    assert_eq!(assets.len(), 4);
    for (address, symbol, name, balance, status, updated) in assets {
        let renamed = address == "hidden" || address == "visible";
        assert_eq!(symbol, if renamed { "GRAM" } else { "TON" });
        assert_eq!(name, if renamed { "GRAM" } else { "Ton" });
        assert_eq!(balance, "1.23456789");
        assert_eq!(status, if address == "hidden" { 0 } else { 1 });
        assert_eq!(updated, "2026-10-02");
    }
}

#[tokio::test]
async fn native_catalog_rename_conflicting_assets_roll_back_without_merging_balances() {
    let pool = setup_core_pool("native_catalog_rename_collision").await;
    CoinRepo::upsert_multi_coin(&pool, vec![native_coin("TON")]).await.unwrap();
    seed_native_rename_assets(&pool).await;
    sqlx::query("INSERT INTO assets(name,symbol,decimals,address,token_address,chain_code,status,balance,created_at) VALUES('GRAM','GRAM',9,'visible','','ton',1,'9','2026-10-01')")
        .execute(pool.write_ref()).await.unwrap();
    assert!(CoinRepo::upsert_multi_coin(&pool, vec![native_coin("GRAM")]).await.is_err());
    let symbols: Vec<String> = sqlx::query_scalar("SELECT symbol FROM coin WHERE chain_code='ton'")
        .fetch_all(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(symbols, vec!["TON"]);
    let holdings: Vec<(String, String)> =
        sqlx::query_as("SELECT symbol,balance FROM assets WHERE address='visible' ORDER BY symbol")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(holdings, vec![("GRAM".into(), "9".into()), ("TON".into(), "1.23456789".into())]);
}

#[tokio::test]
async fn native_catalog_rename_conflicting_batch_does_not_create_symbol_aliases() {
    let pool = setup_core_pool("native_catalog_rename_batch_conflict").await;
    CoinRepo::upsert_multi_coin(&pool, vec![native_coin("TON")]).await.unwrap();
    assert!(
        CoinRepo::upsert_multi_coin(&pool, vec![native_coin("GRAM"), native_coin("TON")])
            .await
            .is_err()
    );
    let symbols: Vec<String> = sqlx::query_scalar("SELECT symbol FROM coin WHERE chain_code='ton'")
        .fetch_all(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(symbols, vec!["TON"]);
}
