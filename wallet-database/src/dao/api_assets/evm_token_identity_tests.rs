use super::*;
use crate::{dao::api_coin::ApiCoinDao, entities::api_coin::ApiCoinData, init::Migrator};
use sqlx::Connection;

const GUARD: &str =
    include_str!("../../../schema/api_wallet/migrations/20261013000000_evm_token_identity.sql");

async fn db_before_identity_guard() -> sqlx::SqliteConnection {
    let mut conn = sqlx::SqliteConnection::connect("sqlite::memory:").await.unwrap();
    for migration in Migrator::ApiWallet.migrator().unwrap().iter() {
        if migration.version < 20261013000000 {
            sqlx::raw_sql(&migration.sql).execute(&mut conn).await.unwrap();
        }
    }
    conn
}

async fn seed(
    conn: &mut sqlx::SqliteConnection,
    chain: &str,
    token: &str,
    balance: &str,
    date: &str,
) {
    sqlx::query("INSERT INTO api_coin (chain_code,token_address,name,symbol,price,decimals,is_default,status,created_at,updated_at) VALUES (?,?,'USDT','USDT','1',6,1,1,?,?)")
        .bind(chain).bind(token).bind(date).bind(date).execute(&mut *conn).await.unwrap();
    sqlx::query("INSERT INTO api_assets (name,symbol,decimals,address,chain_code,token_address,status,balance,created_at,updated_at) VALUES ('USDT','USDT',6,'address',?,?,1,?,?,?)")
        .bind(chain).bind(token).bind(balance).bind(date).bind(date).execute(conn).await.unwrap();
}

fn coin(chain: &str, token: &str) -> ApiCoinData {
    ApiCoinData::new(
        Some("USDT".into()),
        "USDT",
        chain,
        AssetTokenKey::from_raw(Some(token)),
        Some("1".into()),
        None,
        6,
        1,
        0,
        1,
        chrono::Utc::now(),
        None,
    )
}

#[tokio::test]
async fn evm_token_identity_coin_and_asset_writes_share_one_key() {
    let mut conn = db_before_identity_guard().await;
    sqlx::raw_sql(GUARD).execute(&mut conn).await.unwrap();
    for migration in
        Migrator::ApiWallet.migrator().unwrap().iter().filter(|m| m.version > 20261013000000)
    {
        sqlx::raw_sql(&migration.sql).execute(&mut conn).await.unwrap();
    }
    for token in ["0xAbC", "0xabc"] {
        ApiCoinDao::upsert_multi_coin(&mut conn, vec![coin("base", token)]).await.unwrap();
        let assets = asset("address", "base", "USDT", token, "6");
        ApiAssetsDao::upsert_assets_multi_update_balance(&mut conn, vec![assets]).await.unwrap();
    }
    let coins: i64 =
        sqlx::query_scalar("SELECT count(*) FROM api_coin").fetch_one(&mut conn).await.unwrap();
    assert_eq!(coins, 1);
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT token_address,balance FROM api_assets")
            .fetch_all(&mut conn)
            .await
            .unwrap();
    assert_eq!(rows, vec![("0xabc".into(), "6".into())]);
    let got = ApiCoinDao::list_by_chain_token_map_batch(
        &mut conn,
        &HashMap::from([("base".into(), "0xAbC".into())]),
    )
    .await
    .unwrap();
    assert_eq!(got.len(), 1);
    // Startup supplementation must not overwrite a synced balance.
    ApiAssetsDao::insert_missing_assets_multi(
        &mut conn,
        vec![asset("address", "base", "USDT", "0xABC", "0")],
    )
    .await
    .unwrap();
    ApiAssetsDao::upsert_assets(&mut conn, asset("address", "base", "USDT", "0xABC", "0"))
        .await
        .unwrap();
    let balance: String =
        sqlx::query_scalar("SELECT balance FROM api_assets").fetch_one(&mut conn).await.unwrap();
    assert_eq!(balance, "6");
}

fn asset(
    address: &str,
    chain: &str,
    symbol: &str,
    token: &str,
    balance: &str,
) -> ApiCreateAssetsVo {
    ApiCreateAssetsVo::new(
        AssetsId::new(address, chain, AssetTokenKey::from_raw(Some(token))),
        symbol,
        6,
        None,
        0,
    )
    .with_name(symbol)
    .with_balance(balance)
}

#[tokio::test]
async fn evm_token_identity_guard_and_transaction_rollback() {
    let mut conn = db_before_identity_guard().await;
    seed(&mut conn, "base", "0xabc", "6", "2026-10-08T00:00:00Z").await;
    let mut tx = conn.begin().await.unwrap();
    sqlx::raw_sql(GUARD).execute(&mut *tx).await.unwrap();
    let indexes:i64=sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN ('api_coin_evm_identity_idx','api_assets_evm_identity_idx')").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(indexes, 2);
    tx.rollback().await.unwrap();
    let indexes:i64=sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN ('api_coin_evm_identity_idx','api_assets_evm_identity_idx')").fetch_one(&mut conn).await.unwrap();
    assert_eq!(indexes, 0);
    sqlx::raw_sql(GUARD).execute(&mut conn).await.unwrap();
    let error = sqlx::query("INSERT INTO api_assets (name,symbol,decimals,address,chain_code,token_address,status,balance,created_at) VALUES ('USDT','USDT',6,'address','base','0xABC',1,'99',CURRENT_TIMESTAMP)").execute(&mut conn).await;
    assert!(error.unwrap_err().as_database_error().unwrap().is_unique_violation());
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT token_address,balance FROM api_assets")
            .fetch_all(&mut conn)
            .await
            .unwrap();
    assert_eq!(rows, vec![("0xabc".into(), "6".into())]);
}

#[test]
fn evm_token_identity_migration_matches_supported_routes() {
    for chain in crate::entities::api_withdraw::signed_evm_chain_codes() {
        assert!(GUARD.contains(&format!("'{}'", chain)), "migration misses {chain}");
    }
}

#[tokio::test]
async fn evm_token_identity_native_and_non_evm_writes_preserve_identity() {
    let mut conn = db_before_identity_guard().await;
    sqlx::raw_sql(GUARD).execute(&mut conn).await.unwrap();
    for migration in
        Migrator::ApiWallet.migrator().unwrap().iter().filter(|m| m.version > 20261013000000)
    {
        sqlx::raw_sql(&migration.sql).execute(&mut conn).await.unwrap();
    }
    for (chain, token) in [("base", ""), ("tron", "TAbC"), ("tron", "Tabc")] {
        ApiCoinDao::upsert_multi_coin(&mut conn, vec![coin(chain, token)]).await.unwrap();
        ApiAssetsDao::upsert_assets_multi(
            &mut conn,
            vec![asset("address", chain, "USDT", token, "3")],
        )
        .await
        .unwrap();
    }
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT chain_code,token_address FROM api_assets ORDER BY chain_code,token_address",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            ("base".into(), "".into()),
            ("tron".into(), "TAbC".into()),
            ("tron".into(), "Tabc".into())
        ]
    );
}

#[tokio::test]
async fn evm_token_identity_summary_and_checksum_queries_agree_after_canonical_writes() {
    let mut conn = db_before_identity_guard().await;
    seed(&mut conn, "base", "0xabc", "6", "2026-10-08T00:00:00Z").await;
    sqlx::raw_sql("INSERT INTO api_chain (name,chain_code,protocols,status,main_symbol,created_at) VALUES ('BASE','base','[]',1,'ETH',CURRENT_TIMESTAMP); INSERT INTO api_account (account_id,name,address,wallet_address,chain_code,api_wallet_type,status,is_init,created_at) VALUES (1,'account','address','wallet','base',1,1,1,CURRENT_TIMESTAMP);").execute(&mut conn).await.unwrap();
    sqlx::raw_sql(GUARD).execute(&mut conn).await.unwrap();
    // Current read APIs require the current read-model schema. Other guard tests
    // intentionally remain on the historical migration boundary.
    for migration in Migrator::ApiWallet.migrator().unwrap().iter() {
        if migration.version > 20261013000000 {
            sqlx::raw_sql(&migration.sql).execute(&mut conn).await.unwrap();
        }
    }

    ApiCoinDao::upsert_multi_coin(&mut conn, vec![coin("base", "0xAbC")]).await.unwrap();
    ApiAssetsDao::upsert_assets_multi_update_balance(
        &mut conn,
        vec![asset("address", "base", "USDT", "0xABC", "6")],
    )
    .await
    .unwrap();
    let summary = ApiAssetsDao::get_api_wallet_assets_v2(&mut conn, "wallet", Some(1), None, true)
        .await
        .unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].total_coins_quantity, 6.0);
    let details = ApiAssetsDao::get_api_assets_by_address(
        &mut conn,
        vec!["address".into()],
        Some("base".into()),
        Some("USDT"),
        Some("0xABC"),
        None,
    )
    .await
    .unwrap();
    assert_eq!(details.len(), 1);
    assert_eq!(details[0].balance, "6");
    ApiCoinDao::update_price_unit1(&mut conn, "base", "0xABC", "2").await.unwrap();
    let coin = ApiCoinDao::get_coin_by_chain_code_token_address(
        &mut conn,
        "base",
        AssetTokenKey::from_raw(Some("0xAbC")),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(coin.price, "2");
    ApiAssetsDao::delete_assets(&mut conn, "address", "base", "0xABC").await.unwrap();
    let status: i64 =
        sqlx::query_scalar("SELECT status FROM api_assets").fetch_one(&mut conn).await.unwrap();
    assert_eq!(status, 0);
}

#[tokio::test]
async fn evm_token_identity_batch_coin_upsert_returns_one_identity() {
    let mut conn = db_before_identity_guard().await;
    sqlx::raw_sql(GUARD).execute(&mut conn).await.unwrap();
    let rows = ApiCoinDao::upsert_multi_coin(
        &mut conn,
        vec![coin("base", "0xAbC"), coin("base", "0xabc")],
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].token_address.as_db_str(), "0xabc");
    let pairs =
        ApiCoinDao::list_by_chain_token_pairs_batch(&mut conn, &[("base".into(), "0xABC".into())])
            .await
            .unwrap();
    assert_eq!(pairs.len(), 1);
}

#[tokio::test]
async fn evm_token_identity_schema_guard_preserves_existing_sync_state() {
    let mut conn = db_before_identity_guard().await;
    seed(&mut conn, "base", "0xabc", "6", "2026-10-08T00:00:00Z").await;
    sqlx::raw_sql("INSERT INTO api_asset_sync_state(address,chain_code,token_address,status,attempt_count,last_error,created_at,updated_at) VALUES ('address','base','0xAbC',3,4,'retry','2026-10-08T00:00:00Z','2026-10-12T00:00:00Z')").execute(&mut conn).await.unwrap();
    let before: Vec<(String, i64, i64, String, String)> = sqlx::query_as(
        "SELECT token_address,status,attempt_count,last_error,updated_at FROM api_asset_sync_state",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    sqlx::raw_sql(GUARD).execute(&mut conn).await.unwrap();
    let after: Vec<(String, i64, i64, String, String)> = sqlx::query_as(
        "SELECT token_address,status,attempt_count,last_error,updated_at FROM api_asset_sync_state",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(after, before);
    let snapshot: Vec<(String, String, String)> =
        sqlx::query_as("SELECT token_address,balance,updated_at FROM api_assets")
            .fetch_all(&mut conn)
            .await
            .unwrap();
    assert_eq!(snapshot, vec![("0xabc".into(), "6".into(), "2026-10-08T00:00:00Z".into())]);
    // Index-only migration can be rerun without touching persisted state.
    sqlx::raw_sql(GUARD).execute(&mut conn).await.unwrap();
    let after: Vec<(String, i64, i64, String, String)> = sqlx::query_as(
        "SELECT token_address,status,attempt_count,last_error,updated_at FROM api_asset_sync_state",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn evm_token_identity_migration_leaves_existing_contract_keys_untouched() {
    let mut conn = db_before_identity_guard().await;
    seed(&mut conn, "eth", "0xAbC", "6", "2026-10-08T00:00:00Z").await;
    seed(&mut conn, "tron", "TAbC", "3", "2026-10-08T00:00:00Z").await;
    sqlx::raw_sql(GUARD).execute(&mut conn).await.unwrap();
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT chain_code,token_address,balance FROM api_assets ORDER BY chain_code",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            ("eth".into(), "0xAbC".into(), "6".into()),
            ("tron".into(), "TAbC".into(), "3".into())
        ]
    );
    let coins: Vec<(String, String)> =
        sqlx::query_as("SELECT chain_code,token_address FROM api_coin ORDER BY chain_code")
            .fetch_all(&mut conn)
            .await
            .unwrap();
    assert_eq!(coins, vec![("eth".into(), "0xAbC".into()), ("tron".into(), "TAbC".into())]);
}
