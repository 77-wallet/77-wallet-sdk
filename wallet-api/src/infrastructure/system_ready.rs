use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

static SYSTEM_READY: AtomicBool = AtomicBool::new(false);
static READY_NOTIFY: Lazy<Notify> = Lazy::new(Notify::new);

pub async fn wait_system_ready() {
    wait_ready(&SYSTEM_READY, &READY_NOTIFY, || {}).await;
}

async fn wait_ready(ready: &AtomicBool, notify: &Notify, after_check: impl FnOnce()) {
    // notify_waiters covers futures created before the broadcast, even before polling.
    let notified = notify.notified();
    if ready.load(Ordering::Acquire) {
        tracing::info!("system already ready");
        return;
    }
    after_check();
    tracing::info!("waiting system ready...");
    notified.await;
    tracing::info!("system ready notified");
}

pub fn mark_system_ready() {
    if !SYSTEM_READY.swap(true, Ordering::Release) {
        READY_NOTIFY.notify_waiters();
    }
    tracing::info!("system marked ready");
}

pub fn is_system_ready() -> bool {
    SYSTEM_READY.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn readiness_between_check_and_first_poll_is_not_lost() {
        // Arrange: publish readiness in the exact gap after the false check.
        let ready = AtomicBool::new(false);
        let notify = Notify::new();
        let wait = wait_ready(&ready, &notify, || {
            ready.store(true, Ordering::Release);
            notify.notify_waiters();
        });
        // Act/Assert: notification before the first poll must still complete.
        tokio::time::timeout(Duration::from_millis(100), wait).await.unwrap();
        assert!(ready.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn already_ready_does_not_wait_for_another_notification() {
        let ready = AtomicBool::new(true);
        let notify = Notify::new();
        tokio::time::timeout(
            Duration::from_millis(100),
            wait_ready(&ready, &notify, || panic!("already-ready state must bypass waiting")),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn one_ready_broadcast_releases_all_waiters() {
        let ready = AtomicBool::new(false);
        let notify = Notify::new();
        let waits = async {
            tokio::join!(
                wait_ready(&ready, &notify, || {}),
                wait_ready(&ready, &notify, || {}),
                async {
                    ready.store(true, Ordering::Release);
                    notify.notify_waiters();
                }
            );
        };
        tokio::time::timeout(Duration::from_millis(100), waits).await.unwrap();
    }
}
