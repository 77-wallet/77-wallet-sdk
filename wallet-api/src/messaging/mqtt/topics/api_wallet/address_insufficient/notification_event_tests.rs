use super::*;
use crate::service::api_wallet::business_notification::global_business_query_tests::isolated_context;

fn alert() -> AwmAddressInsufficient {
    serde_json::from_value(serde_json::json!({
        "uid": "owner",
        "body": {"remaining":"1", "thresholdValue":"20", "token":"", "symbol":"TRX",
            "merchantName":"fixture", "waddressType":"1", "chainCode":"tron",
            "targetAddress":"address"}
    }))
    .unwrap()
}

#[tokio::test]
async fn notification_event_is_published_after_persistence_and_replay_preserves_read_state() {
    // Arrange: isolated SQLite and the real frontend channel, without task dispatch handles.
    let (_dir, ctx) = isolated_context().await;
    let pool = ctx.api_wallet_pool().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    ctx.set_frontend_notify_sender(Some(tx)).await;
    let alert = alert();

    // Act: optional balance-check dispatch can fail after persistence.
    let _ = alert.exec(ctx, "event-1").await;

    // Assert: a refresh event sees durable data and carries no stale count/snapshot.
    let event = rx.try_recv().expect("persisted notification must invalidate frontend state");
    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({"event":"API_WALLET_NOTIFICATION_CHANGED", "data":{}})
    );
    let before =
        ApiBusinessNotificationRepo::find(&pool, "owner", "event-1").await.unwrap().unwrap();
    ApiBusinessNotificationRepo::mark_read(&pool, "owner", "event-1").await.unwrap();
    let _ = alert.exec(ctx, "event-1").await;
    assert_eq!(rx.try_recv().unwrap().event, "API_WALLET_NOTIFICATION_CHANGED");
    let after =
        ApiBusinessNotificationRepo::find(&pool, "owner", "event-1").await.unwrap().unwrap();
    assert!(after.is_read);
    assert_eq!(after.payload, before.payload);
    assert_eq!(after.created_at, before.created_at);
}

#[tokio::test]
async fn failed_notification_persistence_emits_no_event() {
    // Arrange: a real database with a failing insert, and an active frontend receiver.
    let (_dir, ctx) = isolated_context().await;
    let pool = ctx.api_wallet_pool().unwrap();
    sqlx::query("CREATE TRIGGER reject_notification BEFORE INSERT ON api_business_notification BEGIN SELECT RAISE(ABORT, 'fixture'); END")
        .execute(pool.write_ref()).await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    ctx.set_frontend_notify_sender(Some(tx)).await;
    // Act / Assert: persistence failure cannot announce data that does not exist.
    assert!(alert().exec(ctx, "event-failed").await.is_err());
    assert!(rx.try_recv().is_err());
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "owner", "event-failed").await.unwrap().is_none()
    );
}

#[tokio::test]
async fn unavailable_frontend_does_not_prevent_balance_check_enqueue() {
    // Arrange: both absent and closed receivers must preserve MQTT work.
    for closed in [false, true] {
        let (_dir, ctx) = isolated_context().await;
        if closed {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            drop(rx);
            ctx.set_frontend_notify_sender(Some(tx)).await;
        }
        // Act: dispatch has no handles, but its durable task must still be enqueued.
        let _ = alert().exec(ctx, "event-offline").await;
        // Assert: neither persistence nor directed check creation depends on frontend delivery.
        let pool = ctx.api_wallet_pool().unwrap();
        assert!(
            ApiBusinessNotificationRepo::find(&pool, "owner", "event-offline")
                .await
                .unwrap()
                .is_some()
        );
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM task_queue WHERE id LIKE 'api-address-balance:%'",
        )
        .fetch_one(ctx.task_pool().unwrap().read_ref())
        .await
        .unwrap();
        assert_eq!(count, 1);
    }
}
