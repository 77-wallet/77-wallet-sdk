//! Opt-in offline component probe: real migrated SQLite, public repos and synthetic data only.
use super::ApiCoinRepo;
use crate::{
    entities::{api_coin::ApiCoinData, api_wallet::ApiWalletType},
    repositories::{
        api_wallet::wallet::ApiWalletRepo,
        test_helper::setup_api_wallet_pool_before_label_removal as setup_api_wallet_pool,
    },
};
use std::time::Instant;

fn catalog(name: &str, symbol: &str) -> Vec<ApiCoinData> {
    (0..512)
        .map(|i| {
            let native = i == 0;
            ApiCoinData::new(
                Some(if native { name.into() } else { format!("Token{i}") }),
                &if native { symbol.into() } else { format!("T{i}") },
                "eth",
                if native { "".into() } else { format!("0x{i:040x}").into() },
                Some("1".into()),
                None,
                18,
                1,
                0,
                1,
                chrono::Utc::now(),
                None,
            )
        })
        .collect()
}

type Snapshot = (i64, i64, i64, i64, i64, i64, i64);
async fn snapshot(pool: &crate::ApiWalletDbPool) -> Snapshot {
    sqlx::query_as("SELECT count(*),count(DISTINCT address),sum(CAST(balance AS INTEGER)),sum(status),sum(decimals),sum(is_multisig),sum(created_at!='2026-01-01T00:00:00Z' OR updated_at!='2026-01-02T00:00:00Z') FROM api_assets")
        .fetch_one(pool.read_ref()).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "500k-address offline scale probe; run explicitly and inspect recorded timings"]
async fn api_coin_metadata_scale_500k() {
    let pool = setup_api_wallet_pool("coin_metadata_scale_500k").await;
    let engine: String =
        sqlx::query_scalar("SELECT sqlite_version()").fetch_one(pool.read_ref()).await.unwrap();
    ApiCoinRepo::upsert_multi_coin(&pool, catalog("Ether", "ETH")).await.unwrap();
    ApiWalletRepo::upsert(
        &pool,
        "synthetic-uid",
        "fixture",
        "fixture-wallet",
        b"synthetic",
        b"synthetic",
        ApiWalletType::SubAccount,
        None,
        "synthetic-sn",
        0,
    )
    .await
    .unwrap();
    let seed = Instant::now();
    sqlx::raw_sql("WITH RECURSIVE seq(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM seq WHERE i<499999)
        INSERT INTO api_account(account_id,name,address,address_type,wallet_address,derivation_path,derivation_path_index,chain_code,uid,api_wallet_type,status,is_init,created_at)
        SELECT i,'synthetic','synthetic-'||i,'','fixture-wallet','synthetic',i,'eth','synthetic-uid',1,1,1,'2026-01-01T00:00:00Z' FROM seq;
        INSERT INTO api_assets(name,symbol,decimals,address,token_address,chain_code,status,balance,is_multisig,created_at,updated_at)
        SELECT 'Ether','ETH',18,address,'','eth',account_id%2,CAST(account_id%7 AS TEXT),account_id%3,'2026-01-01T00:00:00Z','2026-01-02T00:00:00Z' FROM api_account;
        ANALYZE;")
        .execute(pool.write_ref()).await.unwrap();
    let before = snapshot(&pool).await;
    assert_eq!((before.0, before.1), (500000, 500000));
    assert_eq!(before.2, 1499994);
    assert_eq!(before.6, 0);
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM api_account")
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(accounts, 500000);
    println!(
        "SCALE {}",
        serde_json::json!({"event":"seed","sqlite":engine,"accounts":accounts,"assets":before.0,"catalog":512,"seed_ms":seed.elapsed().as_secs_f64()*1000.0})
    );
    let has_index: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE name='api_assets_metadata_nocase_idx'",
    )
    .fetch_one(pool.read_ref())
    .await
    .unwrap();
    if has_index == 1 {
        // Measure an existing populated DB upgrade, not only empty-schema index creation.
        sqlx::raw_sql("DROP VIEW IF EXISTS api_assets_with_coin_labels;DELETE FROM _sqlx_migrations WHERE version=20261105010000;DROP INDEX api_assets_metadata_nocase_idx;DROP INDEX api_coin_metadata_nocase_idx;DELETE FROM _sqlx_migrations WHERE version=20261105000000;")
            .execute(pool.write_ref()).await.unwrap();
        let migration = Instant::now();
        let mut migrator = crate::init::Migrator::ApiWallet.migrator().unwrap();
        migrator.migrations = std::borrow::Cow::Owned(
            migrator.iter().filter(|m| m.version < 20261105020000).cloned().collect(),
        );
        migrator.run(pool.write_ref()).await.unwrap();
        println!(
            "SCALE {}",
            serde_json::json!({"event":"populated_upgrade","elapsed_ms":migration.elapsed().as_secs_f64()*1000.0})
        );
        assert_eq!(snapshot(&pool).await, before);
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    pool.set_writer_diagnostics(tx);
    let run_pool = pool.clone();
    let started = Instant::now();
    let bulk = tokio::spawn(async move {
        ApiCoinRepo::upsert_multi_coin(&run_pool, catalog("Ether", "ETH")).await.unwrap().len()
    });
    loop {
        let event = rx.recv().await.unwrap();
        if event.op == "api_coin_metadata" && event.event == "acquired" {
            break;
        }
    }
    let queued_pool = pool.clone();
    let queued = tokio::spawn(async move {
        ApiWalletRepo::upsert_with_timing(
            &queued_pool,
            "probe-uid",
            "probe",
            "probe-wallet",
            b"synthetic",
            b"synthetic",
            ApiWalletType::SubAccount,
            None,
            "synthetic-sn",
            0,
        )
        .await
    });
    assert_eq!(bulk.await.unwrap(), 512);
    let bulk_ms = started.elapsed().as_secs_f64() * 1000.0;
    let (wallet, timing) = queued.await.unwrap();
    assert_eq!(wallet.unwrap().address, "probe-wallet");
    assert_eq!(snapshot(&pool).await, before);
    let drift: i64 =
        sqlx::query_scalar("SELECT count(*) FROM api_assets WHERE name!='Ether' OR symbol!='ETH'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(drift, 0);
    println!(
        "SCALE {}",
        serde_json::json!({"event":"unchanged_catalog_repo","elapsed_ms":bulk_ms,"wallet_writer_wait_ms":timing.writer_wait_ms,"wallet_write_action_ms":timing.write_action_ms,"indexes":has_index==1})
    );
    if std::env::var_os("METADATA_SCALE_BASELINE").is_some() {
        return;
    }
    sqlx::raw_sql("CREATE TRIGGER forbid_catalog_fanout BEFORE UPDATE OF name,symbol ON api_assets BEGIN SELECT RAISE(ABORT,'catalog must not fan out into asset labels'); END;")
        .execute(pool.write_ref()).await.unwrap();
    let rename = Instant::now();
    ApiCoinRepo::upsert_multi_coin(&pool, catalog("Renamed Ether", "ETH2")).await.unwrap();
    let rename_ms = rename.elapsed().as_secs_f64() * 1000.0;
    let renamed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM api_assets_with_coin_labels WHERE name='Renamed Ether' AND symbol='ETH2'",
    )
    .fetch_one(pool.read_ref())
    .await
    .unwrap();
    assert_eq!(renamed, 500000);
    let cached: i64 =
        sqlx::query_scalar("SELECT count(*) FROM api_assets WHERE name='Ether' AND symbol='ETH'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(cached, 500000);
    assert_eq!(snapshot(&pool).await, before);
    let replay = Instant::now();
    ApiCoinRepo::upsert_multi_coin(&pool, catalog("Renamed Ether", "ETH2")).await.unwrap();
    let replay_ms = replay.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(snapshot(&pool).await, before);
    println!(
        "SCALE {}",
        serde_json::json!({"event":"catalog_rename_and_replay","canonical_label_rows":renamed,"untouched_compat_rows":cached,"rename_ms":rename_ms,"replay_ms":replay_ms,"preserved":"balances,status,precision,multisig,timestamps,address count"})
    );
}
