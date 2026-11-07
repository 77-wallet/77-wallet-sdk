use super::collect_worker::ShadowCollectWorker;
use crate::{
    error::{
        business::{BusinessError, chain::ChainError},
        service::ServiceError,
    },
    infrastructure::api_trans::{
        AddressLockManager,
        collect::shadow::{CollectIntent, ShadowAdvancer},
    },
};
use std::{future::Future, sync::Arc};
use wallet_database::{
    entities::api_collect::ApiCollectStatus, repositories::api_wallet::collect::ApiCollectRepo,
};

async fn fixture() -> (
    &'static crate::context::Context,
    ShadowCollectWorker,
    String,
    tokio::sync::mpsc::Receiver<CollectIntent>,
    Arc<AddressLockManager>,
) {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let locks = Arc::new(AddressLockManager::new());
    let worker =
        ShadowCollectWorker::new(ctx, locks.clone(), Arc::new(ShadowAdvancer::new(ctx, tx, None)));
    let trade = format!("api_evm_fee_{}", uuid::Uuid::new_v4().simple());
    let pool = ctx.api_transaction_pool().unwrap();
    ApiCollectRepo::upsert_api_collect(
        &pool,
        "uid",
        "collect",
        "from",
        "to",
        "1",
        "digest",
        "base",
        None,
        "ETH",
        &trade,
        2,
        ApiCollectStatus::Init,
        1,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE api_collect SET order_ack_sent_at=strftime('%Y-%m-%dT%H:%M:%SZ','now'), building_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE trade_no=?").bind(&trade).execute(pool.as_ref()).await.unwrap();
    (ctx, worker, trade, rx, locks)
}

#[tokio::test]
async fn api_evm_fee_pre_raw_shortage_reopens_existing_fee_cycle() {
    let (ctx, worker, trade, mut intents, _locks) = fixture().await;
    worker
        .handle_collect_tx_failed(
            &trade,
            ServiceError::Business(BusinessError::Chain(ChainError::InsufficientFeeBalance)),
        )
        .await
        .unwrap();
    let row =
        ApiCollectRepo::get_api_collect_by_trade_no(&ctx.api_transaction_pool().unwrap(), &trade)
            .await
            .unwrap();
    assert_eq!(row.need_service_fee, Some(true));
    assert!(row.raw_tx.is_none());
    assert!(row.building_at.is_none());
    let intent = tokio::time::timeout(std::time::Duration::from_secs(2), intents.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(format!("{intent:?}").contains("UploadServiceFee"));
}

#[tokio::test]
async fn api_evm_fee_completed_cycle_waits_for_reestimate_without_second_payment() {
    let (ctx, worker, trade, mut intents, _locks) = fixture().await;
    let pool = ctx.api_transaction_pool().unwrap();
    sqlx::query("UPDATE api_collect SET service_fee_uploaded_at=strftime('%Y-%m-%dT%H:%M:%SZ','now'), tx_fee_res_ack_sent_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE trade_no=?").bind(&trade).execute(pool.as_ref()).await.unwrap();
    worker
        .handle_collect_tx_failed(
            &trade,
            ServiceError::Business(BusinessError::Chain(ChainError::InsufficientFeeBalance)),
        )
        .await
        .unwrap();
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
    assert_eq!(row.status, ApiCollectStatus::InsufficientBalance);
    assert!(row.err_code.is_none(), "first post-payment shortage must remain recoverable");
    assert_ne!(row.need_service_fee, Some(true));
    assert!(row.service_fee_uploaded_at.is_some());
    assert!(row.tx_fee_res_ack_sent_at.is_some());
    assert!(row.building_at.is_none());
    assert!(intents.try_recv().is_err(), "must not request a second payment or spin build");
}

#[tokio::test]
async fn api_evm_fee_classifier_protects_raw_hash_and_other_error_classes() {
    let (ctx, _worker, trade, _intents, _locks) = fixture().await;
    let pool = ctx.api_transaction_pool().unwrap();
    let mut row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
    let shortage = ServiceError::Business(BusinessError::Chain(ChainError::InsufficientFeeBalance));
    assert!(ShadowCollectWorker::should_reopen_evm_fee_cycle(&row, &shortage));
    assert!(!ShadowCollectWorker::should_reopen_evm_fee_cycle(
        &row,
        &ServiceError::Parameter("RPC failure".to_string())
    ));
    row.raw_tx = Some("persisted raw".to_string());
    assert!(!ShadowCollectWorker::should_reopen_evm_fee_cycle(&row, &shortage));
    row.raw_tx = None;
    row.tx_hash = Some("persisted hash".to_string());
    assert!(!ShadowCollectWorker::should_reopen_evm_fee_cycle(&row, &shortage));
    row.tx_hash = None;
    for code in ["eth", "bnb", "polygon", "avalanche", "arbitrum", "tron"] {
        row.chain_code = code.to_string();
        assert!(!ShadowCollectWorker::should_reopen_evm_fee_cycle(&row, &shortage));
    }
    row.chain_code = "tron".to_string();
    assert!(!ShadowCollectWorker::should_reopen_evm_fee_cycle(&row, &shortage));
}

#[tokio::test]
async fn api_evm_fee_stale_shortage_preserves_committed_raw_under_shared_lock() {
    let (ctx, worker, trade, mut intents, locks) = fixture().await;
    let pool = ctx.api_transaction_pool().unwrap();
    let stale = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
    let shortage = ServiceError::Business(BusinessError::Chain(ChainError::InsufficientFeeBalance));
    // Hold the exact lock used by Build phase 3, then poll recovery to pending.
    let guard = locks.acquire(&stale.from_addr).await.unwrap();
    let recovery = worker.reopen_evm_fee_cycle_if_unbuilt(&stale, &shortage);
    tokio::pin!(recovery);
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(recovery.as_mut().poll(cx).is_pending()))
            .await
    );
    ApiCollectRepo::update_after_build(&pool, &trade, "persisted hash", "persisted raw", "0.1", 7)
        .await
        .unwrap();
    drop(guard);
    assert_eq!(recovery.await.unwrap(), 0);
    // Also cover a previously captured input after the write has already completed.
    assert_eq!(worker.reopen_evm_fee_cycle_if_unbuilt(&stale, &shortage).await.unwrap(), 0);
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
    assert_eq!(row.raw_tx.as_deref(), Some("persisted raw"));
    assert_eq!(row.tx_hash.as_deref(), Some("persisted hash"));
    assert_eq!(row.nonce, 7);
    assert_ne!(row.need_service_fee, Some(true));
    assert!(intents.try_recv().is_err());
}

#[tokio::test]
async fn api_evm_fee_reestimate_deadline_expires_without_second_payment() {
    let (ctx, worker, trade, mut intents, _locks) = fixture().await;
    let pool = ctx.api_transaction_pool().unwrap();
    sqlx::query("UPDATE api_collect SET service_fee_uploaded_at=datetime('now','-4 minutes'), tx_fee_res_ack_sent_at=datetime('now','-4 minutes') WHERE trade_no=?").bind(&trade).execute(pool.as_ref()).await.unwrap();
    worker
        .handle_collect_tx_failed(
            &trade,
            ServiceError::Business(BusinessError::Chain(ChainError::InsufficientFeeBalance)),
        )
        .await
        .unwrap();
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
    assert!(row.err_code.is_some());
    assert_ne!(row.need_service_fee, Some(true));
    assert!(row.service_fee_uploaded_at.is_some());
    assert!(row.tx_fee_res_ack_sent_at.is_some());
    assert!(intents.try_recv().is_err());
}

#[tokio::test]
async fn api_evm_fee_duplicate_shortage_preserves_ack_deadline_and_scan_recovery() {
    let (ctx, worker, trade, mut intents, _locks) = fixture().await;
    let pool = ctx.api_transaction_pool().unwrap();
    sqlx::query("UPDATE api_collect SET service_fee_uploaded_at=strftime('%Y-%m-%dT%H:%M:%SZ','now'), tx_fee_res_ack_sent_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE trade_no=?").bind(&trade).execute(pool.as_ref()).await.unwrap();
    let before = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
    for _ in 0..2 {
        worker
            .handle_collect_tx_failed(
                &trade,
                ServiceError::Business(BusinessError::Chain(ChainError::InsufficientFeeBalance)),
            )
            .await
            .unwrap();
    }
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
    assert!(row.err_code.is_none());
    assert_eq!(row.tx_fee_res_ack_sent_at, before.tx_fee_res_ack_sent_at);
    assert_eq!(row.service_fee_uploaded_at, before.service_fee_uploaded_at);
    assert!(row.raw_tx.is_none());
    assert!(row.tx_hash.is_none());
    assert!(row.building_at.is_none());
    assert!(intents.try_recv().is_err());
    sqlx::query("UPDATE api_collect SET updated_at=datetime('now','-31 seconds') WHERE trade_no=?")
        .bind(&trade)
        .execute(pool.as_ref())
        .await
        .unwrap();
    let candidates = ApiCollectRepo::scan_can_build(&pool, 1000).await.unwrap();
    assert!(candidates.iter().any(|r| r.trade_no == trade));
}

#[tokio::test]
async fn api_evm_fee_stale_build_preserves_pending_single_payment() {
    let (ctx, worker, trade, mut intents, _locks) = fixture().await;
    let pool = ctx.api_transaction_pool().unwrap();
    let stale = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
    let shortage = ServiceError::Business(BusinessError::Chain(ChainError::InsufficientFeeBalance));
    for needs_fee in [true, false] {
        sqlx::query("UPDATE api_collect SET need_service_fee=?, ever_needed_service_fee=true, service_fee_uploaded_at=strftime('%Y-%m-%dT%H:%M:%SZ','now'), tx_fee_res_ack_sent_at=NULL WHERE trade_no=?").bind(needs_fee).bind(&trade).execute(pool.as_ref()).await.unwrap();
        let before = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
        assert_eq!(
            worker.invalidate_build_attempt_after_fee_check_failure(&stale).await.unwrap(),
            0
        );
        assert_eq!(worker.reopen_evm_fee_cycle_if_unbuilt(&stale, &shortage).await.unwrap(), 0);
        let after = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
        assert_eq!(after.need_service_fee, Some(needs_fee));
        assert_eq!(after.service_fee_uploaded_at, before.service_fee_uploaded_at);
        assert!(after.tx_fee_res_ack_sent_at.is_none());
        assert!(after.err_code.is_none());
        assert!(intents.try_recv().is_err());
    }
}
