use super::{global_business_query_tests::isolated_context, *};
use crate::{error::service::ServiceError, manager::WalletManager};
use wallet_database::{
    entities::{api_business_notification::NewApiBusinessNotification, api_wallet::ApiWalletType},
    repositories::api_wallet::wallet::ApiWalletRepo,
};

#[tokio::test]
async fn public_detail_banner_and_history_preserve_scope_and_read_semantics() {
    // Arrange: isolated DB, active owners plus subaccount, disabled and orphan messages.
    let (_dir, ctx) = isolated_context().await;
    let manager = WalletManager { ctx, handles: None };
    let pool = ctx.api_wallet_pool().unwrap();
    let payload = serde_json::json!({"remaining":"500", "thresholdValue":"600", "token":"",
        "symbol":"USDT", "merchantName":"Demo", "targetAddress":"address", "chainCode":"tron",
        "waddressType":"2", "notifiedAt":"2026-10-07T11:31:00Z"})
    .to_string();
    for (uid, kind, status) in [
        ("a", ApiWalletType::Withdrawal, 1),
        ("b", ApiWalletType::Withdrawal, 1),
        ("sub", ApiWalletType::SubAccount, 1),
        ("off", ApiWalletType::Withdrawal, 0),
    ] {
        ApiWalletRepo::upsert(&pool, uid, "fixture", uid, &[], &[], kind, None, "fixture", 0)
            .await
            .unwrap();
        sqlx::query("UPDATE api_wallet SET status = ? WHERE uid = ?")
            .bind(status)
            .bind(uid)
            .execute(pool.write_ref())
            .await
            .unwrap();
    }
    for uid in ["a", "b", "sub", "off", "orphan"] {
        ApiBusinessNotificationRepo::upsert(
            &pool,
            NewApiBusinessNotification {
                id: uid,
                uid,
                kind: "AWM_ADDRESS_INSUFFICIENT",
                business_key: uid,
                payload: &payload,
            },
        )
        .await
        .unwrap();
    }
    ApiBusinessNotificationRepo::mark_resolved(&pool, "a", "a", true).await.unwrap();
    let before = ApiBusinessNotificationRepo::find(&pool, "a", "a").await.unwrap().unwrap();
    let guard = ctx.address_balance_check.lock().await;
    // Act: detail and banner are local, even when the chain-refresh lock is held.
    let detail = tokio::time::timeout(
        Duration::from_secs(1),
        manager.get_api_business_notification_detail("a"),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    let history = manager.page_api_business_notifications(0, 10).await.unwrap();
    let banner = manager.get_unresolved_api_address_balance_notifications().await.unwrap();
    // Assert: resolved history is readable and remains unread; banner excludes resolved rows.
    assert_eq!(detail.remaining, "500");
    assert!(!detail.is_read);
    assert!(detail.resolved_at.is_some());
    assert_eq!(history.total_count, 2);
    assert_eq!(history.data.len(), 2);
    assert_eq!(banner.len(), 1);
    assert_eq!(banner[0].id, "b");
    assert_eq!(banner[0].notified_at.unwrap().to_rfc3339(), "2026-10-07T11:31:00+00:00");
    assert_eq!(manager.count_unread_api_wallet_notifications().await.unwrap(), 2);
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(
            ApiBusinessNotificationRepo::find(&pool, "a", "a").await.unwrap().unwrap()
        )
        .unwrap()
    );
    for id in ["sub", "off", "orphan", "missing"] {
        assert!(manager.get_api_business_notification_detail(id).await.unwrap().is_none());
    }
    assert!(matches!(
        manager.get_api_business_notification_detail(" ").await,
        Err(ServiceError::Parameter(_))
    ));
    manager.mark_api_business_notification_read("a").await.unwrap();
    assert!(manager.get_api_business_notification_detail("a").await.unwrap().unwrap().is_read);
    assert!(
        manager
            .get_api_business_notification_detail("a")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_some()
    );
    sqlx::query("UPDATE api_wallet SET status = 0 WHERE uid = 'a'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert!(manager.get_api_business_notification_detail("a").await.unwrap().is_none());
    drop(guard);
}

#[tokio::test]
async fn invalid_detail_preserves_history_and_unread_count() {
    // Arrange: an eligible future kind and a corrupt balance record in local history.
    let (_dir, ctx) = isolated_context().await;
    let manager = WalletManager { ctx, handles: None };
    let pool = ctx.api_wallet_pool().unwrap();
    ApiWalletRepo::upsert(
        &pool,
        "a",
        "fixture",
        "a",
        &[],
        &[],
        ApiWalletType::Withdrawal,
        None,
        "fixture",
        0,
    )
    .await
    .unwrap();
    for (id, kind, payload) in [
        ("unknown", "FUTURE_WITHDRAWAL", r#"{"amount":"500"}"#),
        ("corrupt", "AWM_ADDRESS_INSUFFICIENT", "{}"),
    ] {
        ApiBusinessNotificationRepo::upsert(
            &pool,
            NewApiBusinessNotification { id, uid: "a", kind, business_key: id, payload },
        )
        .await
        .unwrap();
    }
    // Act/Assert: failing detail has no write side effects; list totals and entries remain intact.
    for id in ["unknown", "corrupt"] {
        assert!(matches!(
            manager.get_api_business_notification_detail(id).await,
            Err(ServiceError::Parameter(_))
        ));
        assert!(!ApiBusinessNotificationRepo::find(&pool, "a", id).await.unwrap().unwrap().is_read);
    }
    let page = manager.page_api_business_notifications(0, 1).await.unwrap();
    assert_eq!(page.total_count, 2);
    assert_eq!(page.page_size, 1);
    assert_eq!(page.data.len(), 1);
    assert!(page.data[0].remaining.is_none());
    assert!(page.data[0].symbol.is_none());
    assert_eq!(manager.count_unread_api_wallet_notifications().await.unwrap(), 2);
}

async fn wallet_2946_fixture() -> (tempfile::TempDir, &'static crate::context::Context) {
    let (dir, ctx) = isolated_context().await;
    let pool = ctx.api_wallet_pool().unwrap();
    ApiWalletRepo::upsert(
        &pool,
        "repro",
        "fixture",
        "repro",
        &[],
        &[],
        ApiWalletType::Withdrawal,
        None,
        "fixture",
        0,
    )
    .await
    .unwrap();
    let payload = serde_json::json!({"remaining":"1.1", "thresholdValue":"2", "token":"",
        "symbol":"USDC", "merchantName":"Demo", "targetAddress":"address", "chainCode":"eth",
        "waddressType":"2", "notifiedAt":"2026-10-24T09:36:00Z"})
    .to_string();
    ApiBusinessNotificationRepo::ingest(
        &pool,
        NewApiBusinessNotification {
            id: "repro",
            uid: "repro",
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key: "repro",
            payload: &payload,
        },
    )
    .await
    .unwrap();
    (dir, ctx)
}

#[tokio::test]
async fn wallet_2946_detail_remains_available_while_writer_connection_is_held() {
    // Arrange: the actual Context must preserve the provider's separate pools.
    let (_dir, ctx) = wallet_2946_fixture().await;
    let manager = WalletManager { ctx, handles: None };
    let pool = ctx.api_wallet_pool().unwrap();
    assert!(!std::sync::Arc::ptr_eq(&pool.read_pool(), &pool.write_pool()));
    let writer = pool.write_ref().begin_with("BEGIN IMMEDIATE").await.unwrap();
    // Act/Assert: a busy writer cannot consume the reader's connection budget.
    let detail = tokio::time::timeout(
        Duration::from_secs(2),
        manager.get_api_business_notification_detail("repro"),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert!(!detail.is_read);
    writer.rollback().await.unwrap();
    manager.mark_api_business_notification_read("repro").await.unwrap();
    assert!(manager.get_api_business_notification_detail("repro").await.unwrap().unwrap().is_read);
}

#[tokio::test]
async fn wallet_2946_context_calls_share_the_same_write_gate() {
    // Arrange: separately requested wrappers must share the coordination lock.
    let (_dir, ctx) = wallet_2946_fixture().await;
    let first = ctx.api_wallet_pool().unwrap();
    let second = ctx.api_wallet_pool().unwrap();
    let guard = first.lock_write().await;
    // Act/Assert: a second acquisition waits, then succeeds after release.
    assert!(tokio::time::timeout(Duration::from_millis(50), second.lock_write()).await.is_err());
    drop(guard);
    let _guard = tokio::time::timeout(Duration::from_secs(2), second.lock_write()).await.unwrap();
}

#[tokio::test]
async fn wallet_2946_mark_read_waits_for_gate_and_cancelled_wait_preserves_unread() {
    // Arrange: another operation holds the Context-wide API writer gate.
    let (_dir, ctx) = wallet_2946_fixture().await;
    let pool = ctx.api_wallet_pool().unwrap();
    let guard = pool.lock_write().await;
    // Act/Assert: a cancelled wait performs no update; a later attempt succeeds.
    let manager = WalletManager { ctx, handles: None };
    assert!(
        tokio::time::timeout(
            Duration::from_millis(50),
            manager.mark_api_business_notification_read("repro")
        )
        .await
        .is_err()
    );
    assert!(!manager.get_api_business_notification_detail("repro").await.unwrap().unwrap().is_read);
    drop(guard);
    manager.mark_api_business_notification_read("repro").await.unwrap();
    assert!(manager.get_api_business_notification_detail("repro").await.unwrap().unwrap().is_read);
}
