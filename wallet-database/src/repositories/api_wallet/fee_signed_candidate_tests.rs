use super::{ApiFeeRepo, SignedFeeCandidate};
use crate::{
    ApiTransactionDbPool,
    repositories::{api_wallet::nonce::ApiNonceRepo, test_helper::setup_api_transaction_pool},
};

async fn setup(name: &str, trade: &str) -> ApiTransactionDbPool {
    let pool = setup_api_transaction_pool(name).await;
    ApiFeeRepo::upsert_api_fee(
        &pool, "uid", "fee", "sender", "receiver", "1", "v", "arbitrum", None, "ETH", trade, 0,
    )
    .await
    .unwrap();
    ApiFeeRepo::set_tx_ack_sent(&pool, trade).await.unwrap();
    assert_eq!(ApiFeeRepo::update_building_at(&pool, trade).await.unwrap(), 1);
    pool
}
fn candidate(trade: &str) -> SignedFeeCandidate<'_> {
    SignedFeeCandidate {
        trade_no: trade,
        from_addr: "sender",
        chain_code: "arbitrum",
        tx_hash: "hash",
        raw_tx: "raw",
        transaction_fee: "0.1",
        nonce: 6,
        chain_next: 6,
    }
}

#[tokio::test]
async fn fee_signed_candidate_success_commits_raw_status_and_nonce_once() {
    let pool = setup("fee_candidate_success", "fee").await;
    assert_eq!(ApiFeeRepo::reserve_signed_fee(&pool, &candidate("fee")).await.unwrap(), 1);
    let row = ApiFeeRepo::get_api_fee_by_trade_no(&pool, "fee").await.unwrap();
    assert_eq!(row.raw_tx.as_deref(), Some("raw"));
    assert_eq!(row.nonce, 6);
    assert_eq!(row.status, row.recompute_status());
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
    assert_eq!(ApiFeeRepo::reserve_signed_fee(&pool, &candidate("fee")).await.unwrap(), 0);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
    let mut different = candidate("fee");
    different.raw_tx = "different";
    assert!(ApiFeeRepo::reserve_signed_fee(&pool, &different).await.is_err());
}

#[tokio::test]
async fn fee_signed_candidate_competing_allocator_leaves_no_raw() {
    let pool = setup("fee_candidate_conflict", "fee").await;
    ApiNonceRepo::set_nonce_floor(&pool, "sender", "arbitrum", 6).await.unwrap();
    assert!(ApiFeeRepo::reserve_signed_fee(&pool, &candidate("fee")).await.is_err());
    assert!(ApiFeeRepo::get_api_fee_by_trade_no(&pool, "fee").await.unwrap().raw_tx.is_none());
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
}

#[tokio::test]
async fn fee_signed_candidate_stale_lifecycle_and_identity_roll_back_nonce() {
    let pool = setup("fee_candidate_stale", "fee").await;
    let mut wrong = candidate("fee");
    wrong.from_addr = "other";
    assert!(ApiFeeRepo::reserve_signed_fee(&pool, &wrong).await.is_err());
    assert!(
        ApiNonceRepo::get_api_nonce_optional(&pool, "other", "arbitrum").await.unwrap().is_none()
    );
    sqlx::query("UPDATE api_fee SET finished_at = CURRENT_TIMESTAMP WHERE trade_no = 'fee'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert!(ApiFeeRepo::reserve_signed_fee(&pool, &candidate("fee")).await.is_err());
    assert!(
        ApiNonceRepo::get_api_nonce_optional(&pool, "sender", "arbitrum").await.unwrap().is_none()
    );
    assert!(ApiFeeRepo::get_api_fee_by_trade_no(&pool, "fee").await.unwrap().raw_tx.is_none());
}

#[tokio::test]
async fn fee_signed_candidate_concurrent_same_sender_commits_only_one() {
    let pool = setup("fee_candidate_concurrent", "fee1").await;
    ApiFeeRepo::upsert_api_fee(
        &pool, "uid", "fee", "sender", "receiver", "1", "v", "arbitrum", None, "ETH", "fee2", 0,
    )
    .await
    .unwrap();
    ApiFeeRepo::set_tx_ack_sent(&pool, "fee2").await.unwrap();
    ApiFeeRepo::update_building_at(&pool, "fee2").await.unwrap();
    let first = candidate("fee1");
    let second = candidate("fee2");
    let (a, b) = tokio::join!(
        ApiFeeRepo::reserve_signed_fee(&pool, &first),
        ApiFeeRepo::reserve_signed_fee(&pool, &second)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let a = ApiFeeRepo::get_api_fee_by_trade_no(&pool, "fee1").await.unwrap();
    let b = ApiFeeRepo::get_api_fee_by_trade_no(&pool, "fee2").await.unwrap();
    assert_eq!(usize::from(a.raw_tx.is_some()) + usize::from(b.raw_tx.is_some()), 1);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
}

#[tokio::test]
async fn fee_signed_candidate_result_ack_terminal_rolls_back_nonce() {
    let pool = setup("fee_candidate_result_ack", "fee").await;
    sqlx::query("UPDATE api_fee SET tx_res_ack_sent_at = CURRENT_TIMESTAMP WHERE trade_no = 'fee'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert!(ApiFeeRepo::reserve_signed_fee(&pool, &candidate("fee")).await.is_err());
    let row = ApiFeeRepo::get_api_fee_by_trade_no(&pool, "fee").await.unwrap();
    assert!(row.raw_tx.is_none());
    assert!(row.tx_hash.is_none());
    assert!(row.finished_at.is_none());
    assert!(row.tx_res_ack_sent_at.is_some());
    assert!(
        ApiNonceRepo::get_api_nonce_optional(&pool, "sender", "arbitrum").await.unwrap().is_none()
    );
}

#[tokio::test]
async fn shared_nonce_recovery_fee_fills_unsigned_hole_preserving_signed_six() {
    let pool = setup("shared_nonce_fee_hole", "saved-six").await;
    ApiFeeRepo::reserve_signed_fee(&pool, &candidate("saved-six")).await.unwrap();
    ApiFeeRepo::upsert_api_fee(
        &pool, "uid", "fee", "sender", "receiver", "1", "v", "arbitrum", None, "ETH", "new-five", 0,
    )
    .await
    .unwrap();
    ApiFeeRepo::set_tx_ack_sent(&pool, "new-five").await.unwrap();
    ApiFeeRepo::update_building_at(&pool, "new-five").await.unwrap();
    let mut five = candidate("new-five");
    five.nonce = 5;
    five.chain_next = 5;
    five.tx_hash = "hash-five";
    five.raw_tx = "raw-five";
    assert_eq!(ApiFeeRepo::reserve_signed_fee(&pool, &five).await.unwrap(), 1);
    assert_eq!(ApiFeeRepo::get_api_fee_by_trade_no(&pool, "saved-six").await.unwrap().nonce, 6);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
}

#[tokio::test]
async fn fee_signed_candidate_missing_order_keeps_nonce_unchanged() {
    let pool = setup("fee_candidate_missing_order", "fee").await;
    ApiNonceRepo::set_nonce_floor(&pool, "sender", "arbitrum", 5).await.unwrap();
    assert!(ApiFeeRepo::reserve_signed_fee(&pool, &candidate("missing")).await.is_err());
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 5);
    assert!(ApiFeeRepo::get_api_fee_by_trade_no(&pool, "fee").await.unwrap().raw_tx.is_none());
}
