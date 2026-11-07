use super::ApiBusinessNotificationService;
use crate::service::api_wallet::withdraw::WithdrawService;
use wallet_database::{
    entities::{
        api_business_notification::NewApiBusinessNotification, api_trade_type::ApiTradeType,
        api_wallet::ApiWalletType, api_withdraw::ApiWithdrawStatus,
    },
    repositories::{
        api_business_notification::ApiBusinessNotificationRepo,
        api_wallet::{wallet::ApiWalletRepo, withdraw::ApiWithdrawRepo},
    },
};

#[tokio::test]
async fn local_queries_scope_no_rpc_no_locks_and_failure_independence() {
    // Arrange: isolated empty databases, then two owners and excluded history.
    let (_dir, ctx) = isolated_context().await;
    let service = ApiBusinessNotificationService::new(ctx);
    let withdrawals = WithdrawService::new(ctx);
    assert!(service.unresolved_address_balance_notifications().await.unwrap().is_empty());
    assert!(withdrawals.pending_withdrawals().await.unwrap().is_empty());
    let pool = ctx.api_wallet_pool().unwrap();
    let tx_pool = ctx.api_transaction_pool().unwrap();
    for (uid, address, kind, status) in [
        ("a", "wallet-a", ApiWalletType::Withdrawal, 1),
        ("a", "duplicate-a", ApiWalletType::Withdrawal, 1),
        ("b", "wallet-b", ApiWalletType::Withdrawal, 1),
        ("sub", "wallet-sub", ApiWalletType::SubAccount, 1),
        ("disabled", "wallet-disabled", ApiWalletType::Withdrawal, 0),
    ] {
        ApiWalletRepo::upsert(&pool, uid, "fixture", address, &[], &[], kind, None, "fixture", 0)
            .await
            .unwrap();
        sqlx::query("UPDATE api_wallet SET status = ? WHERE address = ?")
            .bind(status)
            .bind(address)
            .execute(pool.write_ref())
            .await
            .unwrap();
    }
    for uid in ["a", "b", "sub", "disabled", "orphan"] {
        let payload = serde_json::json!({"remaining":2,"thresholdValue":3000,"token":"","symbol":"trx","merchantName":"fixture","waddressType":"2","chainCode":"tron","targetAddress":uid}).to_string();
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
        ApiWithdrawRepo::upsert_api_withdraw(
            &tx_pool,
            uid,
            "account",
            "from",
            "to",
            "10",
            "validate",
            "tron",
            None,
            "TRX",
            uid,
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
    }
    // Act/Assert: no configured chain exists, and both asynchronous locks/permits are held.
    let balance_guard = ctx.address_balance_check.lock().await;
    let pool_guard = ctx.get_global_background_task_pool().hold_all_permits_for_test().await;
    let (alerts, orders) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::join!(
            service.unresolved_address_balance_notifications(),
            withdrawals.pending_withdrawals()
        )
    })
    .await
    .unwrap();
    let alerts = alerts.unwrap();
    assert_eq!(alerts.len(), 2);
    assert_eq!(orders.unwrap().len(), 2);
    assert!(alerts.iter().all(|r| !r.is_read && r.resolved_at.is_none()));
    assert!(alerts.iter().all(|r| r.uid == "a" || r.uid == "b"));
    drop(pool_guard);
    drop(balance_guard);
    ApiWithdrawRepo::set_audit_passed(&tx_pool, "a").await.unwrap();
    assert_eq!(withdrawals.pending_withdrawals().await.unwrap().len(), 1);
    assert_eq!(service.unresolved_address_balance_notifications().await.unwrap().len(), 2);
    // A reminder-table failure does not affect the independent withdrawal read.
    sqlx::query("ALTER TABLE api_business_notification RENAME TO unavailable_alerts")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert!(service.unresolved_address_balance_notifications().await.is_err());
    assert_eq!(withdrawals.pending_withdrawals().await.unwrap().len(), 1);
    sqlx::query("ALTER TABLE unavailable_alerts RENAME TO api_business_notification")
        .execute(pool.write_ref())
        .await
        .unwrap();
    sqlx::query("ALTER TABLE api_withdraws RENAME TO unavailable_orders")
        .execute(tx_pool.write_ref())
        .await
        .unwrap();
    assert!(withdrawals.pending_withdrawals().await.is_err());
    assert_eq!(service.unresolved_address_balance_notifications().await.unwrap().len(), 2);
    sqlx::query("UPDATE api_wallet SET status=0 WHERE uid IN ('a','b')")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert!(service.unresolved_address_balance_notifications().await.unwrap().is_empty());
}

pub(crate) async fn isolated_context() -> (tempfile::TempDir, &'static crate::context::Context) {
    let dir = tempfile::tempdir().unwrap();
    let config = crate::config::Config::new(
        r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
aggregate_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
"#,
    )
    .unwrap();
    let dirs = crate::dirs::Dirs::new(&dir.path().to_string_lossy()).unwrap();
    let ctx = crate::context::Context::new_without_handles_for_test(dirs, config).await.unwrap();
    (dir, Box::leak(Box::new(ctx)))
}
