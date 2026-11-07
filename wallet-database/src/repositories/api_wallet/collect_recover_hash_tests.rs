use super::ApiCollectRepo;
use crate::{
    ApiTransactionDbPool, entities::api_collect::ApiCollectStatus,
    repositories::test_helper::setup_api_transaction_pool,
};

const HASH: &str = "confirmed-chain-hash";
const TIME: &str = "2026-09-07T01:18:35Z";

async fn seed(pool: &ApiTransactionDbPool, hash: Option<&str>) {
    ApiCollectRepo::upsert_api_collect(
        pool,
        "user",
        "collect",
        "sender",
        "recipient",
        "1.1",
        "digest",
        "eth",
        None,
        "USDT",
        "order",
        0,
        ApiCollectStatus::SendingTx,
        0,
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE api_collect SET tx_hash=?, raw_tx='original-raw', nonce=1,
        order_ack_sent_at=datetime('now'), broadcast_uncertain_since_at=datetime('now'),
        broadcast_uncertain_retry_count=2 WHERE trade_no='order'",
    )
    .bind(hash)
    .execute(pool.as_ref())
    .await
    .unwrap();
}

async fn confirm(pool: &ApiTransactionDbPool, hash: &str) -> u64 {
    ApiCollectRepo::confirm_onchain_transaction_fact_with_recover(
        pool, "order", hash, TIME, TIME, "0.001", "0",
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn collect_recover_hash_completes_missing_or_matching_identity_once() {
    for hash in [None, Some(""), Some("   "), Some(HASH)] {
        let pool = setup_api_transaction_pool("collect_recover_hash_complete").await;
        seed(&pool, hash).await;

        assert_eq!(confirm(&pool, HASH).await, 1, "stored hash {hash:?}");

        let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "order").await.unwrap();
        assert_eq!(row.tx_hash.as_deref(), Some(HASH));
        assert_eq!(row.raw_tx.as_deref(), Some("original-raw"));
        assert_eq!(row.nonce, 1);
        assert_eq!(row.transaction_time.unwrap().timestamp(), 1788743915);
        assert_eq!(row.last_broadcast_at, row.transaction_time);
        assert!(row.broadcast_uncertain_since_at.is_none());
        assert_eq!(row.broadcast_uncertain_retry_count, 0);
        assert!(
            ApiCollectRepo::scan_need_tx_exec_receipt_upload(&pool, 10)
                .await
                .unwrap()
                .iter()
                .any(|row| row.trade_no == "order")
        );
        let before = serde_json::to_value(&row).unwrap();
        assert_eq!(confirm(&pool, HASH).await, 0);
        assert_eq!(
            serde_json::to_value(
                ApiCollectRepo::get_api_collect_by_trade_no(&pool, "order").await.unwrap()
            )
            .unwrap(),
            before
        );
    }
}

#[tokio::test]
async fn collect_recover_hash_preserves_replaced_or_confirmed_rows() {
    for (hash, confirmed) in [(Some("different-hash"), false), (Some(HASH), true), (Some(""), true)]
    {
        let pool = setup_api_transaction_pool("collect_recover_hash_protected").await;
        seed(&pool, hash).await;
        if confirmed {
            sqlx::query("UPDATE api_collect SET transaction_time=? WHERE trade_no='order'")
                .bind(TIME)
                .execute(pool.as_ref())
                .await
                .unwrap();
        }
        let before = serde_json::to_value(
            ApiCollectRepo::get_api_collect_by_trade_no(&pool, "order").await.unwrap(),
        )
        .unwrap();

        assert_eq!(confirm(&pool, HASH).await, 0);

        assert_eq!(
            serde_json::to_value(
                ApiCollectRepo::get_api_collect_by_trade_no(&pool, "order").await.unwrap()
            )
            .unwrap(),
            before
        );
    }
}

#[tokio::test]
async fn collect_recover_hash_rejects_blank_confirmation_identity() {
    for hash in ["", "   "] {
        let pool = setup_api_transaction_pool("collect_recover_hash_blank").await;
        seed(&pool, None).await;
        let before = serde_json::to_value(
            ApiCollectRepo::get_api_collect_by_trade_no(&pool, "order").await.unwrap(),
        )
        .unwrap();

        assert_eq!(confirm(&pool, hash).await, 0);

        assert_eq!(
            serde_json::to_value(
                ApiCollectRepo::get_api_collect_by_trade_no(&pool, "order").await.unwrap()
            )
            .unwrap(),
            before
        );
    }
}
