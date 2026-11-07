use wallet_database::{
    entities::api_wallet::{ApiWalletEntity, ApiWalletType},
    repositories::api_wallet::wallet::ApiWalletRepo,
};

use crate::harness::{
    self, find_wallet_by_uid, load_wallet_by_uid, next_tag, open_api_wallet_pool,
    upsert_wallet_with_import_stage,
};

use super::fixtures::RechargeWalletFixture;

pub(crate) async fn seed_recharge_wallet(
    env: &harness::TestEnv,
    uid_prefix: &str,
    import_stage: u8,
) -> RechargeWalletFixture {
    let uid = next_tag(uid_prefix);
    let address = upsert_wallet_with_import_stage(
        &env.db_dir,
        &env.sn,
        &uid,
        ApiWalletType::SubAccount,
        None,
        import_stage,
    )
    .await;

    RechargeWalletFixture { uid, address }
}

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

pub(crate) async fn mark_wallet_pair_on_other_device(
    env: &harness::TestEnv,
    recharge_address: &str,
    withdrawal_uid: &str,
) {
    let pool = open_api_wallet_pool(&env.db_dir).await;
    let withdrawal = ApiWalletRepo::find_by_uid(&pool, withdrawal_uid)
        .await
        .expect("query prepared withdrawal wallet")
        .expect("prepared withdrawal wallet should exist");
    ApiWalletRepo::update_sn(&pool, recharge_address, "different-device-sn")
        .await
        .expect("move prepared recharge wallet to another device");
    ApiWalletRepo::update_sn(&pool, &withdrawal.address, "different-device-sn")
        .await
        .expect("move prepared withdrawal wallet to another device");
}
