//! Context-local ordering for ordinary asset balance observations.
//! Register before remote reads; keep the current guard through DB commit only.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use wallet_database::entities::{asset_token_key::AssetTokenKey, assets::AssetsId};

type Key = (String, String, AssetTokenKey);
fn key(id: &AssetsId) -> Key {
    let address = AssetTokenKey::Contract(id.address.clone())
        .normalized_for_chain(&id.chain_code)
        .as_db_str()
        .to_string();
    (address, id.chain_code.clone(), id.token_address.normalized_for_chain(&id.chain_code))
}
#[derive(Default)]
pub(crate) struct RefreshOrder {
    cells: Mutex<HashMap<Key, Weak<AsyncMutex<u64>>>>,
}
pub(crate) struct Ticket<'a> {
    order: &'a RefreshOrder,
    key: Key,
    cell: Arc<AsyncMutex<u64>>,
    version: u64,
}
impl RefreshOrder {
    pub async fn begin(&self, id: &AssetsId) -> Ticket<'_> {
        let key = key(id);
        let cell = {
            let mut cells = self.cells.lock().expect("balance order registry");
            let cell = cells
                .get(&key)
                .and_then(Weak::upgrade)
                .unwrap_or_else(|| Arc::new(AsyncMutex::new(0)));
            cells.insert(key.clone(), Arc::downgrade(&cell));
            cell
        };
        // Own the ticket before awaiting the commit lock, so cancellation also cleans the registry.
        let mut ticket = Ticket { order: self, key, cell, version: 0 };
        ticket.version = {
            let mut latest = ticket.cell.lock().await;
            *latest = latest.checked_add(1).expect("balance request version exhausted");
            *latest
        };
        ticket
    }
    pub async fn batch(&self, ids: impl IntoIterator<Item = AssetsId>) -> Batch<'_> {
        let mut tickets = HashMap::new();
        for id in ids {
            tickets.insert(key(&id), self.begin(&id).await);
        }
        Batch(tickets)
    }
}
impl Ticket<'_> {
    pub async fn current(&self) -> Option<MutexGuard<'_, u64>> {
        let latest = self.cell.lock().await;
        (*latest == self.version).then_some(latest)
    }
}
impl Drop for Ticket<'_> {
    fn drop(&mut self) {
        let mut cells = self.order.cells.lock().expect("balance order registry");
        if Arc::strong_count(&self.cell) == 1
            && cells.get(&self.key).is_some_and(|cell| cell.as_ptr() == Arc::as_ptr(&self.cell))
        {
            cells.remove(&self.key);
        }
    }
}
pub(crate) struct Batch<'a>(HashMap<Key, Ticket<'a>>);
impl Batch<'_> {
    pub fn get(&self, id: &AssetsId) -> Option<&Ticket<'_>> {
        self.0.get(&key(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_registration_and_completed_tickets_release_registry() {
        let order = RefreshOrder::default();
        let id = AssetsId::new("owner", "eth", AssetTokenKey::Native);
        let old = order.begin(&id).await;
        let commit = old.current().await.unwrap();
        // A registration cannot interleave the check and commit; cancelling that
        // blocked registration must not leave a strong reference or registry entry.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), order.begin(&id))
                .await
                .is_err()
        );
        drop(commit);
        drop(old);
        assert!(order.cells.lock().unwrap().is_empty());
        let next = order.begin(&id).await;
        assert!(next.current().await.is_some());
        drop(next);
        assert!(order.cells.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn commit_lock_does_not_block_other_assets() {
        let order = RefreshOrder::default();
        let old = order.begin(&AssetsId::new("owner", "eth", AssetTokenKey::Native)).await;
        let commit = old.current().await.unwrap();
        let other = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            order.begin(&AssetsId::new("owner", "polygon", AssetTokenKey::Native)),
        )
        .await
        .unwrap();
        assert!(other.current().await.is_some());
        drop(commit);
        drop(old);
        drop(other);
        assert!(order.cells.lock().unwrap().is_empty());
    }
}
