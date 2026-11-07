use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::{sync::Notify, time::Instant};

const DEFAULT_FOREGROUND_LEASE: Duration = Duration::from_secs(120);

#[derive(Debug, Default)]
struct RecoveryState {
    globally_paused: bool,
    foreground: HashMap<String, ForegroundEntry>,
}

#[derive(Debug, Clone, Copy)]
struct ForegroundEntry {
    lease_id: u64,
    expires_at: Instant,
}

/// Central admission policy for API-wallet recovery work.
///
/// Priority order:
/// 1. a global pause, used by destructive foreground operations;
/// 2. recovery belonging to newly imported wallets;
/// 3. unrelated background recovery.
#[derive(Debug)]
pub struct RecoveryCoordinator {
    state: Mutex<RecoveryState>,
    changed: Notify,
    foreground_lease: Duration,
    next_lease_id: AtomicU64,
}

/// A provisional foreground lease held while an import is being validated.
///
/// Dropping an uncommitted lease removes the priority immediately. Committing
/// it leaves the wallet prioritized for the coordinator's normal timed lease.
pub struct ForegroundImportLease<'a> {
    coordinator: &'a RecoveryCoordinator,
    uid: String,
    lease_id: u64,
    committed: bool,
}

impl ForegroundImportLease<'_> {
    pub fn commit(mut self) {
        self.coordinator.renew_wallet(&self.uid, self.lease_id);
        self.committed = true;
    }
}

impl Drop for ForegroundImportLease<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.coordinator.remove_wallet(&self.uid, self.lease_id);
        }
    }
}

impl Default for RecoveryCoordinator {
    fn default() -> Self {
        Self::new(DEFAULT_FOREGROUND_LEASE)
    }
}

impl RecoveryCoordinator {
    fn new(foreground_lease: Duration) -> Self {
        Self {
            state: Mutex::new(RecoveryState::default()),
            changed: Notify::new(),
            foreground_lease,
            next_lease_id: AtomicU64::new(1),
        }
    }

    pub fn set_globally_paused(&self, paused: bool) {
        self.state.lock().expect("recovery coordinator mutex poisoned").globally_paused = paused;
        self.changed.notify_waiters();
    }

    pub fn is_globally_paused(&self) -> bool {
        self.state.lock().expect("recovery coordinator mutex poisoned").globally_paused
    }

    /// Starts foreground priority before backend import validation.
    pub fn begin_wallet_import<'a>(&'a self, uid: &str) -> ForegroundImportLease<'a> {
        let lease_id = self.next_lease_id.fetch_add(1, Ordering::Relaxed);
        self.state.lock().expect("recovery coordinator mutex poisoned").foreground.insert(
            uid.to_owned(),
            ForegroundEntry { lease_id, expires_at: Instant::now() + self.foreground_lease },
        );
        self.changed.notify_waiters();
        ForegroundImportLease { coordinator: self, uid: uid.to_owned(), lease_id, committed: false }
    }

    fn renew_wallet(&self, uid: &str, lease_id: u64) {
        let mut state = self.state.lock().expect("recovery coordinator mutex poisoned");
        if let Some(entry) =
            state.foreground.get_mut(uid).filter(|entry| entry.lease_id == lease_id)
        {
            entry.expires_at = Instant::now() + self.foreground_lease;
        }
        self.changed.notify_waiters();
    }

    fn remove_wallet(&self, uid: &str, lease_id: u64) {
        let mut state = self.state.lock().expect("recovery coordinator mutex poisoned");
        if state.foreground.get(uid).is_some_and(|entry| entry.lease_id == lease_id) {
            state.foreground.remove(uid);
        }
        self.changed.notify_waiters();
    }

    /// Waits for admission at a safe task/page boundary.
    pub async fn wait_for_turn(&self, wallet_uid: Option<&str>) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);

            let wait_until = {
                let now = Instant::now();
                let mut state = self.state.lock().expect("recovery coordinator mutex poisoned");
                state.foreground.retain(|_, entry| entry.expires_at > now);

                if state.globally_paused {
                    None
                } else if wallet_uid.is_none()
                    || state.foreground.is_empty()
                    || wallet_uid.is_some_and(|uid| state.foreground.contains_key(uid))
                {
                    return;
                } else {
                    Some(
                        state
                            .foreground
                            .values()
                            .map(|entry| entry.expires_at)
                            .min()
                            .expect("foreground is not empty"),
                    )
                }
            };

            match wait_until {
                Some(deadline) => {
                    tokio::select! {
                        _ = &mut notified => {}
                        _ = tokio::time::sleep_until(deadline) => {}
                    }
                }
                None => notified.await,
            }
        }
    }

    #[cfg(test)]
    fn finish_foreground_wallet(&self, uid: &str) {
        self.state.lock().expect("recovery coordinator mutex poisoned").foreground.remove(uid);
        self.changed.notify_waiters();
    }
}

#[cfg(test)]
mod recovery_coordinator_tests {
    use std::{sync::Arc, time::Duration};

    use super::RecoveryCoordinator;

    #[tokio::test]
    async fn recovery_coordinator_allows_foreground_wallet() {
        let coordinator = RecoveryCoordinator::new(Duration::from_secs(1));
        coordinator.begin_wallet_import("new-wallet").commit();
        tokio::time::timeout(
            Duration::from_millis(20),
            coordinator.wait_for_turn(Some("new-wallet")),
        )
        .await
        .expect("foreground wallet should not wait");
    }

    #[tokio::test]
    async fn recovery_coordinator_waits_for_unrelated_wallet() {
        let coordinator = Arc::new(RecoveryCoordinator::new(Duration::from_secs(1)));
        coordinator.begin_wallet_import("new-wallet").commit();
        let waiting = {
            let coordinator = Arc::clone(&coordinator);
            tokio::spawn(async move { coordinator.wait_for_turn(Some("old-wallet")).await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiting.is_finished());
        coordinator.finish_foreground_wallet("new-wallet");
        tokio::time::timeout(Duration::from_millis(100), waiting)
            .await
            .expect("old wallet should resume")
            .expect("wait task should succeed");
    }

    #[tokio::test]
    async fn recovery_coordinator_global_pause_blocks_foreground_wallet() {
        let coordinator = Arc::new(RecoveryCoordinator::new(Duration::from_secs(1)));
        coordinator.begin_wallet_import("new-wallet").commit();
        coordinator.set_globally_paused(true);
        let waiting = {
            let coordinator = Arc::clone(&coordinator);
            tokio::spawn(async move { coordinator.wait_for_turn(Some("new-wallet")).await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiting.is_finished());
        coordinator.set_globally_paused(false);
        tokio::time::timeout(Duration::from_millis(100), waiting)
            .await
            .expect("foreground wallet should resume after global pause")
            .expect("wait task should succeed");
    }

    #[tokio::test]
    async fn recovery_coordinator_allows_multiple_foreground_wallets() {
        let coordinator = RecoveryCoordinator::new(Duration::from_secs(1));
        coordinator.begin_wallet_import("recharge-wallet").commit();
        coordinator.begin_wallet_import("withdraw-wallet").commit();
        for uid in ["recharge-wallet", "withdraw-wallet"] {
            tokio::time::timeout(Duration::from_millis(20), coordinator.wait_for_turn(Some(uid)))
                .await
                .expect("each foreground wallet should proceed");
        }
    }

    #[tokio::test]
    async fn recovery_coordinator_expires_stale_foreground_lease() {
        let coordinator = RecoveryCoordinator::new(Duration::from_millis(20));
        coordinator.begin_wallet_import("abandoned-import").commit();
        tokio::time::timeout(
            Duration::from_millis(100),
            coordinator.wait_for_turn(Some("old-wallet")),
        )
        .await
        .expect("stale foreground lease should expire");
    }

    #[tokio::test]
    async fn recovery_coordinator_releases_failed_import_priority() {
        let coordinator = RecoveryCoordinator::new(Duration::from_secs(1));
        let lease = coordinator.begin_wallet_import("invalid-wallet");
        drop(lease);
        tokio::time::timeout(
            Duration::from_millis(20),
            coordinator.wait_for_turn(Some("existing-wallet")),
        )
        .await
        .expect("failed import should release priority immediately");
    }
}
