use super::ApiBusinessNotificationService;
use rust_decimal::Decimal;
use serde_json::json;
use wallet_database::{
    entities::{api_business_notification::NewApiBusinessNotification, api_wallet::ApiWalletType},
    repositories::{
        api_business_notification::ApiBusinessNotificationRepo, api_wallet::wallet::ApiWalletRepo,
        config::ConfigRepo,
    },
};

async fn fixture() -> (tempfile::TempDir, &'static crate::context::Context, String) {
    let (dir, ctx) = super::global_business_query_tests::isolated_context().await;
    let pool = ctx.api_wallet_pool().unwrap();
    ApiWalletRepo::upsert(
        &pool,
        "config-owner",
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
    ApiWalletRepo::update_merchant_id(&pool, "wallet", "merchant").await.unwrap();
    ApiWalletRepo::update_app_id(&pool, "wallet", Some("app")).await.unwrap();
    let payload = json!({"remaining":"1", "thresholdValue":"20", "token":"", "symbol":"POL", "merchantName":"fixture", "waddressType":"2", "chainCode":"polygon", "targetAddress":"address"}).to_string();
    ApiBusinessNotificationRepo::upsert(
        &pool,
        NewApiBusinessNotification {
            id: "alert",
            uid: "config-owner",
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key: "alert",
            payload: &payload,
        },
    )
    .await
    .unwrap();
    (dir, ctx, payload)
}

async fn snapshot(ctx: &'static crate::context::Context, org: &str, items: serde_json::Value) {
    let value = json!({"config":{"configs":{"balanceNotifyCfgList":items}},"synced_at":chrono::Utc::now().timestamp_millis()+1}).to_string();
    sqlx::query("INSERT OR REPLACE INTO api_app_warning_config (org_id, app_id, value) VALUES (?, 'app', ?)")
        .bind(org).bind(value).execute(ctx.api_wallet_pool().unwrap().write_ref()).await.unwrap();
}

fn rule(amount: i64, status: bool) -> serde_json::Value {
    json!({"amount":amount,"chainCode":"polygon","contractAddress":"","status":status,"tokenCode":"pol"})
}

#[tokio::test]
async fn latest_config_threshold_resolves_banner_without_rewriting_history() {
    let (_dir, ctx, payload) = fixture().await;
    snapshot(ctx, "merchant", json!([rule(5, true)])).await;
    let service = ApiBusinessNotificationService::new(ctx);
    let rows = service
        .unresolved_balance_notifications_for_uid_using(
            "config-owner",
            std::time::Instant::now,
            |_, _, _| async { Ok(Decimal::from(10)) },
        )
        .await
        .unwrap();
    assert!(rows.is_empty(), "new5 threshold must replace historical20 for evaluation");
    let history =
        ApiBusinessNotificationRepo::find(&ctx.api_wallet_pool().unwrap(), "config-owner", "alert")
            .await
            .unwrap()
            .unwrap();
    assert_eq!(history.payload, payload);
    assert!(history.resolved_at.is_some());
    assert!(!history.is_read);
}

#[tokio::test]
async fn latest_disabled_or_removed_rule_resolves_without_balance_rpc() {
    for rules in [
        json!([rule(5, false)]),
        json!([]),
        json!([{"amount":5,"chainCode":"tron","contractAddress":"","status":true,"tokenCode":"trx"}]),
    ] {
        let (_dir, ctx, _) = fixture().await;
        snapshot(ctx, "merchant", rules).await;
        let rows = ApiBusinessNotificationService::new(ctx)
            .unresolved_balance_notifications_for_uid_using(
                "config-owner",
                std::time::Instant::now,
                |_, _, _| async { panic!("disabled/missing config must not require RPC") },
            )
            .await
            .unwrap();
        assert!(rows.is_empty());
    }
}

#[tokio::test]
async fn foreign_configuration_and_failed_balance_do_not_clear_warning() {
    let (_dir, ctx, _) = fixture().await;
    snapshot(ctx, "other-merchant", json!([])).await;
    let service = ApiBusinessNotificationService::new(ctx);
    let rows = service
        .unresolved_balance_notifications_for_uid_using(
            "config-owner",
            std::time::Instant::now,
            |_, _, _| async { Ok(Decimal::from(10)) },
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "other merchant's empty config must not clear this owner");
    snapshot(ctx, "merchant", json!([rule(50, true)])).await;
    let later = std::time::Instant::now() + std::time::Duration::from_secs(6);
    let result = service
        .unresolved_balance_notifications_for_uid_using(
            "config-owner",
            || later,
            |_, _, _| async { Err(crate::error::service::ServiceError::Timeout) },
        )
        .await;
    assert!(result.is_ok());
    assert_eq!(service.unresolved_address_balance_notifications().await.unwrap().len(), 1);
}

#[tokio::test]
async fn rebind_during_balance_query_cannot_resolve_previous_binding_warning() {
    let (_dir, ctx, _) = fixture().await;
    snapshot(ctx, "merchant", json!([rule(5, true)])).await;
    let rows = ApiBusinessNotificationService::new(ctx)
        .unresolved_balance_notifications_for_uid_using(
            "config-owner",
            std::time::Instant::now,
            move |_, _, _| async move {
                ApiWalletRepo::update_merchant_id(
                    &ctx.api_wallet_pool()?,
                    "wallet",
                    "new-merchant",
                )
                .await?;
                Ok(Decimal::from(10))
            },
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "old in-flight result cannot resolve after rebind");
}

#[tokio::test]
async fn newer_notification_is_not_cleared_by_older_empty_config() {
    let (_dir, ctx, _) = fixture().await;
    snapshot(ctx, "merchant", json!([])).await;
    sqlx::query("UPDATE api_business_notification SET created_at = ? WHERE id = 'alert'")
        .bind(chrono::Utc::now() + chrono::Duration::seconds(2))
        .execute(ctx.api_wallet_pool().unwrap().write_ref())
        .await
        .unwrap();
    let rows = ApiBusinessNotificationService::new(ctx)
        .unresolved_balance_notifications_for_uid_using(
            "config-owner",
            std::time::Instant::now,
            |_, _, _| async { Ok(Decimal::from(10)) },
        )
        .await
        .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "notification's newer20 threshold remains authoritative until fresh config"
    );
}

#[tokio::test]
async fn persisted_asset_balance_uses_current_configuration_threshold() {
    let (_dir, ctx, payload) = fixture().await;
    snapshot(ctx, "merchant", json!([rule(5, true)])).await;
    let service = ApiBusinessNotificationService::new(ctx);
    let events = service.address_balance_notification_events().await.unwrap();
    service
        .resolve_from_persisted_balances(
            "config-owner",
            &[(
                wallet_database::entities::assets::AssetsId::new(
                    "address",
                    "polygon",
                    wallet_database::entities::asset_token_key::AssetTokenKey::Native,
                ),
                "10".into(),
            )],
            &events,
        )
        .await
        .unwrap();
    assert!(service.unresolved_address_balance_notifications().await.unwrap().is_empty());
    assert_eq!(
        ApiBusinessNotificationRepo::find(&ctx.api_wallet_pool().unwrap(), "config-owner", "alert")
            .await
            .unwrap()
            .unwrap()
            .payload,
        payload
    );
}

#[tokio::test]
async fn successful_config_sync_queues_existing_check_and_local_banner_disappears() {
    use wallet_database::{
        entities::task_queue::{KnownTaskName, TaskName},
        repositories::task_queue::TaskQueueRepo,
    };
    let (_dir, ctx, payload) = fixture().await;
    let (_manager, _guard) = crate::testkit::ordinary_business::manager(ctx);
    // Only dispatch is suppressed; durable task insertion and the real task handler run.
    super::super::app_warning_config::AppWarningConfigService::new(ctx)
        .sync_using(|_| {
            std::future::ready(Ok(serde_json::from_value(
                json!({"configs":{"balanceNotifyCfgList":[]}}),
            )
            .unwrap()))
        })
        .await
        .unwrap();
    let tasks = TaskQueueRepo::all_tasks_queue(&ctx.task_pool().unwrap()).await.unwrap();
    let checks: Vec<_> = tasks
        .iter()
        .filter(|task| task.task_name == TaskName::Known(KnownTaskName::CheckApiAddressBalance))
        .collect();
    assert_eq!(checks.len(), 1);
    let check: crate::application::api_wallet_balance_check::ApiAddressBalanceCheck =
        serde_json::from_str(&checks[0].request_body).unwrap();
    assert_eq!(check.uid, "config-owner");
    check.execute(ctx).await.unwrap(); // Disabled config needs no RPC, even without asset rows.
    assert!(
        ApiBusinessNotificationService::new(ctx)
            .unresolved_address_balance_notifications()
            .await
            .unwrap()
            .is_empty()
    );
    let history =
        ApiBusinessNotificationRepo::find(&ctx.api_wallet_pool().unwrap(), "config-owner", "alert")
            .await
            .unwrap()
            .unwrap();
    assert_eq!(history.payload, payload);
    assert!(!history.is_read);
}

#[tokio::test]
async fn same_application_rebind_during_balance_query_cannot_use_removed_config() {
    let (_dir, ctx, _) = fixture().await;
    snapshot(ctx, "merchant", json!([rule(5, true)])).await;
    let rows = ApiBusinessNotificationService::new(ctx)
        .unresolved_balance_notifications_for_uid_using(
            "config-owner",
            std::time::Instant::now,
            move |_, _, _| async move {
                let pool = ctx.api_wallet_pool()?;
                ApiWalletRepo::unbind_uid(&pool, "wallet").await?;
                ApiWalletRepo::update_merchant_id(&pool, "wallet", "merchant").await?;
                ApiWalletRepo::update_app_id(&pool, "wallet", Some("app")).await?;
                Ok(Decimal::from(10))
            },
        )
        .await
        .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "removed snapshot cannot resolve an old alert after same-scope rebind"
    );
}

#[tokio::test]
async fn legacy_core_cache_is_not_used_after_api_storage_upgrade() {
    let (_dir, ctx, _) = fixture().await;
    ConfigRepo::upsert(
        "api_balance_notify_config:[\"merchant\",\"app\"]",
        &json!({"config":{"configs":{"balanceNotifyCfgList":[]}},"synced_at":chrono::Utc::now().timestamp_millis()+1}).to_string(),
        Some(1), &ctx.core_pool().unwrap(),
    ).await.unwrap();
    let rows = ApiBusinessNotificationService::new(ctx)
        .unresolved_balance_notifications_for_uid_using(
            "config-owner",
            std::time::Instant::now,
            |_, _, _| async { Ok(Decimal::from(10)) },
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "old Core cache must not override historical threshold20");
}
