mod support;

use super::*;
use crate::service::api_wallet::business_notification::{
    ApiBusinessNotificationService, global_business_query_tests::isolated_context,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use wallet_database::{
    entities::api_business_notification::NewApiBusinessNotification,
    repositories::api_business_notification::ApiBusinessNotificationRepo,
};

static NEXT_CHAIN: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    _dir: tempfile::TempDir,
    ctx: &'static Context,
    chain: String,
    adapter: Arc<support::BalanceAdapter>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        crate::testkit::adapter_factory::clear_test_transaction_adapter_override(&self.chain);
    }
}

impl Fixture {
    async fn new() -> Self {
        let (_dir, ctx) = isolated_context().await;
        let chain = format!("notification-recovery-{}", NEXT_CHAIN.fetch_add(1, Ordering::Relaxed));
        let adapter = Arc::new(support::BalanceAdapter::default());
        crate::testkit::adapter_factory::set_test_transaction_adapter_override(
            &chain,
            adapter.clone(),
        );
        let pool = ctx.api_wallet_pool().unwrap();
        wallet_database::repositories::api_wallet::chain::ApiChainRepo::add(
            &pool,
            wallet_database::entities::api_chain::ApiChainCreateVo::new(
                "fixture",
                &chain,
                &vec!["evm".into()],
                wallet_database::entities::api_chain::NodeBindType::AutoLocal,
                "TEST",
            ),
        )
        .await
        .unwrap();
        ApiCoinRepo::upsert_multi_coin(
            &pool,
            vec![wallet_database::entities::api_coin::ApiCoinData::new(
                Some("TEST".into()),
                "TEST",
                &chain,
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
        ApiWalletRepo::upsert(
            &pool,
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
        Self { _dir, ctx, chain, adapter }
    }

    async fn asset(&self, address: &str, alert: bool) {
        let pool = self.ctx.api_wallet_pool().unwrap();
        sqlx::query("INSERT INTO api_account(account_id,name,address,wallet_address,chain_code,uid,api_wallet_type,status,is_init,created_at) VALUES(1,'fixture',?,'wallet',?,'owner',2,1,1,'2026-10-30T00:00:00Z')")
            .bind(address).bind(&self.chain).execute(pool.write_ref()).await.unwrap();
        ApiAssetsRepo::upsert_assets(
            &pool,
            ApiCreateAssetsVo::new(
                AssetsId::new(address, &self.chain, "".into()),
                "TEST",
                0,
                None,
                0,
            )
            .with_name("TEST")
            .with_balance("2"),
        )
        .await
        .unwrap();
        ApiAssetSyncStateRepo::upsert_pending(&pool, address, &self.chain, "".into())
            .await
            .unwrap();
        if alert {
            let payload = serde_json::json!({"remaining":2,"thresholdValue":20,"token":"","symbol":"TEST","merchantName":"fixture","waddressType":"1","chainCode":self.chain,"targetAddress":address}).to_string();
            ApiBusinessNotificationRepo::ingest(
                &pool,
                NewApiBusinessNotification {
                    id: address,
                    uid: "owner",
                    kind: "AWM_ADDRESS_INSUFFICIENT",
                    business_key: address,
                    payload: &payload,
                },
            )
            .await
            .unwrap();
        }
    }

    async fn sync(&self, addresses: &[&str]) -> Result<(), crate::error::service::ServiceError> {
        ApiAssetsDomain::sync_assets_by_addr_chain_durable_with_ctx(
            self.ctx,
            addresses.iter().map(|s| s.to_string()).collect(),
            Some(self.chain.clone()),
            "".into(),
            0,
        )
        .await
    }

    async fn state(&self, address: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT status FROM api_asset_sync_state WHERE address=? AND chain_code=?",
        )
        .bind(address)
        .bind(&self.chain)
        .fetch_one(self.ctx.api_wallet_pool().unwrap().read_ref())
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn notification_recovery_snapshot_failure_retains_dirty_target_before_rpc() {
    // Arrange: a dirty asset and an unavailable notification table.
    let f = Fixture::new().await;
    f.asset("target", true).await;
    let pool = f.ctx.api_wallet_pool().unwrap();
    sqlx::query("ALTER TABLE api_business_notification RENAME TO unavailable_notifications")
        .execute(pool.write_ref())
        .await
        .unwrap();

    // Act / Assert: failed snapshot cannot become an empty successful observation.
    assert!(f.sync(&["target"]).await.is_err());
    assert!(f.adapter.calls.lock().unwrap().is_empty());
    assert_eq!(f.state("target").await, 0);
    let balance: String =
        sqlx::query_scalar("SELECT balance FROM api_assets WHERE address='target'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(balance, "2");

    // Recovery uses the same address, without a wallet-wide scan.
    sqlx::query("ALTER TABLE unavailable_notifications RENAME TO api_business_notification")
        .execute(pool.write_ref())
        .await
        .unwrap();
    f.sync(&["target"]).await.unwrap();
    assert_eq!(f.state("target").await, 2);
    assert_eq!(f.adapter.calls.lock().unwrap().len(), 1);
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "owner", "target")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_some()
    );
}

#[tokio::test]
async fn notification_recovery_empty_snapshot_is_success_without_scope_expansion() {
    // Arrange: many unrelated assets, one selected target, and no notifications.
    let f = Fixture::new().await;
    f.asset("target", false).await;
    let pool = f.ctx.api_wallet_pool().unwrap();
    sqlx::query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000) INSERT INTO api_assets(decimals,address,chain_code,token_address,status,balance,created_at) SELECT 0,'other-'||x,?,'',1,'2','2026-10-30T00:00:00Z' FROM n")
        .bind(&f.chain).execute(pool.write_ref()).await.unwrap();
    // Act / Assert: the empty snapshot must not cause failure or extra RPCs.
    f.sync(&["target"]).await.unwrap();
    assert_eq!(*f.adapter.calls.lock().unwrap(), vec![("target".into(), "".into())]);
    assert_eq!(f.state("target").await, 2);
    let changed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM api_assets WHERE address LIKE 'other-%' AND balance!='2'",
    )
    .fetch_one(pool.read_ref())
    .await
    .unwrap();
    assert_eq!(changed, 0);
}

#[tokio::test]
async fn notification_recovery_failed_compensation_keeps_only_affected_asset_dirty() {
    // Arrange: one failing resolution and one successful sibling in the same batch.
    let f = Fixture::new().await;
    f.asset("failed", true).await;
    f.asset("success", true).await;
    let pool = f.ctx.api_wallet_pool().unwrap();
    sqlx::query("CREATE TRIGGER reject_resolution BEFORE UPDATE OF resolved_at ON api_business_notification WHEN OLD.id='failed' AND NEW.resolved_at IS NOT NULL BEGIN SELECT RAISE(FAIL,'fixture resolution failure'); END").execute(pool.write_ref()).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_compensation BEFORE INSERT ON task_queue BEGIN SELECT RAISE(FAIL,'fixture task failure'); END").execute(f.ctx.task_pool().unwrap().write_ref()).await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    f.ctx.set_frontend_notify_sender(Some(tx)).await;

    // Act: successful asset persistence must survive a failed notification update.
    assert!(f.sync(&["failed", "success"]).await.is_err());
    assert_ne!(f.state("failed").await, 2);
    assert_eq!(f.state("success").await, 2);
    let stored: String =
        sqlx::query_scalar("SELECT balance FROM api_assets WHERE address='failed'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(stored, "20");
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "owner", "success")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_some()
    );
    let mut refreshes = 0;
    while let Ok(event) = rx.try_recv() {
        if event.event == "API_WALLET_NOTIFICATION_CHANGED" {
            refreshes += 1;
        }
    }
    assert_eq!(refreshes, 1, "partial success still invalidates notification state");

    // Assert: recover only the failed target, not the already completed sibling.
    sqlx::query("DROP TRIGGER reject_resolution").execute(pool.write_ref()).await.unwrap();
    sqlx::query("DROP TRIGGER reject_compensation")
        .execute(f.ctx.task_pool().unwrap().write_ref())
        .await
        .unwrap();
    f.sync(&["failed"]).await.unwrap();
    assert_eq!(f.state("failed").await, 2);
    assert_eq!(f.adapter.calls.lock().unwrap().len(), 3);
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "owner", "failed")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_some()
    );
}

#[tokio::test]
async fn notification_recovery_durable_compensation_allows_asset_completion_without_dispatch() {
    let f = Fixture::new().await;
    f.asset("target", true).await;
    let pool = f.ctx.api_wallet_pool().unwrap();
    sqlx::query("CREATE TRIGGER reject_resolution BEFORE UPDATE OF resolved_at ON api_business_notification BEGIN SELECT RAISE(FAIL,'fixture'); END").execute(pool.write_ref()).await.unwrap();
    // No runtime dispatcher exists, but the durable compensation is sufficient.
    f.sync(&["target"]).await.unwrap();
    assert_eq!(f.state("target").await, 2);
    let tasks = wallet_database::repositories::task_queue::TaskQueueRepo::all_tasks_queue(
        &f.ctx.task_pool().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(tasks.len(), 1);
    sqlx::query("DROP TRIGGER reject_resolution").execute(pool.write_ref()).await.unwrap();
    let check: crate::application::api_wallet_balance_check::ApiAddressBalanceCheck =
        serde_json::from_str(&tasks[0].request_body).unwrap();
    ApiBusinessNotificationService::new(f.ctx)
        .check_address_balance_using(&check, std::time::Instant::now, |_, _, _| async {
            panic!("compensation should reuse its recent observation")
        })
        .await
        .unwrap();
    assert_eq!(f.adapter.calls.lock().unwrap().len(), 1);
    assert!(
        ApiBusinessNotificationRepo::find(&pool, "owner", "target")
            .await
            .unwrap()
            .unwrap()
            .resolved_at
            .is_some()
    );
}
