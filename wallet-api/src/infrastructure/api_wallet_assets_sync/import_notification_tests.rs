use super::*;
use crate::service::api_wallet::business_notification::global_business_query_tests::isolated_context;
use wallet_database::{
    entities::{api_business_notification::NewApiBusinessNotification, api_wallet::ApiWalletType},
    repositories::{
        api_business_notification::ApiBusinessNotificationRepo,
        api_wallet::{coin::ApiCoinRepo, wallet::ApiWalletRepo},
    },
};
async fn fixture(kind: ApiWalletType) -> (tempfile::TempDir, &'static crate::context::Context) {
    let (dir, ctx) = isolated_context().await;
    let pool = ctx.api_wallet_pool().unwrap();
    ApiWalletRepo::upsert(&pool, "owner", "fixture", "wallet", &[], &[], kind, None, "fixture", 0)
        .await
        .unwrap();
    ApiCoinRepo::upsert_multi_coin(
        &pool,
        vec![wallet_database::entities::api_coin::ApiCoinData::new(
            Some("TEST".into()),
            "TEST",
            "tron",
            "".into(),
            Some("1".into()),
            None,
            0,
            1,
            1,
            1,
            chrono::Utc::now(),
            None,
        )],
    )
    .await
    .unwrap();
    alert(ctx, "old").await;
    (dir, ctx)
}
async fn alert(ctx: &'static crate::context::Context, id: &str) {
    let payload=serde_json::json!({"remaining":2,"thresholdValue":20,"token":"","symbol":"TEST","merchantName":"fixture","waddressType":"1","chainCode":"tron","targetAddress":"a"}).to_string();
    ApiBusinessNotificationRepo::ingest(
        &ctx.api_wallet_pool().unwrap(),
        NewApiBusinessNotification {
            id,
            uid: "owner",
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key: "a",
            payload: &payload,
        },
    )
    .await
    .unwrap();
}
fn response(
    amount: f64,
) -> wallet_transport_backend::response_vo::api_wallet::address::AssetsListRes {
    serde_json::from_value(serde_json::json!([{"index":0,"addressList":[{"address":"a","tokenInfos":[{"tokenCode":"TEST","tokenAddress":"","amount":amount,"assets":0}]}]}])).unwrap()
}
async fn resolved(ctx: &'static crate::context::Context, id: &str) -> bool {
    ApiBusinessNotificationRepo::find(&ctx.api_wallet_pool().unwrap(), "owner", id)
        .await
        .unwrap()
        .unwrap()
        .resolved_at
        .is_some()
}
async fn run(ctx: &'static crate::context::Context, amount: f64) -> Result<(), ServiceError> {
    query_and_upsert_assets_using(
        ctx,
        &ctx.api_wallet_pool().unwrap(),
        &AssetListReq::new("owner", "tron", vec![0]),
        async { Ok(response(amount)) },
    )
    .await
}
#[tokio::test]
async fn import_notification_no_account_or_handles_resolves_and_replay_is_silent() {
    // Arrange: locally absent account, no RPC/handles. The backend import is the only observation.
    let (_dir, ctx) = fixture(ApiWalletType::Withdrawal).await;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM api_account")
        .fetch_one(ctx.api_wallet_pool().unwrap().read_ref())
        .await
        .unwrap();
    assert_eq!(n, 0);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    ctx.set_frontend_notify_sender(Some(tx)).await;
    // Act / Assert: persisted balance resolves without any follow-up memory event.
    run(ctx, 25.).await.unwrap();
    assert!(resolved(ctx, "old").await);
    // Existing DEBUG diagnostics share this channel; assert business events independently.
    let mut events = Vec::new();
    while let Ok(message) = rx.try_recv() {
        if message.event != "DEBUG" {
            events.push(message.event);
        }
    }
    assert_eq!(events, vec!["API_WALLET_NOTIFICATION_CHANGED"]);
    run(ctx, 25.).await.unwrap();
    while let Ok(message) = rx.try_recv() {
        assert_eq!(message.event, "DEBUG", "replay must not repeat a business event");
    }
}
#[tokio::test]
async fn import_notification_stale_observation_and_below_threshold_keep_alert() {
    let (_dir, ctx) = fixture(ApiWalletType::Withdrawal).await;
    run(ctx, 19.).await.unwrap();
    assert!(!resolved(ctx, "old").await);
    query_and_upsert_assets_using(
        ctx,
        &ctx.api_wallet_pool().unwrap(),
        &AssetListReq::new("owner", "tron", vec![0]),
        async {
            alert(ctx, "new").await;
            Ok(response(25.))
        },
    )
    .await
    .unwrap();
    assert!(!resolved(ctx, "new").await);
}
#[tokio::test]
async fn import_notification_snapshot_error_stops_before_backend() {
    let (_dir, ctx) = fixture(ApiWalletType::Withdrawal).await;
    sqlx::query("ALTER TABLE api_business_notification RENAME TO unavailable")
        .execute(ctx.api_wallet_pool().unwrap().write_ref())
        .await
        .unwrap();
    let called = std::sync::atomic::AtomicBool::new(false);
    let result = query_and_upsert_assets_using(
        ctx,
        &ctx.api_wallet_pool().unwrap(),
        &AssetListReq::new("owner", "tron", vec![0]),
        async {
            called.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(response(25.))
        },
    )
    .await;
    assert!(result.is_err());
    assert!(!called.load(std::sync::atomic::Ordering::Relaxed));
}
#[tokio::test]
async fn import_notification_subaccount_never_reads_alerts() {
    let (_dir, ctx) = fixture(ApiWalletType::SubAccount).await;
    sqlx::query("DROP TABLE api_business_notification")
        .execute(ctx.api_wallet_pool().unwrap().write_ref())
        .await
        .unwrap();
    run(ctx, 25.).await.unwrap();
}
async fn reject_resolution(ctx: &'static crate::context::Context) {
    sqlx::query("CREATE TRIGGER reject_resolution BEFORE UPDATE OF resolved_at ON api_business_notification BEGIN SELECT RAISE(FAIL,'fixture'); END").execute(ctx.api_wallet_pool().unwrap().write_ref()).await.unwrap();
}
#[tokio::test]
async fn import_notification_resolution_failure_saves_compensation() {
    let (_dir, ctx) = fixture(ApiWalletType::Withdrawal).await;
    reject_resolution(ctx).await;
    run(ctx, 25.).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM task_queue")
        .fetch_one(ctx.task_pool().unwrap().read_ref())
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert!(!resolved(ctx, "old").await);
}
#[tokio::test]
async fn import_notification_failed_compensation_retains_only_selected_target() {
    let (_dir, ctx) = fixture(ApiWalletType::Withdrawal).await;
    reject_resolution(ctx).await;
    sqlx::query("CREATE TRIGGER reject_task BEFORE INSERT ON task_queue BEGIN SELECT RAISE(FAIL,'fixture'); END").execute(ctx.task_pool().unwrap().write_ref()).await.unwrap();
    run(ctx, 25.).await.unwrap();
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM api_asset_sync_state WHERE address='a' AND status<>2",
    )
    .fetch_one(ctx.api_wallet_pool().unwrap().read_ref())
    .await
    .unwrap();
    assert_eq!(n, 1);
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM api_asset_sync_state")
        .fetch_one(ctx.api_wallet_pool().unwrap().read_ref())
        .await
        .unwrap();
    assert_eq!(total, 1);
}
#[tokio::test]
async fn import_notification_resolved_latest_does_not_revive_old_history_or_other_targets() {
    let (_dir, ctx) = fixture(ApiWalletType::Withdrawal).await;
    alert(ctx, "new").await;
    ApiBusinessNotificationRepo::mark_resolved(
        &ctx.api_wallet_pool().unwrap(),
        "owner",
        "new",
        true,
    )
    .await
    .unwrap();
    let pool = ctx.api_wallet_pool().unwrap();
    // Other imported addresses must not become balance or recovery targets.
    sqlx::query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1000) INSERT INTO api_assets(decimals,address,chain_code,token_address,protocol,status,balance,is_multisig,created_at) SELECT 0,'unrelated-'||x,'tron','','fixture',1,'0',0,strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM n").execute(pool.write_ref()).await.unwrap();
    run(ctx, 25.).await.unwrap();
    assert!(!resolved(ctx, "old").await);
    let unchanged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM api_assets WHERE address LIKE 'unrelated-%' AND balance='0'",
    )
    .fetch_one(pool.read_ref())
    .await
    .unwrap();
    assert_eq!(unchanged, 1000);
}
