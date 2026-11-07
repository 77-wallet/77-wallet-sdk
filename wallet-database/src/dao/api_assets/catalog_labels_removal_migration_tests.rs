use crate::repositories::test_helper::setup_api_wallet_pool_before_label_removal;

#[tokio::test]
async fn label_removal_upgrade_backfills_missing_identities_without_overwriting_catalog() {
    let pool = setup_api_wallet_pool_before_label_removal("label_removal_upgrade").await;
    sqlx::raw_sql("INSERT INTO api_coin(name,symbol,chain_code,token_address,price,decimals,status,created_at,updated_at) VALUES('Canonical','CAN','eth','0xAbC','7',18,0,'2026-01-01T00:00:00Z','2026-01-02T00:00:00Z');
        INSERT INTO api_assets(name,symbol,decimals,address,token_address,protocol,chain_code,status,balance,is_multisig,created_at,updated_at) VALUES
        ('Cached','OLD',6,'owner','0xabc','asset-protocol','eth',0,'4',2,'2026-01-01T00:00:00Z','2026-01-02T00:00:00Z'),
        ('Native','ETH',18,'native','','native-protocol','eth',1,'8',0,'2026-01-01T00:00:00Z',NULL),
        ('Upper','UP',9,'upper','AbC',NULL,'sol',1,'2',0,'2026-01-01T00:00:00Z',NULL),
        ('Lower','LOW',6,'lower','abc',NULL,'sol',0,'3',0,'2026-01-01T00:00:00Z',NULL);
        CREATE TABLE expected_asset_fields AS SELECT id,decimals,address,token_address,protocol,chain_code,status,is_multisig,balance,created_at,updated_at FROM api_assets;")
        .execute(pool.write_ref()).await.unwrap();
    let catalog_before: (String, String, String, i64, i64, String, String) = sqlx::query_as(
        "SELECT name,symbol,price,decimals,status,created_at,updated_at FROM api_coin",
    )
    .fetch_one(pool.read_ref())
    .await
    .unwrap();
    crate::init::Migrator::ApiWallet.migrator().unwrap().run(pool.write_ref()).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM pragma_table_info('api_assets') WHERE name IN ('name','symbol')"
        )
        .fetch_one(pool.read_ref())
        .await
        .unwrap(),
        0
    );
    let catalog_after: (String,String,String,i64,i64,String,String) = sqlx::query_as("SELECT name,symbol,price,decimals,status,created_at,updated_at FROM api_coin WHERE token_address='0xAbC'").fetch_one(pool.read_ref()).await.unwrap();
    assert_eq!(catalog_after, catalog_before);
    let inserted: Vec<(String,String,String,i64,Option<String>,String)> = sqlx::query_as("SELECT token_address,name,symbol,decimals,protocol,price FROM api_coin WHERE NOT(token_address='0xAbC' AND chain_code='eth') ORDER BY token_address").fetch_all(pool.read_ref()).await.unwrap();
    assert_eq!(
        inserted,
        vec![
            (
                "".into(),
                "Native".into(),
                "ETH".into(),
                18,
                Some("native-protocol".into()),
                "0".into()
            ),
            ("AbC".into(), "Upper".into(), "UP".into(), 9, None, "0".into()),
            ("abc".into(), "Lower".into(), "LOW".into(), 6, None, "0".into())
        ]
    );
    for (a, b) in [("api_assets", "expected_asset_fields"), ("expected_asset_fields", "api_assets")]
    {
        let sql = format!(
            "SELECT count(*) FROM (SELECT id,decimals,address,token_address,protocol,chain_code,status,is_multisig,balance,created_at,updated_at FROM {a} EXCEPT SELECT * FROM {b})"
        );
        assert_eq!(sqlx::query_scalar::<_, i64>(&sql).fetch_one(pool.read_ref()).await.unwrap(), 0);
    }
    let labels: Vec<(String, String)> = sqlx::query_as(
        "SELECT name,symbol FROM api_assets_with_coin_labels WHERE address='native'",
    )
    .fetch_all(pool.read_ref())
    .await
    .unwrap();
    assert_eq!(labels, vec![("Native".into(), "ETH".into())]);
    let last = crate::dao::api_coin::ApiCoinDao::get_last_coin(pool.read_ref(), true)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(last.symbol, "CAN", "synthetic metadata cannot advance backend catalog watermark");
    crate::init::Migrator::ApiWallet.migrator().unwrap().run(pool.write_ref()).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM api_coin")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn label_removal_backfill_failure_rolls_back_and_retry_succeeds() {
    let pool = setup_api_wallet_pool_before_label_removal("label_removal_rollback").await;
    sqlx::raw_sql("INSERT INTO api_assets(name,symbol,decimals,address,token_address,chain_code,status,balance,created_at) VALUES('Legacy','LEG',6,'owner','0xabc','eth',1,'7','2026-01-01T00:00:00Z');
        CREATE TRIGGER reject_backfill BEFORE INSERT ON api_coin BEGIN SELECT RAISE(ABORT,'injected backfill failure'); END;")
        .execute(pool.write_ref()).await.unwrap();
    let result = crate::init::Migrator::ApiWallet.migrator().unwrap().run(pool.write_ref()).await;
    assert!(result.is_err(), "migration must attempt missing-catalog backfill");
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM api_assets")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        "Legacy"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM api_coin")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        0
    );
    sqlx::query("DROP TRIGGER reject_backfill").execute(pool.write_ref()).await.unwrap();
    crate::init::Migrator::ApiWallet.migrator().unwrap().run(pool.write_ref()).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT symbol FROM api_coin")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        "LEG"
    );
}

#[tokio::test]
async fn label_removal_backfill_newest_snapshot_and_id_tiebreak_are_deterministic() {
    let pool = setup_api_wallet_pool_before_label_removal("label_removal_conflicts").await;
    sqlx::raw_sql("INSERT INTO api_assets(name,symbol,decimals,address,token_address,protocol,chain_code,status,balance,created_at,updated_at) VALUES
        ('Older','OLD',6,'a','0xAbC','old','eth',1,'1','2026-01-01T00:00:00Z','2026-01-02T00:00:00Z'),
        ('Newer','NEW',9,'b','0xabc','new','eth',0,'2','2026-01-01T00:00:00Z','2026-01-03T00:00:00Z'),
        ('Tie winner','WIN',8,'c','0xABC','winner','eth',0,'3','2026-01-01T00:00:00Z','2026-01-03T00:00:00Z');")
        .execute(pool.write_ref()).await.unwrap();
    crate::init::Migrator::ApiWallet.migrator().unwrap().run(pool.write_ref()).await.unwrap();
    let rows: Vec<(String, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("SELECT token_address,name,symbol,decimals,protocol,status FROM api_coin")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![("0xabc".into(), "Tie winner".into(), "WIN".into(), 8, Some("winner".into()), 1)]
    );
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM api_assets_with_coin_labels WHERE name='Tie winner' AND symbol='WIN'").fetch_one(pool.read_ref()).await.unwrap(),3);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT sum(status) FROM api_assets")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn label_removal_late_ddl_failure_rolls_back_backfill_columns_and_migration_marker() {
    let pool = setup_api_wallet_pool_before_label_removal("label_removal_late_failure").await;
    sqlx::raw_sql("INSERT INTO api_assets(name,symbol,decimals,address,token_address,chain_code,status,balance,created_at) VALUES('Legacy','LEG',6,'owner','0xabc','eth',1,'7','2026-01-01T00:00:00Z');
        CREATE VIEW injected_dependency AS SELECT name FROM api_assets;")
        .execute(pool.write_ref()).await.unwrap();
    let error = crate::init::Migrator::ApiWallet
        .migrator()
        .unwrap()
        .run(pool.write_ref())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("injected_dependency"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM api_coin")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM _sqlx_migrations WHERE version=20261105020000"
        )
        .fetch_one(pool.read_ref())
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT symbol FROM api_assets_with_coin_labels")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        "LEG"
    );
    sqlx::query("DROP VIEW injected_dependency").execute(pool.write_ref()).await.unwrap();
    crate::init::Migrator::ApiWallet.migrator().unwrap().run(pool.write_ref()).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT symbol FROM api_coin")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        "LEG"
    );
}

#[tokio::test]
async fn label_removal_newest_timestamp_retains_submillisecond_precision_and_offset_equivalence() {
    let pool = setup_api_wallet_pool_before_label_removal("label_removal_precise_time").await;
    sqlx::raw_sql("INSERT INTO api_assets(name,symbol,decimals,address,token_address,chain_code,status,balance,created_at,updated_at) VALUES
        ('Newer','NEW',6,'new','micro','sol',1,'1','2026-01-01T00:00:00Z','2026-11-05T10:00:00.000400Z'),
        ('Older larger id','OLD',6,'old','micro','sol',1,'1','2026-01-01T00:00:00Z','2026-11-05T10:00:00.000100Z'),
        ('Fraction later','FRA',6,'frac','fraction','sol',1,'1','2026-01-01T00:00:00Z','2026-11-05T10:00:00.1Z'),
        ('Whole earlier','WHO',6,'whole','fraction','sol',1,'1','2026-01-01T00:00:00Z','2026-11-05T10:00:00Z'),
        ('UTC','UTC',6,'utc','offset','sol',1,'1','2026-01-01T00:00:00Z','2026-11-05T10:00:00.100Z'),
        ('Equivalent higher id','EQ',6,'offset','offset','sol',1,'1','2026-01-01T00:00:00Z','2026-11-05T18:00:00.1+08:00');")
        .execute(pool.write_ref()).await.unwrap();
    crate::init::Migrator::ApiWallet.migrator().unwrap().run(pool.write_ref()).await.unwrap();
    let symbols: Vec<String> =
        sqlx::query_scalar("SELECT symbol FROM api_coin ORDER BY token_address")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(symbols, vec!["FRA", "NEW", "EQ"]);
}

#[tokio::test]
#[ignore = "requires WALLET_LABEL_REMOVAL_OLD_DB; upgrades an isolated copy only"]
async fn label_removal_saved_old_database_copy_upgrade_preserves_every_asset_field() {
    use sqlx::Connection;
    let source =
        std::env::var("WALLET_LABEL_REMOVAL_OLD_DB").expect("explicit old-backup path required");
    let dir = crate::repositories::test_helper::make_temp_dir("label_removal_old_backup");
    let copy = std::path::Path::new(&dir).join("api_wallet.db");
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&source).read_only(true);
    let mut original = sqlx::SqliteConnection::connect_with(&options).await.unwrap();
    // VACUUM INTO takes a consistent snapshot, including any committed WAL data.
    sqlx::query("VACUUM INTO ?").bind(copy.to_str().unwrap()).execute(&mut original).await.unwrap();
    original.close().await.unwrap();
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&copy))
        .await
        .unwrap();
    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM api_assets").fetch_one(&pool).await.unwrap();
    let addresses: i64 = sqlx::query_scalar("SELECT count(DISTINCT address) FROM api_account")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE retained_field_snapshot AS SELECT id,decimals,address,token_address,protocol,chain_code,status,is_multisig,balance,created_at,updated_at FROM api_assets").execute(&pool).await.unwrap();
    let started = std::time::Instant::now();
    crate::init::Migrator::ApiWallet.migrator().unwrap().run(&pool).await.unwrap();
    let upgrade_ms = started.elapsed().as_secs_f64() * 1000.0;
    let fields = "id,decimals,address,token_address,protocol,chain_code,status,is_multisig,balance,created_at,updated_at";
    for (a, b) in
        [("api_assets", "retained_field_snapshot"), ("retained_field_snapshot", "api_assets")]
    {
        let sql = format!(
            "SELECT count(*) FROM (SELECT {fields} FROM {a} EXCEPT SELECT {fields} FROM {b})"
        );
        assert_eq!(sqlx::query_scalar::<_, i64>(&sql).fetch_one(&pool).await.unwrap(), 0);
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM api_assets")
            .fetch_one(&pool)
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM pragma_table_info('api_assets') WHERE name IN ('name','symbol')"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM api_assets_with_coin_labels")
            .fetch_one(&pool)
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check").fetch_one(&pool).await.unwrap(),
        "ok"
    );
    pool.close().await;
    println!(
        "OLD_BACKUP_UPGRADE {}",
        serde_json::json!({"addresses":addresses,"assets":before,"upgrade_ms":upgrade_ms,"retained_field_mismatches":0,"integrity":"ok"})
    );
}
