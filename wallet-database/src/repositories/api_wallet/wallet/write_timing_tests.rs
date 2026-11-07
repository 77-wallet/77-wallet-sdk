use super::*;
use crate::repositories::test_helper::setup_api_wallet_pool;
use std::{future::Future, task::Poll, time::Duration};

#[tokio::test]
async fn upsert_timing_separates_writer_queue_from_action_and_preserves_row() {
    let pool = setup_api_wallet_pool("upsert_timing").await;
    let held = pool.lock_write().await;
    let mut write = Box::pin(ApiWalletRepo::upsert_with_timing(
        &pool,
        "uid",
        "test",
        "address",
        &[1],
        &[2],
        ApiWalletType::SubAccount,
        None,
        "test-device",
        0,
    ));
    std::future::poll_fn(|cx| {
        assert!(write.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    drop(held);
    let (result, timing) = write.await;
    let row = result.unwrap();
    assert_eq!(row.uid, "uid");
    assert_eq!(row.address, "address");
    let stored = ApiWalletRepo::find_by_address(&pool, "address").await.unwrap().unwrap();
    assert_eq!(stored.uid, "uid");
    assert!(timing.writer_wait_ms >= 20.0, "must include controlled writer queue delay");
    assert!(timing.write_action_ms.is_finite() && timing.write_action_ms > 0.0);
}

#[tokio::test]
async fn measured_upsert_preserves_closed_pool_failure() {
    let pool = setup_api_wallet_pool("upsert_timing_failure").await;
    pool.write_ref().close().await;
    let ordinary = ApiWalletRepo::upsert(
        &pool,
        "uid",
        "test",
        "address",
        &[1],
        &[2],
        ApiWalletType::SubAccount,
        None,
        "test-device",
        0,
    )
    .await
    .unwrap_err();
    let (measured, timing) = ApiWalletRepo::upsert_with_timing(
        &pool,
        "uid",
        "test",
        "address",
        &[1],
        &[2],
        ApiWalletType::SubAccount,
        None,
        "test-device",
        0,
    )
    .await;
    assert_eq!(measured.unwrap_err().to_string(), ordinary.to_string());
    assert!(timing.writer_wait_ms.is_finite());
    assert!(timing.write_action_ms.is_finite());
}
