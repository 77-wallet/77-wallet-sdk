// WorkerTestEnv and the ordinary transfer fixture share a process-global Context.
// Keep the real HTTP receipt regression in its own integration test process.
#![cfg(feature = "integration-tests")]
#![recursion_limit = "256"]

#[allow(dead_code)]
#[path = "integration/api_wallet/transfer_nonce/support/adapter.rs"]
mod adapter;
#[allow(dead_code)]
#[path = "integration/api_wallet/transfer_nonce/support/db.rs"]
mod db;
#[allow(dead_code, unused_imports)]
mod harness;
#[allow(dead_code)]
#[path = "integration/api_wallet/transfer_nonce/support/request.rs"]
mod request;
#[allow(dead_code)]
#[path = "integration/api_wallet/transfer_nonce/support/withdraw_broadcast.rs"]
mod withdraw_broadcast;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn withdrawal_broadcast_terminal_gap_uploads_failure_receipts_once() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("wallet_api::infrastructure::api_trans::withdraw=debug")
        .with_test_writer()
        .try_init();
    let scenario =
        withdraw_broadcast::WithdrawBroadcastScenario::given_signed_batch_with_receipts().await?;
    scenario.given_predecessor_failed().await?;
    for nonce in [15, 16] {
        scenario.when_broadcast(nonce).await?;
        scenario.when_failure_receipt_uploaded(nonce).await?;
        scenario.when_failure_receipt_uploaded(nonce).await?;
    }
    scenario.then_failure_receipts_uploaded_once().await?;
    scenario.then_next_nonce_reusable().await?;
    assert!(scenario.adapter.recorded_nonces().is_empty());
    Ok(())
}
