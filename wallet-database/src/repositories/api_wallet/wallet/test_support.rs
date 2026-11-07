use super::ApiWalletRepo;

pub(super) async fn seed_delete_wallet(pool: &crate::ApiWalletDbPool, uid: &str, address: &str) {
    ApiWalletRepo::upsert(
        pool,
        uid,
        "wallet",
        address,
        b"phrase",
        b"seed",
        crate::entities::api_wallet::ApiWalletType::SubAccount,
        None,
        "sn",
        0,
    )
    .await
    .unwrap();
}
