use super::*;
use wallet_database::{
    entities::{api_business_notification::NewApiBusinessNotification, api_wallet::ApiWalletType},
    repositories::api_wallet::wallet::ApiWalletRepo,
};

async fn fixture() -> (tempfile::TempDir, &'static Context) {
    let (dir, ctx) = super::global_business_query_tests::isolated_context().await;
    ApiWalletRepo::upsert(
        &ctx.api_wallet_pool().unwrap(),
        "owner",
        "fixture",
        "wallet",
        &[],
        &[],
        ApiWalletType::Withdrawal,
        None,
        "fixture",
        0,
    )
    .await
    .unwrap();
    (dir, ctx)
}

async fn alert(ctx: &'static Context, id: &str, address: &str) -> ApiBusinessNotification {
    let payload=serde_json::json!({"remaining":2,"thresholdValue":20,"token":"","symbol":"TEST","merchantName":"fixture","waddressType":"1","chainCode":"tron","targetAddress":address}).to_string();
    let pool = ctx.api_wallet_pool().unwrap();
    ApiBusinessNotificationRepo::ingest(
        &pool,
        NewApiBusinessNotification {
            id,
            uid: "owner",
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key: address,
            payload: &payload,
        },
    )
    .await
    .unwrap();
    ApiBusinessNotificationRepo::find(&pool, "owner", id).await.unwrap().unwrap()
}

#[tokio::test]
async fn notification_recovery_persisted_resolution_emits_once_after_commit_and_not_on_replay() {
    // Arrange: two alert objects and a real frontend channel.
    let (_dir, ctx) = fixture().await;
    let rows = vec![alert(ctx, "a", "a").await, alert(ctx, "b", "b").await];
    let balances = vec![
        (wallet_database::entities::assets::AssetsId::new("a", "tron", "".into()), "20".into()),
        (wallet_database::entities::assets::AssetsId::new("b", "tron", "".into()), "20".into()),
    ];
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    ctx.set_frontend_notify_sender(Some(tx)).await;
    let service = ApiBusinessNotificationService::new(ctx);
    // Act: both transitions in a single resolution call should coalesce.
    service.resolve_from_persisted_balances("owner", &balances, &rows).await.unwrap();
    // Assert: committed state is already visible, payload/read facts are preserved.
    let event = rx.try_recv().expect("resolution must invalidate frontend notifications");
    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({"event":"API_WALLET_NOTIFICATION_CHANGED","data":{}})
    );
    assert!(rx.try_recv().is_err());
    for before in &rows {
        let after =
            ApiBusinessNotificationRepo::find(&ctx.api_wallet_pool().unwrap(), "owner", &before.id)
                .await
                .unwrap()
                .unwrap();
        assert!(after.resolved_at.is_some());
        assert!(!after.is_read);
        assert_eq!(after.payload, before.payload);
    }
    service.resolve_from_persisted_balances("owner", &balances, &rows).await.unwrap();
    assert!(rx.try_recv().is_err(), "stale duplicate snapshots cause no new transition/event");
}

#[tokio::test]
async fn notification_recovery_directed_checks_publish_only_actual_resolution() {
    let (_dir, ctx) = fixture().await;
    let row = alert(ctx, "a", "a").await;
    let check = crate::application::api_wallet_balance_check::ApiAddressBalanceCheck::from(&row);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    ctx.set_frontend_notify_sender(Some(tx)).await;
    let service = ApiBusinessNotificationService::new(ctx);
    let now = Instant::now();
    service
        .check_address_balance_using(&check, || now, |_, _, _| async { Ok(Decimal::from(19)) })
        .await
        .unwrap();
    assert!(rx.try_recv().is_err(), "insufficient balance remains unresolved without notification");
    service
        .check_address_balance_using(
            &check,
            || now + Duration::from_secs(5),
            |_, _, _| async { Ok(Decimal::from(20)) },
        )
        .await
        .unwrap();
    assert_eq!(
        rx.try_recv().expect("directed resolution must notify").event,
        "API_WALLET_NOTIFICATION_CHANGED"
    );
    service
        .check_address_balance_using(&check, Instant::now, |_, _, _| async {
            panic!("resolved task must not query again")
        })
        .await
        .unwrap();
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn notification_recovery_closed_frontend_keeps_resolution_committed_without_compensation() {
    let (_dir, ctx) = fixture().await;
    let row = alert(ctx, "a", "a").await;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    drop(rx);
    ctx.set_frontend_notify_sender(Some(tx)).await;
    ApiBusinessNotificationService::new(ctx)
        .resolve_from_persisted_balances(
            "owner",
            &[(
                wallet_database::entities::assets::AssetsId::new("a", "tron", "".into()),
                "20".into(),
            )],
            &[row],
        )
        .await
        .unwrap();
    assert!(
        ApiBusinessNotificationRepo::find(&ctx.api_wallet_pool().unwrap(), "owner", "a")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_some()
    );
    let tasks = wallet_database::repositories::task_queue::TaskQueueRepo::all_tasks_queue(
        &ctx.task_pool().unwrap(),
    )
    .await
    .unwrap();
    assert!(tasks.is_empty(), "frontend delivery must not create synchronization retries");
}

#[tokio::test]
async fn notification_recovery_old_snapshot_does_not_resolve_new_alert_or_publish() {
    let (_dir, ctx) = fixture().await;
    let old = alert(ctx, "old", "a").await;
    let newer = alert(ctx, "new", "a").await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    ctx.set_frontend_notify_sender(Some(tx)).await;
    ApiBusinessNotificationService::new(ctx)
        .resolve_from_persisted_balances(
            "owner",
            &[(
                wallet_database::entities::assets::AssetsId::new("a", "tron", "".into()),
                "20".into(),
            )],
            &[old],
        )
        .await
        .unwrap();
    assert!(
        ApiBusinessNotificationRepo::find(&ctx.api_wallet_pool().unwrap(), "owner", &newer.id)
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_none()
    );
    // Resolving the observed old event is allowed; the new event is never cleared.
    let _ = rx.try_recv();
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn notification_recovery_enqueue_success_means_durable_even_without_dispatcher() {
    let (_dir, ctx) = fixture().await;
    let row = alert(ctx, "a", "a").await;
    assert!(
        crate::application::api_wallet_balance_check::enqueue(ctx, &row, Some("recovery"))
            .await
            .is_ok()
    );
    let tasks = wallet_database::repositories::task_queue::TaskQueueRepo::all_tasks_queue(
        &ctx.task_pool().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].request_body.contains("\"notification_id\":\"a\""));
}
