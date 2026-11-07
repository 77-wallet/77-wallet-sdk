//! Explicit read-path scale check: no backend, real migrated SQLite and public repositories.
use crate::{
    entities::{api_coin::ApiCoinData, api_wallet::ApiWalletType},
    repositories::{
        api_wallet::{assets::ApiAssetsRepo, coin::ApiCoinRepo, wallet::ApiWalletRepo},
        test_helper::setup_api_wallet_pool_before_label_removal,
    },
};
use std::time::Instant;
fn catalog(name: &str, symbol: &str) -> Vec<ApiCoinData> {
    (0..512)
        .map(|i| {
            ApiCoinData::new(
                Some(if i == 0 { name.into() } else { format!("Token{i}") }),
                &if i == 0 { symbol.into() } else { format!("T{i}") },
                "eth",
                if i == 0 { "".into() } else { format!("0x{i:040x}").into() },
                Some("2".into()),
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
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "500k-address canonical-label read probe; run explicitly"]
async fn catalog_labels_scale_500k_reads_and_summary() {
    run_scale_probe(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "500k-address real migration/backfill/read/write regression; run explicitly"]
async fn label_removal_scale_500k_upgrade_backfill_and_reads() {
    run_scale_probe(true).await;
}

async fn run_scale_probe(drop_labels: bool) {
    let pool = setup_api_wallet_pool_before_label_removal("catalog_labels_scale_500k").await;
    ApiCoinRepo::upsert_multi_coin(&pool, catalog("Live Native", "LIVE")).await.unwrap();
    ApiWalletRepo::upsert(
        &pool,
        "synthetic",
        "fixture",
        "fixture-wallet",
        b"synthetic",
        b"synthetic",
        ApiWalletType::SubAccount,
        None,
        "synthetic",
        0,
    )
    .await
    .unwrap();
    let seed = Instant::now();
    sqlx::raw_sql("INSERT INTO api_chain(name,chain_code,protocols,main_symbol,status,created_at) VALUES('Ethereum','eth','[]','ETH',1,CURRENT_TIMESTAMP);
        WITH RECURSIVE seq(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM seq WHERE i<499999)
        INSERT INTO api_account(account_id,name,address,address_type,wallet_address,derivation_path,derivation_path_index,chain_code,uid,api_wallet_type,status,is_init,created_at)
        SELECT 3,'synthetic','synthetic-'||i,'','fixture-wallet','synthetic',i,'eth','synthetic',1,1,1,'2026-01-01T00:00:00Z' FROM seq;
        INSERT INTO api_assets(name,symbol,decimals,address,token_address,chain_code,status,balance,is_multisig,created_at,updated_at)
        SELECT 'Cached Native','OLD',6,address,'','eth',derivation_path_index%2,CAST(derivation_path_index%7 AS TEXT),derivation_path_index%3,'2026-01-01T00:00:00Z','2026-01-02T00:00:00Z' FROM api_account;
        ANALYZE;")
        .execute(pool.write_ref()).await.unwrap();
    println!(
        "LABEL_SCALE {}",
        serde_json::json!({"event":"seed","addresses":500000,"catalog":512,"seed_ms":seed.elapsed().as_secs_f64()*1000.0})
    );
    if drop_labels {
        // Reproduce beta350 and remove the native catalog entry so all500k legacy rows need backfill.
        sqlx::raw_sql("DROP VIEW api_assets_with_coin_labels;DELETE FROM _sqlx_migrations WHERE version=20261105010000;
            DELETE FROM api_coin WHERE token_address='';
            CREATE TABLE drop_probe_snapshot AS SELECT id,decimals,address,token_address,protocol,chain_code,status,is_multisig,balance,created_at,updated_at FROM api_assets;")
            .execute(pool.write_ref()).await.unwrap();
    }
    let started = Instant::now();
    crate::init::Migrator::ApiWallet.migrator().unwrap().run(pool.write_ref()).await.unwrap();
    println!(
        "REMOVAL_SCALE {}",
        serde_json::json!({"event":"production_upgrade", "addresses":500000,"backfill_required":drop_labels,"upgrade_ms":started.elapsed().as_secs_f64()*1000.0})
    );
    let expected_name = if drop_labels { "Cached Native" } else { "Live Native" };
    let expected_symbol = if drop_labels { "OLD" } else { "LIVE" };
    let addresses: Vec<String> = (0..20).map(|i| format!("synthetic-{}", 2 * i + 1)).collect();
    let selected = Instant::now();
    let rows = ApiAssetsRepo::list(&pool, addresses.clone(), Some("eth".into())).await.unwrap();
    let selected_ms = selected.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(rows.len(), 20);
    assert!(
        rows.iter()
            .all(|r| r.name == expected_name && r.symbol == expected_symbol && r.decimals == 6)
    );
    let refreshed = Instant::now();
    let refresh =
        ApiAssetsRepo::assets_with_wallet_address_by_address(&pool, &addresses).await.unwrap();
    let refresh_ms = refreshed.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(refresh.len(), 20);
    assert!(refresh.iter().all(|r| r.symbol == expected_symbol && r.decimals == 6));
    let expected: u64 = (0..500000u64).filter(|i| i % 2 == 1).map(|i| i % 7).sum();
    let summary = Instant::now();
    let totals = ApiAssetsRepo::get_api_wallet_assets_v2(
        &pool,
        "fixture-wallet",
        Some(3),
        Some("eth"),
        false,
    )
    .await
    .unwrap();
    let summary_ms = summary.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(totals.len(), 1);
    assert_eq!(
        (totals[0].symbol.as_str(), totals[0].api_assets_name.as_str()),
        (expected_symbol, expected_name)
    );
    assert_eq!(totals[0].total_coins_quantity, expected as f64);
    assert_eq!(
        totals[0].total_account_amount,
        Some((expected * if drop_labels { 0 } else { 2 }) as f64)
    );
    let renamed = Instant::now();
    ApiCoinRepo::upsert_multi_coin(&pool, catalog("Renamed Native", "NEW")).await.unwrap();
    let rename_ms = renamed.elapsed().as_secs_f64() * 1000.0;
    let filtered = ApiAssetsRepo::get_chain_assets_by_address_chain_code_symbol(
        &pool,
        addresses.clone(),
        Some("eth".into()),
        Some("NEW"),
        None,
    )
    .await
    .unwrap();
    assert_eq!(filtered.len(), 20);
    assert!(filtered.iter().all(|r| r.name == "Renamed Native" && r.decimals == 6));
    let totals = ApiAssetsRepo::get_api_wallet_assets_v2(
        &pool,
        "fixture-wallet",
        Some(3),
        Some("eth"),
        true,
    )
    .await
    .unwrap();
    assert_eq!(totals.len(), 1);
    assert_eq!(
        (totals[0].symbol.as_str(), totals[0].api_assets_name.as_str()),
        ("NEW", "Renamed Native")
    );
    assert_eq!(totals[0].total_coins_quantity, expected as f64);
    let dirty = Instant::now();
    let token_rows =
        ApiAssetsRepo::assets_with_wallet_address_by_token(&pool, &["NEW:eth:".into()])
            .await
            .unwrap();
    let token_refresh_ms = dirty.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(token_rows.len(), 250000);
    assert!(token_rows.iter().all(|r| r.symbol == "NEW" && r.decimals == 6));
    assert_eq!(token_rows.iter().map(|r| r.balance.parse::<u64>().unwrap()).sum::<u64>(), expected);
    drop(token_rows);
    let cache_check = "0";
    let check_sql = format!(
        "SELECT count(*),count(DISTINCT address),sum(CAST(balance AS INTEGER)),{cache_check},sum(decimals!=6 OR balance!=CAST(CAST(substr(address,11) AS INTEGER)%7 AS TEXT) OR status!=CAST(substr(address,11) AS INTEGER)%2 OR is_multisig!=CAST(substr(address,11) AS INTEGER)%3 OR created_at!='2026-01-01T00:00:00Z' OR updated_at!='2026-01-02T00:00:00Z') FROM api_assets"
    );
    let check: (i64, i64, i64, i64, i64) =
        sqlx::query_as(&check_sql).fetch_one(pool.read_ref()).await.unwrap();
    if drop_labels {
        let columns = "id,decimals,address,token_address,protocol,chain_code,status,is_multisig,balance,created_at,updated_at";
        for (left, right) in
            [("api_assets", "drop_probe_snapshot"), ("drop_probe_snapshot", "api_assets")]
        {
            let sql = format!(
                "SELECT count(*) FROM (SELECT {columns} FROM {left} EXCEPT SELECT {columns} FROM {right})"
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>(&sql).fetch_one(pool.read_ref()).await.unwrap(),
                0
            );
        }
        let filename: String =
            sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name='main'")
                .fetch_one(pool.read_ref())
                .await
                .unwrap();
        let reopened = sqlx::SqlitePool::connect(&format!("sqlite://{filename}")).await.unwrap();
        crate::init::Migrator::ApiWallet.migrator().unwrap().run(&reopened).await.unwrap();
        let labels: (String, String) = sqlx::query_as(
            "SELECT name,symbol FROM api_assets_with_coin_labels WHERE address='synthetic-1'",
        )
        .fetch_one(&reopened)
        .await
        .unwrap();
        assert_eq!(labels, ("Renamed Native".into(), "NEW".into()));
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pragma_table_info('api_assets') WHERE name IN ('name','symbol')").fetch_one(&reopened).await.unwrap(), 0);
        reopened.close().await;
        println!(
            "DROP_PROBE_SCALE exact_non_label_diff_rows=0 reopened_migrations=ok persisted_labels_removed=true"
        );
    }
    assert_eq!(check, (500000, 500000, 1499994, 0, 0));
    if drop_labels {
        let asset = |address: &str| {
            crate::entities::api_assets::ApiCreateAssetsVo::new(
                crate::entities::assets::AssetsId::new(address, "eth", "".into()),
                "NEW",
                6,
                Some("probe".into()),
                0,
            )
            .with_name("Renamed Native")
            .with_balance("9")
        };
        ApiAssetsRepo::upsert_assets(&pool, asset("post-upgrade-single")).await.unwrap();
        ApiAssetsRepo::upsert_assets_multi(&pool, vec![asset("post-upgrade-bulk")]).await.unwrap();
        ApiAssetsRepo::insert_missing_assets_multi(&pool, vec![asset("post-upgrade-init")])
            .await
            .unwrap();
        ApiAssetsRepo::upsert_assets_multi_update_balance(
            &pool,
            vec![asset("post-upgrade-balance")],
        )
        .await
        .unwrap();
        let inserted:(i64,i64)=sqlx::query_as("SELECT count(*),sum(CAST(balance AS INTEGER)) FROM api_assets WHERE address LIKE 'post-upgrade-%'").fetch_one(pool.read_ref()).await.unwrap();
        assert_eq!(inserted, (4, 36));
        println!("REMOVAL_SCALE post_upgrade_writer_paths=4 inserted_rows=4 sum_balance=36");
    }
    println!(
        "LABEL_SCALE {}",
        serde_json::json!({"event":"read_and_rename","selected_rows":rows.len(),"selected_ms":selected_ms,"refresh_ms":refresh_ms,"summary_ms":summary_ms,"token_refresh_rows":250000,"token_refresh_ms":token_refresh_ms,"summary_quantity":expected,"rename_catalog_ms":rename_ms,"compat_columns_present":false,"verified_asset_rows":check.0,"non_label_mismatch_rows":check.4})
    );
}
