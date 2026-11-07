use std::path::Path;

use wallet_database::{
    ApiTransactionDbPool, SqliteContext,
    entities::{api_collect::ApiCollectEntity, api_wallet::ApiWalletType},
    repositories::api_wallet::{collect::ApiCollectRepo, wallet::ApiWalletRepo},
};

use crate::harness::{SMOKE_WALLET_PASSWORD, next_unique_id, open_api_wallet_pool};

const COLLECT_NOTIFICATION_TEST_SN: &str = "collect-notification-test-sn";

pub(crate) async fn open_transaction_pool(db_dir: &Path) -> ApiTransactionDbPool {
    let tx_pool_ctx = SqliteContext::new(&db_dir.to_string_lossy(), Some("api_transaction.db"))
        .await
        .expect("open api transaction sqlite");
    tx_pool_ctx.into_transaction_db_pool().expect("transaction pool")
}

pub(crate) async fn seed_wallet(
    db_dir: &Path,
    uid: &str,
    wallet_name: &str,
    wallet_type: ApiWalletType,
) -> String {
    let pool = open_api_wallet_pool(db_dir).await;
    let address = format!("0xwallet{:016x}", next_unique_id());
    let seed_enc = wallet_api::testkit::seed::encrypt_seed(SMOKE_WALLET_PASSWORD, b"seed").await;
    ApiWalletRepo::upsert(
        &pool,
        uid,
        wallet_name,
        &address,
        b"phrase",
        &seed_enc,
        wallet_type,
        None,
        COLLECT_NOTIFICATION_TEST_SN,
        0,
    )
    .await
    .expect("seed wallet");
    address
}

pub(crate) async fn load_collect(
    tx_pool: &ApiTransactionDbPool,
    trade_no: &str,
) -> ApiCollectEntity {
    ApiCollectRepo::get_api_collect_by_trade_no(tx_pool, trade_no).await.expect("load collect")
}
