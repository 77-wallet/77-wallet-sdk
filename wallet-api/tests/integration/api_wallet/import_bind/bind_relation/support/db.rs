use wallet_database::entities::api_wallet::{ApiWalletEntity, ApiWalletType};

use crate::harness::{
    self, BindSnapshot, WalletPair, load_wallet_by_uid, next_tag, prepare_wallet_pair,
    snapshot_bind_fields, upsert_wallet,
};

pub(crate) type PairBindSnapshot = (BindSnapshot, BindSnapshot);

pub(crate) async fn seed_wallet_pair(env: &harness::TestEnv) -> WalletPair {
    prepare_wallet_pair(env).await
}

pub(crate) async fn seed_recharge_wallet(env: &harness::TestEnv, uid_prefix: &str) -> String {
    let uid = next_tag(uid_prefix);
    upsert_wallet(&env.db_dir, &env.sn, &uid, ApiWalletType::SubAccount, None).await;
    uid
}

pub(crate) async fn load_wallet(env: &harness::TestEnv, uid: &str) -> ApiWalletEntity {
    load_wallet_by_uid(env, uid).await
}

pub(crate) async fn snapshot_wallet_bind_fields(env: &harness::TestEnv, uid: &str) -> BindSnapshot {
    snapshot_bind_fields(&load_wallet(env, uid).await)
}

pub(crate) async fn snapshot_pair_bind_fields(
    env: &harness::TestEnv,
    pair: &WalletPair,
) -> PairBindSnapshot {
    (
        snapshot_wallet_bind_fields(env, &pair.recharge_uid).await,
        snapshot_wallet_bind_fields(env, &pair.withdrawal_uid).await,
    )
}
