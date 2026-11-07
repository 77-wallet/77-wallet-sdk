use super::{ApiFeeRepo, SignedFeeCandidate};
use crate::{
    ApiTransactionDbPool,
    entities::api_fee::{ApiFeeStatus, ErrCode},
    repositories::{api_wallet::nonce::ApiNonceRepo, test_helper::setup_api_transaction_pool},
};

async fn setup(name: &str) -> ApiTransactionDbPool {
    let pool = setup_api_transaction_pool(name).await;
    ApiFeeRepo::upsert_api_fee(
        &pool, "uid", "fee", "payer", "receiver", "1", "v", "arbitrum", None, "ETH", "fee", 3,
    )
    .await
    .unwrap();
    ApiFeeRepo::set_tx_ack_sent(&pool, "fee").await.unwrap();
    ApiFeeRepo::update_building_at(&pool, "fee").await.unwrap();
    pool
}

fn candidate() -> SignedFeeCandidate<'static> {
    SignedFeeCandidate {
        trade_no: "fee",
        from_addr: "payer",
        chain_code: "arbitrum",
        tx_hash: "hash",
        raw_tx: "raw",
        transaction_fee: "0.1",
        nonce: 6,
        chain_next: 6,
    }
}

#[tokio::test]
async fn fee_build_failure_is_durable_idempotent_and_reportable() {
    let pool = setup("fee_build_failure_durable").await;
    assert_eq!(
        ApiFeeRepo::fail_unbuilt_fee(&pool, "fee", ErrCode::BalanceInsufficient, "payer empty")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        ApiFeeRepo::fail_unbuilt_fee(&pool, "fee", ErrCode::FeeInsufficient, "late error")
            .await
            .unwrap(),
        0
    );
    ApiFeeRepo::upsert_api_fee(
        &pool, "uid", "fee", "payer", "receiver", "1", "v", "arbitrum", None, "ETH", "fee", 3,
    )
    .await
    .unwrap();
    let row = ApiFeeRepo::get_api_fee_by_trade_no(&pool, "fee").await.unwrap();
    assert_eq!(row.err_code, Some(ErrCode::BalanceInsufficient));
    assert_eq!(row.err_msg.as_deref(), Some("payer empty"));
    assert_eq!(row.status, ApiFeeStatus::SendingTxFailed);
    assert!(row.building_at.is_none());
    assert!(row.finished_at.is_none());
    assert!(ApiFeeRepo::scan_can_build(&pool, 10).await.unwrap().is_empty());
    assert_eq!(ApiFeeRepo::scan_need_tx_exec_receipt_upload(&pool, 10).await.unwrap().len(), 1);
    assert!(
        ApiNonceRepo::get_api_nonce_optional(&pool, "payer", "arbitrum").await.unwrap().is_none()
    );
    assert!(ApiFeeRepo::reserve_signed_fee(&pool, &candidate()).await.is_err());
    assert_eq!(
        ApiFeeRepo::update_after_build(&pool, "fee", "hash", "raw", "0.1", 6).await.unwrap(),
        0
    );
    assert!(ApiFeeRepo::get_api_fee_by_trade_no(&pool, "fee").await.unwrap().raw_tx.is_none());
}

#[tokio::test]
async fn fee_build_failure_preserves_a_concurrently_committed_candidate() {
    let pool = setup("fee_build_failure_candidate_wins").await;
    assert_eq!(ApiFeeRepo::reserve_signed_fee(&pool, &candidate()).await.unwrap(), 1);
    assert_eq!(
        ApiFeeRepo::fail_unbuilt_fee(&pool, "fee", ErrCode::BalanceInsufficient, "stale shortage")
            .await
            .unwrap(),
        0
    );
    let row = ApiFeeRepo::get_api_fee_by_trade_no(&pool, "fee").await.unwrap();
    assert!(row.err_code.is_none());
    assert_eq!(row.raw_tx.as_deref(), Some("raw"));
    assert_eq!(row.tx_hash.as_deref(), Some("hash"));
    assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "payer", "arbitrum").await.unwrap(), 6);
}

#[tokio::test]
async fn fee_build_failure_preserves_existing_execution_and_terminal_facts() {
    for field in [
        "last_broadcast_at",
        "transaction_time",
        "broadcast_uncertain_since_at",
        "tx_res_received_at",
        "tx_res_ack_sent_at",
        "tx_exec_receipt_uploaded_at",
        "finished_at",
    ] {
        let pool = setup(&format!("fee_build_failure_preserve_{field}")).await;
        sqlx::query(&format!(
            "UPDATE api_fee SET {field} = CURRENT_TIMESTAMP WHERE trade_no = 'fee'"
        ))
        .execute(pool.write_ref())
        .await
        .unwrap();
        assert_eq!(
            ApiFeeRepo::fail_unbuilt_fee(
                &pool,
                "fee",
                ErrCode::BalanceInsufficient,
                "stale shortage"
            )
            .await
            .unwrap(),
            0,
            "must preserve {field}"
        );
        assert!(
            ApiFeeRepo::get_api_fee_by_trade_no(&pool, "fee").await.unwrap().err_code.is_none()
        );
    }
}
