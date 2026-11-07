use super::ShadowCollectWorker;
use crate::infrastructure::api_trans::{AddressLockManager, collect::shadow::ShadowAdvancer};
use wallet_database::{
    entities::api_collect::ApiCollectStatus, repositories::api_wallet::collect::ApiCollectRepo,
};

use crate::domain::{api_wallet::trans::RecoveredTxObservation, chain::TransferResp};

fn confirmed_response(fee: &str) -> TransferResp {
    let mut response = TransferResp::new("0xpending".into(), fee.into());
    response.with_transaction_time(chrono::Utc::now().timestamp_millis() as u128);
    response
}

async fn run_case(observation: RecoveredTxObservation) {
    let query_failed = matches!(&observation, RecoveredTxObservation::QueryUncertain);
    let confirmed = matches!(&observation, RecoveredTxObservation::Confirmed(_));
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    let pool = ctx.api_transaction_pool().unwrap();
    let trade = format!("receipt_observation_{}", uuid::Uuid::new_v4().simple());
    let (tx, mut intents) = tokio::sync::mpsc::channel(8);
    let worker = ShadowCollectWorker::new(
        ctx,
        std::sync::Arc::new(AddressLockManager::new()),
        std::sync::Arc::new(ShadowAdvancer::new(ctx, tx, None)),
    );
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
        ApiCollectStatus::SendingTx,
        1,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE api_collect SET err_code=NULL,err_msg=NULL,finished_at=NULL,transaction_time=NULL,nonce=7,tx_hash='0xpending',raw_tx='{}',broadcast_uncertain_since_at=strftime('%Y-%m-%dT%H:%M:%SZ','now','-25 hours'),order_ack_sent_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE trade_no=?").bind(&trade).execute(pool.as_ref()).await.unwrap();
    if matches!(&observation, RecoveredTxObservation::Missing) {
        sqlx::query("UPDATE api_collect SET broadcast_uncertain_since_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE trade_no=?").bind(&trade).execute(pool.as_ref()).await.unwrap();
    }
    let before = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
    if confirmed {
        // The same persisted order first sees a query failure, then confirmation.
        worker
            .apply_recover_observation(&before, Ok(RecoveredTxObservation::QueryUncertain))
            .await
            .unwrap();
        let still_pending =
            ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
        assert_eq!(
            serde_json::to_value(&still_pending).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        assert!(intents.try_recv().is_err());
    }
    worker.apply_recover_observation(&before, Ok(observation)).await.unwrap();
    let after = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
    assert!(after.err_code.is_none());
    assert!(after.finished_at.is_none());
    assert_eq!(after.tx_hash, before.tx_hash);
    assert_eq!(after.nonce, before.nonce);
    if confirmed {
        assert!(after.transaction_time.is_some());
        assert_eq!(after.transaction_fee, "0.1");
        worker
            .apply_recover_observation(
                &before,
                Ok(RecoveredTxObservation::Confirmed(confirmed_response("999"))),
            )
            .await
            .unwrap();
        let again = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade).await.unwrap();
        assert_eq!(again.transaction_time, after.transaction_time);
        assert_eq!(again.transaction_fee, after.transaction_fee);
        assert_eq!(again.nonce, after.nonce);
    } else {
        assert_eq!(after.status, ApiCollectStatus::SendingTx);
        assert!(after.transaction_time.is_none());
        assert!(after.tx_exec_receipt_uploaded_at.is_none());
        assert_eq!(after.raw_tx, before.raw_tx);
        if query_failed {
            assert_eq!(
                serde_json::to_value(&after).unwrap(),
                serde_json::to_value(&before).unwrap()
            );
        }
        assert!(
            intents.try_recv().is_err(),
            "observation must not schedule broadcast or failure upload"
        );
    }
}

#[tokio::test]
async fn receipt_observation_gateway_error_does_not_mutate_old_order() {
    run_case(RecoveredTxObservation::QueryUncertain).await;
}

#[tokio::test]
async fn receipt_observation_empty_receipt_keeps_order_pending() {
    run_case(RecoveredTxObservation::Missing).await;
}

#[tokio::test]
async fn receipt_observation_success_after_retry_is_idempotent() {
    run_case(RecoveredTxObservation::Confirmed(confirmed_response("0.1"))).await;
}
