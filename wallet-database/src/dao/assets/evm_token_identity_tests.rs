use super::*;
use crate::{dao::coin::CoinDao, entities::coin::CoinData, init::Migrator};
use sqlx::Connection;

const REPAIR: &str =
    include_str!("../../../schema/core/migrations/20261020000000_ordinary_evm_token_identity.sql");

async fn legacy_db() -> sqlx::SqliteConnection {
    let mut db = sqlx::SqliteConnection::connect("sqlite::memory:").await.unwrap();
    for migration in Migrator::Core.migrator().unwrap().iter() {
        if migration.version < 20261020000000 {
            sqlx::raw_sql(&migration.sql).execute(&mut db).await.unwrap();
        }
    }
    db
}

async fn seed(db: &mut sqlx::SqliteConnection, chain: &str, token: &str, balance: &str) {
    sqlx::query("INSERT OR IGNORE INTO chain(name,chain_code,protocols,main_symbol,status,created_at) VALUES ('fixture',?,'[]','ETH',1,'2026-01-01T00:00:00Z')")
        .bind(chain).execute(&mut *db).await.unwrap();
    sqlx::query("INSERT INTO coin(name,symbol,chain_code,token_address,decimals,is_default,status,created_at,updated_at) VALUES ('USDT','USDT',?,?,6,1,1,'2026-01-01T00:00:00Z','2026-01-02T00:00:00Z')")
        .bind(chain).bind(token).execute(&mut *db).await.unwrap();
    sqlx::query("INSERT INTO assets(name,symbol,decimals,address,chain_code,token_address,status,balance,created_at,updated_at) VALUES ('USDT','USDT',6,'owner',?,?,1,?,'2026-01-01T00:00:00Z','2026-01-02T00:00:00Z')")
        .bind(chain).bind(token).bind(balance).execute(db).await.unwrap();
}

fn asset(chain: &str, token: &str, balance: &str) -> CreateAssetsVo {
    CreateAssetsVo::new(AssetsId::new("owner", chain, token.into()), "USDT", 6, None, 0)
        .with_name("USDT")
        .with_balance(balance)
}

fn coin(chain: &str, token: &str) -> CoinData {
    CoinData::new(
        Some("USDT".into()),
        "USDT",
        chain,
        token.into(),
        Some("1".into()),
        None,
        6,
        1,
        0,
        1,
        false,
        chrono::Utc::now(),
        chrono::Utc::now(),
    )
}

#[tokio::test]
async fn ordinary_evm_token_identity_repairs_five_chain_snapshot_without_summing() {
    let mut db = legacy_db().await;
    for chain in ["base", "arbitrum", "polygon", "scroll", "avalanche"] {
        seed(&mut db, chain, "0xabc", "2").await;
        seed(&mut db, chain, "0xAbC", "0").await;
    }
    sqlx::raw_sql(REPAIR).execute(&mut db).await.unwrap();
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT token_address,balance FROM assets")
        .fetch_all(&mut db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|r| r == &("0xabc".into(), "2".into())));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM coin").fetch_one(&mut db).await.unwrap();
    assert_eq!(count, 5);
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM ordinary_asset_identity_refresh")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(pending, 5);
    // Reapplication must not create extra records or consume outstanding refreshes.
    sqlx::raw_sql(REPAIR).execute(&mut db).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ordinary_asset_identity_refresh")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        5
    );
}

#[tokio::test]
async fn ordinary_evm_token_identity_writes_queries_and_refresh_share_one_record() {
    let mut db = legacy_db().await;
    sqlx::raw_sql(REPAIR).execute(&mut db).await.unwrap();
    seed(&mut db, "base", "0xabc", "2").await;
    for token in ["0xAbC", "0xABC"] {
        CoinDao::upsert_multi_coin(&mut db, vec![coin("base", token)]).await.unwrap();
        AssetsDao::upsert_assets(&mut db, asset("base", token, "0")).await.unwrap();
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM assets")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        1
    );
    let id = AssetsId::new("owner", "base", "0xABC".into());
    assert_eq!(AssetsDao::assets_by_id(&mut db, &id).await.unwrap().unwrap().balance, "2");
    assert!(
        CoinDao::get_coin_by_chain_code_token_address(&mut db, "base", "0xABC".into())
            .await
            .unwrap()
            .is_some()
    );
    AssetsDao::update_balance(&mut db, &id, "7").await.unwrap();
    assert_eq!(
        AssetsDao::get_by_addr_token(&mut db, "base", "0xAbC", "owner")
            .await
            .unwrap()
            .unwrap()
            .balance,
        "7"
    );
    AssetsDao::delete_assets(&mut db, &id).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT status FROM assets").fetch_one(&mut db).await.unwrap(),
        0
    );
}

#[tokio::test]
async fn ordinary_evm_token_identity_conflict_retry_rollback_and_non_evm_invariants() {
    let mut db = legacy_db().await;
    seed(&mut db, "base", "0xabc", "2").await;
    seed(&mut db, "base", "0xAbC", "9").await;
    seed(&mut db, "tron", "TAbC", "3").await;
    seed(&mut db, "tron", "Tabc", "4").await;
    seed(&mut db, "eth", "", "5").await;
    let mut tx = db.begin().await.unwrap();
    sqlx::raw_sql(REPAIR).execute(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM assets WHERE chain_code='base'")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        2
    );
    sqlx::raw_sql(REPAIR).execute(&mut db).await.unwrap();
    let id = AssetsId::new("owner", "base", "0xABC".into());
    assert_eq!(AssetsDao::assets_by_id(&mut db, &id).await.unwrap().unwrap().balance, "2");
    sqlx::raw_sql("CREATE TRIGGER fail_balance BEFORE UPDATE OF balance ON assets BEGIN SELECT RAISE(ABORT,'offline refresh failure'); END;")
        .execute(&mut db).await.unwrap();
    assert!(AssetsDao::update_balance(&mut db, &id, "11").await.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ordinary_asset_identity_refresh")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        1
    );
    sqlx::raw_sql("DROP TRIGGER fail_balance").execute(&mut db).await.unwrap();
    let mut tx = db.begin().await.unwrap();
    AssetsDao::update_balance(&mut *tx, &id, "11").await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ordinary_asset_identity_refresh")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        1
    );
    AssetsDao::update_balance(&mut db, &id, "11").await.unwrap();
    assert_eq!(AssetsDao::assets_by_id(&mut db, &id).await.unwrap().unwrap().balance, "11");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ordinary_asset_identity_refresh")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        0
    );
    let other: Vec<(String, String)> = sqlx::query_as(
        "SELECT token_address,balance FROM assets WHERE chain_code='tron' ORDER BY token_address",
    )
    .fetch_all(&mut db)
    .await
    .unwrap();
    assert_eq!(other, vec![("TAbC".into(), "3".into()), ("Tabc".into(), "4".into())]);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT balance FROM assets WHERE chain_code='eth'")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        "5"
    );
    let duplicate = sqlx::query("INSERT INTO assets(name,symbol,decimals,address,chain_code,token_address,protocol,status,is_multisig,balance,created_at,updated_at) SELECT name,symbol,decimals,address,'base','0xAbC',protocol,status,is_multisig,balance,created_at,updated_at FROM assets WHERE chain_code='base'").execute(&mut db).await;
    assert!(duplicate.unwrap_err().as_database_error().unwrap().is_unique_violation());
}

#[tokio::test]
async fn ordinary_evm_token_identity_normalizes_singletons_and_uses_latest_fallback() {
    let mut db = legacy_db().await;
    seed(&mut db, "base", "0xAbC", "2").await;
    seed(&mut db, "base", "0xaBc", "7").await;
    seed(&mut db, "eth", "0xDeF", "3").await;
    sqlx::query("UPDATE assets SET updated_at='2026-10-20T00:00:00Z' WHERE token_address='0xaBc'")
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::raw_sql(REPAIR).execute(&mut db).await.unwrap();
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT chain_code,token_address,balance FROM assets ORDER BY chain_code")
            .fetch_all(&mut db)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            ("base".into(), "0xabc".into(), "7".into()),
            ("eth".into(), "0xdef".into(), "3".into())
        ]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ordinary_asset_identity_refresh")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn ordinary_evm_token_identity_refresh_hints_are_bounded_and_account_scoped() {
    let mut db = legacy_db().await;
    seed(&mut db, "base", "0xabc", "2").await;
    seed(&mut db, "base", "0xAbC", "0").await;
    sqlx::query("INSERT INTO account(account_id,name,address,wallet_address,chain_code,status,is_init,created_at) VALUES (3,'fixture','owner','wallet','base',1,1,'2026-01-01T00:00:00Z')").execute(&mut db).await.unwrap();
    sqlx::raw_sql(REPAIR).execute(&mut db).await.unwrap();
    assert_eq!(
        AssetsDao::pending_identity_refreshes(&mut db, "wallet", Some(3), None)
            .await
            .unwrap()
            .len(),
        1
    );
    for (wallet, account, chain) in
        [("wallet", 4, None), ("other", 3, None), ("wallet", 3, Some("eth"))]
    {
        assert!(
            AssetsDao::pending_identity_refreshes(&mut db, wallet, Some(account), chain)
                .await
                .unwrap()
                .is_empty()
        );
    }
    // Reading/dispatching a hint must not remove it.
    assert_eq!(
        AssetsDao::pending_identity_refreshes(&mut db, "wallet", Some(3), Some("base"))
            .await
            .unwrap()
            .len(),
        1
    );
    for i in 0..105 {
        let address = format!("extra-{i:03}");
        sqlx::query("INSERT INTO assets(name,symbol,decimals,address,chain_code,token_address,status,balance,created_at) VALUES ('USDT','USDT',6,?,'base','0xabc',1,'2','2026-01-01T00:00:00Z')").bind(&address).execute(&mut db).await.unwrap();
        sqlx::query("INSERT INTO ordinary_asset_identity_refresh(address,chain_code,token_address) VALUES (?,'base','0xabc')")
            .bind(&address)
            .execute(&mut db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO account(account_id,name,address,wallet_address,chain_code,status,is_init,created_at) VALUES (3,'fixture',?,'wallet','base',1,1,'2026-01-01T00:00:00Z')").bind(address).execute(&mut db).await.unwrap();
    }
    let mut seen = std::collections::HashSet::new();
    for _ in 0..3 {
        let batch =
            AssetsDao::pending_identity_refreshes(&mut db, "wallet", Some(3), None).await.unwrap();
        assert_eq!(batch.len(), 100);
        let ids: Vec<_> = batch.iter().map(|row| row.get_assets_id()).collect();
        seen.extend(batch.into_iter().map(|row| row.address));
        AssetsDao::mark_identity_refresh_dispatched(&mut db, &ids).await.unwrap();
    }
    // Simulate every dispatched RPC failing: the tail still gets a turn, and
    // all hints remain pending. No wall-clock progress is needed to rotate.
    assert_eq!(seen.len(), 106);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ordinary_asset_identity_refresh")
            .fetch_one(&mut db)
            .await
            .unwrap(),
        106
    );
}

#[tokio::test]
async fn ordinary_evm_token_identity_preserves_existing_symbol_alias_dimension() {
    let mut db = legacy_db().await;
    seed(&mut db, "base", "0xabc", "2").await;
    seed(&mut db, "base", "0xAbC", "0").await;
    sqlx::query("INSERT INTO coin(name,symbol,chain_code,token_address,decimals,is_default,status,created_at) VALUES ('alias','OLD','base','0xabc',6,1,1,'2026-01-01T00:00:00Z')").execute(&mut db).await.unwrap();
    sqlx::query("INSERT INTO assets(name,symbol,decimals,address,chain_code,token_address,status,balance,created_at) VALUES ('alias','OLD',6,'owner','base','0xabc',1,'3','2026-01-01T00:00:00Z')").execute(&mut db).await.unwrap();
    sqlx::raw_sql(REPAIR).execute(&mut db).await.unwrap();
    let assets: Vec<(String, String)> =
        sqlx::query_as("SELECT symbol,balance FROM assets ORDER BY symbol")
            .fetch_all(&mut db)
            .await
            .unwrap();
    assert_eq!(assets, vec![("OLD".into(), "3".into()), ("USDT".into(), "2".into())]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM coin").fetch_one(&mut db).await.unwrap(),
        2
    );
}

#[tokio::test]
async fn ordinary_evm_token_identity_precision_repair_cannot_consume_refresh_hint() {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./schema/core/migrations").run(&db).await.unwrap();
    let pool = crate::CoreDbPool::new(std::sync::Arc::new(db));
    {
        let mut conn = pool.write_ref().acquire().await.unwrap();
        seed(&mut conn, "base", "0xabc", "2").await;
        sqlx::query("INSERT INTO ordinary_asset_identity_refresh(address,chain_code,token_address) VALUES ('owner','base','0xabc')")
            .execute(&mut *conn)
            .await
            .unwrap();
    }
    let mut update = coin("base", "0xAbC");
    update.decimals = 8;
    crate::repositories::coin::CoinRepo::upsert_multi_coin(&pool, vec![update]).await.unwrap();
    let row: (i64, String) = sqlx::query_as("SELECT decimals,balance FROM assets")
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(row, (8, "0.02".into()));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ordinary_asset_identity_refresh")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        1
    );
}

#[test]
fn ordinary_evm_token_identity_migration_covers_supported_chains() {
    for chain in crate::entities::api_withdraw::signed_evm_chain_codes() {
        assert!(REPAIR.contains(&format!("'{chain}'")));
    }
}
