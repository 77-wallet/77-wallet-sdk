use super::*;
use crate::{
    SqliteContext,
    repositories::{
        api_wallet::nonce::ApiNonceRepo,
        test_helper::{make_temp_dir, setup_api_transaction_pool},
    },
};

fn signed() -> SignedSelfTransfer<'static> {
    SignedSelfTransfer {
        trade_no: "ordinary-4",
        uid: "wallet",
        name: "wallet",
        from_addr: "sender",
        to_addr: "receiver",
        chain_code: "eth",
        token_addr: AssetTokenKey::Native,
        symbol: "ETH",
        value: "1",
        nonce: 4,
        tx_hash: "hash-4",
        raw_tx: "signed-bytes-4",
        transaction_fee: "0.001",
        notes: Some("note"),
    }
}

async fn row(pool: &ApiTransactionDbPool) -> ApiWithdrawEntity {
    sqlx::query_as("SELECT * FROM api_withdraws WHERE trade_no = 'ordinary-4'")
        .fetch_one(pool.read_ref())
        .await
        .unwrap()
}

#[tokio::test]
async fn self_transfer_survives_reopen_and_acceptance_is_not_confirmation() {
    let dir = make_temp_dir("self_transfer_reopen");
    let pool = SqliteContext::new(&dir, Some("api_transaction.db"))
        .await
        .unwrap()
        .into_transaction_db_pool()
        .unwrap();
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    pool.read_ref().close().await;
    pool.write_ref().close().await;
    let pool = SqliteContext::new(&dir, Some("api_transaction.db"))
        .await
        .unwrap()
        .into_transaction_db_pool()
        .unwrap();

    let pending =
        ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, "sender", "eth").await.unwrap();
    assert_eq!(pending.len(), 1);
    // Ordinary records must not be picked up by the backend withdrawal workers.
    assert!(
        crate::repositories::api_wallet::withdraw::ApiWithdrawRepo::scan_can_broadcast(&pool, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        crate::repositories::api_wallet::withdraw::ApiWithdrawRepo::scan_need_recover(&pool, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(pending[0].nonce, 4);
    assert_eq!(pending[0].raw_tx.as_deref(), Some("signed-bytes-4"));
    assert_eq!(pending[0].tx_hash.as_deref(), Some("hash-4"));
    assert!(pending[0].broadcast_uncertain_since_at.is_some());
    assert!(pending[0].last_broadcast_at.is_none());
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 4);
    assert!(
        !ApiWithdrawRepo::mark_self_transfer_accepted(&pool, "ordinary-4", "wrong-hash")
            .await
            .unwrap()
    );
    assert!(
        ApiWithdrawRepo::mark_self_transfer_accepted(&pool, "ordinary-4", "hash-4").await.unwrap()
    );
    assert!(
        !ApiWithdrawRepo::mark_self_transfer_accepted(&pool, "ordinary-4", "hash-4").await.unwrap()
    );
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    let accepted = row(&pool).await;
    assert!(accepted.last_broadcast_at.is_some());
    assert!(accepted.broadcast_uncertain_since_at.is_none());
    assert!(accepted.transaction_time.is_none());
    assert!(accepted.chain_success_at.is_none());
    assert!(accepted.finished_at.is_none());
    assert!(accepted.tx_ack_sent_at.is_none());
    assert_eq!(
        ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, "sender", "eth")
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn self_transfer_conflicting_retry_preserves_original_and_nonce_floor() {
    let pool = setup_api_transaction_pool("self_transfer_conflict").await;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    let mut conflict = signed();
    conflict.nonce = 5;
    conflict.raw_tx = "different-bytes";
    assert!(ApiWithdrawRepo::persist_signed_self_transfer(&pool, &conflict).await.is_err());
    let original = row(&pool).await;
    assert_eq!(original.nonce, 4);
    assert_eq!(original.raw_tx.as_deref(), Some("signed-bytes-4"));
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 4);
    ApiNonceRepo::set_nonce_floor(&pool, "sender", "eth", 9).await.unwrap();
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 9);
}

#[tokio::test]
async fn self_transfer_nonce_write_failure_rolls_back_signed_record() {
    let pool = setup_api_transaction_pool("self_transfer_rollback").await;
    ApiNonceRepo::set_nonce_floor(&pool, "sender", "eth", 2).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_nonce BEFORE UPDATE ON api_nonce BEGIN SELECT RAISE(ABORT, 'injected failure'); END")
        .execute(pool.write_ref()).await.unwrap();
    assert!(ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM api_withdraws")
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 2);
    sqlx::query("DROP TRIGGER reject_nonce").execute(pool.write_ref()).await.unwrap();
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    assert_eq!(row(&pool).await.nonce, 4);
}

#[tokio::test]
async fn self_transfer_isolates_sender_chain_trade_type_and_terminal_rows() {
    let pool = setup_api_transaction_pool("self_transfer_isolation").await;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    assert!(
        ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, "other", "eth")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, "sender", "bnb")
            .await
            .unwrap()
            .is_empty()
    );
    for mutation in ["trade_type = 1", "trade_type = 4, finished_at = '2026-09-08T00:00:00Z'"] {
        sqlx::query(&format!("UPDATE api_withdraws SET {mutation}"))
            .execute(pool.write_ref())
            .await
            .unwrap();
        assert!(
            !ApiWithdrawRepo::mark_self_transfer_accepted(&pool, "ordinary-4", "hash-4")
                .await
                .unwrap()
        );
        assert!(
            ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, "sender", "eth")
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn self_transfer_bnb_zero_nonce_and_invalid_input() {
    let pool = setup_api_transaction_pool("self_transfer_bnb").await;
    let mut input = signed();
    input.chain_code = "tron";
    assert!(ApiWithdrawRepo::persist_signed_self_transfer(&pool, &input).await.is_err());
    input.chain_code = "bnb";
    input.nonce = -1;
    assert!(ApiWithdrawRepo::persist_signed_self_transfer(&pool, &input).await.is_err());
    input.nonce = 0;
    input.raw_tx = "";
    assert!(ApiWithdrawRepo::persist_signed_self_transfer(&pool, &input).await.is_err());
    assert!(ApiNonceRepo::get_api_nonce_optional(&pool, "sender", "bnb").await.unwrap().is_none());
    input.raw_tx = "bnb-signed-zero";
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &input).await.unwrap();
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "bnb").await.unwrap(), 0);
    assert_eq!(
        ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, "sender", "bnb")
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn self_transfer_concurrent_identical_retries_write_one_record() {
    let pool = setup_api_transaction_pool("self_transfer_retry").await;
    let input = signed();
    let (a, b) = tokio::join!(
        ApiWithdrawRepo::persist_signed_self_transfer(&pool, &input),
        ApiWithdrawRepo::persist_signed_self_transfer(&pool, &input)
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(
        ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, "sender", "eth")
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 4);
}

fn receipt(success: bool) -> SelfTransferReceipt<'static> {
    SelfTransferReceipt {
        trade_no: "ordinary-4",
        chain_code: "eth",
        tx_hash: "hash-4",
        success,
        transaction_time: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        block_height: "1234",
        transaction_fee: "0.00002",
        resource_consume: "21000",
    }
}

#[tokio::test]
async fn self_transfer_receipt_finishes_success_and_failure_without_nonce_reuse() {
    for success in [true, false] {
        let pool = setup_api_transaction_pool("self_transfer_receipt").await;
        ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();

        assert!(
            ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(success)).await.unwrap()
        );

        let confirmed = row(&pool).await;
        assert_eq!(
            confirmed.status,
            if success { ApiWithdrawStatus::Success } else { ApiWithdrawStatus::Failure }
        );
        assert_eq!(confirmed.transaction_time, Some(receipt(success).transaction_time));
        assert_eq!(confirmed.block_height.as_deref(), Some("1234"));
        assert_eq!(confirmed.transaction_fee, "0.00002");
        assert_eq!(confirmed.resource_consume, "21000");
        assert_eq!(confirmed.chain_success_at.is_some(), success);
        assert_eq!(confirmed.chain_failed_at.is_some(), !success);
        assert!(confirmed.finished_at.is_some());
        assert!(confirmed.broadcast_uncertain_since_at.is_none());
        assert!(confirmed.tx_ack_sent_at.is_none());
        assert!(confirmed.tx_res_ack_sent_at.is_none());
        assert!(confirmed.tx_exec_receipt_uploaded_at.is_none());
        assert_eq!(confirmed.raw_tx.as_deref(), Some("signed-bytes-4"));
        assert_eq!(confirmed.tx_hash.as_deref(), Some("hash-4"));
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 4);
        assert!(
            ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, "sender", "eth")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            !ApiWithdrawRepo::mark_self_transfer_accepted(&pool, "ordinary-4", "hash-4")
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn self_transfer_receipt_rejects_wrong_identity_and_backend_withdrawal() {
    let pool = setup_api_transaction_pool("self_transfer_receipt_identity").await;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    let mut wrong = receipt(true);
    wrong.tx_hash = "other-hash";
    assert!(!ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &wrong).await.unwrap());
    wrong = receipt(true);
    wrong.chain_code = "bnb";
    assert!(!ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &wrong).await.unwrap());
    wrong = receipt(true);
    wrong.trade_no = "other-order";
    assert!(!ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &wrong).await.unwrap());
    sqlx::query("UPDATE api_withdraws SET trade_type = 1").execute(pool.write_ref()).await.unwrap();
    assert!(!ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(true)).await.unwrap());
    let unchanged = row(&pool).await;
    assert!(unchanged.transaction_time.is_none());
    assert!(unchanged.finished_at.is_none());
    assert!(
        !ApiWithdrawRepo::self_transfer_receipt_refresh_pending(&pool, "ordinary-4").await.unwrap()
    );
    assert!(unchanged.broadcast_uncertain_since_at.is_some());
    assert_eq!(unchanged.transaction_fee, "0.001");
}

#[tokio::test]
async fn self_transfer_receipt_concurrent_retry_and_opposite_result_cannot_overwrite() {
    let pool = setup_api_transaction_pool("self_transfer_receipt_retry").await;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    let fact = receipt(true);
    let (a, b) = tokio::join!(
        ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &fact),
        ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &fact),
    );
    assert_ne!(a.unwrap(), b.unwrap(), "exactly one caller records the receipt");
    let original = row(&pool).await;
    assert!(!ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(false)).await.unwrap());
    let final_row = row(&pool).await;
    assert_eq!(final_row.status, ApiWithdrawStatus::Success);
    assert_eq!(final_row.finished_at, original.finished_at);
    assert_eq!(final_row.chain_success_at, original.chain_success_at);
    assert!(final_row.chain_failed_at.is_none());
}

#[tokio::test]
async fn self_transfer_receipt_terminal_write_failure_rolls_back_then_recovers() {
    let pool = setup_api_transaction_pool("self_transfer_receipt_rollback").await;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_finish BEFORE UPDATE OF finished_at ON api_withdraws BEGIN SELECT RAISE(ABORT, 'injected finish failure'); END")
        .execute(pool.write_ref()).await.unwrap();

    assert!(ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(true)).await.is_err());

    let unchanged = row(&pool).await;
    assert_eq!(unchanged.status, ApiWithdrawStatus::Init);
    assert!(unchanged.transaction_time.is_none());
    assert!(unchanged.chain_success_at.is_none());
    assert!(unchanged.finished_at.is_none());
    assert!(unchanged.broadcast_uncertain_since_at.is_some());
    assert_eq!(unchanged.transaction_fee, "0.001");
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 4);
    sqlx::query("DROP TRIGGER reject_finish").execute(pool.write_ref()).await.unwrap();
    assert!(ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(true)).await.unwrap());
    assert!(row(&pool).await.finished_at.is_some());
}

#[tokio::test]
async fn self_transfer_scan_cursor_survives_earlier_completion() {
    let pool = setup_api_transaction_pool("self_transfer_scan").await;
    let first = signed();
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &first).await.unwrap();
    let mut second = signed();
    second.trade_no = "ordinary-5";
    second.nonce = 5;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &second).await.unwrap();
    let page = ApiWithdrawRepo::scan_pending_self_transfers(&pool, 0, 1).await.unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].trade_no, "ordinary-4");
    ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(true)).await.unwrap();
    let next = ApiWithdrawRepo::scan_pending_self_transfers(&pool, page[0].id, 1).await.unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].trade_no, "ordinary-5");
    assert!(
        ApiWithdrawRepo::scan_pending_self_transfers(&pool, next[0].id, 1)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(ApiWithdrawRepo::scan_pending_self_transfers(&pool, 0, 0).await.unwrap().is_empty());
    sqlx::query("UPDATE api_withdraws SET trade_type = 1 WHERE trade_no = 'ordinary-5'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert!(ApiWithdrawRepo::scan_pending_self_transfers(&pool, 0, 10).await.unwrap().is_empty());
}

async fn age_rebroadcast(pool: &ApiTransactionDbPool) {
    sqlx::query("UPDATE api_withdraws SET broadcast_uncertain_last_checked_at = strftime('%Y-%m-%dT%H:%M:%SZ','now','-61 seconds')")
        .execute(pool.write_ref()).await.unwrap();
}

async fn claim_rebroadcast(pool: &ApiTransactionDbPool) -> bool {
    ApiWithdrawRepo::claim_self_transfer_rebroadcast(pool, "ordinary-4", "hash-4", "signed-bytes-4")
        .await
        .unwrap()
}

#[tokio::test]
async fn self_transfer_rebroadcast_claim_serializes_cools_down_and_caps_retries() {
    let pool = setup_api_transaction_pool("self_transfer_rebroadcast").await;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    assert!(!claim_rebroadcast(&pool).await);
    age_rebroadcast(&pool).await;
    let (a, b) = tokio::join!(claim_rebroadcast(&pool), claim_rebroadcast(&pool));
    assert_ne!(a, b);
    assert!(!claim_rebroadcast(&pool).await);
    assert_eq!(row(&pool).await.broadcast_uncertain_rebroadcast_count, 1);
    for count in [2, 3] {
        age_rebroadcast(&pool).await;
        assert!(claim_rebroadcast(&pool).await);
        assert_eq!(row(&pool).await.broadcast_uncertain_rebroadcast_count, count);
    }
    age_rebroadcast(&pool).await;
    assert!(!claim_rebroadcast(&pool).await);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 4);
    assert_eq!(row(&pool).await.raw_tx.as_deref(), Some("signed-bytes-4"));
}

#[tokio::test]
async fn self_transfer_rebroadcast_claim_guards_identity_and_terminal_state() {
    let pool = setup_api_transaction_pool("self_transfer_rebroadcast_identity").await;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    age_rebroadcast(&pool).await;
    assert!(
        !ApiWithdrawRepo::claim_self_transfer_rebroadcast(
            &pool,
            "ordinary-4",
            "wrong",
            "signed-bytes-4"
        )
        .await
        .unwrap()
    );
    assert!(
        !ApiWithdrawRepo::claim_self_transfer_rebroadcast(&pool, "ordinary-4", "hash-4", "changed")
            .await
            .unwrap()
    );
    assert_eq!(row(&pool).await.broadcast_uncertain_rebroadcast_count, 0);
    ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(true)).await.unwrap();
    assert!(!claim_rebroadcast(&pool).await);
}

#[tokio::test]
async fn self_transfer_rebroadcast_acceptance_keeps_budget_across_reopen() {
    let dir = make_temp_dir("self_transfer_rebroadcast_reopen");
    let pool = SqliteContext::new(&dir, Some("api_transaction.db"))
        .await
        .unwrap()
        .into_transaction_db_pool()
        .unwrap();
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    ApiWithdrawRepo::mark_self_transfer_accepted(&pool, "ordinary-4", "hash-4").await.unwrap();
    age_rebroadcast(&pool).await;
    assert!(claim_rebroadcast(&pool).await);
    pool.read_ref().close().await;
    pool.write_ref().close().await;
    let pool = SqliteContext::new(&dir, Some("api_transaction.db"))
        .await
        .unwrap()
        .into_transaction_db_pool()
        .unwrap();
    assert!(!claim_rebroadcast(&pool).await);
    assert_eq!(row(&pool).await.broadcast_uncertain_rebroadcast_count, 1);
    assert!(
        ApiWithdrawRepo::mark_self_transfer_accepted(&pool, "ordinary-4", "hash-4").await.unwrap()
    );
    assert!(row(&pool).await.broadcast_uncertain_since_at.is_none());
    assert_eq!(row(&pool).await.broadcast_uncertain_rebroadcast_count, 1);
}

async fn legacy_account_change(pool: &ApiTransactionDbPool) -> u64 {
    ApiWithdrawRepo::update_api_withdraw_tx_status(
        pool,
        "ordinary-4",
        0,
        "hash-4",
        "wrong-resource",
        "999",
        Some(chrono::Utc::now()),
        "999",
        ApiWithdrawStatus::ConfirmSuccessReport,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn self_transfer_account_change_cannot_override_receipt_or_finish_pending() {
    for chain in ["eth", "bnb"] {
        let pool = setup_api_transaction_pool("self_transfer_acct_guard").await;
        let mut signed = signed();
        signed.chain_code = chain;
        ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed).await.unwrap();
        assert_eq!(legacy_account_change(&pool).await, 0);
        let pending = row(&pool).await;
        assert!(pending.transaction_time.is_none());
        assert_eq!(pending.nonce, 4);
        assert_eq!(pending.transaction_fee, "0.001");
        let mut receipt = receipt(false);
        receipt.chain_code = chain;
        let (confirmed, ignored) = tokio::join!(
            ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt),
            legacy_account_change(&pool),
        );
        assert!(confirmed.unwrap());
        assert_eq!(ignored, 0);
        assert_eq!(legacy_account_change(&pool).await, 0);
        let failed = row(&pool).await;
        assert_eq!(failed.status, ApiWithdrawStatus::Failure);
        assert!(failed.chain_failed_at.is_some());
        assert!(failed.chain_success_at.is_none());
        assert_eq!(failed.nonce, 4);
        assert_eq!(failed.tx_hash.as_deref(), Some("hash-4"));
        assert_eq!(failed.raw_tx.as_deref(), Some("signed-bytes-4"));
        assert_eq!(failed.transaction_fee, receipt.transaction_fee);
        assert_eq!(failed.block_height.as_deref(), Some(receipt.block_height));
    }
}

#[tokio::test]
async fn self_transfer_unsigned_legacy_account_change_remains_supported() {
    let pool = setup_api_transaction_pool("self_transfer_acct_legacy").await;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    sqlx::query("UPDATE api_withdraws SET raw_tx = NULL WHERE trade_no = 'ordinary-4'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    assert_eq!(legacy_account_change(&pool).await, 1);
    assert_eq!(row(&pool).await.status, ApiWithdrawStatus::ConfirmSuccessReport);
}

#[tokio::test]
async fn self_transfer_polygon_zero_nonce_persists_before_broadcast() {
    let pool = setup_api_transaction_pool("self_transfer_polygon").await;
    let mut input = signed();
    input.chain_code = "polygon";
    input.nonce = 0;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &input).await.unwrap();
    let saved = row(&pool).await;
    assert_eq!(saved.chain_code, "polygon");
    assert_eq!(saved.nonce, 0);
    assert!(saved.broadcast_uncertain_since_at.is_some());
    assert!(saved.last_broadcast_at.is_none());
    assert_eq!(saved.raw_tx.as_deref(), Some(input.raw_tx));
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "polygon").await.unwrap(), 0);
}

fn supported_evm_chains() -> Vec<String> {
    wallet_types::chain::chain::ChainCodes::try_from(wallet_types::constant::chain_type::ETH_TYPE)
        .unwrap()
        .0
        .into_iter()
        .map(|chain| chain.to_string())
        .collect()
}

#[tokio::test]
async fn self_transfer_all_evm_chains_preserve_signed_lifecycle_and_nonce_zero() {
    for chain in supported_evm_chains() {
        let chain = chain.as_str();
        let pool = setup_api_transaction_pool("self_transfer_evm_lifecycle").await;
        let mut input = signed();
        input.chain_code = chain;
        input.nonce = 0;
        ApiWithdrawRepo::persist_signed_self_transfer(&pool, &input).await.unwrap();
        ApiWithdrawRepo::persist_signed_self_transfer(&pool, &input).await.unwrap();
        assert_eq!(
            ApiWithdrawRepo::scan_pending_self_transfers(&pool, 0, 10).await.unwrap().len(),
            1,
            "{chain}"
        );
        assert_eq!(
            ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, "sender", chain)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(ApiWithdrawRepo::scan_can_broadcast(&pool, 10).await.unwrap().is_empty());
        assert!(ApiWithdrawRepo::scan_need_recover(&pool, 10).await.unwrap().is_empty());
        let mut conflicting = signed();
        conflicting.chain_code = chain;
        conflicting.nonce = 0;
        conflicting.raw_tx = "different signed bytes";
        assert!(ApiWithdrawRepo::persist_signed_self_transfer(&pool, &conflicting).await.is_err());
        assert_eq!(legacy_account_change(&pool).await, 0);
        assert!(!claim_rebroadcast(&pool).await);
        for count in 1..=3 {
            age_rebroadcast(&pool).await;
            assert!(claim_rebroadcast(&pool).await, "{chain}");
            assert_eq!(row(&pool).await.broadcast_uncertain_rebroadcast_count, count);
        }
        age_rebroadcast(&pool).await;
        assert!(!claim_rebroadcast(&pool).await);
        assert!(
            ApiWithdrawRepo::mark_self_transfer_accepted(&pool, input.trade_no, input.tx_hash)
                .await
                .unwrap()
        );
        assert!(row(&pool).await.transaction_time.is_none());
        let mut receipt = receipt(false);
        receipt.chain_code = chain;
        assert!(ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt).await.unwrap());
        assert!(!ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt).await.unwrap());
        assert_eq!(legacy_account_change(&pool).await, 0);
        let final_row = row(&pool).await;
        assert_eq!(final_row.status, ApiWithdrawStatus::Failure);
        assert!(final_row.finished_at.is_some());
        assert!(final_row.chain_failed_at.is_some());
        assert_eq!(final_row.raw_tx.as_deref(), Some(input.raw_tx));
        assert_eq!(final_row.nonce, 0);
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", chain).await.unwrap(), 0);
        assert!(
            ApiWithdrawRepo::scan_pending_self_transfers(&pool, 0, 10).await.unwrap().is_empty()
        );
    }
}

#[tokio::test]
async fn self_transfer_all_non_evm_chains_rejected_without_signed_facts() {
    for chain in ["tron", "btc", "sol", "ltc", "dog", "sui", "ton", "ftm", "unknown"] {
        let pool = setup_api_transaction_pool("self_transfer_not_evm").await;
        let mut input = signed();
        input.chain_code = chain;
        assert!(ApiWithdrawRepo::persist_signed_self_transfer(&pool, &input).await.is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_withdraws")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
        assert_eq!(count, 0);
        let mut receipt = receipt(true);
        receipt.chain_code = chain;
        assert!(ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt).await.is_err());
    }
}

#[tokio::test]
async fn self_transfer_candidate_commits_nonce_with_raw() {
    let pool = setup_api_transaction_pool("self_transfer_candidate").await;
    ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &signed()).await.unwrap();
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 4);
    assert_eq!(row(&pool).await.raw_tx.as_deref(), Some("signed-bytes-4"));
}

#[tokio::test]
async fn self_transfer_stale_candidate_leaves_no_signed_row() {
    let pool = setup_api_transaction_pool("self_transfer_candidate").await;
    ApiNonceRepo::set_nonce_floor(&pool, "sender", "eth", 4).await.unwrap();
    assert!(ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &signed()).await.is_err());
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 4);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM api_withdraws")
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn self_transfer_failed_insert_rolls_back_candidate_counter() {
    let pool = setup_api_transaction_pool("self_transfer_candidate").await;
    sqlx::query("CREATE TRIGGER reject_signed BEFORE INSERT ON api_withdraws BEGIN SELECT RAISE(ABORT, 'injected failure'); END")
        .execute(pool.write_ref()).await.unwrap();
    assert!(ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &signed()).await.is_err());
    assert_eq!(ApiNonceRepo::get_api_nonce_optional(&pool, "sender", "eth").await.unwrap(), None);
}

#[tokio::test]
async fn self_transfer_competing_candidates_only_one_commits() {
    let pool = setup_api_transaction_pool("self_transfer_competing").await;
    let first = signed();
    let mut second = signed();
    second.trade_no = "competing";
    second.tx_hash = "other-hash";
    second.raw_tx = "other-bytes";
    let (a, b) = tokio::join!(
        ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &first),
        ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &second),
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM api_withdraws")
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 4);
}

#[tokio::test]
async fn self_transfer_candidate_all_evm_zero_nonce_commits_signed_facts() {
    for chain in supported_evm_chains() {
        let pool = setup_api_transaction_pool("self_transfer_candidate_evm_zero").await;
        let mut input = signed();
        input.chain_code = &chain;
        input.nonce = 0;
        ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &input).await.unwrap();
        let saved = row(&pool).await;
        assert_eq!(saved.chain_code, chain);
        assert_eq!(saved.nonce, 0);
        assert_eq!(saved.raw_tx.as_deref(), Some(input.raw_tx));
        assert!(saved.broadcast_uncertain_since_at.is_some());
        assert!(saved.last_broadcast_at.is_none());
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", &chain).await.unwrap(), 0);
    }
}

#[tokio::test]
async fn self_transfer_case_alias_cannot_commit_a_second_nonce_owner() {
    let pool = setup_api_transaction_pool("ordinary_nonce_alias").await;
    let first = signed();
    ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &first).await.unwrap();
    let mut alias = signed();
    alias.trade_no = "ordinary-alias";
    alias.from_addr = "SENDER";
    alias.tx_hash = "alias-hash";
    alias.raw_tx = "alias-raw";
    assert!(ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &alias).await.is_err());
    let owners: i64 = sqlx::query_scalar("SELECT count(*) FROM api_withdraws WHERE lower(from_addr)='sender' AND chain_code='eth' AND nonce=4")
        .fetch_one(pool.read_ref()).await.unwrap();
    assert_eq!(owners, 1);
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "SENDER", "eth").await.unwrap(), 4);
}

#[tokio::test]
async fn self_transfer_case_alias_cannot_ignore_signed_claim_without_counter() {
    let pool = setup_api_transaction_pool("ordinary_alias_claim").await;
    ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &signed()).await.unwrap();
    sqlx::query("DELETE FROM api_nonce").execute(pool.write_ref()).await.unwrap();
    let mut alias = signed();
    alias.from_addr = "SENDER";
    alias.trade_no = "other";
    alias.tx_hash = "other-hash";
    alias.raw_tx = "other-raw";
    assert!(ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &alias).await.is_err());
    assert_eq!(ApiNonceRepo::get_api_nonce_optional(&pool, "sender", "eth").await.unwrap(), None);
    assert_eq!(row(&pool).await.raw_tx.as_deref(), Some("signed-bytes-4"));
}
#[tokio::test]
async fn self_transfer_case_alias_concurrent_commit_has_one_owner() {
    let pool = setup_api_transaction_pool("ordinary_alias_race").await;
    let first = signed();
    let mut alias = signed();
    alias.from_addr = "SENDER";
    alias.trade_no = "alias";
    alias.tx_hash = "alias-hash";
    alias.raw_tx = "alias-raw";
    let (a, b) = tokio::join!(
        ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &first),
        ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &alias)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let owners: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM api_withdraws WHERE lower(from_addr)='sender' AND nonce=4",
    )
    .fetch_one(pool.read_ref())
    .await
    .unwrap();
    assert_eq!(owners, 1);
}

#[tokio::test]
async fn self_transfer_case_alias_respects_collect_signed_owner() {
    use crate::{
        entities::api_collect::ApiCollectStatus,
        repositories::api_wallet::collect::{ApiCollectRepo, SignedCollectCandidate},
    };
    let pool = setup_api_transaction_pool("ordinary_collect_alias").await;
    ApiCollectRepo::upsert_api_collect(
        &pool,
        "wallet",
        "collect",
        "SENDER",
        "receiver",
        "1",
        "v",
        "eth",
        None,
        "ETH",
        "collect",
        2,
        ApiCollectStatus::Init,
        1,
    )
    .await
    .unwrap();
    ApiCollectRepo::set_order_ack_sent(&pool, "collect").await.unwrap();
    ApiCollectRepo::update_building_at(&pool, "collect").await.unwrap();
    ApiCollectRepo::reserve_signed_collect(
        &pool,
        &SignedCollectCandidate {
            trade_no: "collect",
            from_addr: "SENDER",
            chain_code: "eth",
            tx_hash: "collect-hash",
            raw_tx: "collect-raw",
            transaction_fee: "0.001",
            nonce: 4,
            chain_next: 4,
        },
    )
    .await
    .unwrap();
    assert!(ApiWithdrawRepo::reserve_signed_self_transfer(&pool, &signed()).await.is_err());
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 4);
}

#[tokio::test]
async fn self_transfer_receipt_records_pending_refresh_atomically() {
    for success in [true, false] {
        let pool = setup_api_transaction_pool("self_transfer_refresh_debt").await;
        ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
        assert!(
            ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(success)).await.unwrap()
        );
        let pending: i64 = sqlx::query_scalar(
            "SELECT receipt_refresh_pending FROM api_withdraws WHERE trade_no='ordinary-4'",
        )
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
        assert_eq!(pending, 1);
        assert!(row(&pool).await.finished_at.is_some());
        assert!(
            !ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(success))
                .await
                .unwrap()
        );
        let pending: i64 = sqlx::query_scalar(
            "SELECT receipt_refresh_pending FROM api_withdraws WHERE trade_no='ordinary-4'",
        )
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
        assert_eq!(pending, 1);
    }
}

#[tokio::test]
async fn receipt_refresh_ack_requires_matching_terminal_identity() {
    let pool = setup_api_transaction_pool("receipt_refresh_ack").await;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    assert!(
        !ApiWithdrawRepo::acknowledge_self_transfer_receipt_refresh(&pool, "ordinary-4", "hash-4")
            .await
            .unwrap()
    );
    ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(true)).await.unwrap();
    assert_eq!(
        ApiWithdrawRepo::scan_self_transfer_receipt_refresh(&pool, 0, 32).await.unwrap().len(),
        1
    );
    assert!(
        !ApiWithdrawRepo::acknowledge_self_transfer_receipt_refresh(&pool, "ordinary-4", "other")
            .await
            .unwrap()
    );
    assert!(
        ApiWithdrawRepo::acknowledge_self_transfer_receipt_refresh(&pool, "ordinary-4", "hash-4")
            .await
            .unwrap()
    );
    assert!(
        !ApiWithdrawRepo::acknowledge_self_transfer_receipt_refresh(&pool, "ordinary-4", "hash-4")
            .await
            .unwrap()
    );
    assert!(
        ApiWithdrawRepo::scan_self_transfer_receipt_refresh(&pool, 0, 32).await.unwrap().is_empty()
    );
    assert!(!ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(true)).await.unwrap());
    assert!(
        ApiWithdrawRepo::scan_self_transfer_receipt_refresh(&pool, 0, 32).await.unwrap().is_empty()
    );
}

#[tokio::test]
async fn signed_self_transfer_mqtt_identity_is_chain_scoped_and_case_normalized() {
    let pool = setup_api_transaction_pool("receipt_mqtt_identity").await;
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    assert!(
        ApiWithdrawRepo::should_defer_self_transfer_asset_refresh(&pool, "SENDER", "HASH-4", "eth")
            .await
            .unwrap()
    );
    ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(true)).await.unwrap();
    assert!(
        ApiWithdrawRepo::should_defer_self_transfer_asset_refresh(&pool, "SENDER", "HASH-4", "eth")
            .await
            .unwrap()
    );
    ApiWithdrawRepo::acknowledge_self_transfer_receipt_refresh(&pool, "ordinary-4", "hash-4")
        .await
        .unwrap();
    assert!(
        !ApiWithdrawRepo::should_defer_self_transfer_asset_refresh(
            &pool, "SENDER", "HASH-4", "eth"
        )
        .await
        .unwrap()
    );
    for (from, hash, chain) in [
        ("sender", "hash-4", "polygon"),
        ("other", "hash-4", "eth"),
        ("sender", "unknown", "eth"),
        ("sender", "hash-4", "tron"),
    ] {
        assert!(
            !ApiWithdrawRepo::should_defer_self_transfer_asset_refresh(&pool, from, hash, chain)
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn receipt_refresh_debt_survives_database_reopen() {
    let dir = make_temp_dir("receipt_refresh_reopen");
    let pool = SqliteContext::new(&dir, Some("api_transaction.db"))
        .await
        .unwrap()
        .into_transaction_db_pool()
        .unwrap();
    ApiWithdrawRepo::persist_signed_self_transfer(&pool, &signed()).await.unwrap();
    ApiWithdrawRepo::confirm_self_transfer_receipt(&pool, &receipt(false)).await.unwrap();
    pool.read_ref().close().await;
    pool.write_ref().close().await;
    let pool = SqliteContext::new(&dir, Some("api_transaction.db"))
        .await
        .unwrap()
        .into_transaction_db_pool()
        .unwrap();
    assert_eq!(
        ApiWithdrawRepo::scan_self_transfer_receipt_refresh(&pool, 0, 32).await.unwrap().len(),
        1
    );
    assert!(row(&pool).await.chain_failed_at.is_some());
    assert!(row(&pool).await.finished_at.is_some());
}

#[tokio::test]
async fn receipt_refresh_migration_preserves_historical_rows_without_republishing() {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql("CREATE TABLE api_withdraws (id INTEGER PRIMARY KEY, trade_no TEXT, chain_code TEXT, tx_hash TEXT, from_addr TEXT, trade_type INTEGER, raw_tx TEXT); INSERT INTO api_withdraws(id,trade_no) VALUES (1, 'historical');")
        .execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!("../../../../schema/api_transaction/migrations/20261030000000_self_transfer_receipt_refresh.sql"))
        .execute(&pool).await.unwrap();
    let result: (String, i64) =
        sqlx::query_as("SELECT trade_no, receipt_refresh_pending FROM api_withdraws WHERE id=1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(result, ("historical".into(), 0));
}

#[tokio::test]
async fn receipt_refresh_identity_lookup_uses_hash_expression_index() {
    let pool = setup_api_transaction_pool("receipt_refresh_lookup_plan").await;
    let plan: Vec<(i64, i64, i64, String)> =
        sqlx::query_as(&format!("EXPLAIN QUERY PLAN {SELF_TRANSFER_ASSET_REFRESH_MATCH_SQL}"))
            .bind("sender")
            .bind("hash-4")
            .bind("eth")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert!(plan.iter().any(|row| row.3.contains("idx_signed_self_transfer_identity")), "{plan:?}");
}
