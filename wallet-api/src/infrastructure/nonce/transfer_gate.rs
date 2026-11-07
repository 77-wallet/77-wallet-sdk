//! Per-context, per-chain/sender serialization shared by both wallet flows.
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Default)]
pub(crate) struct TransferGates {
    gates: DashMap<(String, String), Arc<Semaphore>>,
}

impl TransferGates {
    pub(crate) async fn acquire(&self, address: &str, chain: &str) -> OwnedSemaphorePermit {
        let key = (
            wallet_database::entities::api_nonce::nonce_address_key(address, chain),
            chain.to_owned(),
        );
        let semaphore =
            self.gates.entry(key).or_insert_with(|| Arc::new(Semaphore::new(1))).clone();
        semaphore.acquire_owned().await.expect("transfer gate closed unexpectedly")
    }
}
