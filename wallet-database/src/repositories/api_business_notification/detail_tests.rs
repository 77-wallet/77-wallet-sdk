use super::*;
use crate::{
    entities::api_wallet::ApiWalletType, repositories::test_helper::setup_api_wallet_pool,
};

#[tokio::test]
async fn detail_scope_history_and_read_state() {
    // Arrange: eligible duplicate rows, subaccount, inactive, orphan and blank owners.
    let pool = setup_api_wallet_pool("notification_detail_scope").await;
    for (uid, address, kind, status) in [
        ("a", "a1", ApiWalletType::Withdrawal, 1),
        ("a", "a2", ApiWalletType::Withdrawal, 1),
        ("sub", "s1", ApiWalletType::SubAccount, 1),
        ("off", "o1", ApiWalletType::Withdrawal, 0),
        (" ", "blank", ApiWalletType::Withdrawal, 1),
    ] {
        crate::dao::api_wallet::ApiWalletDao::upsert(
            pool.write_ref(),
            address,
            uid,
            "fixture",
            b"",
            b"",
            status,
            kind,
            None,
            "fixture",
            0,
        )
        .await
        .unwrap();
    }
    for uid in ["a", "sub", "off", "orphan", " "] {
        ApiBusinessNotificationRepo::upsert(
            &pool,
            NewApiBusinessNotification {
                id: uid,
                uid,
                kind: "AWM_ADDRESS_INSUFFICIENT",
                business_key: uid,
                payload: "{}",
            },
        )
        .await
        .unwrap();
    }
    ApiBusinessNotificationRepo::mark_resolved(&pool, "a", "a", true).await.unwrap();
    let before = ApiBusinessNotificationRepo::find(&pool, "a", "a").await.unwrap().unwrap();
    // Act: detail reads resolved history without marking it read or changing timestamps.
    let detail = ApiBusinessNotificationRepo::find_for_withdrawal_wallets(&pool, "a")
        .await
        .unwrap()
        .unwrap();
    // Assert: single eligible row, exact id binding and unchanged durable record.
    assert_eq!(detail.id, "a");
    assert!(!detail.is_read);
    assert!(detail.resolved_at.is_some());
    assert_eq!(serde_json::to_value(before).unwrap(), serde_json::to_value(detail).unwrap());
    for id in ["sub", "off", "orphan", " ", "missing", "' OR 1=1 --"] {
        assert!(
            ApiBusinessNotificationRepo::find_for_withdrawal_wallets(&pool, id)
                .await
                .unwrap()
                .is_none()
        );
    }
    sqlx::query("UPDATE api_wallet SET status = 0 WHERE uid = 'a'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert!(
        ApiBusinessNotificationRepo::find_for_withdrawal_wallets(&pool, "a")
            .await
            .unwrap()
            .is_none()
    );
    assert!(!ApiBusinessNotificationRepo::find(&pool, "a", "a").await.unwrap().unwrap().is_read);
}

async fn wallet_2946_locked_fixture()
-> (String, ApiWalletDbPool, sqlx::Transaction<'static, sqlx::Sqlite>) {
    let dir = crate::repositories::test_helper::make_temp_dir("wallet_2946_lock");
    let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
    let pool = ctx.clone().into_api_wallet_db_pool().unwrap();
    crate::dao::api_wallet::ApiWalletDao::upsert(
        pool.write_ref(),
        "address",
        "owner",
        "fixture",
        b"",
        b"",
        1,
        ApiWalletType::Withdrawal,
        None,
        "fixture",
        0,
    )
    .await
    .unwrap();
    ApiBusinessNotificationRepo::ingest(
        &pool,
        NewApiBusinessNotification {
            id: "alert",
            uid: "owner",
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key: "balance",
            payload: "{}",
        },
    )
    .await
    .unwrap();
    // Use an independent pool for the external writer; the Repo cannot coordinate this lock.
    let external = crate::SqliteContext::new(&dir, Some("api_wallet.db"))
        .await
        .unwrap()
        .into_api_wallet_db_pool()
        .unwrap();
    // Shorten only this test's writer busy wait, keeping the real retry implementation.
    sqlx::query("PRAGMA busy_timeout = 100").execute(pool.write_ref()).await.unwrap();
    let lock = external.write_ref().begin_with("BEGIN IMMEDIATE").await.unwrap();
    (dir, pool, lock)
}

#[tokio::test]
async fn wallet_2946_mark_read_retries_external_writer_and_preserves_history() {
    // Arrange: external writer survives the first 100ms SQLite busy wait.
    let (_dir, pool, lock) = wallet_2946_locked_fixture().await;
    let before = ApiBusinessNotificationRepo::find(&pool, "owner", "alert").await.unwrap().unwrap();
    // Act: release during the existing bounded retry window.
    let release = async {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        lock.rollback().await.unwrap();
    };
    let (result, ()) = tokio::join!(
        ApiBusinessNotificationRepo::mark_read_for_withdrawal_wallets(&pool, "alert"),
        release
    );
    result.unwrap();
    // Assert: only read-state bookkeeping changes, and duplicate/missing requests are no-ops.
    ApiBusinessNotificationRepo::mark_read_for_withdrawal_wallets(&pool, "alert").await.unwrap();
    ApiBusinessNotificationRepo::mark_read_for_withdrawal_wallets(&pool, "missing").await.unwrap();
    let after = ApiBusinessNotificationRepo::find(&pool, "owner", "alert").await.unwrap().unwrap();
    assert!(after.is_read);
    assert_eq!(after.payload, before.payload);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.resolved_at, before.resolved_at);
    assert_eq!(after.business_key, before.business_key);
}

#[tokio::test]
async fn wallet_2946_exhausted_retry_preserves_unread_and_releases_gate() {
    // Arrange: keep the external lock until bounded retries have finished.
    let (_dir, pool, lock) = wallet_2946_locked_fixture().await;
    // Act/Assert: permanent contention is reported without marking anything read.
    let err = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        ApiBusinessNotificationRepo::mark_read_for_withdrawal_wallets(&pool, "alert"),
    )
    .await
    .unwrap()
    .unwrap_err();
    match err {
        Error::Database(crate::DatabaseError::Sqlx(sqlx::Error::Database(db))) => {
            assert_eq!(db.code().as_deref(), Some("5"))
        }
        other => panic!("expected SQLITE_BUSY, got {other:?}"),
    }
    assert!(
        !ApiBusinessNotificationRepo::find(&pool, "owner", "alert").await.unwrap().unwrap().is_read
    );
    lock.rollback().await.unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ApiBusinessNotificationRepo::mark_read_for_withdrawal_wallets(&pool, "alert"),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "owner", "alert").await.unwrap().unwrap().is_read
    );
}
