//! Index maintenance must not duplicate a usable UID prefix.
use super::{ApiWalletRepo, seed_deletion_data};
use crate::repositories::test_helper::setup_api_wallet_pool;

#[tokio::test]
async fn deletion_has_one_uid_prefix_index() {
    let pool = setup_api_wallet_pool("delete_uid_prefix").await;
    seed_deletion_data(&pool).await;
    let indices: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='api_account_uid_chaincode_idx'")
        .fetch_one(pool.read_ref()).await.unwrap();
    assert_eq!(indices, 0, "the longer UID/chain/index already supports this prefix");
    let long: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='api_account_uid_chain_index_idx'")
        .fetch_one(pool.read_ref()).await.unwrap();
    assert_eq!(long, 1);
    assert!(ApiWalletRepo::find_by_address(&pool, "other").await.unwrap().is_some());
}

#[tokio::test]
async fn populated_uid_prefix_upgrade_preserves_facts_and_lookup() {
    let pool = setup_api_wallet_pool("delete_uid_upgrade").await;
    seed_deletion_data(&pool).await;
    sqlx::query("UPDATE api_account SET status=1 WHERE wallet_address='a'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS api_account_uid_chaincode_idx ON api_account(uid,chain_code)",
    )
    .execute(pool.write_ref())
    .await
    .unwrap();
    let before: Vec<(i64, String, String, String, i64)> =
        sqlx::query_as("SELECT id,uid,chain_code,address,status FROM api_account ORDER BY id")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    sqlx::raw_sql(include_str!("../../../../schema/api_wallet/migrations/20261102020000_remove_redundant_account_uid_prefix.sql"))
        .execute(pool.write_ref()).await.unwrap();
    let after: Vec<(i64, String, String, String, i64)> =
        sqlx::query_as("SELECT id,uid,chain_code,address,status FROM api_account ORDER BY id")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(before, after);
    let (uid, keyword, input_index): (String, String, i64) = sqlx::query_as("SELECT uid,address,COALESCE(derivation_path_index,0) FROM api_account WHERE wallet_address='a'")
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
    sqlx::query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<10000) INSERT INTO api_account(account_id,name,address,wallet_address,uid,chain_code,api_wallet_type,status,is_init,created_at) SELECT x,'foreign','foreign-'||x,'other','foreign-'||x,'base',1,1,0,'2026-11-02T00:00:00Z' FROM n")
        .execute(pool.write_ref()).await.unwrap();
    let mut conn = pool.read_ref().acquire().await.unwrap();
    let steps = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let measured = steps.clone();
    conn.lock_handle().await.unwrap().set_progress_handler(1, move || {
        measured.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        true
    });
    let found =
        crate::dao::api_account::ApiAccountDao::search_address_by_uid(&mut *conn, &uid, &keyword)
            .await
            .unwrap();
    conn.lock_handle().await.unwrap().remove_progress_handler();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].address, keyword);
    let work = steps.load(std::sync::atomic::Ordering::Relaxed);
    assert!(work < 1000, "UID lookup must not scan10000foreign accounts: {work}");
    // Expansion's uid/chain/input_index equality still has all three indexed keys.
    let plan: Vec<(i64,i64,i64,String)>=sqlx::query_as("EXPLAIN QUERY PLAN SELECT id FROM api_account WHERE uid=? AND chain_code=? AND derivation_path_index=?")
        .bind(&uid).bind("base").bind(input_index).fetch_all(&mut *conn).await.unwrap();
    assert!(
        plan.iter().any(|p| p.3.contains("api_account_uid_chain_index_idx")
            && p.3.contains("derivation_path_index=?")),
        "{plan:?}"
    );
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM api_account WHERE uid=? AND chain_code=? AND derivation_path_index=?",
    )
    .bind(&uid)
    .bind("base")
    .bind(input_index)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert!(ids.contains(&found[0].id));
}

// Exercise real migration history/checksums and the same startup entrypoint as the SDK.
async fn check_forward_startup(label: &str, legacy: sqlx::migrate::Migrator) {
    use std::sync::Arc;
    let dir = crate::repositories::test_helper::make_temp_dir(label);
    let file = std::path::Path::new(&dir).join("api_wallet.db");
    let raw = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&file)
                .create_if_missing(true)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
        )
        .await
        .unwrap();
    legacy.run(&raw).await.unwrap();
    let old = crate::ApiWalletDbPool::new_split(Arc::new(raw.clone()), Arc::new(raw.clone()));
    seed_deletion_data(&old).await;
    sqlx::query("UPDATE api_wallet SET binding_address=CASE address WHEN 'a' THEN 'b' WHEN 'b' THEN 'a' ELSE NULL END, api_wallet_type=CASE address WHEN 'b' THEN 2 ELSE 1 END").execute(&raw).await.unwrap();
    let account_sql = "SELECT json_group_array(json_array(id,account_id,name,address,pubkey,address_type,wallet_address,uid,derivation_path,derivation_path_index,chain_code,api_wallet_type,status,is_init,is_expand,is_used,created_at,updated_at)) FROM (SELECT * FROM api_account ORDER BY id)";
    let wallet_sql =
        "SELECT id,uid,address,name,binding_address,api_wallet_type FROM api_wallet ORDER BY id";
    let history_sql = "SELECT version,checksum,success FROM _sqlx_migrations ORDER BY version";
    let payload_sql = "SELECT id,phrase,seed FROM api_wallet ORDER BY id";
    let payloads: Vec<(i64, Vec<u8>, Option<Vec<u8>>)> =
        sqlx::query_as(payload_sql).fetch_all(&raw).await.unwrap();
    let accounts: String = sqlx::query_scalar(account_sql).fetch_one(&raw).await.unwrap();
    let wallets: Vec<(i64, String, String, String, Option<String>, i64)> =
        sqlx::query_as(wallet_sql).fetch_all(&raw).await.unwrap();
    let old_history: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as(history_sql).fetch_all(&raw).await.unwrap();
    assert_eq!(old_history.len(), legacy.iter().count());
    let prefix: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE name='api_account_uid_chaincode_idx'",
    )
    .fetch_one(&raw)
    .await
    .unwrap();
    assert_eq!(prefix, 1);
    raw.close().await;
    let full = crate::init::Migrator::ApiWallet.migrator().unwrap();
    let mut first_history = None;
    for start in 0..2 {
        let pool = crate::SqliteContext::new(&dir, Some("api_wallet.db"))
            .await
            .unwrap()
            .into_api_wallet_db_pool()
            .unwrap();
        let integrity: String =
            sqlx::query_scalar("PRAGMA integrity_check").fetch_one(pool.read_ref()).await.unwrap();
        assert_eq!(integrity, "ok");
        let actual_payloads: Vec<(i64, Vec<u8>, Option<Vec<u8>>)> =
            sqlx::query_as(payload_sql).fetch_all(pool.read_ref()).await.unwrap();
        assert!(
            actual_payloads == payloads,
            "wallet opaque payload changed; never log its contents"
        );
        let actual_accounts: String =
            sqlx::query_scalar(account_sql).fetch_one(pool.read_ref()).await.unwrap();
        let actual_wallets: Vec<(i64, String, String, String, Option<String>, i64)> =
            sqlx::query_as(wallet_sql).fetch_all(pool.read_ref()).await.unwrap();
        assert_eq!(actual_accounts, accounts, "existing account fields survive startup{start}");
        assert_eq!(actual_wallets, wallets, "wallet identity/binding survives startup{start}");
        let (uid, keyword): (String, String) =
            sqlx::query_as("SELECT uid,address FROM api_account WHERE wallet_address='b'")
                .fetch_one(pool.read_ref())
                .await
                .unwrap();
        let results =
            crate::repositories::api_wallet::account::ApiAccountRepo::search_address_by_uid(
                &pool, &uid, &keyword,
            )
            .await
            .unwrap();
        assert!(results.iter().any(|row| row.address == keyword && row.wallet_address == "b"));
        let history: Vec<(i64, Vec<u8>, bool)> =
            sqlx::query_as(history_sql).fetch_all(pool.read_ref()).await.unwrap();
        assert_eq!(history.len(), full.iter().count());
        assert!(history.iter().all(|r| r.2));
        for prior in &old_history {
            assert!(history.contains(prior), "old checksum/version changed");
        }
        for migration in full.iter() {
            assert!(
                history
                    .iter()
                    .any(|r| r.0 == migration.version
                        && r.1.as_slice() == migration.checksum.as_ref())
            );
        }
        if let Some(previous) = &first_history {
            assert_eq!(&history, previous, "repeat startup must not reapply migration");
        }
        first_history = Some(history);
        let indices: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='api_account'",
        )
        .fetch_all(pool.read_ref())
        .await
        .unwrap();
        assert!(!indices.iter().any(|n| n == "api_account_uid_chaincode_idx"));
        assert!(indices.iter().any(|n| n == "api_account_uid_chain_index_idx"));
        assert!(indices.iter().any(|n| n == "api_account_wallet_id_idx"));
        let columns: Vec<(i64, i64, String)> =
            sqlx::query_as("PRAGMA index_info(api_account_wallet_id_idx)")
                .fetch_all(pool.read_ref())
                .await
                .unwrap();
        assert_eq!(columns.iter().map(|r| r.2.as_str()).collect::<Vec<_>>(), ["wallet_address"]);
        pool.read_ref().close().await;
        pool.write_ref().close().await;
    }
    println!(
        "FORWARD_UPGRADE_PASS {label} old_migrations={} new_migrations={} startups=2",
        old_history.len(),
        full.iter().count()
    );
}

#[tokio::test]
async fn forward_startup_preserves_history_and_data() {
    for cutoff in [20261102000000_i64, 20261102020000_i64] {
        let mut legacy = crate::init::Migrator::ApiWallet.migrator().unwrap();
        legacy.migrations = std::borrow::Cow::Owned(
            legacy.iter().filter(|m| m.version < cutoff).cloned().collect(),
        );
        check_forward_startup(&format!("forward_upgrade_{cutoff}"), legacy).await;
    }
}

#[tokio::test]
#[ignore = "manual: export historical release catalogs under WALLET_UPGRADE_FIXTURE_DIR"]
async fn released_catalogs_forward_startup() {
    let base = std::path::PathBuf::from(
        std::env::var("WALLET_UPGRADE_FIXTURE_DIR")
            .expect("export release migration catalogs first"),
    );
    for tag in ["v2.0.0", "v2.1.0"] {
        let legacy = sqlx::migrate::Migrator::new(base.join(tag)).await.unwrap();
        check_forward_startup(tag, legacy).await;
    }
}
