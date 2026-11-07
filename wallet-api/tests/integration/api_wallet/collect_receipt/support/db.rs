use wallet_database::{
    ApiTransactionDbPool, entities::api_collect::ApiCollectStatus,
    repositories::api_wallet::collect::ApiCollectRepo,
};

pub(crate) async fn insert_collect(
    pool: &ApiTransactionDbPool,
    trade_no: &str,
    from_addr: &str,
    to_addr: &str,
    status: ApiCollectStatus,
) {
    ApiCollectRepo::upsert_api_collect(
        pool,
        "uid",
        "collect",
        from_addr,
        to_addr,
        "1.12",
        "digest",
        "sol",
        Some("token".to_string()),
        "USDC",
        trade_no,
        2,
        status,
        1,
    )
    .await
    .expect("insert collect");
}

pub(crate) async fn persist_stale_build_facts(pool: &ApiTransactionDbPool, trade_no: &str) {
    sqlx::query(
        r#"
        UPDATE api_collect
        SET raw_tx = $2,
            tx_hash = $3,
            status = $4,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = $1
        "#,
    )
    .bind(trade_no)
    .bind("{\"stale\":true}")
    .bind("old-hash")
    .bind(ApiCollectStatus::SendingTx)
    .execute(pool.as_ref())
    .await
    .expect("set stale build facts");
}

pub(crate) async fn persist_rebuilt_execution_facts(
    pool: &ApiTransactionDbPool,
    trade_no: &str,
    to_addr: &str,
    tx_hash: &str,
) {
    sqlx::query(
        r#"
        UPDATE api_collect
        SET to_addr = $2,
            tx_hash = $3,
            raw_tx = $4,
            transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = $1
        "#,
    )
    .bind(trade_no)
    .bind(to_addr)
    .bind(tx_hash)
    .bind("{\"rebuilt\":true}")
    .execute(pool.as_ref())
    .await
    .expect("persist rebuilt execution facts");
}

pub(crate) async fn persist_scanner_receipt_facts(
    pool: &ApiTransactionDbPool,
    trade_no: &str,
    tx_hash: &str,
) {
    sqlx::query(
        r#"
        UPDATE api_collect
        SET tx_hash = $2,
            raw_tx = $3,
            transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            tx_exec_receipt_uploaded_at = NULL,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = $1
        "#,
    )
    .bind(trade_no)
    .bind(tx_hash)
    .bind("{\"rebuilt\":true}")
    .execute(pool.as_ref())
    .await
    .expect("persist scan facts");
}
