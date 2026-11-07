use wallet_database::{
    entities::api_wallet::{ApiWalletEntity, ApiWalletType},
    repositories::api_wallet::wallet::ApiWalletRepo,
};

use crate::harness::{
    self, find_wallet_by_uid, load_wallet_by_uid, next_tag, open_api_wallet_pool, upsert_wallet,
};

pub(crate) async fn load_wallet(env: &harness::TestEnv, uid: &str) -> ApiWalletEntity {
    load_wallet_by_uid(env, uid).await
}

pub(crate) async fn find_wallet(env: &harness::TestEnv, uid: &str) -> Option<ApiWalletEntity> {
    find_wallet_by_uid(env, uid).await
}

pub(crate) async fn persisted_import_stage(env: &harness::TestEnv, uid: &str) -> Option<u8> {
    let pool = open_api_wallet_pool(&env.db_dir).await;
    ApiWalletRepo::find_by_uid(&pool, uid)
        .await
        .expect("query wallet by uid")
        .map(|wallet| wallet.import_stage)
}

pub(crate) async fn seed_bound_withdrawal_wallet(
    env: &harness::TestEnv,
    subaccount_uid: &str,
    withdrawal_sn: &str,
) -> String {
    let subaccount = load_wallet(env, subaccount_uid).await;
    let withdrawal_uid = next_tag("withdrawal-uid");
    let withdrawal_address = upsert_wallet(
        &env.db_dir,
        withdrawal_sn,
        &withdrawal_uid,
        ApiWalletType::Withdrawal,
        Some(&subaccount.address),
    )
    .await;
    let pool = open_api_wallet_pool(&env.db_dir).await;
    ApiWalletRepo::bind_withdraw_and_subaccount_relation(
        &pool,
        &subaccount.address,
        &withdrawal_address,
    )
    .await
    .expect("bind prepared wallet pair");
    withdrawal_address
}

pub(crate) async fn load_wallet_by_address(
    env: &harness::TestEnv,
    address: &str,
) -> ApiWalletEntity {
    let pool = open_api_wallet_pool(&env.db_dir).await;
    ApiWalletRepo::find_by_address(&pool, address)
        .await
        .expect("query wallet by address")
        .expect("wallet should exist")
}

pub(crate) async fn find_wallet_by_address(
    env: &harness::TestEnv,
    address: &str,
) -> Option<ApiWalletEntity> {
    let pool = open_api_wallet_pool(&env.db_dir).await;
    ApiWalletRepo::find_by_address(&pool, address).await.expect("query wallet by address")
}
