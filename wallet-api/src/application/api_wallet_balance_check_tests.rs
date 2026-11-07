use super::*;
use crate::service::api_wallet::business_notification::global_business_query_tests::isolated_context;
use rust_decimal::Decimal;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
use wallet_database::{
    entities::{api_business_notification::NewApiBusinessNotification, api_wallet::ApiWalletType},
    repositories::{
        api_business_notification::ApiBusinessNotificationRepo, api_wallet::wallet::ApiWalletRepo,
    },
};

async fn owner(ctx: &'static Context, uid: &str, kind: ApiWalletType) {
    ApiWalletRepo::upsert(
        &ctx.api_wallet_pool().unwrap(),
        uid,
        "fixture",
        uid,
        &[],
        &[],
        kind,
        None,
        "fixture",
        0,
    )
    .await
    .unwrap();
}
async fn alert(
    ctx: &'static Context,
    uid: &str,
    id: &str,
    threshold: i64,
) -> ApiBusinessNotification {
    let pool = ctx.api_wallet_pool().unwrap();
    let payload=serde_json::json!({"remaining":2,"thresholdValue":threshold,"token":"","symbol":"trx","merchantName":"fixture","waddressType":"2","chainCode":"tron","targetAddress":"TAddress"}).to_string();
    let business_key = format!("{uid}:tron:TAddress:");
    ApiBusinessNotificationRepo::ingest(
        &pool,
        NewApiBusinessNotification {
            id,
            uid,
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key: &business_key,
            payload: &payload,
        },
    )
    .await
    .unwrap();
    ApiBusinessNotificationRepo::find(&pool, uid, id).await.unwrap().unwrap()
}

#[tokio::test]
async fn directed_checks_need_no_asset_row_and_coalesce_exact_payloads() {
    // Arrange: two active owners, with no account or asset rows.
    let (_dir, ctx) = isolated_context().await;
    owner(ctx, "owner", ApiWalletType::Withdrawal).await;
    owner(ctx, "other", ApiWalletType::Withdrawal).await;
    let row = alert(ctx, "owner", "alert", 20).await;
    let other = alert(ctx, "other", "other-alert", 30).await;
    let check = ApiAddressBalanceCheck::from(&row);
    let service = ApiBusinessNotificationService::new(ctx);
    let now = Instant::now();
    let calls = AtomicUsize::new(0);
    // Act: simultaneous tasks share one completed observation, preserving read state.
    let balance_for = |address: String, chain: String, token: String| {
        assert_eq!((address.as_str(), chain.as_str(), token.as_str()), ("TAddress", "tron", ""));
        calls.fetch_add(1, Ordering::Relaxed);
        async { Ok(Decimal::new(10, 0)) }
    };
    // Either caller may acquire the asynchronous lock first. Count real calls
    // rather than assigning the first query to a particular future.
    let a = service.check_address_balance_using(&check, || now, &balance_for);
    let b = service.check_address_balance_using(&check, || now, &balance_for);
    let (a, b) = tokio::join!(a, b);
    a.unwrap();
    b.unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let pool = ctx.api_wallet_pool().unwrap();
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "owner", "alert")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_none()
    );
    service
        .check_address_balance_using(
            &ApiAddressBalanceCheck::from(&other),
            || now,
            |_, _, _| async { Ok(Decimal::new(30, 0)) },
        )
        .await
        .unwrap();
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "other", "other-alert")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_some()
    );
    // A genuinely new event with a changed threshold needs a fresh observation.
    // The older event keeps its own cached snapshot and history.
    let changed = alert(ctx, "owner", "alert-new", 25).await;
    service
        .check_address_balance_using(&check, || now, |_, _, _| async { panic!("stale task") })
        .await
        .unwrap();
    service
        .check_address_balance_using(
            &ApiAddressBalanceCheck::from(&changed),
            || now,
            |_, _, _| async { Ok(Decimal::new(25, 0)) },
        )
        .await
        .unwrap();
    let final_row =
        ApiBusinessNotificationRepo::find(&pool, "owner", "alert-new").await.unwrap().unwrap();
    assert!(final_row.resolved_at.is_some());
    assert!(!final_row.is_read);
}

#[tokio::test]
async fn failed_checks_retain_rows_and_retry_after_five_seconds() {
    let (_dir, ctx) = isolated_context().await;
    owner(ctx, "owner", ApiWalletType::Withdrawal).await;
    let row = alert(ctx, "owner", "failure", 20).await;
    let check = ApiAddressBalanceCheck::from(&row);
    let service = ApiBusinessNotificationService::new(ctx);
    let now = Instant::now();
    assert!(
        service
            .check_address_balance_using(
                &check,
                || now,
                |_, _, _| async { Err(ServiceError::Parameter("offline".into())) }
            )
            .await
            .is_err()
    );
    assert!(
        service
            .check_address_balance_using(
                &check,
                || now,
                |_, _, _| async { panic!("failed observation must coalesce") }
            )
            .await
            .is_err()
    );
    assert!(
        ApiBusinessNotificationRepo::find(&ctx.api_wallet_pool().unwrap(), "owner", "failure")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_none()
    );
    service
        .check_address_balance_using(
            &check,
            || now + std::time::Duration::from_secs(5),
            |_, _, _| async { Ok(Decimal::new(20, 0)) },
        )
        .await
        .unwrap();
    assert!(
        ApiBusinessNotificationRepo::find(&ctx.api_wallet_pool().unwrap(), "owner", "failure")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_some()
    );
}

#[tokio::test]
async fn durable_tasks_deduplicate_delivery_and_seed_only_active_owners() {
    let (_dir, ctx) = isolated_context().await;
    owner(ctx, "owner", ApiWalletType::Withdrawal).await;
    let row = alert(ctx, "owner", "queue", 20).await;
    alert(ctx, "orphan", "orphan", 20).await;
    // Dispatch may be unavailable in this isolated context, but persistence precedes it.
    let _ = enqueue(ctx, &row, None).await;
    let _ = enqueue(ctx, &row, None).await;
    let pool = ctx.task_pool().unwrap();
    let rows = TaskQueueRepo::all_tasks_queue(&pool).await.unwrap();
    let directed: Vec<_> = rows
        .iter()
        .filter(|r| r.task_name == TaskName::Known(KnownTaskName::CheckApiAddressBalance))
        .collect();
    assert_eq!(directed.len(), 1);
    let _ = seed_ready_unresolved(ctx).await;
    let rows = TaskQueueRepo::all_tasks_queue(&pool).await.unwrap();
    let directed: Vec<_> = rows
        .iter()
        .filter(|r| r.task_name == TaskName::Known(KnownTaskName::CheckApiAddressBalance))
        .collect();
    assert_eq!(directed.len(), 2);
    for task in directed {
        let restored =
            <Box<dyn crate::infrastructure::task_queue::task::TaskTrait>>::try_from(task).unwrap();
        assert_eq!(
            restored.get_type(),
            crate::infrastructure::task_queue::task::task_type::TaskType::Common
        );
        let body: ApiAddressBalanceCheck = serde_json::from_str(&task.request_body).unwrap();
        assert_eq!(body.uid, "owner");
        assert_eq!(body.payload, row.payload);
    }
}

#[tokio::test]
async fn resolution_write_failure_persists_compensation_and_reuses_observation() {
    use wallet_database::entities::{asset_token_key::AssetTokenKey, assets::AssetsId};
    let (_dir, ctx) = isolated_context().await;
    owner(ctx, "owner", ApiWalletType::Withdrawal).await;
    let row = alert(ctx, "owner", "write-failure", 20).await;
    let pool = ctx.api_wallet_pool().unwrap();
    sqlx::query("CREATE TRIGGER fail_resolution BEFORE UPDATE OF resolved_at ON api_business_notification WHEN NEW.resolved_at IS NOT NULL BEGIN SELECT RAISE(FAIL, 'fixture failure'); END")
        .execute(pool.write_ref()).await.unwrap();
    let service = ApiBusinessNotificationService::new(ctx);
    assert!(
        service
            .resolve_from_persisted_balances(
                "owner",
                &[(
                    AssetsId::new("TAddress", "tron", AssetTokenKey::from_raw(Some(""))),
                    "20".into()
                )],
                &[row.clone()]
            )
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "owner", &row.id)
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_none()
    );
    let tasks = TaskQueueRepo::all_tasks_queue(&ctx.task_pool().unwrap()).await.unwrap();
    assert!(
        tasks.iter().any(|t| t.task_name == TaskName::Known(KnownTaskName::CheckApiAddressBalance)
            && t.request_body.contains("write-failure"))
    );
    sqlx::query("DROP TRIGGER fail_resolution").execute(pool.write_ref()).await.unwrap();
    service
        .check_address_balance_using(
            &ApiAddressBalanceCheck::from(&row),
            Instant::now,
            |_, _, _| async { panic!("persisted successful observation must be reused") },
        )
        .await
        .unwrap();
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "owner", &row.id)
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_some()
    );
}

#[tokio::test]
async fn newer_event_arriving_during_check_cannot_be_resolved_by_old_result() {
    let (_dir, ctx) = isolated_context().await;
    owner(ctx, "owner", ApiWalletType::Withdrawal).await;
    let row = alert(ctx, "owner", "in-flight", 20).await;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let channels = std::sync::Mutex::new(Some((started_tx, release_rx)));
    let service = ApiBusinessNotificationService::new(ctx);
    let check = ApiAddressBalanceCheck::from(&row);
    let query = service.check_address_balance_using(&check, Instant::now, |_, _, _| {
        let (started, release) = channels.lock().unwrap().take().unwrap();
        async move {
            started.send(()).unwrap();
            release.await.unwrap();
            Ok(Decimal::new(20, 0))
        }
    });
    let replacement = async {
        started_rx.await.unwrap();
        alert(ctx, "owner", "in-flight-new", 10).await;
        release_tx.send(()).unwrap();
    };
    let (result, _) = tokio::join!(query, replacement);
    result.unwrap();
    let current = ApiBusinessNotificationRepo::find(
        &ctx.api_wallet_pool().unwrap(),
        "owner",
        "in-flight-new",
    )
    .await
    .unwrap()
    .unwrap();
    assert!(current.resolved_at.is_none());
    assert_ne!(current.payload, row.payload);
    let original =
        ApiBusinessNotificationRepo::find(&ctx.api_wallet_pool().unwrap(), "owner", "in-flight")
            .await
            .unwrap()
            .unwrap();
    assert!(original.resolved_at.is_some());
    assert_eq!(original.payload, row.payload);
}
