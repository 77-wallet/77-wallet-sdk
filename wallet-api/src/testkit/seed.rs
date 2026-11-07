//! Test-only seed fixture helpers.

use crate::domain::api_wallet::wallet::ApiWalletDomain;

pub async fn encrypt_seed(password: &str, seed: &[u8]) -> Vec<u8> {
    ApiWalletDomain::encrypt_seed_bundle(password, seed).await.expect("encrypt test seed")
}

/// Decode stored synthetic bundles to assert password rotation persistence.
pub async fn decrypt_seed(
    password: &str,
    seed: &[u8],
) -> Result<Vec<u8>, crate::error::service::ServiceError> {
    ApiWalletDomain::decrypt_seed(password, seed).await
}
