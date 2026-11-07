mod support;

use serial_test::serial;
use support::DeletionScenario;

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_from_subaccount_removes_pair_and_preserves_other_wallet() {
    // Given a linked pair with accounts and an unrelated wallet.
    let scenario = DeletionScenario::given().await;
    // When deleting the subaccount end.
    scenario.when_delete_subaccount().await.unwrap();
    // Then both wallets/accounts are gone and the backend retains the unrelated UID.
    scenario.then_pair_deleted_and_other_unchanged().await;
    scenario.then_backend_calls(1);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_backend_failure_preserves_pair_and_accounts() {
    let scenario = DeletionScenario::given().await;
    scenario.given_backend_fails();
    let error = scenario.when_delete_subaccount().await.unwrap_err();
    assert!(error.to_string().contains("delete backend unavailable"));
    scenario.then_pair_and_accounts_present().await;
    scenario.then_backend_calls(1);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_retry_after_backend_failure_completes_cleanup() {
    let scenario = DeletionScenario::given().await;
    scenario.given_backend_fails();
    assert!(scenario.when_delete_subaccount().await.is_err());
    scenario.given_backend_recovers();
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_backend_calls(2);
    scenario.then_pair_deleted_and_other_unchanged().await;
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_repeated_success_does_not_repeat_backend_cleanup() {
    let scenario = DeletionScenario::given().await;
    scenario.when_delete_subaccount().await.unwrap();
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_pair_deleted_and_other_unchanged().await;
    scenario.then_backend_calls(1);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_cleanup_persistence_failure_keeps_retry_facts() {
    let scenario = DeletionScenario::given().await;
    scenario.given_cleanup_persistence_fails().await;
    let result = scenario.when_delete_subaccount().await;
    scenario.given_cleanup_persistence_recovers().await;
    assert!(result.is_err());
    scenario.then_pair_and_accounts_present().await;
    scenario.given_backend_recovers();
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_pair_deleted_and_other_unchanged().await;
    scenario.then_backend_calls(2);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_concurrent_repeated_requests_cleanup_once() {
    let scenario = DeletionScenario::given().await;
    let (first, second) =
        tokio::join!(scenario.when_delete_subaccount(), scenario.when_delete_subaccount(),);
    first.unwrap();
    second.unwrap();
    scenario.then_pair_deleted_and_other_unchanged().await;
    scenario.then_backend_calls(1);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_inconsistent_binding_does_not_delete_other_wallets() {
    let scenario = DeletionScenario::given().await;
    scenario.given_partner_points_elsewhere().await;
    assert!(scenario.when_delete_subaccount().await.is_err());
    scenario.then_pair_and_accounts_present().await;
    scenario.then_backend_calls(0);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_retry_finishes_device_cleanup_after_local_commit() {
    let scenario = DeletionScenario::given().await;
    scenario.given_device_uid_write_fails().await;
    let result = scenario.when_delete_subaccount().await;
    scenario.given_device_uid_write_recovers().await;
    assert!(result.is_err());
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_device_uid_references_surviving_wallet().await;
    scenario.then_pair_deleted_and_other_unchanged().await;
    scenario.then_backend_calls(1);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_concurrent_new_account_is_not_deleted_without_cleanup() {
    let scenario = DeletionScenario::given().await;
    assert!(
        scenario.when_account_arrives_during_backend().await.is_err(),
        "changed account set must roll back and remain retryable"
    );
    scenario.then_late_account_and_pair_survive().await;
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_retry_retains_cleanup_for_late_account().await;
    scenario.then_backend_calls(2);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_concurrent_binding_change_preserves_stale_pair() {
    let scenario = DeletionScenario::given().await;
    assert!(
        scenario.when_binding_changes_during_backend().await.is_err(),
        "changed wallet relation must roll back deletion"
    );
    scenario.then_pair_survives_local_rollback().await;
    scenario.then_backend_calls(1);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_public_rebinding_waits_and_cannot_rebind_deleted_origin() {
    let scenario = DeletionScenario::given().await;
    scenario.when_public_rebinding_waits_for_delete().await;
    scenario.then_pair_deleted_and_other_unchanged().await;
    scenario.then_backend_calls(1);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_cleanup_requests_are_bounded_and_durable() {
    let scenario = DeletionScenario::given().await;
    scenario.given_many_addresses(2500).await;
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_bounded_cleanup_covers_addresses(2504).await;
}

#[tokio::test]
#[serial(import_bind)]
#[ignore = "explicit 500,000-address SQLite/fake-backend scale validation"]
async fn wallet_deletion_500000_addresses_have_bounded_durable_cleanup() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("off,wallet_deletion_perf=info")
        .with_ansi(false)
        .with_test_writer()
        .try_init();
    let scenario = DeletionScenario::given().await;
    scenario.given_many_addresses(499_996).await;
    let started = std::time::Instant::now();
    scenario.when_delete_subaccount().await.unwrap();
    println!("WALLET-2980 deletion addresses=500000 elapsed_ms={}", started.elapsed().as_millis());
    scenario.then_bounded_cleanup_covers_addresses(500_000).await;
    scenario.then_backend_calls(1);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_cleanup_requests_bound_escaped_payload_bytes() {
    let scenario = DeletionScenario::given().await;
    scenario.given_many_addresses(1100).await;
    scenario.given_long_addresses(1024).await;
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_bounded_cleanup_covers_addresses(1104).await;
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_partial_cleanup_persistence_failure_is_retryable() {
    let scenario = DeletionScenario::given().await;
    scenario.given_many_addresses(20500).await;
    scenario.given_cleanup_persistence_fails_after_16_tasks().await;
    let result = scenario.when_delete_subaccount().await;
    scenario.given_cleanup_persistence_recovers().await;
    assert!(result.is_err());
    scenario.then_large_pair_survives_partial_persistence(20504).await;
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_bounded_cleanup_covers_addresses(20504).await;
    scenario.then_backend_calls(2);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_single_oversized_address_keeps_retry_facts() {
    let scenario = DeletionScenario::given().await;
    scenario.given_many_addresses(1).await;
    scenario.given_long_addresses(70_000).await;
    let error = scenario.when_delete_subaccount().await.unwrap_err();
    assert!(error.to_string().contains("exceeds request limit"));
    scenario.then_large_pair_present_without_cleanup(5).await;
    scenario.then_backend_calls(1);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_backend_timeout_releases_read_snapshot_and_retries() {
    let scenario = DeletionScenario::given().await;
    scenario.when_backend_timeout_releases_snapshot().await;
    scenario.then_pair_and_accounts_present().await;
    scenario.given_backend_recovers();
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_pair_deleted_and_other_unchanged().await;
    scenario.then_backend_calls(2);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_caller_deadline_releases_read_snapshot_and_retries() {
    let scenario = DeletionScenario::given().await;
    scenario.when_caller_cancels_releases_snapshot(false).await;
    scenario.then_pair_and_accounts_present().await;
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_pair_deleted_and_other_unchanged().await;
    scenario.then_backend_calls(2);
}

#[tokio::test]
#[serial(import_bind)]
async fn wallet_deletion_abort_releases_read_snapshot_and_retries() {
    let scenario = DeletionScenario::given().await;
    scenario.when_caller_cancels_releases_snapshot(true).await;
    scenario.then_pair_and_accounts_present().await;
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_pair_deleted_and_other_unchanged().await;
    scenario.then_backend_calls(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial(import_bind)]
#[ignore = "standalone 500,000-address deletion reader/writer latency experiment"]
async fn wallet_deletion_500000_addresses_read_write_waits() {
    let phases = support::deletion_phase_trace();
    let scenario = DeletionScenario::given().await;
    scenario.given_many_addresses(499_996).await;
    scenario.when_delete_with_read_write_probes(phases).await;
    scenario.then_bounded_cleanup_covers_addresses(500_000).await;
    scenario.then_backend_calls(1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial(import_bind)]
#[ignore = "standalone synthetic deletion scale characterization"]
async fn wallet_deletion_normal_scale_timing() {
    let (addresses, mode) = support::normal_scale_parameters();
    let phases = support::deletion_phase_trace();
    let scenario = DeletionScenario::given().await;
    scenario.given_many_addresses(addresses - 4).await;
    scenario.when_normal_scale_timing(phases, addresses, &mode).await;
    scenario.then_bounded_cleanup_covers_addresses(addresses).await;
    scenario.then_backend_calls(1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial(import_bind)]
#[ignore = "standalone deletion timing with another wallet owning500000addresses"]
async fn wallet_deletion_mixed_scale_timing() {
    let (addresses, mode) = support::normal_scale_parameters();
    let phases = support::deletion_phase_trace();
    let scenario = DeletionScenario::given().await;
    let background = scenario.given_background_addresses().await;
    scenario.given_many_addresses(addresses - 4).await;
    scenario.when_normal_scale_timing(phases, addresses, &mode).await;
    scenario.then_bounded_cleanup_covers_addresses(addresses).await;
    scenario.then_background_unchanged(&background).await;
    scenario.then_backend_calls(1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial(import_bind)]
#[ignore = "standalone exclusive-address-table deletion timing"]
async fn wallet_deletion_exclusive_accounts_scale_timing() {
    let (addresses, mode) = support::normal_scale_parameters();
    let phases = support::deletion_phase_trace();
    let mut scenario = DeletionScenario::given().await;
    scenario.given_all_addresses_belong_to_pair().await;
    scenario.given_many_addresses(addresses - 4).await;
    scenario.when_normal_scale_timing(phases, addresses, &mode).await;
    scenario.then_bounded_cleanup_covers_addresses(addresses).await;
    scenario.then_backend_calls(1);
}
