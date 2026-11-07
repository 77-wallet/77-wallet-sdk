use super::{collect::ApiCollectDomain, fee::ApiFeeDomain, withdraw::ApiWithdrawDomain};
use sqlx;
use tempfile::TempDir;
use wallet_database::{
    SqliteContext,
    entities::{
        api_collect::{ApiCollectStatus, ErrCode as ApiCollectErrCode},
        api_fee::ApiFeeStatus,
        api_trade_type::ApiTradeType,
        api_withdraw::ApiWithdrawStatus,
        asset_token_key::AssetTokenKey,
    },
    repositories::api_wallet::{
        collect::ApiCollectRepo, fee::ApiFeeRepo, withdraw::ApiWithdrawRepo,
    },
};

struct TestFundsDb {
    _dir: TempDir,
    pool: wallet_database::ApiTransactionDbPool,
}

impl TestFundsDb {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx =
            SqliteContext::new(dir.path().to_string_lossy().as_ref(), Some("api_transaction.db"))
                .await
                .expect("init api_transaction.db");
        let pool = ctx.into_transaction_db_pool().expect("transaction pool");
        Self { _dir: dir, pool }
    }
}

#[tokio::test]
async fn collect_repeat_should_still_write_transaction_time() {
    let db = TestFundsDb::new().await;
    let trade_no = "T_collect_repeat_tx_time";

    ApiCollectRepo::upsert_api_collect(
        &db.pool,
        "uid",
        "name",
        "from",
        "to",
        "1",
        "validate",
        "tron",
        AssetTokenKey::Native,
        "USDT",
        trade_no,
        2,
        ApiCollectStatus::Success,
        0,
    )
    .await
    .expect("insert collect");

    let before =
        ApiCollectRepo::get_api_collect_by_trade_no(&db.pool, trade_no).await.expect("get collect");
    assert!(before.transaction_time.is_none(), "precondition: transaction_time should be NULL");

    ApiCollectDomain::confirm_tx_in_pool(&db.pool, trade_no, true, 0)
        .await
        .expect("confirm collect");

    let after = ApiCollectRepo::get_api_collect_by_trade_no(&db.pool, trade_no)
        .await
        .expect("get collect after");
    assert!(after.transaction_time.is_some(), "transaction_time should be written");
}

#[tokio::test]
async fn collect_not_found_is_ignored_idempotently() {
    let db = TestFundsDb::new().await;
    let res = ApiCollectDomain::confirm_tx_in_pool(&db.pool, "T_collect_missing", true, 0).await;
    assert!(res.is_ok(), "a cleaned collect row is an idempotent no-op");
}

#[tokio::test]
async fn collect_pre_broadcast_fee_fail_should_finish_without_tx_exec_receipt() {
    let db = TestFundsDb::new().await;
    let trade_no = "T_collect_pre_broadcast_fee_fail";

    ApiCollectRepo::upsert_api_collect(
        &db.pool,
        "uid",
        "name",
        "from",
        "to",
        "1",
        "validate",
        "bnb",
        AssetTokenKey::Contract("0xtoken".to_string()),
        "USDT",
        trade_no,
        2,
        ApiCollectStatus::Init,
        0,
    )
    .await
    .expect("insert collect");

    let before =
        ApiCollectRepo::get_api_collect_by_trade_no(&db.pool, trade_no).await.expect("get collect");
    assert!(before.transaction_time.is_none());
    assert!(before.last_broadcast_at.is_none());
    assert!(before.tx_hash.is_none());

    ApiCollectDomain::confirm_tx_in_pool(&db.pool, trade_no, false, 2)
        .await
        .expect("confirm collect pre-broadcast fee fail");

    let after = ApiCollectRepo::get_api_collect_by_trade_no(&db.pool, trade_no)
        .await
        .expect("get collect after");
    assert!(
        after.transaction_time.is_none(),
        "pre-broadcast fee fail must not write transaction_time"
    );
    assert_eq!(after.status, ApiCollectStatus::Failure);
    assert_eq!(after.err_code, Some(ApiCollectErrCode::FeeInsufficient));
    assert!(
        after.finished_at.is_some(),
        "pre-broadcast backend failure should finish locally without waiting for receipt upload"
    );
    assert!(
        after.tx_exec_receipt_uploaded_at.is_none(),
        "pre-broadcast backend failure must not upload a tx execution receipt"
    );
}

#[tokio::test]
async fn collect_fail_with_broadcast_fact_should_still_write_transaction_time() {
    let db = TestFundsDb::new().await;
    let trade_no = "T_collect_fail_after_broadcast";

    ApiCollectRepo::upsert_api_collect(
        &db.pool,
        "uid",
        "name",
        "from",
        "to",
        "1",
        "validate",
        "bnb",
        AssetTokenKey::Contract("0xtoken".to_string()),
        "USDT",
        trade_no,
        2,
        ApiCollectStatus::SendingTxReport,
        0,
    )
    .await
    .expect("insert collect");

    sqlx::query(
        r#"
        UPDATE api_collect
        SET
            tx_hash = $2,
            last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = $1
        "#,
    )
    .bind(trade_no)
    .bind("0xcollect_fail_after_broadcast_hash")
    .execute(db.pool.as_ref())
    .await
    .expect("set collect broadcast fact");

    ApiCollectDomain::confirm_tx_in_pool(&db.pool, trade_no, false, 0)
        .await
        .expect("confirm collect fail after broadcast");

    let after = ApiCollectRepo::get_api_collect_by_trade_no(&db.pool, trade_no)
        .await
        .expect("get collect after");
    assert!(
        after.transaction_time.is_some(),
        "failed on-chain result after broadcast should write transaction_time"
    );
}

#[tokio::test]
async fn fee_repeat_should_still_write_transaction_time() {
    let db = TestFundsDb::new().await;
    let trade_no = "T_fee_repeat_tx_time";

    ApiFeeRepo::upsert_api_fee(
        &db.pool,
        "uid",
        "name",
        "from",
        "to",
        "1",
        "validate",
        "tron",
        AssetTokenKey::Native,
        "USDT",
        trade_no,
        3,
    )
    .await
    .expect("insert fee");

    // 强制设置为 Success 且 transaction_time 为空（复现“repeat early return 不补事实”场景）
    sqlx::query(
        r#"
        UPDATE api_fee
        SET
            status = $2,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = $1
        "#,
    )
    .bind(trade_no)
    .bind(ApiFeeStatus::Success)
    .execute(db.pool.as_ref())
    .await
    .expect("set fee status");

    let before = ApiFeeRepo::get_api_fee_by_trade_no(&db.pool, trade_no).await.expect("get fee");
    assert!(before.transaction_time.is_none(), "precondition: transaction_time should be NULL");

    ApiFeeDomain::confirm_tx_in_pool(&db.pool, trade_no, true).await.expect("confirm fee");

    let after =
        ApiFeeRepo::get_api_fee_by_trade_no(&db.pool, trade_no).await.expect("get fee after");
    assert!(after.transaction_time.is_some(), "transaction_time should be written");
}

#[tokio::test]
async fn fee_not_found_is_ignored_idempotently() {
    let db = TestFundsDb::new().await;
    let res = ApiFeeDomain::confirm_tx_in_pool(&db.pool, "T_fee_missing", true).await;
    assert!(res.is_ok(), "a cleaned fee row is an idempotent no-op");
}

#[tokio::test]
async fn fee_pre_broadcast_fail_should_not_write_transaction_time() {
    let db = TestFundsDb::new().await;
    let trade_no = "T_fee_pre_broadcast_fail";

    ApiFeeRepo::upsert_api_fee(
        &db.pool,
        "uid",
        "name",
        "from",
        "to",
        "1",
        "validate",
        "bnb",
        AssetTokenKey::Native,
        "BNB",
        trade_no,
        3,
    )
    .await
    .expect("insert fee");

    sqlx::query(
        r#"
        UPDATE api_fee
        SET
            status = $2,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = $1
        "#,
    )
    .bind(trade_no)
    .bind(ApiFeeStatus::SendingTxReport)
    .execute(db.pool.as_ref())
    .await
    .expect("set fee status");

    ApiFeeDomain::confirm_tx_in_pool(&db.pool, trade_no, false)
        .await
        .expect("confirm fee pre-broadcast fail");

    let after =
        ApiFeeRepo::get_api_fee_by_trade_no(&db.pool, trade_no).await.expect("get fee after");
    assert!(
        after.transaction_time.is_none(),
        "pre-broadcast fee failure must not write transaction_time"
    );
    assert_eq!(after.status, ApiFeeStatus::Failure);
}

#[tokio::test]
async fn fee_fail_after_broadcast_should_write_transaction_time() {
    let db = TestFundsDb::new().await;
    let trade_no = "T_fee_fail_after_broadcast";

    ApiFeeRepo::upsert_api_fee(
        &db.pool,
        "uid",
        "name",
        "from",
        "to",
        "1",
        "validate",
        "bnb",
        AssetTokenKey::Native,
        "BNB",
        trade_no,
        3,
    )
    .await
    .expect("insert fee");

    sqlx::query(
        r#"
        UPDATE api_fee
        SET
            status = $2,
            tx_hash = $3,
            last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = $1
        "#,
    )
    .bind(trade_no)
    .bind(ApiFeeStatus::SendingTxReport)
    .bind("0xfee_fail_after_broadcast_hash")
    .execute(db.pool.as_ref())
    .await
    .expect("set fee broadcast fact");

    ApiFeeDomain::confirm_tx_in_pool(&db.pool, trade_no, false)
        .await
        .expect("confirm fee fail after broadcast");

    let after =
        ApiFeeRepo::get_api_fee_by_trade_no(&db.pool, trade_no).await.expect("get fee after");
    assert!(
        after.transaction_time.is_some(),
        "failed on-chain fee result after broadcast should write transaction_time"
    );
}

#[tokio::test]
async fn withdraw_repeat_should_still_write_transaction_time() {
    let db = TestFundsDb::new().await;
    let trade_no = "T_withdraw_repeat_tx_time";

    ApiWithdrawRepo::upsert_api_withdraw(
        &db.pool,
        "uid",
        "name",
        "from",
        "to",
        "1",
        "validate",
        "tron",
        AssetTokenKey::Native,
        "USDT",
        trade_no,
        None,
        None,
        None,
        ApiTradeType::Withdraw,
        0,
        None,
        ApiWithdrawStatus::Success,
        ApiWithdrawStatus::Success,
        "0",
        "0",
        None,
        None,
    )
    .await
    .expect("insert withdraw");

    let before =
        ApiWithdrawRepo::get_api_withdraw_by_trade_no(&db.pool, trade_no, ApiTradeType::Withdraw)
            .await
            .expect("get withdraw");
    assert!(before.transaction_time.is_none(), "precondition: transaction_time should be NULL");

    let _outcome = ApiWithdrawDomain::confirm_tx_in_pool(&db.pool, trade_no, true)
        .await
        .expect("confirm withdraw");

    let after =
        ApiWithdrawRepo::get_api_withdraw_by_trade_no(&db.pool, trade_no, ApiTradeType::Withdraw)
            .await
            .expect("get withdraw after");
    assert!(after.transaction_time.is_some(), "transaction_time should be written");
}

#[tokio::test]
async fn withdraw_not_found_should_error() {
    let db = TestFundsDb::new().await;
    let res = ApiWithdrawDomain::confirm_tx_in_pool(&db.pool, "T_withdraw_missing", true).await;
    assert!(res.is_err(), "missing withdraw order remains an error");
}

#[tokio::test]
async fn collect_and_fee_database_errors_are_not_idempotent_success() {
    // Arrange: real temporary SQLite with unavailable business tables, not missing rows.
    let db = TestFundsDb::new().await;
    sqlx::query("DROP TABLE api_collect").execute(db.pool.as_ref()).await.unwrap();
    sqlx::query("DROP TABLE api_fee").execute(db.pool.as_ref()).await.unwrap();
    // Act/Assert: storage failures must remain errors rather than being ACKed as duplicates.
    assert!(matches!(
        ApiCollectDomain::confirm_tx_in_pool(&db.pool, "missing", true, 0).await,
        Err(crate::error::service::ServiceError::Database(_))
    ));
    assert!(matches!(
        ApiFeeDomain::confirm_tx_in_pool(&db.pool, "missing", true).await,
        Err(crate::error::service::ServiceError::Database(_))
    ));
}
