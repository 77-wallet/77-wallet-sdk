use super::*;
use crate::repositories::{
    api_wallet::nonce::ApiNonceRepo, test_helper::setup_api_transaction_pool,
};

async fn fixture(pool: &ApiTransactionDbPool, trade: &str) {
    ApiWithdrawRepo::upsert_api_withdraw(
        pool,
        "uid",
        "withdraw",
        "sender",
        "receiver",
        "1",
        "digest",
        "arbitrum",
        None,
        "ETH",
        trade,
        None,
        None,
        None,
        ApiTradeType::Withdraw,
        1,
        None,
        ApiWithdrawStatus::AuditPass,
        ApiWithdrawStatus::InitOrder,
        "",
        "",
        None,
        None,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE api_withdraws SET tx_ack_sent_at=CURRENT_TIMESTAMP, audit_passed_at=CURRENT_TIMESTAMP, building_at=CURRENT_TIMESTAMP WHERE trade_no=?")
        .bind(trade).execute(pool.write_ref()).await.unwrap();
}

fn signed(trade: &str) -> SignedWithdrawCandidate<'_> {
    SignedWithdrawCandidate {
        trade_no: trade,
        from_addr: "sender",
        chain_code: "arbitrum",
        nonce: 16,
        chain_next: 16,
        tx_hash: "hash-16",
        raw_tx: "signed-16",
        transaction_fee: "0.01",
    }
}

#[tokio::test]
async fn withdrawal_signed_nonce_commits_bytes_and_reservation_atomically() {
    let pool = setup_api_transaction_pool("withdrawal_signed_nonce_success").await;
    fixture(&pool, "new").await;
    assert_eq!(ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("new")).await.unwrap(), 1);
    let row = ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, "new", ApiTradeType::Withdraw)
        .await
        .unwrap();
    assert_eq!(row.raw_tx.as_deref(), Some("signed-16"));
    assert_eq!(row.tx_hash.as_deref(), Some("hash-16"));
    assert_eq!(row.nonce, 16);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 16);
    assert_eq!(ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("new")).await.unwrap(), 0);
}

#[tokio::test]
async fn withdrawal_signed_nonce_losing_candidate_does_not_publish() {
    let pool = setup_api_transaction_pool("withdrawal_signed_nonce_stale").await;
    fixture(&pool, "new").await;
    ApiNonceRepo::set_nonce_floor(&pool, "sender", "arbitrum", 16).await.unwrap();
    assert!(ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("new")).await.is_err());
    let row = ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, "new", ApiTradeType::Withdraw)
        .await
        .unwrap();
    assert!(row.raw_tx.is_none());
    assert!(row.tx_hash.is_none());
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 16);
}

#[tokio::test]
async fn withdrawal_signed_nonce_failure_rolls_back_floor_and_bytes() {
    let pool = setup_api_transaction_pool("withdrawal_signed_nonce_rollback").await;
    fixture(&pool, "new").await;
    sqlx::query("CREATE TRIGGER reject_signed BEFORE UPDATE OF raw_tx ON api_withdraws WHEN NEW.trade_no='new' BEGIN SELECT RAISE(ABORT,'signed publication rejected'); END")
        .execute(pool.write_ref()).await.unwrap();
    assert!(ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("new")).await.is_err());
    assert_eq!(
        ApiNonceRepo::get_api_nonce_optional(&pool, "sender", "arbitrum").await.unwrap(),
        None
    );
    let row = ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, "new", ApiTradeType::Withdraw)
        .await
        .unwrap();
    assert!(row.raw_tx.is_none());
}

#[tokio::test]
async fn withdrawal_signed_nonce_ineligible_order_does_not_reserve() {
    let pool = setup_api_transaction_pool("withdrawal_signed_nonce_ineligible").await;
    fixture(&pool, "new").await;
    sqlx::query("UPDATE api_withdraws SET err_code=6008 WHERE trade_no='new'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert_eq!(ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("new")).await.unwrap(), 0);
    assert_eq!(
        ApiNonceRepo::get_api_nonce_optional(&pool, "sender", "arbitrum").await.unwrap(),
        None
    );
}

async fn fail_rejected(pool: &ApiTransactionDbPool, trade: &str) {
    sqlx::query("UPDATE api_withdraws SET err_code=6008,err_msg='node response: nonce too high',failure_stage=2,finished_at=CURRENT_TIMESTAMP WHERE trade_no=?")
        .bind(trade).execute(pool.write_ref()).await.unwrap();
}

#[tokio::test]
async fn withdrawal_nonce_reuse_explicit_rejection_observes_chain_nonce_not_17() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup_api_transaction_pool("withdrawal_nonce_reuse_rejected").await;
    fixture(&pool, "old").await;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("old")).await.unwrap();
    fail_rejected(&pool, "old").await;
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
        WithdrawNonceDecision::Ready(16)
    );
    assert_eq!(
        ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(),
        16,
        "observation must not rewind the counter"
    );
}

#[tokio::test]
async fn withdrawal_nonce_reuse_live_predecessor_keeps_reservation() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup_api_transaction_pool("withdrawal_nonce_reuse_live").await;
    fixture(&pool, "old").await;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("old")).await.unwrap();
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
        WithdrawNonceDecision::Ready(17)
    );
}

#[tokio::test]
async fn withdrawal_nonce_reuse_timeout_or_unknown_reservation_cannot_rewind() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup_api_transaction_pool("withdrawal_nonce_reuse_unknown").await;
    ApiNonceRepo::set_nonce_floor(&pool, "sender", "arbitrum", 16).await.unwrap();
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
        WithdrawNonceDecision::Wait
    );
    fixture(&pool, "old").await;
    sqlx::query("UPDATE api_withdraws SET nonce=16,raw_tx='signed',tx_hash='hash',err_code=NULL,err_msg=NULL,broadcast_uncertain_since_at=CURRENT_TIMESTAMP WHERE trade_no='old'").execute(pool.write_ref()).await.unwrap();
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
        WithdrawNonceDecision::Ready(17)
    );
}

#[tokio::test]
async fn withdrawal_nonce_reuse_known_failed_gap_blocks_future_signed_order() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup_api_transaction_pool("withdrawal_nonce_reuse_gap").await;
    fixture(&pool, "old").await;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("old")).await.unwrap();
    fixture(&pool, "future").await;
    let mut future = signed("future");
    future.nonce = 17;
    future.tx_hash = "hash17";
    future.raw_tx = "raw17";
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &future).await.unwrap();
    fail_rejected(&pool, "old").await;
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
        WithdrawNonceDecision::TerminalGap(16)
    );
}

#[tokio::test]
async fn withdrawal_nonce_reuse_failed_tail_commits_new_16_and_preserves_old_audit() {
    let pool = setup_api_transaction_pool("withdrawal_nonce_reuse_new16").await;
    fixture(&pool, "old").await;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("old")).await.unwrap();
    fail_rejected(&pool, "old").await;
    fixture(&pool, "new").await;
    assert_eq!(ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("new")).await.unwrap(), 1);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 16);
    let old = ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, "old", ApiTradeType::Withdraw)
        .await
        .unwrap();
    assert_eq!(old.err_code, Some(ErrCode::SDKInternalError));
    assert_eq!(old.raw_tx.as_deref(), Some("signed-16"));
    assert!(old.finished_at.is_some());
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
        crate::entities::api_nonce_claim::WithdrawNonceDecision::Ready(17)
    );
}

#[tokio::test]
async fn withdrawal_nonce_reuse_terminal_gap_failure_is_idempotent_and_reusable() {
    let pool = setup_api_transaction_pool("withdrawal_nonce_reuse_fail_gap").await;
    fixture(&pool, "old").await;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("old")).await.unwrap();
    fixture(&pool, "future").await;
    let mut future = signed("future");
    future.nonce = 17;
    future.tx_hash = "hash17";
    future.raw_tx = "raw17";
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &future).await.unwrap();
    fail_rejected(&pool, "old").await;
    assert_eq!(
        ApiWithdrawRepo::fail_on_terminal_nonce_gap(&pool, "future", "hash17", 16).await.unwrap(),
        1
    );
    assert_eq!(
        ApiWithdrawRepo::fail_on_terminal_nonce_gap(&pool, "future", "hash17", 16).await.unwrap(),
        0
    );
    let row =
        ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, "future", ApiTradeType::Withdraw)
            .await
            .unwrap();
    assert_eq!(row.err_code, Some(ErrCode::SDKInternalError));
    assert_eq!(row.failure_stage, Some(WithdrawFailureStage::Broadcast));
    assert!(row.err_msg.as_deref().unwrap().starts_with("nonce_gap_predecessor_failed:"));
    assert_eq!(row.raw_tx.as_deref(), Some("raw17"));
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
        crate::entities::api_nonce_claim::WithdrawNonceDecision::Ready(16)
    );
    assert_eq!(
        ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(),
        17,
        "failure must not eagerly rewind"
    );
}

#[tokio::test]
async fn withdrawal_nonce_reuse_uncertain_target_is_not_failed_or_released() {
    let pool = setup_api_transaction_pool("withdrawal_nonce_reuse_uncertain_target").await;
    fixture(&pool, "old").await;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("old")).await.unwrap();
    fixture(&pool, "future").await;
    let mut future = signed("future");
    future.nonce = 17;
    future.tx_hash = "hash17";
    future.raw_tx = "raw17";
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &future).await.unwrap();
    fail_rejected(&pool, "old").await;
    ApiWithdrawRepo::mark_broadcast_uncertain_attempt(&pool, "future").await.unwrap();
    assert_eq!(
        ApiWithdrawRepo::fail_on_terminal_nonce_gap(&pool, "future", "hash17", 16).await.unwrap(),
        0
    );
    let row =
        ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, "future", ApiTradeType::Withdraw)
            .await
            .unwrap();
    assert!(row.err_code.is_none());
    assert!(row.broadcast_uncertain_since_at.is_some());
}

#[tokio::test]
async fn withdrawal_nonce_reuse_other_flow_claim_prevents_releasing_same_nonce() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup_api_transaction_pool("withdrawal_nonce_reuse_fee_claim").await;
    fixture(&pool, "old").await;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("old")).await.unwrap();
    fail_rejected(&pool, "old").await;
    sqlx::query("INSERT INTO api_fee(name,from_addr,to_addr,value,validate,chain_code,symbol,trade_no,trade_type,nonce,raw_tx,tx_hash,created_at) VALUES('fee','sender','receiver','1','','arbitrum','ETH','fee',3,16,'fee-raw','fee-hash',CURRENT_TIMESTAMP)")
        .execute(pool.write_ref()).await.unwrap();
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
        WithdrawNonceDecision::Ready(17)
    );
    fixture(&pool, "new").await;
    assert!(ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("new")).await.is_err());
}

#[tokio::test]
async fn withdrawal_signed_nonce_competing_candidates_publish_only_one() {
    let pool = setup_api_transaction_pool("withdrawal_signed_nonce_competing").await;
    fixture(&pool, "first").await;
    fixture(&pool, "second").await;
    let first = signed("first");
    let second = signed("second");
    let (a, b) = tokio::join!(
        ApiWithdrawRepo::reserve_signed_withdraw(&pool, &first),
        ApiWithdrawRepo::reserve_signed_withdraw(&pool, &second)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_withdraws WHERE raw_tx IS NOT NULL")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(count, 1);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 16);
}

#[tokio::test]
async fn withdrawal_nonce_reuse_deterministic_error_does_not_depend_on_text() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup_api_transaction_pool("withdrawal_nonce_generic_error").await;
    fixture(&pool, "old").await;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("old")).await.unwrap();
    sqlx::query("UPDATE api_withdraws SET err_code=6001,err_msg='insufficient balance',failure_stage=2 WHERE trade_no='old'").execute(pool.write_ref()).await.unwrap();
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
        WithdrawNonceDecision::Ready(16)
    );
    sqlx::query("UPDATE api_withdraws SET transaction_time=CURRENT_TIMESTAMP,chain_failed_at=CURRENT_TIMESTAMP WHERE trade_no='old'").execute(pool.write_ref()).await.unwrap();
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
        WithdrawNonceDecision::Wait
    );
}

#[tokio::test]
async fn withdrawal_nonce_manual_review_error_keeps_uncertainty_and_blocks_reuse() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup_api_transaction_pool("withdrawal_nonce_manual_review").await;
    fixture(&pool, "old").await;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("old")).await.unwrap();
    ApiWithdrawRepo::mark_broadcast_uncertain_attempt(&pool, "old").await.unwrap();
    assert_eq!(
        ApiWithdrawRepo::mark_uncertain_manual_review(&pool, "old", "manual review required")
            .await
            .unwrap(),
        1
    );
    let row = ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, "old", ApiTradeType::Withdraw)
        .await
        .unwrap();
    assert!(row.broadcast_uncertain_since_at.is_some());
    assert_eq!(row.err_code, Some(ErrCode::TransactionOnChainException));
    assert_eq!(row.failure_stage, Some(WithdrawFailureStage::Chain));
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
        WithdrawNonceDecision::Wait
    );
    assert_eq!(row.raw_tx.as_deref(), Some("signed-16"));
    assert_eq!(
        ApiWithdrawRepo::mark_uncertain_manual_review(&pool, "old", "retry").await.unwrap(),
        0
    );
}

#[tokio::test]
async fn shared_nonce_recovery_withdraw_fills_unsigned_hole_preserving_high_water() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup_api_transaction_pool("shared_nonce_withdraw_hole").await;
    fixture(&pool, "saved-six").await;
    let mut old = signed("saved-six");
    old.nonce = 6;
    old.chain_next = 6;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &old).await.unwrap();
    fixture(&pool, "new-five").await;
    let mut five = signed("new-five");
    five.nonce = 5;
    five.chain_next = 5;
    five.tx_hash = "hash-five";
    five.raw_tx = "raw-five";
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 5).await.unwrap(),
        WithdrawNonceDecision::Ready(5)
    );
    assert_eq!(ApiWithdrawRepo::reserve_signed_withdraw(&pool, &five).await.unwrap(), 1);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 6);
    assert_eq!(
        ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, "saved-six", ApiTradeType::Withdraw)
            .await
            .unwrap()
            .raw_tx
            .as_deref(),
        Some("signed-16")
    );
    assert_eq!(
        ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 5).await.unwrap(),
        WithdrawNonceDecision::Ready(7)
    );
}

#[tokio::test]
async fn withdrawal_nonce_reuse_unknown_or_post_send_failure_stage_keeps_owner() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup_api_transaction_pool("withdrawal_nonce_unproven_failure").await;
    fixture(&pool, "old").await;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("old")).await.unwrap();
    for stage in [None, Some(0_i64), Some(3), Some(4), Some(99)] {
        sqlx::query("UPDATE api_withdraws SET err_code=6001,err_msg='failed',failure_stage=?,finished_at=CURRENT_TIMESTAMP WHERE trade_no='old'")
            .bind(stage).execute(pool.write_ref()).await.unwrap();
        assert_eq!(
            ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
            WithdrawNonceDecision::Wait,
            "stage={stage:?}"
        );
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "arbitrum").await.unwrap(), 16);
    }
}

#[tokio::test]
async fn withdrawal_nonce_reuse_known_pre_send_failure_requires_absent_send_facts() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    let pool = setup_api_transaction_pool("withdrawal_nonce_known_failure_facts").await;
    fixture(&pool, "old").await;
    ApiWithdrawRepo::reserve_signed_withdraw(&pool, &signed("old")).await.unwrap();
    for stage in [WithdrawFailureStage::Build, WithdrawFailureStage::Broadcast] {
        sqlx::query("UPDATE api_withdraws SET err_code=6001,err_msg='failed',failure_stage=?,finished_at=CURRENT_TIMESTAMP WHERE trade_no='old'")
            .bind(stage).execute(pool.write_ref()).await.unwrap();
        assert_eq!(
            ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16).await.unwrap(),
            WithdrawNonceDecision::Ready(16)
        );
        for column in ["last_broadcast_at", "broadcast_uncertain_since_at", "transaction_time"] {
            sqlx::query(&format!(
                "UPDATE api_withdraws SET {column}=CURRENT_TIMESTAMP WHERE trade_no='old'"
            ))
            .execute(pool.write_ref())
            .await
            .unwrap();
            assert_eq!(
                ApiWithdrawRepo::observe_withdraw_nonce(&pool, "sender", "arbitrum", 16)
                    .await
                    .unwrap(),
                WithdrawNonceDecision::Wait,
                "column={column}"
            );
            sqlx::query(&format!("UPDATE api_withdraws SET {column}=NULL WHERE trade_no='old'"))
                .execute(pool.write_ref())
                .await
                .unwrap();
        }
    }
}
