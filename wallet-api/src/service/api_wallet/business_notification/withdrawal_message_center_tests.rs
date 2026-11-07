use super::ApiBusinessNotificationService;
use wallet_database::{
    entities::{api_business_notification::NewApiBusinessNotification, api_wallet::ApiWalletType},
    repositories::{
        api_business_notification::ApiBusinessNotificationRepo, api_wallet::wallet::ApiWalletRepo,
    },
};

#[tokio::test]
async fn no_uid_message_api_keeps_owner_and_reads_without_balance_checks() {
    // Arrange: isolated wallets, resolved unread history and excluded subaccount.
    let (_dir, ctx) = super::global_business_query_tests::isolated_context().await;
    let pool = ctx.api_wallet_pool().unwrap();
    for (uid, kind) in [
        ("a", ApiWalletType::Withdrawal),
        ("b", ApiWalletType::Withdrawal),
        ("sub", ApiWalletType::SubAccount),
    ] {
        ApiWalletRepo::upsert(&pool, uid, "fixture", uid, &[], &[], kind, None, "fixture", 0)
            .await
            .unwrap();
        ApiBusinessNotificationRepo::upsert(
            &pool,
            NewApiBusinessNotification {
                id: uid,
                uid,
                kind: "AWM_ADDRESS_INSUFFICIENT",
                business_key: uid,
                payload: r#"{"remaining":"1","thresholdValue":"20","token":"","symbol":"TRX","merchantName":"fixture","waddressType":"1","chainCode":"tron","targetAddress":"address"}"#,
            },
        )
        .await
        .unwrap();
    }
    ApiBusinessNotificationRepo::mark_resolved(&pool, "a", "a", true).await.unwrap();
    let manager = crate::manager::WalletManager { ctx, handles: None };
    let _guard = ctx.address_balance_check.lock().await;
    // Act: public SDK methods accept no UID and complete while the balance lock is held.
    let page = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        manager.page_api_business_notifications(0, 1),
    )
    .await
    .unwrap()
    .unwrap();
    // Assert: owners remain on records, unread includes resolved history and read writes stay scoped.
    assert_eq!(page.total_count, 2);
    assert_eq!(page.data[0].id, "b");
    assert_eq!(manager.count_unread_api_wallet_notifications().await.unwrap(), 2);
    manager.mark_api_business_notification_read("sub").await.unwrap();
    assert!(
        !ApiBusinessNotificationRepo::find(&pool, "sub", "sub").await.unwrap().unwrap().is_read
    );
    manager.mark_api_business_notification_read("b").await.unwrap();
    manager.mark_all_api_business_notifications_read().await.unwrap();
    assert_eq!(manager.count_unread_api_wallet_notifications().await.unwrap(), 0);
    assert_eq!(manager.get_unresolved_api_address_balance_notifications().await.unwrap().len(), 1);
    assert!(manager.page_api_business_notifications(-1, 10).await.is_err());
    assert!(manager.page_api_business_notifications(0, 0).await.is_err());
    assert!(manager.page_api_business_notifications(i64::MAX, 2).await.is_err());
    assert!(manager.mark_api_business_notification_read(" ").await.is_err());
}

#[tokio::test]
async fn global_unresolved_refresh_filters_before_chain_io_and_repages_after_resolution() {
    // Arrange: only eligible wallets may be checked; failures retain durable messages.
    let (_dir, ctx) = super::global_business_query_tests::isolated_context().await;
    let pool = ctx.api_wallet_pool().unwrap();
    for (uid, kind) in [
        ("a", ApiWalletType::Withdrawal),
        ("b", ApiWalletType::Withdrawal),
        ("sub", ApiWalletType::SubAccount),
    ] {
        ApiWalletRepo::upsert(&pool, uid, "fixture", uid, &[], &[], kind, None, "fixture", 0)
            .await
            .unwrap();
        let payload = serde_json::json!({"remaining":2,"thresholdValue":10,"token":"","symbol":"trx","merchantName":"fixture","waddressType":"2","chainCode":"tron","targetAddress":uid}).to_string();
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
    let service = ApiBusinessNotificationService::new(ctx);
    let mut checked = Vec::new();
    // Act: preserve the existing direct-refresh path while broadening its eligible scope.
    let page = service
        .list_unresolved_scoped_with_refresh_using(None, 0, 1, |address, _, _| {
            checked.push(address.clone());
            async move {
                if address == "a" {
                    Ok(rust_decimal::Decimal::from(10))
                } else {
                    Err(crate::error::service::ServiceError::Timeout)
                }
            }
        })
        .await
        .unwrap();
    // Assert: no subaccount IO, fresh pagination after resolution, no automatic read-state change.
    checked.sort();
    assert_eq!(checked, ["a", "b"]);
    assert_eq!(page.total_count, 1);
    assert_eq!(page.data[0].uid, "b");
    assert!(!page.data[0].is_read);
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "a", "a")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_some()
    );
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "sub", "sub")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_none()
    );
}

#[tokio::test]
async fn api_wallet_unread_count_includes_future_kinds_and_excludes_ineligible_owners() {
    // Arrange: unknown kinds do not require a balance-alert payload to count as unread.
    let (_dir, ctx) = super::global_business_query_tests::isolated_context().await;
    let pool = ctx.api_wallet_pool().unwrap();
    for (uid, kind, active) in [
        ("owner", ApiWalletType::Withdrawal, true),
        ("sub", ApiWalletType::SubAccount, true),
        ("disabled", ApiWalletType::Withdrawal, false),
    ] {
        ApiWalletRepo::upsert(&pool, uid, "fixture", uid, &[], &[], kind, None, "fixture", 0)
            .await
            .unwrap();
        if !active {
            sqlx::query("UPDATE api_wallet SET status = 0 WHERE uid = ?")
                .bind(uid)
                .execute(pool.write_ref())
                .await
                .unwrap();
        }
    }
    for (id, uid) in [
        ("future", "owner"),
        ("read", "owner"),
        ("sub", "sub"),
        ("disabled", "disabled"),
        ("orphan", "orphan"),
    ] {
        ApiBusinessNotificationRepo::ingest(
            &pool,
            NewApiBusinessNotification {
                id,
                uid,
                kind: "FUTURE_API_WALLET_NOTIFICATION",
                business_key: id,
                payload: "{}",
            },
        )
        .await
        .unwrap();
    }
    ApiBusinessNotificationRepo::mark_resolved(&pool, "owner", "future", true).await.unwrap();
    ApiBusinessNotificationRepo::mark_read(&pool, "owner", "read").await.unwrap();
    // Act: query public notification-wide API while the chain-check lock is held.
    let _guard = ctx.address_balance_check.lock().await;
    let manager = crate::manager::WalletManager { ctx, handles: None };
    let count = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        manager.count_unread_api_wallet_notifications(),
    )
    .await
    .unwrap()
    .unwrap();
    // Assert: resolved future-kind unread history counts; read and out-of-scope rows do not.
    assert_eq!(count, 1);
    let rows = ApiBusinessNotificationRepo::list(&pool, "owner", false, 0, 10).await.unwrap();
    assert_eq!(rows.total_count, 2);
}
