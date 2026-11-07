use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use wallet_database::{
    SqliteContext,
    entities::api_collect::ApiCollectStatus,
    repositories::api_wallet::{collect::ApiCollectRepo, fee::ApiFeeRepo},
};

fn make_temp_dir(prefix: &str) -> String {
    let mut path = PathBuf::from(std::env::temp_dir());
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    path.push(format!("{prefix}_{suffix}_{}", std::process::id()));
    fs::create_dir_all(&path).expect("create temp dir");
    path.to_string_lossy().into_owned()
}

async fn setup_transaction_pool(prefix: &str) -> wallet_database::ApiTransactionDbPool {
    let dir = make_temp_dir(prefix);
    let ctx =
        SqliteContext::new(&dir, Some("api_transaction.db")).await.expect("create sqlite context");
    ctx.into_transaction_db_pool().expect("create transaction pool")
}

#[tokio::test]
async fn collect_scan_need_tx_exec_receipt_upload_filters_pending_and_keeps_terminal_facts() {
    let pool = setup_transaction_pool("wallet_db_collect_receipt_integration").await;

    ApiCollectRepo::upsert_api_collect(
        &pool,
        "uid",
        "n",
        "from",
        "to",
        "0",
        "v",
        "c",
        None,
        "s",
        "C_INT_PENDING",
        2,
        ApiCollectStatus::Init,
        0,
    )
    .await
    .unwrap();
    ApiCollectRepo::upsert_api_collect(
        &pool,
        "uid",
        "n",
        "from",
        "to",
        "0",
        "v",
        "c",
        None,
        "s",
        "C_INT_SUCCESS",
        2,
        ApiCollectStatus::Init,
        0,
    )
    .await
    .unwrap();
    ApiCollectRepo::upsert_api_collect(
        &pool,
        "uid",
        "n",
        "from",
        "to",
        "0",
        "v",
        "c",
        None,
        "s",
        "C_INT_FAIL",
        2,
        ApiCollectStatus::Init,
        0,
    )
    .await
    .unwrap();

    sqlx::query(
        "UPDATE api_collect
         SET last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             tx_hash = '0xpending'
         WHERE trade_no = ?",
    )
    .bind("C_INT_PENDING")
    .execute(pool.read_ref())
    .await
    .unwrap();

    sqlx::query(
        "UPDATE api_collect
         SET transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             tx_hash = '0xsuccess'
         WHERE trade_no = ?",
    )
    .bind("C_INT_SUCCESS")
    .execute(pool.read_ref())
    .await
    .unwrap();

    sqlx::query(
        "UPDATE api_collect
         SET err_code = 6099,
             tx_hash = ''
         WHERE trade_no = ?",
    )
    .bind("C_INT_FAIL")
    .execute(pool.read_ref())
    .await
    .unwrap();

    let records = ApiCollectRepo::scan_need_tx_exec_receipt_upload(&pool, 100).await.unwrap();
    let trade_nos: Vec<String> = records.into_iter().map(|r| r.trade_no).collect();

    assert!(!trade_nos.contains(&"C_INT_PENDING".to_string()));
    assert!(trade_nos.contains(&"C_INT_SUCCESS".to_string()));
    assert!(trade_nos.contains(&"C_INT_FAIL".to_string()));
    assert_eq!(trade_nos.len(), 2);
}

#[tokio::test]
async fn fee_scan_need_tx_exec_receipt_upload_filters_pending_and_keeps_terminal_facts() {
    let pool = setup_transaction_pool("wallet_db_fee_receipt_integration").await;

    ApiFeeRepo::upsert_api_fee(
        &pool,
        "uid",
        "n",
        "from",
        "to",
        "0",
        "v",
        "c",
        None,
        "s",
        "F_INT_PENDING",
        0,
    )
    .await
    .unwrap();
    ApiFeeRepo::upsert_api_fee(
        &pool,
        "uid",
        "n",
        "from",
        "to",
        "0",
        "v",
        "c",
        None,
        "s",
        "F_INT_SUCCESS",
        0,
    )
    .await
    .unwrap();
    ApiFeeRepo::upsert_api_fee(
        &pool,
        "uid",
        "n",
        "from",
        "to",
        "0",
        "v",
        "c",
        None,
        "s",
        "F_INT_FAIL",
        0,
    )
    .await
    .unwrap();

    sqlx::query(
        "UPDATE api_fee
         SET last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             tx_hash = '0xpending'
         WHERE trade_no = ?",
    )
    .bind("F_INT_PENDING")
    .execute(pool.read_ref())
    .await
    .unwrap();

    sqlx::query(
        "UPDATE api_fee
         SET transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             tx_hash = '0xsuccess'
         WHERE trade_no = ?",
    )
    .bind("F_INT_SUCCESS")
    .execute(pool.read_ref())
    .await
    .unwrap();

    sqlx::query(
        "UPDATE api_fee
         SET err_code = 6099,
             tx_hash = ''
         WHERE trade_no = ?",
    )
    .bind("F_INT_FAIL")
    .execute(pool.read_ref())
    .await
    .unwrap();

    let records = ApiFeeRepo::scan_need_tx_exec_receipt_upload(&pool, 100).await.unwrap();
    let trade_nos: Vec<String> = records.into_iter().map(|r| r.trade_no).collect();

    assert!(!trade_nos.contains(&"F_INT_PENDING".to_string()));
    assert!(trade_nos.contains(&"F_INT_SUCCESS".to_string()));
    assert!(trade_nos.contains(&"F_INT_FAIL".to_string()));
    assert_eq!(trade_nos.len(), 2);
}
