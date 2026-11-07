use super::*;
use crate::{
    entities::api_wallet::ApiWalletType, repositories::test_helper::setup_api_wallet_pool,
};

#[tokio::test]
async fn global_message_center_filters_orders_pages_and_read_writes_by_active_wallet() {
    // Arrange: duplicate eligible wallet rows, foreign scopes and resolved unread history.
    let pool = setup_api_wallet_pool("withdrawal_message_center_scope").await;
    for (uid, address, kind, status) in [
        ("a", "a1", ApiWalletType::Withdrawal, 1),
        ("a", "a2", ApiWalletType::Withdrawal, 1),
        ("b", "b1", ApiWalletType::Withdrawal, 1),
        ("sub", "sub1", ApiWalletType::SubAccount, 1),
        ("off", "off1", ApiWalletType::Withdrawal, 0),
        (" ", "blank1", ApiWalletType::Withdrawal, 1),
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
    for (uid, id) in [
        ("a", "01"),
        ("b", "02"),
        ("a", "03"),
        ("sub", "04"),
        ("off", "05"),
        ("orphan", "06"),
        (" ", "07"),
    ] {
        ApiBusinessNotificationRepo::upsert(
            &pool,
            NewApiBusinessNotification {
                id,
                uid,
                kind: "AWM_ADDRESS_INSUFFICIENT",
                business_key: id,
                payload: "{}",
            },
        )
        .await
        .unwrap();
    }
    ApiBusinessNotificationRepo::mark_resolved(&pool, "b", "02", true).await.unwrap();
    // Act: global pagination and unread count include two wallets without duplicate messages.
    let first =
        ApiBusinessNotificationRepo::list_for_withdrawal_wallets(&pool, false, 0, 2).await.unwrap();
    let second =
        ApiBusinessNotificationRepo::list_for_withdrawal_wallets(&pool, false, 1, 2).await.unwrap();
    // Assert: global ordering happens before pagination; resolved unread history remains.
    assert_eq!(first.total_count, 3);
    assert_eq!(first.data.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["03", "02"]);
    assert_eq!(second.data[0].id, "01");
    assert_eq!(
        ApiBusinessNotificationRepo::unread_count_for_withdrawal_wallets(&pool).await.unwrap(),
        3
    );
    assert_eq!(
        ApiBusinessNotificationRepo::list_for_withdrawal_wallets(&pool, true, 0, 10)
            .await
            .unwrap()
            .total_count,
        2
    );
    for id in ["04", "05", "06", "07", "missing"] {
        ApiBusinessNotificationRepo::mark_read_for_withdrawal_wallets(&pool, id).await.unwrap();
    }
    assert!(!ApiBusinessNotificationRepo::find(&pool, "sub", "04").await.unwrap().unwrap().is_read);
    ApiBusinessNotificationRepo::mark_read_for_withdrawal_wallets(&pool, "01").await.unwrap();
    ApiBusinessNotificationRepo::mark_read_for_withdrawal_wallets(&pool, "01").await.unwrap();
    assert_eq!(
        ApiBusinessNotificationRepo::unread_count_for_withdrawal_wallets(&pool).await.unwrap(),
        2
    );
    ApiBusinessNotificationRepo::mark_all_read_for_withdrawal_wallets(&pool).await.unwrap();
    ApiBusinessNotificationRepo::mark_all_read_for_withdrawal_wallets(&pool).await.unwrap();
    assert_eq!(
        ApiBusinessNotificationRepo::unread_count_for_withdrawal_wallets(&pool).await.unwrap(),
        0
    );
    for (uid, id) in [("sub", "04"), ("off", "05"), ("orphan", "06"), (" ", "07")] {
        assert!(!ApiBusinessNotificationRepo::find(&pool, uid, id).await.unwrap().unwrap().is_read);
    }
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "b", "02")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_some()
    );
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "a", "03")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_none()
    );
    // The write statement rechecks eligibility after a wallet is disabled.
    sqlx::query("UPDATE api_wallet SET status = 0 WHERE uid IN ('a', 'b')")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert_eq!(
        ApiBusinessNotificationRepo::list_for_withdrawal_wallets(&pool, false, 0, 10)
            .await
            .unwrap()
            .total_count,
        0
    );
    assert_eq!(
        ApiBusinessNotificationRepo::unread_count_for_withdrawal_wallets(&pool).await.unwrap(),
        0
    );
}

#[tokio::test]
async fn missing_wallet_scope_table_fails_without_marking_any_message_read() {
    // Arrange: a real durable row and an unavailable scope table.
    let pool = setup_api_wallet_pool("withdrawal_message_center_failure").await;
    ApiBusinessNotificationRepo::upsert(
        &pool,
        NewApiBusinessNotification {
            id: "failure",
            uid: "a",
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key: "failure",
            payload: "{}",
        },
    )
    .await
    .unwrap();
    sqlx::query("ALTER TABLE api_wallet RENAME TO unavailable_wallet")
        .execute(pool.write_ref())
        .await
        .unwrap();
    // Act/Assert: errors are visible and failed writes preserve the read state.
    assert!(
        ApiBusinessNotificationRepo::list_for_withdrawal_wallets(&pool, false, 0, 10)
            .await
            .is_err()
    );
    assert!(ApiBusinessNotificationRepo::unread_count_for_withdrawal_wallets(&pool).await.is_err());
    assert!(
        ApiBusinessNotificationRepo::mark_read_for_withdrawal_wallets(&pool, "failure")
            .await
            .is_err()
    );
    assert!(
        ApiBusinessNotificationRepo::mark_all_read_for_withdrawal_wallets(&pool).await.is_err()
    );
    assert!(
        !ApiBusinessNotificationRepo::find(&pool, "a", "failure").await.unwrap().unwrap().is_read
    );
}
