use super::*;
use crate::repositories::test_helper::setup_api_wallet_pool;

#[tokio::test]
async fn writer_events_follow_real_sqlite_lock_lifecycle() {
    let pool = setup_api_wallet_pool("writer_diagnostics").await;
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    pool.set_writer_diagnostics(tx);
    let held = pool.lock_write_with_metric("upsert_assets_multi").await;
    let wait = rx.try_recv().expect("must report wait at the shared gate");
    let acquired = rx.try_recv().expect("must report lock owner");
    assert_eq!(wait.event, "wait");
    assert_eq!(acquired.event, "acquired");
    assert_eq!(wait.lock_id, acquired.lock_id);
    assert_eq!(wait.pool_id, acquired.pool_id);
    sqlx::query("CREATE TABLE writer_diagnostic_probe (value INTEGER)")
        .execute(pool.write_ref())
        .await
        .unwrap();
    sqlx::query("INSERT INTO writer_diagnostic_probe VALUES (7)")
        .execute(pool.write_ref())
        .await
        .unwrap();
    drop(held);
    let released = rx.try_recv().expect("must report release");
    assert_eq!(released.event, "released");
    assert_eq!(released.lock_id, acquired.lock_id);
    assert!(released.held_ms.unwrap() >= 0.0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT value FROM writer_diagnostic_probe")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        7
    );
}

#[tokio::test]
async fn cloned_pool_reports_the_holder_and_a_cancelled_waiter() {
    use std::{future::Future, task::Poll, time::Duration};
    let pool = setup_api_wallet_pool("writer_owner_wait").await;
    let clone = pool.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    pool.set_writer_diagnostics(tx);
    let held = pool.lock_write_with_metric("upsert_assets_multi_update_balance").await;
    let first = rx.try_recv().unwrap();
    let owner = rx.try_recv().unwrap();
    let mut waiting = Box::pin(clone.lock_write_with_metric("upsert_api_wallet"));
    std::future::poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let queued = rx.try_recv().unwrap();
    assert_eq!(queued.pool_id, owner.pool_id);
    assert_ne!(queued.lock_id, owner.lock_id);
    assert_eq!(owner.op, "upsert_assets_multi_update_balance");
    assert_eq!(queued.op, "upsert_api_wallet");
    tokio::time::sleep(Duration::from_millis(25)).await;
    drop(waiting);
    let cancelled = rx.try_recv().unwrap();
    assert_eq!(cancelled.event, "cancelled");
    assert_eq!(cancelled.lock_id, queued.lock_id);
    assert!(cancelled.wait_ms.unwrap() >= 20.0);
    drop(held);
    let released = rx.try_recv().unwrap();
    assert_eq!(released.lock_id, first.lock_id);
    assert!(released.held_ms.unwrap() >= 20.0);
    let next = clone.lock_write().await;
    drop(next);
    assert_eq!(rx.try_recv().unwrap().event, "wait");
    assert_eq!(rx.try_recv().unwrap().event, "acquired");
    assert_eq!(rx.try_recv().unwrap().event, "released");
}

#[tokio::test]
async fn cancelling_a_holder_releases_gate_and_emits_release() {
    use std::{future::Future, task::Poll};
    let pool = setup_api_wallet_pool("writer_holder_cancel").await;
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    pool.set_writer_diagnostics(tx);
    let mut future = Box::pin(async {
        let _held = pool.lock_write_with_metric("init_many_accounts").await;
        std::future::pending::<()>().await;
    });
    std::future::poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let wait = rx.try_recv().unwrap();
    assert_eq!(rx.try_recv().unwrap().event, "acquired");
    drop(future);
    let released = rx.try_recv().unwrap();
    assert_eq!(released.event, "released");
    assert_eq!(released.lock_id, wait.lock_id);
    let next =
        tokio::time::timeout(std::time::Duration::from_secs(1), pool.lock_write()).await.unwrap();
    drop(next);
}

#[tokio::test]
async fn bounded_or_closed_diagnostics_never_block_and_redact_unknown_ops() {
    let pool = setup_api_wallet_pool("writer_bounded").await;
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    pool.set_writer_diagnostics(tx);
    let held = pool.lock_write_with_metric("sensitive-operation-sentinel").await;
    drop(held);
    let first = rx.try_recv().unwrap();
    assert_eq!(first.op, "other");
    assert_eq!(first.dropped_events, 0);
    let next = pool.lock_write().await;
    let next_wait = rx.try_recv().unwrap();
    assert_eq!(next_wait.dropped_events, 2);
    drop(rx);
    drop(next);
    let held =
        tokio::time::timeout(std::time::Duration::from_secs(1), pool.lock_write()).await.unwrap();
    drop(held);
    // Other pools and disabled diagnostics retain the same exclusion behavior.
    let raw = std::sync::Arc::new(sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    let other = CoreDbPool::new(raw);
    let held = other.lock_write().await;
    drop(held);
}
