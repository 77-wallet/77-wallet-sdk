use super::{ApiCollectRepo, FinalizedCollectNonceConflict};
use crate::{
    ApiTransactionDbPool,
    entities::api_collect::{ApiCollectStatus, ErrCode},
    repositories::test_helper::setup_api_transaction_pool,
};

fn hash(byte: &str) -> String {
    format!("0x{}", byte.repeat(32))
}

fn evidence() -> FinalizedCollectNonceConflict {
    FinalizedCollectNonceConflict {
        chain_code: "eth".into(),
        from_addr: format!("0x{}", "ab".repeat(20)),
        nonce: 1,
        conflicting_tx_hash: hash("22"),
        block_hash: hash("33"),
        block_number: 100,
    }
}

async fn seed(pool: &ApiTransactionDbPool, trade_no: &str) {
    ApiCollectRepo::upsert_api_collect(
        pool,
        "user",
        "collect",
        &evidence().from_addr,
        "recipient",
        "1.1",
        "v",
        "eth",
        None,
        "USDT",
        trade_no,
        0,
        ApiCollectStatus::SendingTx,
        0,
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE api_collect SET nonce = 1, raw_tx = 'original-raw', tx_hash = ?,
        order_ack_sent_at = datetime('now'), last_broadcast_at = datetime('now'),
        broadcast_uncertain_since_at = datetime('now', '-15 minutes') WHERE trade_no = ?",
    )
    .bind(hash("11"))
    .bind(trade_no)
    .execute(pool.as_ref())
    .await
    .unwrap();
}

#[tokio::test]
async fn collect_nonce_conflict_records_failure_without_replacing_transaction() {
    let pool = setup_api_transaction_pool("collect_nonce_conflict_success").await;
    seed(&pool, "order").await;

    let rows =
        ApiCollectRepo::record_finalized_nonce_conflict(&pool, "order", &hash("11"), &evidence())
            .await
            .unwrap();

    assert_eq!(rows, 1);
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "order").await.unwrap();
    assert_eq!(row.status, ApiCollectStatus::SendingTxFailed);
    assert_eq!(row.err_code, Some(ErrCode::TransactionOnChainException));
    assert!(row.err_msg.as_deref().unwrap().contains(&hash("22")));
    assert!(row.err_msg.as_deref().unwrap().contains(&hash("33")));
    assert_eq!(row.tx_hash.as_deref(), Some(hash("11").as_str()));
    assert_eq!(row.raw_tx.as_deref(), Some("original-raw"));
    assert_eq!(row.nonce, 1);
    assert!(row.transaction_time.is_none());
    assert!(row.finished_at.is_none());
    assert!(row.last_broadcast_at.is_some());
    assert!(ApiCollectRepo::scan_can_build(&pool, 10).await.unwrap().is_empty());
    assert!(ApiCollectRepo::scan_can_broadcast(&pool, 10).await.unwrap().is_empty());
    assert!(ApiCollectRepo::scan_need_recover(&pool, 10).await.unwrap().is_empty());

    assert!(
        ApiCollectRepo::scan_need_tx_exec_receipt_upload(&pool, 10)
            .await
            .unwrap()
            .iter()
            .any(|r| r.trade_no == "order")
    );
    assert_eq!(
        ApiCollectRepo::record_finalized_nonce_conflict(&pool, "order", &hash("11"), &evidence(),)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn collect_nonce_conflict_cannot_overwrite_advanced_or_replaced_order() {
    let pool = setup_api_transaction_pool("collect_nonce_conflict_guard").await;
    for (idx, update) in [
        "transaction_time = datetime('now')",
        "tx_exec_receipt_uploaded_at = datetime('now')",
        "finished_at = datetime('now')",
        "result_ack_sent_at = datetime('now')",
        "err_code = 6099, err_msg = 'existing error'",
        "status = 7",
        "nonce = 2",
        "tx_hash = 'different-hash'",
        "chain_code = 'bnb'",
        "from_addr = 'different-sender'",
        "raw_tx = NULL",
    ]
    .iter()
    .enumerate()
    {
        let trade_no = format!("order-{idx}");
        seed(&pool, &trade_no).await;
        sqlx::query(&format!("UPDATE api_collect SET {update} WHERE trade_no = ?"))
            .bind(&trade_no)
            .execute(pool.as_ref())
            .await
            .unwrap();
        let before = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade_no).await.unwrap();

        let rows = ApiCollectRepo::record_finalized_nonce_conflict(
            &pool,
            &trade_no,
            &hash("11"),
            &evidence(),
        )
        .await
        .unwrap();

        assert_eq!(rows, 0, "must preserve {update}");
        let after = ApiCollectRepo::get_api_collect_by_trade_no(&pool, &trade_no).await.unwrap();
        assert_eq!(serde_json::to_value(before).unwrap(), serde_json::to_value(after).unwrap());
    }
}

#[tokio::test]
async fn collect_nonce_conflict_rejects_same_hash_and_malformed_evidence() {
    let pool = setup_api_transaction_pool("collect_nonce_conflict_invalid").await;
    seed(&pool, "order").await;
    for idx in 0..5 {
        let mut conflict = evidence();
        match idx {
            0 => conflict.conflicting_tx_hash = hash("11"),
            1 => conflict.block_hash.clear(),
            2 => conflict.nonce = -1,
            3 => conflict.chain_code = "tron".into(),
            _ => conflict.from_addr.clear(),
        }
        assert!(ApiCollectRepo::record_finalized_nonce_conflict(
            &pool, "order", &hash("11"), &conflict,
        ).await.is_err());
    }
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "order").await.unwrap();
    assert!(row.err_code.is_none());
    assert_eq!(row.status, ApiCollectStatus::SendingTx);
}

#[tokio::test]
async fn collect_nonce_conflict_concurrent_reports_commit_once() {
    let pool = setup_api_transaction_pool("collect_nonce_conflict_concurrent").await;
    seed(&pool, "order").await;
    let expected = hash("11");
    let conflict = evidence();

    let (a, b) = tokio::join!(
        ApiCollectRepo::record_finalized_nonce_conflict(&pool, "order", &expected, &conflict),
        ApiCollectRepo::record_finalized_nonce_conflict(&pool, "order", &expected, &conflict),
    );

    assert_eq!(a.unwrap() + b.unwrap(), 1);
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "order").await.unwrap();
    assert!(row.err_code.is_some());
    assert_eq!(row.tx_hash.as_deref(), Some(expected.as_str()));
}
