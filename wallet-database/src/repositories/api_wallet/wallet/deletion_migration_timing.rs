//! Manual populated-database upgrade characterization; not a CI latency assertion.
use super::super::ApiWalletDbPool;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "manual 500k-address migration and contention characterization"]
async fn api_wallet_deletion_populated_migration_timing() {
    const WALLET: &str = "0x0000000000000000000000000000000000000001";
    const UID: &str = "migration-wallet-uid-00000000000001";
    let dir = crate::repositories::test_helper::make_temp_dir("deletion_migration_500k");
    let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
    let pool: ApiWalletDbPool = ctx.into_api_wallet_db_pool().unwrap();
    sqlx::query("DROP INDEX api_account_wallet_id_idx").execute(pool.write_ref()).await.unwrap();
    sqlx::query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<500000) INSERT INTO api_account(account_id,name,address,wallet_address,uid,chain_code,api_wallet_type,status,is_init,created_at) SELECT x,'migration',printf('migration-%040d',x),?,?,'base',1,x%2,0,'2026-11-02T00:00:00Z' FROM n")
        .bind(WALLET).bind(UID).execute(pool.write_ref()).await.unwrap();
    for (phase, sql, columns) in [
        (
            "add_wide",
            include_str!(
                "../../../../schema/api_wallet/migrations/20261102000000_api_account_wallet_id.sql"
            ),
            vec!["wallet_address", "id"],
        ),
        (
            "narrow_existing",
            include_str!(
                "../../../../schema/api_wallet/migrations/20261102010000_api_account_wallet_rowid.sql"
            ),
            vec!["wallet_address"],
        ),
    ] {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(pool.write_ref()).await.unwrap();
        let external = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                pool.write_ref()
                    .connect_options()
                    .as_ref()
                    .clone()
                    .busy_timeout(Duration::from_secs(60)),
            )
            .await
            .unwrap();
        let mut tx = pool.write_ref().begin_with("BEGIN IMMEDIATE").await.unwrap();
        let started = Instant::now();
        let (stop, running) = tokio::sync::watch::channel(true);
        let reader_pool = pool.clone();
        let reader = tokio::spawn(async move {
            let mut count = 0;
            let mut max_us = 0;
            while *running.borrow() {
                let at = Instant::now();
                let present: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM api_account WHERE id=250000")
                        .fetch_one(reader_pool.read_ref())
                        .await
                        .unwrap();
                assert_eq!(present, 1);
                max_us = max_us.max(at.elapsed().as_micros());
                count += 1;
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            (count, max_us)
        });
        let (ready, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
        let outside_ready = ready.clone();
        let outside = tokio::spawn(async move {
            let at = Instant::now();
            outside_ready.send(()).unwrap();
            let result = sqlx::query("UPDATE api_account SET name='external-probe' WHERE id=1")
                .execute(&external)
                .await
                .unwrap();
            assert_eq!(result.rows_affected(), 1);
            at.elapsed().as_micros()
        });
        let write_pool = pool.clone();
        let inside = tokio::spawn(async move {
            let at = Instant::now();
            ready.send(()).unwrap();
            let _gate = write_pool.lock_write().await;
            let result = sqlx::query("UPDATE api_account SET name='pooled-probe' WHERE id=2")
                .execute(write_pool.write_ref())
                .await
                .unwrap();
            assert_eq!(result.rows_affected(), 1);
            at.elapsed().as_micros()
        });
        arrivals.recv().await.unwrap();
        arrivals.recv().await.unwrap();
        let path = std::path::PathBuf::from(&dir);
        let sizes = move || -> u64 {
            ["api_wallet.db", "api_wallet.db-wal", "api_wallet.db-shm"]
                .iter()
                .map(|n| std::fs::metadata(path.join(n)).map(|m| m.len()).unwrap_or(0))
                .sum()
        };
        let before_bytes = sizes();
        println!("MIGRATION_BEGIN {phase}");
        let disk_running = stop.subscribe();
        let monitor = tokio::spawn(async move {
            let mut peak = sizes();
            while *disk_running.borrow() {
                peak = peak.max(sizes());
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            peak.max(sizes())
        });
        let build = Instant::now();
        sqlx::raw_sql(sql).execute(tx.as_mut()).await.unwrap();
        let build_us = build.elapsed().as_micros();
        let commit = Instant::now();
        tx.commit().await.unwrap();
        let commit_us = commit.elapsed().as_micros();
        let occupied_us = started.elapsed().as_micros();
        let pooled_write_us = inside.await.unwrap();
        let external_write_us = outside.await.unwrap();
        stop.send(false).unwrap();
        let (read_samples, read_max_us) = reader.await.unwrap();
        assert!(read_samples > 0);
        let peak_db_wal_shm_bytes = monitor.await.unwrap();
        let facts: (i64, i64) = sqlx::query_as("SELECT COUNT(*),SUM(CASE WHEN account_id=id AND address=printf('migration-%040d',id) AND wallet_address=? AND uid=? AND chain_code='base' AND status=id%2 THEN 1 ELSE 0 END) FROM api_account")
            .bind(WALLET).bind(UID).fetch_one(pool.read_ref()).await.unwrap();
        assert_eq!(facts, (500000, 500000));
        let actual: Vec<(i64, i64, String)> =
            sqlx::query_as("PRAGMA index_info(api_account_wallet_id_idx)")
                .fetch_all(pool.read_ref())
                .await
                .unwrap();
        assert_eq!(actual.iter().map(|r| r.2.as_str()).collect::<Vec<_>>(), columns);
        println!(
            "MIGRATION_TIMING {}",
            serde_json::json!({"phase":phase,"addresses":500000,"build_us":build_us,"commit_us":commit_us,"writer_occupied_us":occupied_us,"pooled_write_us":pooled_write_us,"external_write_us":external_write_us,"read_samples":read_samples,"read_max_us":read_max_us,"before_db_wal_shm_bytes":before_bytes,"peak_db_wal_shm_bytes":peak_db_wal_shm_bytes})
        );
    }
}
