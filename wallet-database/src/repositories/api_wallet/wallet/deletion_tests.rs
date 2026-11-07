use super::{
    super::test_support::seed_delete_wallet, ApiWalletDbPool, ApiWalletDeletionRead, ApiWalletRepo,
    WalletDeletionSnapshot,
};
use crate::repositories::test_helper::setup_api_wallet_pool;

#[tokio::test]
async fn api_wallet_deletion_index_migrates_populated_accounts() {
    let pool = setup_api_wallet_pool("deletion_index_existing_data").await;
    seed_deletion_data(&pool).await;
    let before: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id,chain_code,address FROM api_account ORDER BY id")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    let fresh_columns: Vec<(i64, i64, String)> =
        sqlx::query_as("PRAGMA index_info(api_account_wallet_id_idx)")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(
        fresh_columns.iter().map(|row| row.2.as_str()).collect::<Vec<_>>(),
        ["wallet_address"],
        "rowid already supplies the ordered identity"
    );

    sqlx::query("DROP INDEX api_account_wallet_id_idx").execute(pool.write_ref()).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../schema/api_wallet/migrations/20261102000000_api_account_wallet_id.sql"
    ))
    .execute(pool.write_ref())
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../schema/api_wallet/migrations/20261102010000_api_account_wallet_rowid.sql"
    ))
    .execute(pool.write_ref())
    .await
    .unwrap();
    let columns: Vec<(i64, i64, String)> =
        sqlx::query_as("PRAGMA index_info(api_account_wallet_id_idx)")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(columns.iter().map(|row| row.2.as_str()).collect::<Vec<_>>(), ["wallet_address"]);
    let plan: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN SELECT id,chain_code,address FROM api_account INDEXED BY api_account_wallet_id_idx WHERE wallet_address='a' AND id>0 AND id<=100 ORDER BY id LIMIT 1000",
    ).fetch_all(pool.read_ref()).await.unwrap();
    assert!(
        plan.iter().any(|row| row.3.contains("rowid>?") && row.3.contains("rowid<?")),
        "{plan:?}"
    );
    assert!(!plan.iter().any(|row| row.3.contains("TEMP B-TREE")), "{plan:?}");
    let after: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id,chain_code,address FROM api_account ORDER BY id")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(after, before, "adding the seek index preserves existing identities");
}

#[tokio::test]
async fn api_wallet_deletion_page_work_does_not_grow_with_other_wallet_rows() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let pool = setup_api_wallet_pool("deletion_wallet_scoped_work").await;
    seed_deletion_data(&pool).await;
    sqlx::query("WITH RECURSIVE n(x) AS (VALUES(2) UNION ALL SELECT x+1 FROM n WHERE x<10001) INSERT INTO api_account(account_id,name,address,wallet_address,uid,chain_code,api_wallet_type,status,is_init,created_at) SELECT x,'background','unrelated-'||x,'other','other','base',1,x%2,0,'2026-11-02T00:00:00Z' FROM n")
        .execute(pool.write_ref()).await.unwrap();
    sqlx::query("INSERT INTO api_account(account_id,name,address,wallet_address,uid,chain_code,api_wallet_type,status,is_init,created_at) VALUES(2,'tail','target-tail','a','a','eth',1,0,0,'2026-11-02T00:00:00Z')")
        .execute(pool.write_ref()).await.unwrap();
    let mut conn = pool.read_ref().acquire().await.unwrap();
    let cutoff: i64 =
        sqlx::query_scalar("SELECT MAX(id) FROM api_account").fetch_one(&mut *conn).await.unwrap();
    sqlx::query("INSERT INTO api_account(account_id,name,address,wallet_address,uid,chain_code,api_wallet_type,status,is_init,created_at) VALUES(3,'late','beyond-cutoff','b','b','base',1,1,0,'2026-11-02T00:00:00Z')")
        .execute(pool.write_ref()).await.unwrap();
    let expected: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM api_account WHERE wallet_address IN ('a','b') AND id<=? ORDER BY id",
    )
    .bind(cutoff)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    let steps = Arc::new(AtomicUsize::new(0));
    let measured = steps.clone();
    conn.lock_handle().await.unwrap().set_progress_handler(1, move || {
        measured.fetch_add(1, Ordering::Relaxed);
        true
    });
    let page = crate::dao::api_account::ApiAccountDao::deletion_page(
        &mut *conn,
        &["b", "a", "a"],
        0,
        cutoff,
    )
    .await
    .unwrap();
    conn.lock_handle().await.unwrap().remove_progress_handler();
    assert_eq!(page.iter().map(|row| row.id).collect::<Vec<_>>(), expected);
    assert!(
        page.iter().any(|row| row.address == "target-tail"),
        "include inactive targets after a large unrelated gap"
    );
    let steps = steps.load(Ordering::Relaxed);
    println!("WALLET-2980 wallet-scoped page SQLite VM steps={steps}");
    assert!(steps < 1000, "three target rows must not walk10000unrelated rows: {steps} VM steps");
}

#[tokio::test]
async fn api_wallet_deletion_bounded_snapshot_rejects_equal_count_change() {
    let pool = setup_api_wallet_pool("deletion_bounded_identity").await;
    seed_deletion_data(&pool).await;
    let wallets = wallet_deletion_snapshot(&pool).await;
    for change in ["address='changed'", "chain_code='eth'", "id=id+100"] {
        let snapshot = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
        assert_eq!(snapshot.len(), 2);
        let mut tx = pool.write_ref().begin().await.unwrap();
        sqlx::query(&format!("UPDATE api_account SET {change} WHERE wallet_address='a'"))
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert!(ApiWalletRepo::delete_wallet_data(&pool, &wallets, &snapshot).await.is_err());
        assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_some());
    }
}

#[tokio::test]
async fn api_wallet_deletion_pages_include_inactive_rows_without_duplicates() {
    let pool = setup_api_wallet_pool("deletion_keyset").await;
    seed_deletion_data(&pool).await;
    sqlx::query("WITH RECURSIVE n(x) AS (VALUES(2) UNION ALL SELECT x+1 FROM n WHERE x<2100) INSERT INTO api_account(account_id,name,address,wallet_address,uid,derivation_path_index,chain_code,api_wallet_type,status,is_init,created_at) SELECT x,'scale','scale-'||x,'a','a',x,'base',1,x%2,0,'2026-10-30T00:00:00Z' FROM n")
        .execute(pool.write_ref()).await.unwrap();
    let mut cursor = 0;
    let mut total = 0;
    loop {
        let page = ApiWalletRepo::deletion_account_page(&pool, &["a", "b"], cursor, i64::MAX)
            .await
            .unwrap();
        assert!(page.len() <= 1000);
        if page.is_empty() {
            break;
        }
        for row in &page {
            assert!(row.id > cursor);
            cursor = row.id;
        }
        total += page.len();
    }
    assert_eq!(total, 2101);
    let wallets = wallet_deletion_snapshot(&pool).await;
    let snapshot = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    assert_eq!(snapshot.len(), total as u64);
    ApiWalletRepo::delete_wallet_data(&pool, &wallets, &snapshot).await.unwrap();
    assert!(ApiWalletRepo::accounts_for_deletion(&pool, &["a", "b"]).await.unwrap().is_empty());
    assert_eq!(ApiWalletRepo::accounts_for_deletion(&pool, &["other"]).await.unwrap().len(), 1);
}

#[tokio::test]
async fn api_wallet_deletion_fixed_cutoff_rejects_late_rows_at_commit() {
    let pool = setup_api_wallet_pool("deletion_fixed_cutoff").await;
    seed_deletion_data(&pool).await;
    let wallets = wallet_deletion_snapshot(&pool).await;
    let before = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    sqlx::query("INSERT INTO api_account(account_id,name,address,wallet_address,uid,chain_code,api_wallet_type,status,is_init,created_at) VALUES(2,'late','late','a','a','base',1,1,0,'2026-10-30T00:00:00Z')")
        .execute(pool.write_ref()).await.unwrap();
    let original = ApiWalletRepo::deletion_account_page(&pool, &["a", "b"], 0, before.through_id())
        .await
        .unwrap();
    assert_eq!(original.len(), 2, "cleanup scan must stop at its original cutoff");
    assert!(original.iter().all(|row| row.address != "late"));
    assert!(ApiWalletRepo::delete_wallet_data(&pool, &wallets, &before).await.is_err());
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_some());
    let retry = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    assert_eq!(retry.len(), 3);
    ApiWalletRepo::delete_wallet_data(&pool, &wallets, &retry).await.unwrap();
    assert!(
        ApiWalletRepo::deletion_account_page(&pool, &["a", "b"], 0, i64::MAX)
            .await
            .unwrap()
            .is_empty()
    );
}

#[test]
fn api_wallet_deletion_fingerprint_preserves_unambiguous_identity_encoding() {
    use crate::entities::api_account::ApiAccountDeletionRow;
    let rows = vec![
        ApiAccountDeletionRow { id: 7, chain_code: "ab".into(), address: "c".into() },
        ApiAccountDeletionRow { id: 8, chain_code: "base".into(), address: "é".into() },
    ];
    let mut together = super::WalletDeletionSnapshot::default();
    together.include(&rows);
    // Independent SHA-256 golden encoding: signed id LE, byte-length-prefixed UTF-8 fields.
    assert_eq!(
        together
            .hash
            .clone()
            .finish()
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "4f0319083de22b26ee00dc49bda5a6946e7cd7dc113fbd3ed8655ab00fbaca1d"
    );
    let mut paged = super::WalletDeletionSnapshot::default();
    for row in &rows {
        paged.include(std::slice::from_ref(row));
    }
    assert_eq!(paged, together, "pagination must not change the identity fingerprint");
    let ambiguous = vec![
        ApiAccountDeletionRow { id: 7, chain_code: "a".into(), address: "bc".into() },
        ApiAccountDeletionRow { id: 8, chain_code: "base".into(), address: "é".into() },
    ];
    let mut other = super::WalletDeletionSnapshot::default();
    other.include(&ambiguous);
    assert_ne!(
        other, together,
        "same concatenation must not alias distinct chain/address identities"
    );
}

#[tokio::test]
async fn api_wallet_deletion_page_preserves_escaped_utf8_identity_and_scope() {
    let pool = setup_api_wallet_pool("deletion_page_json_encoding").await;
    seed_deletion_data(&pool).await;
    let address = "a\"\\\n字";
    let chain = "ba'se\"\\\n";
    let id = 9_007_199_254_741_001_i64;
    sqlx::query("UPDATE api_account SET id=?,address=?,chain_code=? WHERE wallet_address='a'")
        .bind(id)
        .bind(address)
        .bind(chain)
        .execute(pool.write_ref())
        .await
        .unwrap();
    let page = ApiWalletRepo::deletion_account_page(&pool, &["a"], 0, i64::MAX).await.unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, id, "JSON transfer must preserve integer identities above2^53");
    assert_eq!(page[0].address, address);
    assert_eq!(page[0].chain_code, chain);
    assert!(
        ApiWalletRepo::deletion_account_page(&pool, &["a') OR 1=1 --"], 0, i64::MAX)
            .await
            .unwrap()
            .is_empty()
    );
    let wallets = wallet_deletion_snapshot(&pool).await;
    let snapshot = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    ApiWalletRepo::delete_wallet_data(&pool, &wallets, &snapshot).await.unwrap();
    assert!(ApiWalletRepo::find_by_address(&pool, "other").await.unwrap().is_some());
}

#[tokio::test]
async fn api_wallet_deletion_read_snapshot_is_stable_and_released_before_write() {
    let pool = crate::repositories::test_helper::setup_api_wallet_pool_with_config(
        "delete_stable_read",
        crate::SqlitePoolConfig { reader_max_connections: 1, writer_max_connections: 1 },
    )
    .await;
    seed_deletion_data(&pool).await;
    let mut read = ApiWalletRepo::begin_deletion_read(&pool).await.unwrap();
    let mut wallets = Vec::new();
    for address in ["a", "b"] {
        wallets.push(read.find_by_address(address).await.unwrap().unwrap());
    }
    assert!(!read.uid_list().await.unwrap().is_empty(), "UIDs reuse the sole held connection");
    let mut expected = read.empty_fingerprint();
    sqlx::query("UPDATE api_account SET address='changed' WHERE wallet_address='a'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    let page = read.account_page(&["a", "b"], 0).await.unwrap();
    assert!(page.iter().any(|row| row.address == "account-a"));
    assert!(page.iter().all(|row| row.address != "changed"));
    expected.include(&page);
    read.close().await.unwrap();
    let conn = tokio::time::timeout(std::time::Duration::from_secs(2), pool.read_ref().acquire())
        .await
        .unwrap()
        .unwrap();
    drop(conn);
    assert!(ApiWalletRepo::delete_wallet_data(&pool, &wallets, &expected).await.is_err());
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_some());
    let fresh = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    ApiWalletRepo::delete_wallet_data(&pool, &wallets, &fresh).await.unwrap();
}

#[tokio::test]
async fn api_wallet_deletion_dropped_read_releases_single_reader_and_wal() {
    let pool = crate::repositories::test_helper::setup_api_wallet_pool_with_config(
        "delete_read_wal",
        crate::SqlitePoolConfig { reader_max_connections: 1, writer_max_connections: 1 },
    )
    .await;
    seed_deletion_data(&pool).await;
    sqlx::query("CREATE TABLE wal_probe(id INTEGER PRIMARY KEY,payload BLOB)")
        .execute(pool.write_ref())
        .await
        .unwrap();
    sqlx::query("INSERT INTO wal_probe VALUES(1,zeroblob(65536))")
        .execute(pool.write_ref())
        .await
        .unwrap();
    let cleared: (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(pool.write_ref())
        .await
        .unwrap();
    assert_eq!(cleared.0, 0);
    let read = ApiWalletRepo::begin_deletion_read(&pool).await.unwrap();
    for byte in 0..64u8 {
        sqlx::query("UPDATE wal_probe SET payload=? WHERE id=1")
            .bind(vec![byte; 65536])
            .execute(pool.write_ref())
            .await
            .unwrap();
    }
    let pinned: (i64, i64, i64) =
        sqlx::query_as("PRAGMA wal_checkpoint(PASSIVE)").fetch_one(pool.write_ref()).await.unwrap();
    assert!(pinned.1 > pinned.2, "active snapshot pins newer frames: {pinned:?}");
    println!("WALLET-2980 read-held wal_frames={} checkpointed={}", pinned.1, pinned.2);
    drop(read);
    let conn = tokio::time::timeout(std::time::Duration::from_secs(2), pool.read_ref().acquire())
        .await
        .unwrap()
        .unwrap();
    drop(conn);
    let released: (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(pool.write_ref())
        .await
        .unwrap();
    assert_eq!(released, (0, 0, 0), "dropped read must stop pinning WAL");
    println!("WALLET-2980 read-dropped wal_frames=0 checkpointed=0");
}

#[tokio::test]
async fn api_wallet_deletion_validation_releases_writer_and_detects_gap_changes() {
    let pool = setup_api_wallet_pool("deletion_optimistic_gap").await;
    seed_deletion_data(&pool).await;
    let wallets = wallet_deletion_snapshot(&pool).await;
    let expected = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    // The completed reader-side scan retains its connection, not the writer gate/slot.
    let proof = ApiWalletRepo::deletion_source(&pool).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let _gate = pool.lock_write().await;
        sqlx::query("UPDATE api_account SET address='gap-change' WHERE wallet_address='a'")
            .execute(pool.write_ref())
            .await
            .unwrap();
    })
    .await
    .expect("validation must leave writer available");
    assert!(ApiWalletRepo::finish_deletion(&pool, &wallets, &expected, Some(proof)).await.is_err());
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_some());
    let retry = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    ApiWalletRepo::delete_wallet_data(&pool, &wallets, &retry).await.unwrap();
}

#[tokio::test]
async fn api_wallet_deletion_validation_unrelated_commit_falls_back_and_completes() {
    let pool = setup_api_wallet_pool("deletion_optimistic_unrelated").await;
    seed_deletion_data(&pool).await;
    let wallets = wallet_deletion_snapshot(&pool).await;
    let expected = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    let proof = ApiWalletRepo::deletion_source(&pool).await.unwrap();
    let before = proof.version;
    sqlx::query("CREATE TABLE unrelated_deletion_probe(id INTEGER PRIMARY KEY)")
        .execute(pool.write_ref())
        .await
        .unwrap();
    let mut proof = proof;
    let after: i64 =
        sqlx::query_scalar("PRAGMA data_version").fetch_one(&mut *proof.connection).await.unwrap();
    assert_ne!(before, after, "same reader connection observes external commit");
    ApiWalletRepo::finish_deletion(&pool, &wallets, &expected, Some(proof)).await.unwrap();
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_none());
}

#[tokio::test]
async fn api_wallet_deletion_optimistic_delete_failure_rolls_back_and_retries() {
    let pool = setup_api_wallet_pool("deletion_optimistic_rollback").await;
    seed_deletion_data(&pool).await;
    let wallets = wallet_deletion_snapshot(&pool).await;
    let expected = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_account_deletion BEFORE DELETE ON api_account BEGIN SELECT RAISE(ABORT,'test delete failure'); END")
        .execute(pool.write_ref()).await.unwrap();
    assert!(ApiWalletRepo::delete_wallet_data(&pool, &wallets, &expected).await.is_err());
    assert_eq!(ApiWalletRepo::accounts_for_deletion(&pool, &["a", "b"]).await.unwrap().len(), 2);
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_some());
    sqlx::query("DROP TRIGGER fail_account_deletion").execute(pool.write_ref()).await.unwrap();
    ApiWalletRepo::delete_wallet_data(&pool, &wallets, &expected).await.unwrap();
}

#[tokio::test]
async fn api_wallet_deletion_validation_new_id_and_binding_gap_are_rejected() {
    for (name, change) in [
        (
            "new_id",
            "INSERT INTO api_account(account_id,name,address,wallet_address,uid,chain_code,api_wallet_type,status,is_init,created_at) VALUES(99,'late','late-address','a','a','base',1,1,0,'2026-11-01T00:00:00Z')",
        ),
        ("binding", "UPDATE api_wallet SET binding_address='changed-partner' WHERE address='a'"),
    ] {
        let pool = setup_api_wallet_pool(&format!("deletion_gap_{name}")).await;
        seed_deletion_data(&pool).await;
        let wallets = wallet_deletion_snapshot(&pool).await;
        let expected = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
        let proof = ApiWalletRepo::deletion_source(&pool).await.unwrap();
        // A separate pool/connection also changes the retained reader's token.
        let external =
            sqlx::SqlitePool::connect_with(pool.write_ref().connect_options().as_ref().clone())
                .await
                .unwrap();
        sqlx::query(change).execute(&external).await.unwrap();
        assert!(
            ApiWalletRepo::finish_deletion(&pool, &wallets, &expected, Some(proof)).await.is_err(),
            "gap change: {name}"
        );
        assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_some());
        let count = ApiWalletRepo::accounts_for_deletion(&pool, &["a", "b"]).await.unwrap().len();
        assert_eq!(count, if name == "new_id" { 3 } else { 2 });
        external.close().await;
    }
}

#[tokio::test]
async fn api_wallet_deletion_validation_cancel_releases_single_reader() {
    let pool = crate::repositories::test_helper::setup_api_wallet_pool_with_config(
        "deletion_validation_cancel",
        crate::SqlitePoolConfig { reader_max_connections: 1, writer_max_connections: 1 },
    )
    .await;
    seed_deletion_data(&pool).await;
    let wallets = wallet_deletion_snapshot(&pool).await;
    let expected = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    let proof = ApiWalletRepo::deletion_source(&pool).await.unwrap();
    let guard = pool.lock_write().await;
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(25),
        ApiWalletRepo::finish_deletion(&pool, &wallets, &expected, Some(proof)),
    )
    .await;
    assert!(result.is_err(), "controlled gate keeps finalizer pending");
    let reader = tokio::time::timeout(std::time::Duration::from_secs(2), pool.read_ref().acquire())
        .await
        .unwrap()
        .unwrap();
    drop(reader);
    drop(guard);
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_some());
    ApiWalletRepo::delete_wallet_data(&pool, &wallets, &expected).await.unwrap();
}

#[tokio::test]
async fn api_wallet_deletion_shared_single_connection_retains_safe_fallback() {
    let split = setup_api_wallet_pool("deletion_shared_one").await;
    seed_deletion_data(&split).await;
    let single = std::sync::Arc::new(
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(split.write_ref().connect_options().as_ref().clone())
            .await
            .unwrap(),
    );
    let pool = crate::ApiWalletDbPool::new(single);
    let wallets = wallet_deletion_snapshot(&pool).await;
    let expected = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ApiWalletRepo::delete_wallet_data(&pool, &wallets, &expected),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_none());
}

#[tokio::test]
async fn api_wallet_deletion_shared_cloned_single_connection_retains_safe_fallback() {
    let split = setup_api_wallet_pool("deletion_shared_clones").await;
    seed_deletion_data(&split).await;
    let single = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(split.write_ref().connect_options().as_ref().clone())
        .await
        .unwrap();
    // Different outer handles/Arcs still own the same SQLx internal pool.
    let pool = crate::ApiWalletDbPool::new_split(
        std::sync::Arc::new(single.clone()),
        std::sync::Arc::new(single),
    );
    assert!(!std::ptr::eq(pool.read_ref(), pool.write_ref()));
    let wallets = wallet_deletion_snapshot(&pool).await;
    let expected = ApiWalletRepo::deletion_snapshot(&pool, &["a", "b"]).await.unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ApiWalletRepo::delete_wallet_data(&pool, &wallets, &expected),
    )
    .await
    .expect("same internal single-slot pool must not self-deadlock")
    .unwrap();
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_none());
}

async fn source_snapshot(
    source: &mut super::ApiWalletDeletionSource,
) -> super::WalletDeletionSnapshot {
    let mut read = source.begin_read().await.unwrap();
    let mut snapshot = read.empty_fingerprint();
    let mut after = 0;
    loop {
        let page = read.account_page(&["a", "b"], after).await.unwrap();
        let Some(last) = page.last() else { break };
        after = last.id;
        snapshot.include(&page);
    }
    read.close().await.unwrap();
    snapshot
}

#[tokio::test]
async fn api_wallet_deletion_source_token_precedes_first_snapshot_and_does_not_reset() {
    let pool = setup_api_wallet_pool("deletion_source_baseline").await;
    seed_deletion_data(&pool).await;
    let wallets = wallet_deletion_snapshot(&pool).await;
    let mut source = ApiWalletRepo::deletion_source(&pool).await.unwrap();
    let before = source.version;
    // Commit after token capture but before the first table read. Conservative
    // fallback is required even though this first view already sees the change.
    sqlx::query("UPDATE api_account SET address='before-read' WHERE wallet_address='a'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    let expected = source_snapshot(&mut source).await;
    assert_eq!(source.version, before, "baseline must never be refreshed");
    assert_ne!(super::ApiWalletDao::data_version(&mut source.connection).await.unwrap(), before);
    ApiWalletRepo::delete_wallet_data_with_source(&pool, &wallets, &expected, source)
        .await
        .unwrap();
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_none());
}

#[tokio::test]
async fn api_wallet_deletion_source_detects_change_during_first_snapshot() {
    let pool = setup_api_wallet_pool("deletion_source_held_change").await;
    seed_deletion_data(&pool).await;
    let wallets = wallet_deletion_snapshot(&pool).await;
    let mut source = ApiWalletRepo::deletion_source(&pool).await.unwrap();
    let mut read = source.begin_read().await.unwrap();
    let mut expected = read.empty_fingerprint();
    let old = read.account_page(&["a", "b"], 0).await.unwrap();
    sqlx::query("UPDATE api_account SET address='during-read' WHERE wallet_address='a'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert_eq!(read.account_page(&["a", "b"], 0).await.unwrap()[0].address, old[0].address);
    expected.include(&old);
    read.close().await.unwrap();
    let err = ApiWalletRepo::delete_wallet_data_with_source(&pool, &wallets, &expected, source)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("accounts changed"));
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_some());
    let mut retry = ApiWalletRepo::deletion_source(&pool).await.unwrap();
    let fresh = source_snapshot(&mut retry).await;
    ApiWalletRepo::delete_wallet_data_with_source(&pool, &wallets, &fresh, retry).await.unwrap();
}

#[tokio::test]
async fn api_wallet_deletion_source_locked_retry_cannot_rebaseline_stale_facts() {
    let pool = crate::repositories::test_helper::setup_api_wallet_pool_with_config(
        "deletion_source_locked_retry",
        crate::SqlitePoolConfig { reader_max_connections: 1, writer_max_connections: 1 },
    )
    .await;
    seed_deletion_data(&pool).await;
    sqlx::query("CREATE TRIGGER deletion_busy BEFORE DELETE ON api_wallet BEGIN SELECT RAISE(ABORT,'database is locked'); END")
        .execute(pool.write_ref()).await.unwrap();
    let wallets = wallet_deletion_snapshot(&pool).await;
    let mut source = ApiWalletRepo::deletion_source(&pool).await.unwrap();
    let expected = source_snapshot(&mut source).await;
    let change = async {
        // The only reader is returned when the first final attempt consumes
        // its source. Its writer reservation is already held at this point.
        let reader =
            tokio::time::timeout(std::time::Duration::from_secs(2), pool.read_ref().acquire())
                .await
                .unwrap()
                .unwrap();
        drop(reader);
        let mut tx = pool.write_ref().begin().await.unwrap();
        sqlx::query("DROP TRIGGER deletion_busy").execute(tx.as_mut()).await.unwrap();
        sqlx::query(
            "UPDATE api_account SET address='changed-before-retry' WHERE wallet_address='a'",
        )
        .execute(tx.as_mut())
        .await
        .unwrap();
        tx.commit().await.unwrap();
    };
    let (result, ()) = tokio::join!(
        ApiWalletRepo::delete_wallet_data_with_source(&pool, &wallets, &expected, source),
        change
    );
    assert!(
        result.unwrap_err().to_string().contains("accounts changed"),
        "locked retry must validate the original cleanup facts"
    );
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_some());
    assert_eq!(ApiWalletRepo::accounts_for_deletion(&pool, &["a", "b"]).await.unwrap().len(), 2);
    let mut retry = ApiWalletRepo::deletion_source(&pool).await.unwrap();
    let fresh = source_snapshot(&mut retry).await;
    ApiWalletRepo::delete_wallet_data_with_source(&pool, &wallets, &fresh, retry).await.unwrap();
}

#[tokio::test]
async fn api_wallet_deletion_source_shared_clone_releases_sole_slot_before_write() {
    let split = setup_api_wallet_pool("deletion_source_clone").await;
    seed_deletion_data(&split).await;
    let single = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(split.write_ref().connect_options().as_ref().clone())
        .await
        .unwrap();
    let pool = crate::ApiWalletDbPool::new_split(
        std::sync::Arc::new(single.clone()),
        std::sync::Arc::new(single),
    );
    let wallets = wallet_deletion_snapshot(&pool).await;
    let mut source = ApiWalletRepo::deletion_source(&pool).await.unwrap();
    let expected = source_snapshot(&mut source).await;
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ApiWalletRepo::delete_wallet_data_with_source(&pool, &wallets, &expected, source),
    )
    .await
    .expect("shared cloned source must release sole slot")
    .unwrap();
}

#[tokio::test]
async fn api_wallet_deletion_source_closed_read_releases_wal_before_source_drop() {
    let pool = crate::repositories::test_helper::setup_api_wallet_pool_with_config(
        "deletion_source_wal",
        crate::SqlitePoolConfig { reader_max_connections: 1, writer_max_connections: 1 },
    )
    .await;
    seed_deletion_data(&pool).await;
    sqlx::query("CREATE TABLE source_wal_probe(id INTEGER PRIMARY KEY,payload BLOB)")
        .execute(pool.write_ref())
        .await
        .unwrap();
    sqlx::query("INSERT INTO source_wal_probe VALUES(1,zeroblob(65536))")
        .execute(pool.write_ref())
        .await
        .unwrap();
    let mut source = ApiWalletRepo::deletion_source(&pool).await.unwrap();
    let read = source.begin_read().await.unwrap();
    for _ in 0..8 {
        sqlx::query("UPDATE source_wal_probe SET payload=randomblob(65536)")
            .execute(pool.write_ref())
            .await
            .unwrap();
    }
    let pinned: (i64, i64, i64) =
        sqlx::query_as("PRAGMA wal_checkpoint(PASSIVE)").fetch_one(pool.write_ref()).await.unwrap();
    assert!(pinned.1 > pinned.2);
    read.close().await.unwrap();
    // Still retaining the same physical reader is NOT retaining its WAL view.
    let free: (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(pool.write_ref())
        .await
        .unwrap();
    assert_eq!(free, (0, 0, 0));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(25), pool.read_ref().acquire())
            .await
            .is_err()
    );
    drop(source);
    let connection =
        tokio::time::timeout(std::time::Duration::from_secs(2), pool.read_ref().acquire())
            .await
            .unwrap()
            .unwrap();
    drop(connection);
}

async fn seed_deletion_data(pool: &crate::ApiWalletDbPool) {
    use crate::{
        entities::{
            address_query_state::{AddressQueryStatus, CreateAddressQueryStateEntity},
            api_account::CreateApiAccountVo,
            api_wallet::ApiWalletType,
        },
        repositories::api_wallet::{
            account::ApiAccountRepo, address_query_state::AddressQueryStateRepo,
        },
    };
    for address in ["a", "b", "other"] {
        seed_delete_wallet(pool, address, address).await;
        ApiAccountRepo::upsert_account_multi(
            pool,
            vec![CreateApiAccountVo::new(
                0,
                &format!("account-{address}"),
                "pub",
                address,
                address,
                "path",
                0,
                "base",
                "fixture",
                ApiWalletType::SubAccount,
            )],
        )
        .await
        .unwrap();
        AddressQueryStateRepo::upsert(
            pool,
            CreateAddressQueryStateEntity::new(address, "base", AddressQueryStatus::Done),
        )
        .await
        .unwrap();
    }
    sqlx::query("UPDATE api_account SET status=0 WHERE wallet_address='a'")
        .execute(pool.write_ref())
        .await
        .unwrap();
}

async fn deletion_snapshot(pool: &crate::ApiWalletDbPool) -> super::WalletDeletionSnapshot {
    ApiWalletRepo::deletion_snapshot(pool, &["a", "b"]).await.unwrap()
}

async fn wallet_deletion_snapshot(
    pool: &crate::ApiWalletDbPool,
) -> Vec<crate::entities::api_wallet::ApiWalletEntity> {
    let mut selected = Vec::new();
    for address in ["a", "b"] {
        selected.push(ApiWalletRepo::find_by_address(pool, address).await.unwrap().unwrap());
    }
    selected
}

#[tokio::test]
async fn api_wallet_deletion_changed_wallet_identity_rolls_back() {
    for change in
        ["binding_address='new-partner'", "uid='new-uid'", "api_wallet_type=2", "id=id+1000"]
    {
        let pool = setup_api_wallet_pool(&format!("wallet_delete_identity_{change}")).await;
        seed_deletion_data(&pool).await;
        let wallets = wallet_deletion_snapshot(&pool).await;
        let accounts = deletion_snapshot(&pool).await;
        sqlx::query(&format!("UPDATE api_wallet SET {change} WHERE address='a'"))
            .execute(pool.write_ref())
            .await
            .unwrap();
        assert!(
            ApiWalletRepo::delete_wallet_data(&pool, &wallets, &accounts).await.is_err(),
            "{change}"
        );
        for address in ["a", "b", "other"] {
            assert!(ApiWalletRepo::find_by_address(&pool, address).await.unwrap().is_some());
            assert_eq!(
                ApiWalletRepo::accounts_for_deletion(&pool, &[address]).await.unwrap().len(),
                1
            );
        }
    }
}

#[tokio::test]
async fn api_wallet_deletion_snapshots_inactive_and_deletes_only_selected_data() {
    use crate::repositories::api_wallet::address_query_state::AddressQueryStateRepo;
    let pool = setup_api_wallet_pool("wallet_deletion_atomic_success").await;
    seed_deletion_data(&pool).await;
    let snapshot = ApiWalletRepo::accounts_for_deletion(&pool, &["a", "b"]).await.unwrap();
    assert_eq!(snapshot.len(), 2);
    assert!(snapshot.iter().any(|row| row.wallet_address == "a" && row.status == 0));

    ApiWalletRepo::delete_wallet_data(
        &pool,
        &wallet_deletion_snapshot(&pool).await,
        &deletion_snapshot(&pool).await,
    )
    .await
    .unwrap();

    for address in ["a", "b"] {
        assert!(ApiWalletRepo::find_by_address(&pool, address).await.unwrap().is_none());
        assert!(ApiWalletRepo::accounts_for_deletion(&pool, &[address]).await.unwrap().is_empty());
        assert!(
            AddressQueryStateRepo::get_by_uid_and_chain(&pool, address, "base")
                .await
                .unwrap()
                .is_none()
        );
    }
    ApiWalletRepo::delete_wallet_data(&pool, &[], &super::WalletDeletionSnapshot::default())
        .await
        .unwrap();
    assert!(ApiWalletRepo::accounts_for_deletion(&pool, &[]).await.unwrap().is_empty());
    assert!(ApiWalletRepo::find_by_address(&pool, "other").await.unwrap().is_some());
    assert_eq!(ApiWalletRepo::accounts_for_deletion(&pool, &["other"]).await.unwrap().len(), 1);
    assert!(
        AddressQueryStateRepo::get_by_uid_and_chain(&pool, "other", "base")
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn api_wallet_deletion_local_failure_rolls_back_wallets_accounts_and_query_state() {
    use crate::repositories::api_wallet::address_query_state::AddressQueryStateRepo;
    let pool = setup_api_wallet_pool("wallet_deletion_atomic_rollback").await;
    seed_deletion_data(&pool).await;
    sqlx::query("CREATE TRIGGER reject_query_delete BEFORE DELETE ON address_query_state WHEN OLD.uid='b' BEGIN SELECT RAISE(FAIL,'injected delete failure'); END")
        .execute(pool.write_ref()).await.unwrap();

    assert!(
        ApiWalletRepo::delete_wallet_data(
            &pool,
            &wallet_deletion_snapshot(&pool).await,
            &deletion_snapshot(&pool).await
        )
        .await
        .is_err()
    );

    for address in ["a", "b", "other"] {
        assert!(ApiWalletRepo::find_by_address(&pool, address).await.unwrap().is_some());
        assert_eq!(ApiWalletRepo::accounts_for_deletion(&pool, &[address]).await.unwrap().len(), 1);
        assert!(
            AddressQueryStateRepo::get_by_uid_and_chain(&pool, address, "base")
                .await
                .unwrap()
                .is_some()
        );
    }
    sqlx::query("DROP TRIGGER reject_query_delete").execute(pool.write_ref()).await.unwrap();
    ApiWalletRepo::delete_wallet_data(
        &pool,
        &wallet_deletion_snapshot(&pool).await,
        &deletion_snapshot(&pool).await,
    )
    .await
    .unwrap();
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_none());
}

#[tokio::test]
async fn api_wallet_deletion_clear_rolls_back_on_later_failure_and_retries() {
    let pool = setup_api_wallet_pool("wallet_deletion_clear_rollback").await;
    seed_deletion_data(&pool).await;
    // Keep the unrelated wallet identity but no unrelated account rows.
    sqlx::query("DELETE FROM api_account WHERE wallet_address='other'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_query_delete BEFORE DELETE ON address_query_state WHEN OLD.uid='b' BEGIN SELECT RAISE(FAIL,'after account clear'); END")
        .execute(pool.write_ref()).await.unwrap();
    let wallets = wallet_deletion_snapshot(&pool).await;
    let accounts = deletion_snapshot(&pool).await;
    assert!(ApiWalletRepo::delete_wallet_data(&pool, &wallets, &accounts).await.is_err());
    assert_eq!(ApiWalletRepo::accounts_for_deletion(&pool, &["a", "b"]).await.unwrap().len(), 2);
    for address in ["a", "b", "other"] {
        assert!(ApiWalletRepo::find_by_address(&pool, address).await.unwrap().is_some());
    }
    sqlx::query("DROP TRIGGER reject_query_delete").execute(pool.write_ref()).await.unwrap();
    ApiWalletRepo::delete_wallet_data(&pool, &wallets, &accounts).await.unwrap();
    assert!(ApiWalletRepo::accounts_for_deletion(&pool, &["a", "b"]).await.unwrap().is_empty());
    assert!(ApiWalletRepo::find_by_address(&pool, "a").await.unwrap().is_none());
    assert!(ApiWalletRepo::find_by_address(&pool, "b").await.unwrap().is_none());
    assert!(ApiWalletRepo::find_by_address(&pool, "other").await.unwrap().is_some());
}

impl ApiWalletRepo {
    async fn begin_deletion_read(
        pool: &ApiWalletDbPool,
    ) -> Result<ApiWalletDeletionRead<'static>, crate::Error> {
        let tx = pool.read_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        ApiWalletDeletionRead::from_transaction(tx).await
    }
    async fn deletion_account_page(
        pool: &ApiWalletDbPool,
        addresses: &[&str],
        after_id: i64,
        through_id: i64,
    ) -> Result<Vec<crate::entities::api_account::ApiAccountDeletionRow>, crate::Error> {
        let mut conn =
            pool.read_ref().acquire().await.map_err(|e| crate::Error::Database(e.into()))?;
        crate::dao::api_account::ApiAccountDao::deletion_page(
            &mut conn, addresses, after_id, through_id,
        )
        .await
    }
    async fn deletion_snapshot(
        pool: &ApiWalletDbPool,
        addresses: &[&str],
    ) -> Result<WalletDeletionSnapshot, crate::Error> {
        let started = std::time::Instant::now();
        let mut read_time = std::time::Duration::ZERO;
        let mut hash_time = std::time::Duration::ZERO;
        let mut snapshot = WalletDeletionSnapshot {
            through_id: crate::dao::api_account::ApiAccountDao::account_high_watermark(
                pool.read_ref(),
            )
            .await?,
            ..Default::default()
        };
        let mut cursor = 0;
        loop {
            let stage = std::time::Instant::now();
            let page =
                Self::deletion_account_page(pool, addresses, cursor, snapshot.through_id).await?;
            read_time += stage.elapsed();
            let Some(last) = page.last() else {
                break;
            };
            cursor = last.id;
            let stage = std::time::Instant::now();
            snapshot.include(&page);
            hash_time += stage.elapsed();
        }
        tracing::info!(target: "wallet_deletion_perf", stage="initial_snapshot", rows=snapshot.len(),
            elapsed_ms=started.elapsed().as_millis() as u64, read_ms=read_time.as_millis() as u64,
            hash_ms=hash_time.as_millis() as u64);
        Ok(snapshot)
    }
}

#[path = "deletion_index_tests.rs"]
mod index_tests;
