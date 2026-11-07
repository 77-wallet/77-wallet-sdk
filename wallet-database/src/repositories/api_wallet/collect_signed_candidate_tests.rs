use super::{ApiCollectRepo, SignedCollectCandidate};
use crate::{
    ApiTransactionDbPool,
    repositories::{api_wallet::nonce::ApiNonceRepo, test_helper::setup_api_transaction_pool},
};

async fn setup(name: &str, trade: &str) -> ApiTransactionDbPool {
    let pool = setup_api_transaction_pool(name).await;
    ApiCollectRepo::upsert_api_collect(
        &pool,
        "uid",
        "collect",
        "sender",
        "receiver",
        "1",
        "v",
        "arbitrum",
        None,
        "ETH",
        trade,
        2,
        crate::entities::api_collect::ApiCollectStatus::Init,
        1,
    )
    .await
    .unwrap();
    ApiCollectRepo::set_order_ack_sent(&pool, trade).await.unwrap();
    assert_eq!(ApiCollectRepo::update_building_at(&pool, trade).await.unwrap(), 1);
    pool
}
fn candidate(trade: &str) -> SignedCollectCandidate<'_> {
    SignedCollectCandidate {
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
async fn collect_signed_candidate_success_commits_raw_status_and_nonce_once() {
    let pool = setup("collect_candidate_success", "collect").await;
    assert_eq!(
        ApiCollectRepo::reserve_signed_collect(&pool, &candidate("collect")).await.unwrap(),
        1
    );
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "collect").await.unwrap();
    assert_eq!(row.raw_tx.as_deref(), Some("raw"));
    assert_eq!(row.nonce, 6);
    assert_eq!(row.status, row.recompute_status());
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
    assert_eq!(
        ApiCollectRepo::reserve_signed_collect(&pool, &candidate("collect")).await.unwrap(),
        0
    );
    let mut different = candidate("collect");
    different.raw_tx = "different";
    assert!(ApiCollectRepo::reserve_signed_collect(&pool, &different).await.is_err());
}

#[tokio::test]
async fn collect_signed_candidate_competing_allocator_leaves_no_raw() {
    let pool = setup("collect_candidate_conflict", "collect").await;
    ApiNonceRepo::set_nonce_floor(&pool, "sender", "arbitrum", 6).await.unwrap();
    assert!(ApiCollectRepo::reserve_signed_collect(&pool, &candidate("collect")).await.is_err());
    assert!(
        ApiCollectRepo::get_api_collect_by_trade_no(&pool, "collect")
            .await
            .unwrap()
            .raw_tx
            .is_none()
    );
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
}

#[tokio::test]
async fn collect_signed_candidate_stale_lifecycle_and_identity_roll_back_nonce() {
    let pool = setup("collect_candidate_stale", "collect").await;
    let mut wrong = candidate("collect");
    wrong.from_addr = "other";
    assert!(ApiCollectRepo::reserve_signed_collect(&pool, &wrong).await.is_err());
    assert!(
        ApiNonceRepo::get_api_nonce_optional(&pool, "other", "arbitrum").await.unwrap().is_none()
    );
    sqlx::query(
        "UPDATE api_collect SET finished_at = CURRENT_TIMESTAMP WHERE trade_no = 'collect'",
    )
    .execute(pool.write_ref())
    .await
    .unwrap();
    assert!(ApiCollectRepo::reserve_signed_collect(&pool, &candidate("collect")).await.is_err());
    assert!(
        ApiNonceRepo::get_api_nonce_optional(&pool, "sender", "arbitrum").await.unwrap().is_none()
    );
    assert!(
        ApiCollectRepo::get_api_collect_by_trade_no(&pool, "collect")
            .await
            .unwrap()
            .raw_tx
            .is_none()
    );
}

#[tokio::test]
async fn collect_signed_candidate_concurrent_same_sender_commits_only_one() {
    let pool = setup("collect_candidate_concurrent", "collect1").await;
    ApiCollectRepo::upsert_api_collect(
        &pool,
        "uid",
        "collect",
        "sender",
        "receiver",
        "1",
        "v",
        "arbitrum",
        None,
        "ETH",
        "collect2",
        2,
        crate::entities::api_collect::ApiCollectStatus::Init,
        1,
    )
    .await
    .unwrap();
    ApiCollectRepo::set_order_ack_sent(&pool, "collect2").await.unwrap();
    ApiCollectRepo::update_building_at(&pool, "collect2").await.unwrap();
    let first = candidate("collect1");
    let second = candidate("collect2");
    let (a, b) = tokio::join!(
        ApiCollectRepo::reserve_signed_collect(&pool, &first),
        ApiCollectRepo::reserve_signed_collect(&pool, &second)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let a = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "collect1").await.unwrap();
    let b = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "collect2").await.unwrap();
    assert_eq!(usize::from(a.raw_tx.is_some()) + usize::from(b.raw_tx.is_some()), 1);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
}

#[tokio::test]
async fn collect_signed_candidate_result_ack_terminal_rolls_back_nonce() {
    let pool = setup("collect_candidate_result_ack", "collect").await;
    sqlx::query(
        "UPDATE api_collect SET result_ack_sent_at = CURRENT_TIMESTAMP WHERE trade_no = 'collect'",
    )
    .execute(pool.write_ref())
    .await
    .unwrap();
    assert!(ApiCollectRepo::reserve_signed_collect(&pool, &candidate("collect")).await.is_err());
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "collect").await.unwrap();
    assert!(row.raw_tx.is_none());
    assert!(row.tx_hash.is_none());
    assert!(row.finished_at.is_none());
    assert!(row.result_ack_sent_at.is_some());
    assert!(
        ApiNonceRepo::get_api_nonce_optional(&pool, "sender", "arbitrum").await.unwrap().is_none()
    );
}

#[tokio::test]
async fn collect_signed_candidate_late_lifecycle_change_cannot_reserve_nonce() {
    for (n, assignment) in [
        "need_service_fee = true",
        "ever_needed_service_fee = true",
        "err_code = 6001",
        "order_ack_sent_at = NULL",
        "building_at = NULL",
        "broadcast_uncertain_since_at = CURRENT_TIMESTAMP",
        "tx_exec_receipt_uploaded_at = CURRENT_TIMESTAMP",
    ]
    .into_iter()
    .enumerate()
    {
        let pool = setup(&format!("collect_candidate_gate_{n}"), "collect").await;
        sqlx::query(&format!("UPDATE api_collect SET {assignment} WHERE trade_no = 'collect'"))
            .execute(pool.write_ref())
            .await
            .unwrap();
        assert!(
            ApiCollectRepo::reserve_signed_collect(&pool, &candidate("collect")).await.is_err(),
            "{assignment}"
        );
        assert!(
            ApiNonceRepo::get_api_nonce_optional(&pool, "sender", "arbitrum")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            ApiCollectRepo::get_api_collect_by_trade_no(&pool, "collect")
                .await
                .unwrap()
                .raw_tx
                .is_none()
        );
    }
}

#[tokio::test]
async fn collect_signed_candidate_status_write_failure_rolls_back_signed_facts_and_nonce() {
    let pool = setup("collect_candidate_status_rollback", "collect").await;
    sqlx::query("CREATE TRIGGER reject_collect_status BEFORE UPDATE OF status ON api_collect BEGIN SELECT RAISE(ABORT, 'status write failed'); END")
        .execute(pool.write_ref()).await.unwrap();
    assert!(ApiCollectRepo::reserve_signed_collect(&pool, &candidate("collect")).await.is_err());
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "collect").await.unwrap();
    assert!(row.raw_tx.is_none());
    assert!(row.tx_hash.is_none());
    assert!(
        ApiNonceRepo::get_api_nonce_optional(&pool, "sender", "arbitrum").await.unwrap().is_none()
    );
}

#[tokio::test]
async fn shared_nonce_recovery_collect_fills_unsigned_hole_preserving_signed_six() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup("shared_nonce_collect_hole", "saved-six").await;
    ApiCollectRepo::reserve_signed_collect(&pool, &candidate("saved-six")).await.unwrap();
    sqlx::query(
        "UPDATE api_collect SET last_broadcast_at=CURRENT_TIMESTAMP WHERE trade_no='saved-six'",
    )
    .execute(pool.write_ref())
    .await
    .unwrap();
    ApiCollectRepo::upsert_api_collect(
        &pool,
        "uid",
        "collect",
        "sender",
        "receiver",
        "1",
        "v",
        "arbitrum",
        None,
        "ETH",
        "new-five",
        2,
        crate::entities::api_collect::ApiCollectStatus::Init,
        1,
    )
    .await
    .unwrap();
    ApiCollectRepo::set_order_ack_sent(&pool, "new-five").await.unwrap();
    ApiCollectRepo::update_building_at(&pool, "new-five").await.unwrap();
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&pool, "SENDER", "arbitrum", 5).await.unwrap(),
        WithdrawNonceDecision::Ready(5)
    );
    let mut five = candidate("new-five");
    five.nonce = 5;
    five.chain_next = 5;
    five.tx_hash = "hash-five";
    five.raw_tx = "raw-five";
    assert_eq!(ApiCollectRepo::reserve_signed_collect(&pool, &five).await.unwrap(), 1);
    let old = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "saved-six").await.unwrap();
    assert_eq!(old.raw_tx.as_deref(), Some("raw"));
    assert!(old.last_broadcast_at.is_some());
    assert_eq!(old.nonce, 6);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
    // A fresh observer derives ownership from persisted facts; it cannot select5 again.
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&pool, "sender", "arbitrum", 5).await.unwrap(),
        WithdrawNonceDecision::Ready(7)
    );
}

#[tokio::test]
async fn shared_nonce_recovery_competing_collect_and_fee_commit_hole_once() {
    use crate::repositories::api_wallet::fee::{ApiFeeRepo, SignedFeeCandidate};
    let pool = setup("shared_nonce_mixed_race", "new-five").await;
    ApiFeeRepo::upsert_api_fee(
        &pool,
        "uid",
        "fee",
        "sender",
        "receiver",
        "1",
        "v",
        "arbitrum",
        None,
        "ETH",
        "saved-six",
        0,
    )
    .await
    .unwrap();
    ApiFeeRepo::set_tx_ack_sent(&pool, "saved-six").await.unwrap();
    ApiFeeRepo::update_building_at(&pool, "saved-six").await.unwrap();
    ApiFeeRepo::reserve_signed_fee(
        &pool,
        &SignedFeeCandidate {
            trade_no: "saved-six",
            from_addr: "sender",
            chain_code: "arbitrum",
            nonce: 6,
            chain_next: 6,
            tx_hash: "hash-six",
            raw_tx: "raw-six",
            transaction_fee: "0.1",
        },
    )
    .await
    .unwrap();
    ApiFeeRepo::upsert_api_fee(
        &pool, "uid", "fee", "sender", "receiver", "1", "v", "arbitrum", None, "ETH", "fee-five", 0,
    )
    .await
    .unwrap();
    ApiFeeRepo::set_tx_ack_sent(&pool, "fee-five").await.unwrap();
    ApiFeeRepo::update_building_at(&pool, "fee-five").await.unwrap();
    let mut collect = candidate("new-five");
    collect.nonce = 5;
    collect.chain_next = 5;
    let fee = SignedFeeCandidate {
        trade_no: "fee-five",
        from_addr: "SENDER",
        chain_code: "arbitrum",
        nonce: 5,
        chain_next: 5,
        tx_hash: "hash-five",
        raw_tx: "raw-five",
        transaction_fee: "0.1",
    };
    // Use the same recorded identity; case aliases are verified separately in observation.
    let fee = SignedFeeCandidate { from_addr: "sender", ..fee };
    let (a, b) = tokio::join!(
        ApiCollectRepo::reserve_signed_collect(&pool, &collect),
        ApiFeeRepo::reserve_signed_fee(&pool, &fee)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let count:i64=sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM api_collect WHERE nonce=5 AND raw_tx IS NOT NULL)+(SELECT COUNT(*) FROM api_fee WHERE nonce=5 AND raw_tx IS NOT NULL)").fetch_one(pool.read_ref()).await.unwrap();
    assert_eq!(count, 1);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
}

#[tokio::test]
async fn shared_nonce_recovery_restart_retains_hole_and_rawless_uncertainty() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup("shared_nonce_restart", "saved-six").await;
    ApiCollectRepo::reserve_signed_collect(&pool, &candidate("saved-six")).await.unwrap();
    let file: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name='main'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    pool.read_ref().close().await;
    pool.write_ref().close().await;
    let dir = std::path::Path::new(&file).parent().unwrap();
    let restarted = crate::SqliteContext::new(dir.to_str().unwrap(), Some("api_transaction.db"))
        .await
        .unwrap()
        .into_transaction_db_pool()
        .unwrap();
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&restarted, "sender", "arbitrum", 5).await.unwrap(),
        WithdrawNonceDecision::Ready(5)
    );
    // A durable uncertainty marker is ownership even if raw/hash were lost.
    sqlx::query("INSERT INTO api_collect(uid,name,from_addr,to_addr,value,validate,chain_code,symbol,trade_no,trade_type,status,nonce,created_at,broadcast_uncertain_since_at) VALUES ('uid','uncertain','sender','receiver','1','v','arbitrum','ETH','uncertain-five',2,0,5,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").execute(restarted.write_ref()).await.unwrap();
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&restarted, "sender", "arbitrum", 5).await.unwrap(),
        WithdrawNonceDecision::Ready(7)
    );
}

#[tokio::test]
async fn collect_nonce_gap_failure_is_idempotent_retains_bytes_and_allows_new_five() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup("collect_gap_report", "six").await;
    ApiCollectRepo::reserve_signed_collect(&pool, &candidate("six")).await.unwrap();
    assert_eq!(ApiCollectRepo::fail_verified_nonce_gap(&pool, "six", 5, 6).await.unwrap(), 1);
    assert_eq!(ApiCollectRepo::fail_verified_nonce_gap(&pool, "six", 5, 6).await.unwrap(), 0);
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "six").await.unwrap();
    assert_eq!(row.raw_tx.as_deref(), Some("raw"));
    assert_eq!(row.tx_hash.as_deref(), Some("hash"));
    assert!(row.err_msg.unwrap().contains("missing_nonce=5"));
    assert!(row.transaction_time.is_none());
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&pool, "sender", "arbitrum", 5).await.unwrap(),
        WithdrawNonceDecision::Ready(5)
    );
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&pool, "sender", "arbitrum", 6).await.unwrap(),
        WithdrawNonceDecision::Wait
    );
    assert!(
        ApiCollectRepo::scan_need_tx_exec_receipt_upload(&pool, 100)
            .await
            .unwrap()
            .iter()
            .any(|r| r.trade_no == "six")
    );
    ApiCollectRepo::upsert_api_collect(
        &pool,
        "uid",
        "collect",
        "sender",
        "receiver",
        "1",
        "v",
        "arbitrum",
        None,
        "ETH",
        "new-five",
        2,
        crate::entities::api_collect::ApiCollectStatus::Init,
        1,
    )
    .await
    .unwrap();
    ApiCollectRepo::set_order_ack_sent(&pool, "new-five").await.unwrap();
    ApiCollectRepo::update_building_at(&pool, "new-five").await.unwrap();
    let mut five = candidate("new-five");
    five.nonce = 5;
    five.chain_next = 5;
    assert_eq!(ApiCollectRepo::reserve_signed_collect(&pool, &five).await.unwrap(), 1);
    assert_eq!(
        ApiCollectRepo::get_api_collect_by_trade_no(&pool, "new-five").await.unwrap().nonce,
        5
    );
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
}

#[tokio::test]
async fn collect_nonce_gap_does_not_fail_normal_queue_or_confirmed_order() {
    let pool = setup("collect_gap_guards", "six").await;
    ApiCollectRepo::reserve_signed_collect(&pool, &candidate("six")).await.unwrap();
    assert_eq!(ApiCollectRepo::fail_verified_nonce_gap(&pool, "six", 6, 6).await.unwrap(), 0);
    assert_eq!(ApiCollectRepo::fail_verified_nonce_gap(&pool, "six", 5, 7).await.unwrap(), 0);
    sqlx::query("UPDATE api_collect SET transaction_time=CURRENT_TIMESTAMP WHERE trade_no='six'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert_eq!(ApiCollectRepo::fail_verified_nonce_gap(&pool, "six", 5, 6).await.unwrap(), 0);
    assert!(
        ApiCollectRepo::get_api_collect_by_trade_no(&pool, "six").await.unwrap().err_code.is_none()
    );
}

#[tokio::test]
async fn collect_nonce_gap_preserves_normal_signed_queue_and_rolls_back_failure() {
    let pool = setup("collect_gap_queue", "six").await;
    ApiCollectRepo::reserve_signed_collect(&pool, &candidate("six")).await.unwrap();
    ApiCollectRepo::upsert_api_collect(
        &pool,
        "uid",
        "collect",
        "sender",
        "receiver",
        "1",
        "v",
        "arbitrum",
        None,
        "ETH",
        "five",
        2,
        crate::entities::api_collect::ApiCollectStatus::Init,
        1,
    )
    .await
    .unwrap();
    ApiCollectRepo::set_order_ack_sent(&pool, "five").await.unwrap();
    ApiCollectRepo::update_building_at(&pool, "five").await.unwrap();
    let mut five = candidate("five");
    five.nonce = 5;
    five.chain_next = 5;
    ApiCollectRepo::reserve_signed_collect(&pool, &five).await.unwrap();
    assert_eq!(ApiCollectRepo::fail_verified_nonce_gap(&pool, "six", 5, 6).await.unwrap(), 0);
    assert!(
        ApiCollectRepo::get_api_collect_by_trade_no(&pool, "six").await.unwrap().err_code.is_none()
    );
    sqlx::query("DELETE FROM api_collect WHERE trade_no='five'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_gap_failure BEFORE UPDATE OF status ON api_collect BEGIN SELECT RAISE(ABORT,'failure write rejected'); END").execute(pool.write_ref()).await.unwrap();
    assert!(ApiCollectRepo::fail_verified_nonce_gap(&pool, "six", 5, 6).await.is_err());
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "six").await.unwrap();
    assert!(row.err_code.is_none());
    assert_eq!(row.raw_tx.as_deref(), Some("raw"));
}

#[tokio::test]
async fn collect_query_uncertain_legacy_counter_reports_gap_and_commits_new_five() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup("legacy_counter_query_gap", "six").await;
    ApiCollectRepo::reserve_signed_collect(&pool, &candidate("six")).await.unwrap();
    ApiNonceRepo::set_nonce_exact(&pool, "sender", "arbitrum", 153).await.unwrap();
    assert_eq!(ApiCollectRepo::fail_verified_nonce_gap(&pool, "six", 5, 6).await.unwrap(), 1);
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&pool, "sender", "arbitrum", 5).await.unwrap(),
        WithdrawNonceDecision::Ready(5)
    );
    ApiCollectRepo::upsert_api_collect(
        &pool,
        "uid",
        "collect",
        "sender",
        "receiver",
        "1",
        "v",
        "arbitrum",
        None,
        "ETH",
        "new-five",
        2,
        crate::entities::api_collect::ApiCollectStatus::Init,
        1,
    )
    .await
    .unwrap();
    ApiCollectRepo::set_order_ack_sent(&pool, "new-five").await.unwrap();
    ApiCollectRepo::update_building_at(&pool, "new-five").await.unwrap();
    let mut five = candidate("new-five");
    five.nonce = 5;
    five.chain_next = 5;
    assert_eq!(ApiCollectRepo::reserve_signed_collect(&pool, &five).await.unwrap(), 1);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 153);
    assert_eq!(
        ApiCollectRepo::get_api_collect_by_trade_no(&pool, "six").await.unwrap().raw_tx.as_deref(),
        Some("raw")
    );
}
