use super::*;
use crate::infrastructure::api_trans::collect_fee::shadow::scanner::ScannerConfig;

async fn run_case(observation: RecoveredTxObservation, rebroadcast_count: i64) {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    let pool = ctx.api_transaction_pool().unwrap();
    let trade = format!("fee_receipt_{}", uuid::Uuid::new_v4().simple());
    let (tx, mut intents) = tokio::sync::mpsc::channel(8);
    let scanner = Arc::new(ShadowScanner::new(ctx, ScannerConfig::default(), tx, None));
    let worker = ShadowFeeWorker::new(ctx, Arc::new(AddressLockManager::new()), scanner);
    ApiFeeRepo::upsert_api_fee(
        &pool, "uid", "fee", "from", "to", "1", "v", "base", None, "ETH", &trade, 0,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE api_fee SET err_code=NULL,err_msg=NULL,finished_at=NULL,transaction_time=NULL,nonce=7,tx_hash='0xpending',raw_tx='{}',broadcast_uncertain_since_at=strftime('%Y-%m-%dT%H:%M:%SZ','now','-25 hours'),broadcast_uncertain_reconciled_at=strftime('%Y-%m-%dT%H:%M:%SZ','now'),broadcast_uncertain_rebroadcast_count=? WHERE trade_no=?").bind(rebroadcast_count).bind(&trade).execute(pool.as_ref()).await.unwrap();
    let before = ApiFeeRepo::get_api_fee_by_trade_no(&pool, &trade).await.unwrap();
    let query_failed = matches!(&observation, RecoveredTxObservation::QueryUncertain);
    let confirmed = matches!(&observation, RecoveredTxObservation::Confirmed(_));
    worker
        .apply_recover_observation(&before, Ok(RecoveredTxObservation::QueryUncertain))
        .await
        .unwrap();
    let pending = ApiFeeRepo::get_api_fee_by_trade_no(&pool, &trade).await.unwrap();
    assert_eq!(serde_json::to_value(&pending).unwrap(), serde_json::to_value(&before).unwrap());
    assert!(intents.try_recv().is_err());
    worker.apply_recover_observation(&before, Ok(observation)).await.unwrap();
    let after = ApiFeeRepo::get_api_fee_by_trade_no(&pool, &trade).await.unwrap();
    assert!(after.err_code.is_none(), "missing receipt is not chain failure");
    assert!(after.finished_at.is_none());
    assert_eq!(after.raw_tx, before.raw_tx);
    assert_eq!(after.tx_hash, before.tx_hash);
    assert_eq!(after.nonce, before.nonce);
    if confirmed {
        assert!(after.transaction_time.is_some());
        assert_eq!(after.transaction_fee, before.transaction_fee);
    } else {
        assert!(after.transaction_time.is_none());
        assert!(intents.try_recv().is_err());
    }
    if query_failed {
        assert_eq!(serde_json::to_value(&after).unwrap(), serde_json::to_value(&before).unwrap());
    }
}

#[tokio::test]
async fn receipt_observation_empty_fee_receipt_cannot_fail_or_rebuild_order() {
    for count in [0, 1] {
        run_case(RecoveredTxObservation::Missing, count).await;
    }
}
#[tokio::test]
async fn receipt_observation_fee_gateway_error_preserves_all_facts() {
    run_case(RecoveredTxObservation::QueryUncertain, 1).await;
}
#[tokio::test]
async fn receipt_observation_fee_success_after_query_retry() {
    let mut resp = crate::domain::chain::TransferResp::new("0xpending".into(), "0.1".into());
    resp.with_transaction_time(chrono::Utc::now().timestamp_millis() as u128);
    run_case(RecoveredTxObservation::Confirmed(resp), 1).await;
}
