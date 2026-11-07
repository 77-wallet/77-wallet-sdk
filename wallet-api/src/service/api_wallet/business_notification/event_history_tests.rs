use super::ApiBusinessNotificationService;
use rust_decimal::Decimal;
use wallet_database::{
    ApiWalletDbPool,
    entities::{api_business_notification::NewApiBusinessNotification, api_wallet::ApiWalletType},
    repositories::{
        api_business_notification::ApiBusinessNotificationRepo as Repo,
        api_wallet::wallet::ApiWalletRepo,
    },
};

async fn setup() -> (tempfile::TempDir, &'static crate::context::Context) {
    let (dir, ctx) = super::global_business_query_tests::isolated_context().await;
    let pool = ctx.api_wallet_pool().unwrap();
    for (uid, kind) in [
        ("owner", ApiWalletType::Withdrawal),
        ("other", ApiWalletType::Withdrawal),
        ("sub", ApiWalletType::SubAccount),
        ("disabled", ApiWalletType::Withdrawal),
    ] {
        ApiWalletRepo::upsert(&pool, uid, "fixture", uid, &[], &[], kind, None, "fixture", 0)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE api_wallet SET status=0 WHERE uid='disabled'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    (dir, ctx)
}

async fn event(
    pool: &ApiWalletDbPool,
    uid: &str,
    id: &str,
    address: &str,
    threshold: i64,
    time: &str,
) {
    let key = format!("{uid}:tron:{address}:");
    let payload = serde_json::json!({"remaining":"1","thresholdValue":threshold.to_string(),"token":"","symbol":"TRX","merchantName":"fixture","waddressType":"1","chainCode":"tron","targetAddress":address,"notifiedAt":time}).to_string();
    Repo::ingest(
        pool,
        NewApiBusinessNotification {
            id,
            uid,
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key: &key,
            payload: &payload,
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn banner_aggregates_before_pagination_and_keeps_latest_valid_unresolved_event() {
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    for n in 0..105 {
        event(&pool, "owner", &format!("same-{n:03}"), "address", 20, "2026-10-07T11:00:00Z").await;
    }
    event(&pool, "owner", "other-object", "different", 20, "2026-10-07T10:00:00Z").await;
    event(&pool, "other", "other-wallet", "address", 20, "2026-10-07T09:00:00Z").await;
    for uid in ["sub", "disabled", "orphan"] {
        event(&pool, uid, &format!("excluded-{uid}"), "address", 20, "2026-10-07T12:00:00Z").await;
    }
    Repo::ingest(
        &pool,
        NewApiBusinessNotification {
            id: "bad",
            uid: "owner",
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key: "owner:tron:address:",
            payload: "{}",
        },
    )
    .await
    .unwrap();

    Repo::mark_read(&pool, "owner", "same-104").await.unwrap();
    let service = ApiBusinessNotificationService::new(ctx);
    let local = service.unresolved_address_balance_notifications().await.unwrap();
    assert_eq!(local.len(), 3);
    assert_eq!(local[0].id, "same-104");
    assert!(local[0].is_read);
    let first = service
        .list_unresolved_scoped_with_refresh_using(None, 0, 1, |_, _, _| async {
            Err(crate::error::service::ServiceError::Timeout)
        })
        .await
        .unwrap();
    let second = service
        .list_unresolved_scoped_with_refresh_using(None, 1, 1, |_, _, _| async {
            Err(crate::error::service::ServiceError::Timeout)
        })
        .await
        .unwrap();
    assert_eq!(first.total_count, 3);
    assert_eq!(second.total_count, 3);
    assert_eq!(first.data[0].id, "same-104");
    assert_eq!(second.data[0].id, "other-object");
    let history = service.list_for_withdrawal_wallets(false, 0, 200).await.unwrap();
    assert_eq!(history.total_count, 108); // 105 + two other objects + malformed history
}

#[tokio::test]
async fn latest_higher_threshold_blocks_older_lower_event_without_reading_or_hiding_history() {
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    event(&pool, "owner", "low", "address", 20, "2026-10-07T10:00:00Z").await;
    event(&pool, "owner", "high", "address", 100, "2026-10-07T11:00:00Z").await;
    let service = ApiBusinessNotificationService::new(ctx);
    let mut calls = 0;
    let page = service
        .list_unresolved_scoped_with_refresh_using(None, 0, 10, |_, _, _| {
            calls += 1;
            async { Ok(Decimal::from(50)) }
        })
        .await
        .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(page.total_count, 1);
    assert_eq!(page.data[0].id, "high");
    let low = Repo::find(&pool, "owner", "low").await.unwrap().unwrap();
    let high = Repo::find(&pool, "owner", "high").await.unwrap().unwrap();
    assert!(low.resolved_at.is_none());
    assert!(high.resolved_at.is_none());
    assert!(!low.is_read && !high.is_read);
    assert!(low.payload.contains("\"remaining\":\"1\""));
    assert_eq!(service.list_for_withdrawal_wallets(false, 0, 10).await.unwrap().total_count, 2);
    service.mark_read_for_withdrawal_wallets("low").await.unwrap();
    assert!(!service.detail_for_withdrawal_wallets("high").await.unwrap().unwrap().is_read);
    assert!(
        service.detail_for_withdrawal_wallets("low").await.unwrap().unwrap().resolved_at.is_none()
    );
}

#[tokio::test]
async fn in_flight_balance_observation_does_not_resolve_new_event() {
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    event(&pool, "owner", "old", "address", 20, "2026-10-07T10:00:00Z").await;
    let service = ApiBusinessNotificationService::new(ctx);
    let page = service
        .list_unresolved_scoped_with_refresh_using(None, 0, 10, |_, _, _| {
            let pool = pool.clone();
            async move {
                event(&pool, "owner", "new", "address", 10, "2026-10-07T11:00:00Z").await;
                Ok(Decimal::from(50))
            }
        })
        .await
        .unwrap();
    assert_eq!(page.data[0].id, "new");
    assert!(Repo::find(&pool, "owner", "old").await.unwrap().unwrap().resolved_at.is_some());
    assert!(Repo::find(&pool, "owner", "new").await.unwrap().unwrap().resolved_at.is_none());
}

#[tokio::test]
async fn persisted_balance_uses_latest_higher_threshold_for_whole_group() {
    use wallet_database::entities::{asset_token_key::AssetTokenKey, assets::AssetsId};
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    event(&pool, "owner", "low", "address", 20, "2026-10-07T10:00:00Z").await;
    event(&pool, "owner", "high", "address", 100, "2026-10-07T11:00:00Z").await;
    let id = AssetsId {
        address: "address".into(),
        chain_code: "tron".into(),
        token_address: AssetTokenKey::from_raw(Some("")),
    };
    ApiBusinessNotificationService::new(ctx)
        .resolve_from_persisted_balances(
            "owner",
            &[(id, "50".into())],
            &ApiBusinessNotificationService::new(ctx).all_unresolved("owner").await.unwrap(),
        )
        .await
        .unwrap();
    assert!(Repo::find(&pool, "owner", "low").await.unwrap().unwrap().resolved_at.is_none());
    assert!(Repo::find(&pool, "owner", "high").await.unwrap().unwrap().resolved_at.is_none());
}

#[tokio::test]
async fn cached_observation_cannot_resolve_event_arriving_after_original_check() {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    };
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    event(&pool, "owner", "old-high", "address", 100, "2026-10-07T10:00:00Z").await;
    let old = Repo::find(&pool, "owner", "old-high").await.unwrap().unwrap();
    let service = ApiBusinessNotificationService::new(ctx);
    let now = Instant::now();
    let calls = AtomicUsize::new(0);
    let balance_for = |_, _, _| {
        calls.fetch_add(1, Ordering::Relaxed);
        async { Ok(Decimal::from(50)) }
    };
    service.check_record_using(&old, &old.payload, || now, &balance_for).await.unwrap();
    event(&pool, "owner", "new-low", "address", 20, "2026-10-07T11:00:00Z").await;
    service.check_record_using(&old, &old.payload, || now, &balance_for).await.unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(Repo::find(&pool, "owner", "new-low").await.unwrap().unwrap().resolved_at.is_none());
    let new = Repo::find(&pool, "owner", "new-low").await.unwrap().unwrap();
    service.check_record_using(&new, &new.payload, || now, &balance_for).await.unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(Repo::find(&pool, "owner", "new-low").await.unwrap().unwrap().resolved_at.is_some());
}

#[tokio::test]
async fn asset_sync_observation_cannot_resolve_event_arriving_during_rpc() {
    use wallet_database::entities::{asset_token_key::AssetTokenKey, assets::AssetsId};
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    event(&pool, "owner", "before-rpc", "address", 100, "2026-10-07T10:00:00Z").await;
    let service = ApiBusinessNotificationService::new(ctx);
    let before_rpc = service
        .snapshot_balance_events(&[AssetsId::new(
            "address",
            "tron",
            AssetTokenKey::from_raw(Some("")),
        )])
        .await
        .unwrap();
    // Fake RPC interleaving: a genuinely new event arrives after observation started.
    event(&pool, "owner", "during-rpc", "address", 20, "2026-10-07T11:00:00Z").await;
    let id = AssetsId::new("address", "tron", AssetTokenKey::from_raw(Some("")));
    service
        .resolve_from_persisted_balances("owner", &[(id, "50".into())], &before_rpc)
        .await
        .unwrap();
    assert!(Repo::find(&pool, "owner", "before-rpc").await.unwrap().unwrap().resolved_at.is_none());
    assert!(Repo::find(&pool, "owner", "during-rpc").await.unwrap().unwrap().resolved_at.is_none());
    assert!(
        !ctx.address_balance_check
            .lock()
            .await
            .checked
            .contains_key(&("owner".into(), "during-rpc".into()))
    );
}

#[tokio::test]
async fn latest_lower_threshold_resolves_older_high_events_and_preserves_history_read_state() {
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    event(&pool, "owner", "old-high", "address", 100, "2026-10-07T10:00:00Z").await;
    event(&pool, "owner", "new-low", "address", 20, "2026-10-07T11:00:00Z").await;
    Repo::mark_read(&pool, "owner", "old-high").await.unwrap();
    let service = ApiBusinessNotificationService::new(ctx);
    let page = service
        .list_unresolved_scoped_with_refresh_using(None, 0, 10, |_, _, _| async {
            Ok(Decimal::from(50))
        })
        .await
        .unwrap();
    assert_eq!(page.total_count, 0);
    assert!(service.unresolved_address_balance_notifications().await.unwrap().is_empty());
    let history = service.list_for_withdrawal_wallets(false, 0, 10).await.unwrap();
    assert_eq!(history.total_count, 2);
    assert!(history.data.iter().all(|e| e.resolved_at.is_some()));
    assert!(!history.data[0].is_read);
    assert!(history.data[1].is_read);
}

#[tokio::test]
async fn resolved_latest_never_exposes_or_checks_older_unresolved_event() {
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    event(&pool, "owner", "older", "address", 100, "2026-10-07T10:00:00Z").await;
    event(&pool, "owner", "latest", "address", 20, "2026-10-07T11:00:00Z").await;
    Repo::mark_resolved(&pool, "owner", "latest", true).await.unwrap();
    let service = ApiBusinessNotificationService::new(ctx);
    assert!(service.unresolved_address_balance_notifications().await.unwrap().is_empty());
    let page = service
        .list_unresolved_scoped_with_refresh_using(None, 0, 10, |_, _, _| async {
            panic!("resolved latest must not query")
        })
        .await
        .unwrap();
    assert_eq!(page.total_count, 0);
    assert_eq!(service.list_for_withdrawal_wallets(false, 0, 10).await.unwrap().total_count, 2);
}

#[tokio::test]
async fn old_directed_task_uses_latest_threshold_and_tie_breaks_with_uid_isolation() {
    use std::time::Instant;
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    event(&pool, "owner", "a-old-low", "address", 20, "2026-10-07T10:00:00Z").await;
    event(&pool, "owner", "z-latest-high", "address", 100, "2026-10-07T10:00:00Z").await;
    event(&pool, "other", "foreign", "address", 20, "2026-10-07T09:00:00Z").await;
    let service = ApiBusinessNotificationService::new(ctx);
    let old = Repo::find(&pool, "owner", "a-old-low").await.unwrap().unwrap();
    service
        .check_record_using(&old, &old.payload, Instant::now, |_, _, _| async {
            Ok(Decimal::from(50))
        })
        .await
        .unwrap();
    assert!(Repo::find(&pool, "owner", "a-old-low").await.unwrap().unwrap().resolved_at.is_none());
    assert!(
        Repo::find(&pool, "owner", "z-latest-high").await.unwrap().unwrap().resolved_at.is_none()
    );
    let latest = Repo::find(&pool, "owner", "z-latest-high").await.unwrap().unwrap();
    let later = Instant::now() + std::time::Duration::from_secs(6);
    service
        .check_record_using(
            &latest,
            &latest.payload,
            || later,
            |_, _, _| async { Ok(Decimal::from(100)) },
        )
        .await
        .unwrap();
    assert!(Repo::find(&pool, "owner", "a-old-low").await.unwrap().unwrap().resolved_at.is_some());
    assert!(Repo::find(&pool, "other", "foreign").await.unwrap().unwrap().resolved_at.is_none());
}

#[tokio::test]
async fn group_resolution_write_failure_rolls_back_every_event() {
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    event(&pool, "owner", "old", "address", 20, "2026-10-07T10:00:00Z").await;
    event(&pool, "owner", "latest", "address", 20, "2026-10-07T11:00:00Z").await;
    sqlx::query("CREATE TRIGGER reject_old BEFORE UPDATE OF resolved_at ON api_business_notification WHEN OLD.id='old' BEGIN SELECT RAISE(FAIL,'reject group write'); END").execute(pool.write_ref()).await.unwrap();
    let result = ApiBusinessNotificationService::new(ctx)
        .list_unresolved_scoped_with_refresh_using(None, 0, 10, |_, _, _| async {
            Ok(Decimal::from(50))
        })
        .await;
    assert!(result.is_err());
    let history = Repo::list(&pool, "owner", false, 0, 10).await.unwrap();
    assert!(history.data.iter().all(|e| e.resolved_at.is_none() && !e.is_read));
}

#[tokio::test]
async fn asset_sync_latest_lower_threshold_resolves_observed_older_high_event() {
    use wallet_database::entities::{asset_token_key::AssetTokenKey, assets::AssetsId};
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    event(&pool, "owner", "old-high", "address", 100, "2026-10-07T10:00:00Z").await;
    event(&pool, "owner", "latest-low", "address", 20, "2026-10-07T11:00:00Z").await;
    let service = ApiBusinessNotificationService::new(ctx);
    let id = AssetsId::new("address", "tron", AssetTokenKey::from_raw(Some("")));
    let observed = service.snapshot_balance_events(&[id.clone()]).await.unwrap();
    service
        .resolve_from_persisted_balances("owner", &[(id, "50".into())], &observed)
        .await
        .unwrap();
    let history = Repo::list(&pool, "owner", false, 0, 10).await.unwrap();
    assert_eq!(history.total_count, 2);
    assert!(history.data.iter().all(|e| e.resolved_at.is_some() && !e.is_read));
    assert!(service.unresolved_address_balance_notifications().await.unwrap().is_empty());
}

#[tokio::test]
async fn group_cached_compensation_does_not_apply_old_observation_to_new_latest() {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    };
    let (_dir, ctx) = setup().await;
    let pool = ctx.api_wallet_pool().unwrap();
    event(&pool, "owner", "old-high", "address", 100, "2026-10-07T10:00:00Z").await;
    event(&pool, "owner", "latest-low", "address", 20, "2026-10-07T11:00:00Z").await;
    sqlx::query("CREATE TRIGGER reject_old BEFORE UPDATE OF resolved_at ON api_business_notification WHEN OLD.id='old-high' BEGIN SELECT RAISE(FAIL,'reject group write'); END").execute(pool.write_ref()).await.unwrap();
    let service = ApiBusinessNotificationService::new(ctx);
    let latest = Repo::find(&pool, "owner", "latest-low").await.unwrap().unwrap();
    let now = Instant::now();
    let calls = AtomicUsize::new(0);
    let query = |_, _, _| {
        calls.fetch_add(1, Ordering::Relaxed);
        async { Ok(Decimal::from(50)) }
    };
    assert!(service.check_record_using(&latest, &latest.payload, || now, &query).await.is_err());
    assert!(
        Repo::list(&pool, "owner", false, 0, 10)
            .await
            .unwrap()
            .data
            .iter()
            .all(|e| e.resolved_at.is_none())
    );
    sqlx::query("DROP TRIGGER reject_old").execute(pool.write_ref()).await.unwrap();
    event(&pool, "owner", "newest-low", "address", 10, "2026-10-07T12:00:00Z").await;
    service.check_record_using(&latest, &latest.payload, || now, &query).await.unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(
        Repo::list(&pool, "owner", false, 0, 10)
            .await
            .unwrap()
            .data
            .iter()
            .all(|e| e.resolved_at.is_none())
    );
    let newest = Repo::find(&pool, "owner", "newest-low").await.unwrap().unwrap();
    service.check_record_using(&newest, &newest.payload, || now, &query).await.unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(
        Repo::list(&pool, "owner", false, 0, 10)
            .await
            .unwrap()
            .data
            .iter()
            .all(|e| e.resolved_at.is_some() && !e.is_read)
    );
}
