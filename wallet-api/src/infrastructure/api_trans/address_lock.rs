use std::sync::{Arc, Weak};

use dashmap::DashMap;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::error::{service::ServiceError, system::SystemError};

/// Serializes transaction work for the same account without retaining idle locks.
pub struct AddressLockManager {
    locks: DashMap<String, Weak<Semaphore>>,
}

impl AddressLockManager {
    pub fn new() -> Self {
        Self { locks: DashMap::new() }
    }

    pub async fn acquire(&self, account: &str) -> Result<OwnedSemaphorePermit, ServiceError> {
        self.get_or_create_semaphore(account)
            .acquire_owned()
            .await
            .map_err(|_| ServiceError::System(SystemError::SemaphoreClosed))
    }

    fn get_or_create_semaphore(&self, account: &str) -> Arc<Semaphore> {
        use dashmap::mapref::entry::Entry;

        match self.locks.entry(account.to_string()) {
            Entry::Occupied(mut entry) => {
                if let Some(semaphore) = entry.get().upgrade() {
                    semaphore
                } else {
                    let semaphore = Arc::new(Semaphore::new(1));
                    entry.insert(Arc::downgrade(&semaphore));
                    semaphore
                }
            }
            Entry::Vacant(entry) => {
                let semaphore = Arc::new(Semaphore::new(1));
                entry.insert(Arc::downgrade(&semaphore));
                semaphore
            }
        }
    }
}

impl Default for AddressLockManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::AddressLockManager;

    #[tokio::test]
    async fn same_account_waits_for_the_existing_permit() {
        let manager = Arc::new(AddressLockManager::new());
        let first = manager.acquire("account-a").await.expect("first permit");

        let waiting_manager = Arc::clone(&manager);
        let waiting = tokio::spawn(async move { waiting_manager.acquire("account-a").await });

        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());

        drop(first);
        waiting.await.expect("join").expect("second permit");
    }

    #[tokio::test]
    async fn different_accounts_can_run_concurrently() {
        let manager = AddressLockManager::new();
        let first = manager.acquire("account-a").await.expect("first permit");
        let second = manager.acquire("account-b").await.expect("second permit");

        drop((first, second));
    }
}
