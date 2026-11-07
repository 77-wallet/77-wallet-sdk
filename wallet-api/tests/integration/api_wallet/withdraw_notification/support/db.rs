use std::path::Path;

use wallet_database::{
    ApiTransactionDbPool, SqliteContext,
    entities::{
        api_trade_type::ApiTradeType,
        api_wallet::ApiWalletType,
        api_withdraw::{ApiWithdrawEntity, ApiWithdrawStatus},
    },
    repositories::api_wallet::{wallet::ApiWalletRepo, withdraw::ApiWithdrawRepo},
};

use crate::harness::{SMOKE_WALLET_PASSWORD, next_unique_id, open_api_wallet_pool};

use super::fixtures::{
    WITHDRAW_CHAIN, WITHDRAW_SYMBOL, WITHDRAW_VALIDATE, WITHDRAW_VALUE, WithdrawOrderFixture,
};

const WITHDRAW_NOTIFICATION_TEST_SN: &str = "withdraw-notification-test-sn";

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
        WITHDRAW_NOTIFICATION_TEST_SN,
        0,
    )
    .await
    .expect("seed wallet");
    address
}

pub(crate) async fn insert_withdraw_order(
    tx_pool: &ApiTransactionDbPool,
    order: &WithdrawOrderFixture,
) {
    ApiWithdrawRepo::upsert_api_withdraw(
        tx_pool,
        &order.uid,
        "withdraw",
        &order.from_addr,
        &order.to_addr,
        WITHDRAW_VALUE,
        WITHDRAW_VALIDATE,
        WITHDRAW_CHAIN,
        None,
        WITHDRAW_SYMBOL,
        &order.trade_no,
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
    .expect("insert withdraw");
}

pub(crate) async fn load_withdraw(
    tx_pool: &ApiTransactionDbPool,
    trade_no: &str,
) -> ApiWithdrawEntity {
    ApiWithdrawRepo::get_api_withdraw_by_trade_no(tx_pool, trade_no, ApiTradeType::Withdraw)
        .await
        .expect("load withdraw")
}
