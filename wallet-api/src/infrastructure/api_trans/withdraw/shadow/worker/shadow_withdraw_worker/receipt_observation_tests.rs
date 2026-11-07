use super::ShadowWithdrawWorker;
use crate::infrastructure::api_trans::withdraw::shadow::{ScannerConfig, ShadowScanner};
use wallet_database::{
    entities::{api_trade_type::ApiTradeType, api_withdraw::ApiWithdrawStatus},
    repositories::api_wallet::withdraw::ApiWithdrawRepo,
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
    let scanner = std::sync::Arc::new(ShadowScanner::new(ctx, ScannerConfig::default(), tx, None));
    let worker = ShadowWithdrawWorker::new(ctx, scanner);
    ApiWithdrawRepo::upsert_api_withdraw_with_fact(
        &pool,
        "uid",
        "withdraw",
        "from",
        "to",
        "1",
        "digest",
        "base",
        None,
        "ETH",
        &trade,
        ApiTradeType::Withdraw,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE api_withdraws SET err_code=NULL,err_msg=NULL,finished_at=NULL,transaction_time=NULL,status=3,init_status=3,nonce=7,tx_hash='0xpending',raw_tx='{}',last_broadcast_at=NULL,broadcast_uncertain_since_at=strftime('%Y-%m-%dT%H:%M:%SZ','now','-25 hours'),broadcast_uncertain_reconciled_at=strftime('%Y-%m-%dT%H:%M:%SZ','now'),audit_passed_at=strftime('%Y-%m-%dT%H:%M:%SZ','now'),tx_ack_sent_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE trade_no=?").bind(&trade).execute(pool.as_ref()).await.unwrap();
    let before =
        ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, &trade, ApiTradeType::Withdraw)
            .await
            .unwrap();
    assert!(before.err_code.is_none());
    assert!(before.finished_at.is_none());
    assert!(before.last_broadcast_at.is_none());
    assert!(before.raw_tx.is_some());
    assert!(
        before.broadcast_uncertain_since_at.unwrap()
            < chrono::Utc::now() - chrono::Duration::hours(24)
    );
    if confirmed {
        // The same persisted order first sees a query failure, then confirmation.
        worker
            .apply_recover_observation(&before, Ok(RecoveredTxObservation::QueryUncertain))
            .await
            .unwrap();
        let still_pending =
            ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, &trade, ApiTradeType::Withdraw)
                .await
                .unwrap();
        assert_eq!(
            serde_json::to_value(&still_pending).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        assert!(intents.try_recv().is_err());
    }
    worker.apply_recover_observation(&before, Ok(observation)).await.unwrap();
    let after =
        ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, &trade, ApiTradeType::Withdraw)
            .await
            .unwrap();
    assert!(after.err_code.is_none());
    assert!(after.finished_at.is_none());
    assert_eq!(after.tx_hash, before.tx_hash);
    assert_eq!(after.nonce, before.nonce);
    if confirmed {
        assert!(after.transaction_time.is_some());
        assert_eq!(after.transaction_fee, before.transaction_fee);
        worker
            .apply_recover_observation(
                &before,
                Ok(RecoveredTxObservation::Confirmed(confirmed_response("999"))),
            )
            .await
            .unwrap();
        let again =
            ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, &trade, ApiTradeType::Withdraw)
                .await
                .unwrap();
        assert_eq!(again.transaction_time, after.transaction_time);
        assert_eq!(again.transaction_fee, after.transaction_fee);
        assert_eq!(again.nonce, after.nonce);
    } else {
        assert_eq!(after.status, ApiWithdrawStatus::SendingTx);
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
