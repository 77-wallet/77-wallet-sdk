mod support;
use serial_test::serial;
use support::ChainSyncScenario;

#[tokio::test]
#[serial]
async fn valid_backend_chain_response_wins_over_dev_mainnet_defaults() -> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_backend_disables_polygon_and_gates_base();
    scenario.when_chain_sync_runs().await?;
    scenario.then_disabled_chains_stay_disabled_after_reinitialize().await?;
    scenario.then_one_chain_request_was_sent();
    Ok(())
}

#[tokio::test]
#[serial]
async fn coin_protocol_failure_is_visible_and_valid_empty_response_can_fill_dev_native_coins()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_no_coins_and_invalid_coin_response().await?;
    let error = scenario.when_coin_sync_runs().await.unwrap_err();
    scenario.then_coin_failure_has_no_default_write(error).await?;
    scenario.given_valid_empty_coin_response();
    scenario.when_coin_sync_runs().await?;
    scenario.then_five_native_coins_are_present().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn failed_address_registration_remains_retryable_and_stale_epoch_does_not_call_backend()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_five_unregistered_addresses_and_backend_failure().await?;
    scenario.when_address_registration_runs(777).await.unwrap_err();
    scenario.then_all_addresses_have_init_state(0).await?;
    scenario.given_backend_registration_success();
    scenario.when_address_registration_runs(777).await?;
    scenario.then_all_addresses_have_init_state(1).await?;
    scenario.then_registration_call_count_is(2);
    scenario.when_address_registration_runs(776).await?;
    scenario.then_registration_call_count_is(2);
    Ok(())
}

#[tokio::test]
#[serial]
async fn large_registration_respects_item_and_payload_limits_for_both_account_types()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_many_unregistered_addresses(2001, 1200).await?;
    scenario.when_bounded_registration_runs().await?;
    scenario.then_registration_is_bounded_and_complete(2001).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn missing_chain_sync_preserves_existing_account_semantics_and_is_repeatable()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(201).await?;
    scenario.when_concurrent_account_syncs_run().await?;
    scenario.then_existing_indices_types_and_names_are_complete(201).await?;
    let calls = scenario.registration_call_count();
    assert_eq!(calls, 55, "concurrent runs must not duplicate registered facts");
    scenario.when_existing_accounts_sync_runs().await?;
    assert_eq!(scenario.registration_call_count(), calls);
    Ok(())
}
#[tokio::test]
#[serial]
async fn missing_chain_partial_registration_failure_retries_only_unfinished_facts()
-> anyhow::Result<()> {
    let mut scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(101).await?;
    scenario.given_registration_succeeds_once_then_fails();
    scenario.when_existing_accounts_sync_runs().await?;
    scenario.then_failed_queue_contains("synthetic transient failure").await?;
    scenario.then_failure_preserves_partial_completion().await?;
    scenario.when_context_is_reopened().await?;
    scenario.given_many_registration_successes(100);
    scenario.when_existing_accounts_sync_runs().await?;
    scenario.when_retry_deadlines_arrive().await?;
    scenario.then_existing_indices_types_and_names_are_complete(101).await?;
    Ok(())
}
#[tokio::test]
#[serial]
async fn stale_registration_response_after_epoch_change_does_not_initialize_addresses()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_five_unregistered_addresses_and_backend_failure().await?;
    // Consume the seeded failure before the controlled successful response.
    scenario.when_address_registration_runs(777).await.unwrap_err();
    scenario.when_reset_epoch_changes_during_registration().await?;
    scenario.then_all_addresses_have_init_state(0).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn cancelled_missing_chain_registration_resumes_via_wallet_unlock_after_reopen()
-> anyhow::Result<()> {
    let mut scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(41).await?;
    scenario.when_in_flight_registration_is_cancelled().await?;
    scenario.when_context_is_reopened().await?;
    scenario.given_many_registration_successes(100);
    scenario.when_wallet_unlock_starts_recovery(41).await?;
    assert!(scenario.registration_call_count() >= 16);
    scenario.then_existing_indices_types_and_names_are_complete(41).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn repeated_unlock_reuses_durable_jobs_and_same_context_reunlock_recovers()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(1).await?;
    scenario.when_wallet_unlock_starts_recovery(1).await?;
    scenario.when_same_context_recovery_lifecycle_is_exercised().await?;
    scenario.then_existing_indices_types_and_names_are_complete(1).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn persistent_first_chain_failure_allows_other_chains_and_retries_only_pending()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(1).await?;
    for attempt in 0..2 {
        scenario.given_first_chain_registration_failure();
        if attempt == 0 {
            scenario.when_existing_accounts_sync_runs().await?;
        } else {
            scenario.when_retry_deadlines_arrive().await?;
        }
        scenario.then_failed_queue_contains("synthetic first chain failure").await?;
        scenario.then_first_chain_pending_and_other_chains_complete().await?;
    }
    scenario.given_only_registration_successes();
    scenario.when_retry_deadlines_arrive().await?;
    scenario.then_registration_call_count_is(1);
    scenario.then_existing_indices_types_and_names_are_complete(1).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn every_chain_registration_failure_reports_first_and_keeps_all_pending() -> anyhow::Result<()>
{
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(1).await?;
    scenario.given_each_chain_registration_failure();
    scenario.when_existing_accounts_sync_runs().await?;
    scenario.then_failed_queue_contains("synthetic first chain failure").await?;
    scenario.then_all_rollout_chains_pending().await?;
    scenario.then_registration_call_count_is(5);
    scenario.given_only_registration_successes();
    scenario.when_retry_deadlines_arrive().await?;
    scenario.then_existing_indices_types_and_names_are_complete(1).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn epoch_change_during_missing_chain_registration_stops_following_chain_writes()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(1).await?;
    scenario.when_epoch_changes_during_existing_account_sync().await?;
    scenario.then_derived_addresses_stay_pending().await?;
    scenario.then_registration_call_count_is(1);
    Ok(())
}

#[tokio::test]
#[serial]
async fn registration_database_marker_failure_keeps_each_chain_retryable() -> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(1).await?;
    scenario.given_registration_marker_database_failure().await?;
    scenario.when_existing_accounts_sync_runs().await?;
    scenario.then_failed_queue_contains("synthetic init marker failure").await?;
    scenario.then_derived_addresses_stay_pending().await?;
    scenario.then_registration_call_count_is(5);
    Ok(())
}

#[tokio::test]
#[serial]
async fn epoch_stop_keeps_an_already_observed_first_registration_error() -> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(1).await?;
    scenario.given_first_chain_registration_failure();
    scenario.when_epoch_changes_during_existing_account_sync().await?;
    scenario.then_failed_queue_contains("synthetic first chain failure").await?;
    scenario.then_derived_addresses_stay_pending().await?;
    scenario.then_registration_call_count_is(1);
    Ok(())
}

#[tokio::test]
#[serial]
async fn first_chain_registration_timeout_leaves_facts_retryable_and_later_chains_complete()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(1).await?;
    scenario.when_first_registration_times_out().await?;
    scenario.then_first_chain_pending_and_other_chains_complete().await?;
    scenario.given_only_registration_successes();
    scenario.when_retry_deadlines_arrive().await?;
    scenario.then_registration_call_count_is(1);
    scenario.then_existing_indices_types_and_names_are_complete(1).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn missing_chain_registration_is_persisted_before_transport_runs() -> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(1).await?;

    scenario.when_account_sync_attempts_registration_dispatch().await?;

    scenario.then_registration_is_durable_without_transport().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn rejected_address_is_localized_without_blocking_healthy_batch_members() -> anyhow::Result<()>
{
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_one_rejected_address().await?;
    scenario.when_registration_is_planned().await?;
    scenario.when_saved_tasks_are_consumed().await?;
    scenario.then_only_rejected_address_is_pending().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn same_coordinator_recovers_after_twelve_failures_and_default_consumer_cannot_execute_owned_rows()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_many_unregistered_addresses(8, 10).await?;
    scenario.when_registration_is_planned().await?;
    scenario.when_same_coordinator_recovers_after_twelve_failures().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn completed_stable_registration_id_reactivates_only_for_pending_facts() -> anyhow::Result<()>
{
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_many_unregistered_addresses(8, 10).await?;
    scenario.when_registration_is_planned().await?;
    scenario.when_saved_tasks_are_consumed().await?;
    scenario.when_completed_delivery_is_duplicated().await?;
    scenario.when_finished_stable_tasks_are_reactivated().await?;
    scenario.then_registration_call_count_is(2);
    Ok(())
}

#[tokio::test]
#[serial]
async fn pre_enqueue_gap_and_uncreated_future_pages_resume_from_durable_parent()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(201).await?;
    scenario.when_registration_insert_gap_then_later_pages_resume().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn oversized_item_stays_pending_while_healthy_chunks_and_future_pages_complete()
-> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.given_existing_accounts_of_both_types(101).await?;
    scenario.given_oversized_source_name().await?;
    scenario.when_existing_accounts_sync_runs().await?;
    scenario.then_oversized_item_does_not_block_healthy_chunks_or_future_pages().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn unknown_external_owner_is_retained_before_generic_name_parsing() -> anyhow::Result<()> {
    let scenario = ChainSyncScenario::new().await?;
    scenario.when_unknown_external_owner_is_delivered().await?;
    scenario.then_registration_call_count_is(0);
    Ok(())
}
