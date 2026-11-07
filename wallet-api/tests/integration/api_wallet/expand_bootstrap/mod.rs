mod support;

use support::{Scenario, run_in_child};

#[tokio::test]
async fn expand_bootstrap_connection_loss_does_not_block_scanner_or_recovery() {
    if run_in_child("expand_bootstrap_connection_loss_does_not_block_scanner_or_recovery") {
        return;
    }
    let scenario = Scenario::given_done_batch().await;
    scenario.given_next_connection_drops();
    let result = scenario.when_unlock_bootstrap_runs().await;
    assert!(
        result.is_ok(),
        "offline completion reporting must not prevent scanner startup: {result:?}"
    );
    scenario.then_scanner_running_and_batch_recovers().await;
    scenario.then_completed_with_matching_backend_requests(2).await;
}

#[tokio::test]
async fn expand_bootstrap_concurrent_and_repeated_unlock_start_only_one_scanner() {
    if run_in_child("expand_bootstrap_concurrent_and_repeated_unlock_start_only_one_scanner") {
        return;
    }
    let scenario = Scenario::given_done_batch().await;
    let (first, second) =
        tokio::join!(scenario.when_unlock_bootstrap_runs(), scenario.when_unlock_bootstrap_runs());
    first.unwrap();
    second.unwrap();
    scenario.then_scanner_running_and_batch_recovers().await;
    scenario.then_repeated_unlock_preserves_scanner().await;
    scenario.then_completed_with_matching_backend_requests(1).await;
}
