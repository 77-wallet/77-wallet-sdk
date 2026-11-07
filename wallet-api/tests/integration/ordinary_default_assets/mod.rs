mod support;
use serial_test::serial;
use support::OrdinaryDefaultAssetsScenario;

#[tokio::test]
#[serial]
async fn ordinary_default_assets_wait_for_last_page_then_fill_and_repeat_safely()
-> anyhow::Result<()> {
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_two_successful_pages().await;
    let gate = scenario.given_last_page_paused();
    let task = scenario.when_sync_starts();
    gate.wait_for_request().await;
    scenario.then_contract_assets_are(0).await?;
    scenario.then_progress_is(None).await?;
    gate.release();
    task.await??;
    scenario.then_contract_assets_are(2).await?;
    scenario.then_progress_is(Some("2026-10-09T00:00:00Z")).await?;
    scenario.then_api_assets_are_unchanged().await?;
    scenario.then_refresh_signals_are(1);
    scenario.given_two_successful_pages().await;
    scenario.when_sync_runs().await?;
    scenario.then_contract_assets_are(2).await?;
    scenario.then_only_catalog_endpoints_were_called();
    scenario.then_refresh_signals_are(0);
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_default_assets_failed_later_page_writes_no_partial_assets_or_coins()
-> anyhow::Result<()> {
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_large_legacy_catalog().await?;
    scenario.given_failed_last_page().await;
    assert!(scenario.when_sync_runs().await.is_err());
    scenario.then_contract_assets_are(0).await?;
    scenario.then_contract_coins_are(0).await?;
    scenario.then_legacy_metadata_is_unchanged().await?;
    scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
    scenario.given_two_successful_pages().await;
    scenario.when_sync_runs().await?;
    scenario.then_contract_assets_are(2).await?;
    scenario.then_legacy_repair_and_new_coin_are_persisted().await?;
    scenario.then_only_separate_incremental_queries();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_default_assets_failed_catalog_completion_defers_fill_until_retry()
-> anyhow::Result<()> {
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_failed_catalog_completion().await;
    assert!(scenario.when_sync_runs().await.is_err());
    scenario.then_contract_coins_are(2).await?;
    scenario.then_contract_assets_are(0).await?;
    scenario.given_two_successful_pages().await;
    scenario.when_sync_runs().await?;
    scenario.then_contract_assets_are(2).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_default_assets_cancelled_last_page_does_not_write_or_notify() -> anyhow::Result<()>
{
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_large_legacy_catalog().await?;
    scenario.given_catalog_pages_without_completion().await;
    let gate = scenario.given_last_page_paused();
    let task = scenario.when_sync_starts();
    gate.wait_for_request().await;
    task.abort();
    gate.release();
    assert!(task.await.unwrap_err().is_cancelled());
    scenario.then_contract_coins_are(0).await?;
    scenario.then_contract_assets_are(0).await?;
    scenario.then_refresh_signals_are(0);
    scenario.then_legacy_metadata_is_unchanged().await?;
    scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
    scenario.given_two_successful_pages().await;
    scenario.when_sync_runs().await?;
    scenario.then_legacy_repair_and_new_coin_are_persisted().await?;
    scenario.then_only_separate_incremental_queries();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_catalog_incremental_fetches_future_old_updates_and_new_coins_above_500()
-> anyhow::Result<()> {
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_large_legacy_catalog().await?;
    scenario.given_untrusted_local_update_time().await?;
    for _ in 0..2 {
        scenario.given_two_successful_pages().await;
        scenario.when_sync_runs().await?;
        scenario.then_legacy_repair_and_new_coin_are_persisted().await?;
        scenario.then_contract_assets_are(2).await?;
    }
    scenario.then_api_assets_are_unchanged().await?;
    scenario.then_only_separate_incremental_queries();
    scenario.then_initial_filters_use_creation_baseline();
    scenario.then_successful_boundary_is_replayed();
    scenario.then_progress_is(Some("2026-10-09T00:00:00Z")).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_catalog_does_not_backfill_history_but_fetches_later_updates() -> anyhow::Result<()>
{
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_large_legacy_catalog().await?;
    scenario.given_empty_incremental_responses();
    scenario.when_sync_runs().await?;
    scenario.then_legacy_metadata_is_unchanged().await?;
    scenario.then_contract_assets_are(0).await?;
    scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
    scenario.then_directory_request_count_is(2);
    scenario.then_initial_filters_use_creation_baseline();
    scenario.given_two_successful_pages().await;
    scenario.when_sync_runs().await?;
    scenario.then_legacy_repair_and_new_coin_are_persisted().await?;
    scenario.then_only_separate_incremental_queries();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_catalog_request_and_creation_route_failures_do_not_advance() -> anyhow::Result<()>
{
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_large_legacy_catalog().await?;
    scenario.given_first_request_failure();
    assert!(scenario.when_sync_runs().await.is_err());
    scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
    scenario.given_creation_route_failure();
    assert!(scenario.when_sync_runs().await.is_err());
    scenario.then_legacy_metadata_is_unchanged().await?;
    scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
    scenario.given_two_successful_pages().await;
    scenario.when_sync_runs().await?;
    scenario.then_legacy_repair_and_new_coin_are_persisted().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_catalog_metadata_and_progress_failures_replay_old_window() -> anyhow::Result<()> {
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_large_legacy_catalog().await?;
    scenario.given_metadata_write_failure().await?;
    scenario.given_two_successful_pages().await;
    assert!(scenario.when_sync_runs().await.is_err());
    scenario.then_legacy_metadata_is_unchanged().await?;
    scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
    scenario.given_write_failures_removed().await?;
    scenario.given_progress_save_failure().await?;
    scenario.given_two_successful_pages().await;
    assert!(scenario.when_sync_runs().await.is_err());
    scenario.then_legacy_repair_and_new_coin_are_persisted().await?;
    scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
    scenario.given_write_failures_removed().await?;
    scenario.given_two_successful_pages().await;
    scenario.when_sync_runs().await?;
    scenario.then_progress_is(Some("2026-10-09T00:00:00Z")).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_catalog_serializes_overlapping_syncs_before_cursor_read() -> anyhow::Result<()> {
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_large_legacy_catalog().await?;
    scenario.given_two_successful_pages().await;
    scenario.given_two_successful_pages().await;
    let gate = scenario.given_last_page_paused();
    let first = scenario.when_sync_starts();
    gate.wait_for_request().await;
    let second = scenario.when_sync_starts();
    tokio::task::yield_now().await;
    scenario.then_directory_request_count_is(2);
    scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
    gate.release();
    first.await??;
    second.await??;
    scenario.then_successful_boundary_is_replayed();
    scenario.then_progress_is(Some("2026-10-09T00:00:00Z")).await?;
    scenario.then_legacy_repair_and_new_coin_are_persisted().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_catalog_overlapping_queries_keep_newest_server_version() -> anyhow::Result<()> {
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_large_legacy_catalog().await?;
    scenario.given_overlapping_different_versions();
    scenario.when_sync_runs().await?;
    scenario.then_newest_overlap_is_persisted().await?;
    scenario.then_progress_times_are("2026-10-09T00:00:00Z", "2026-10-09T00:00:02Z").await?;
    scenario.then_only_separate_incremental_queries();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_catalog_invalid_server_time_preserves_cursor_until_retry() -> anyhow::Result<()> {
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_large_legacy_catalog().await?;
    scenario.given_invalid_server_timestamp();
    assert!(scenario.when_sync_runs().await.is_err());
    scenario.then_legacy_metadata_is_unchanged().await?;
    scenario.then_contract_coins_are(0).await?;
    scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
    scenario.given_two_successful_pages().await;
    scenario.when_sync_runs().await?;
    scenario.then_legacy_repair_and_new_coin_are_persisted().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_catalog_rebuilt_context_restores_distinct_successful_watermarks()
-> anyhow::Result<()> {
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_overlapping_different_versions();
    scenario.given_large_legacy_catalog().await?;
    scenario.when_sync_runs().await?;
    let scenario = scenario.when_context_is_rebuilt().await?;
    scenario.then_progress_times_are("2026-10-09T00:00:00Z", "2026-10-09T00:00:02Z").await?;
    scenario.given_empty_incremental_responses();
    scenario.when_sync_runs().await?;
    scenario.then_last_filters_are("2026-10-08 23:59:58", "2026-10-09 00:00:00");
    scenario.then_newest_overlap_is_persisted().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_catalog_rebuilt_context_retries_frozen_baseline_after_network_failure()
-> anyhow::Result<()> {
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_large_legacy_catalog().await?;
    scenario.given_untrusted_local_update_time().await?;
    scenario.given_first_request_failure();
    assert!(scenario.when_sync_runs().await.is_err());
    let scenario = scenario.when_context_is_rebuilt().await?;
    scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
    scenario.given_empty_incremental_responses();
    scenario.when_sync_runs().await?;
    scenario.then_last_filters_are("2026-10-07 23:59:58", "2026-10-07 23:59:58");
    scenario.then_legacy_metadata_is_unchanged().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_catalog_rebuilt_context_avoids_reinitialization_and_fetches_future_updates()
-> anyhow::Result<()> {
    let scenario = OrdinaryDefaultAssetsScenario::new().await?;
    scenario.given_two_successful_pages().await;
    scenario.when_sync_runs().await?;
    let scenario = scenario.when_context_is_rebuilt().await?;
    scenario.then_progress_is(Some("2026-10-09T00:00:00Z")).await?;
    scenario.given_large_legacy_catalog().await?;
    scenario.given_untrusted_local_update_time().await?;
    scenario.given_overlapping_different_versions();
    scenario.when_sync_runs().await?;
    scenario.then_last_filters_are("2026-10-08 23:59:58", "2026-10-08 23:59:58");
    scenario.then_newest_overlap_is_persisted().await?;
    scenario.then_progress_times_are("2026-10-09T00:00:00Z", "2026-10-09T00:00:02Z").await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_catalog_process_exit_restores_persisted_watermarks() -> anyhow::Result<()> {
    if let Ok(phase) = std::env::var("CATALOG_RESTART_TEST_PHASE") {
        let scenario = OrdinaryDefaultAssetsScenario::new().await?;
        if phase == "initialize" {
            scenario.given_large_legacy_catalog().await?;
            scenario.given_untrusted_local_update_time().await?;
            scenario.given_overlapping_different_versions();
            scenario.when_sync_runs().await?;
            scenario
                .then_progress_times_are("2026-10-09T00:00:00Z", "2026-10-09T00:00:02Z")
                .await?;
        } else if phase == "failed_initialize" {
            scenario.given_large_legacy_catalog().await?;
            scenario.given_untrusted_local_update_time().await?;
            scenario.given_first_request_failure();
            assert!(scenario.when_sync_runs().await.is_err());
            scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
        } else if phase == "resume_failure" {
            scenario.then_progress_is(Some("2026-10-08T00:00:00Z")).await?;
            scenario.given_empty_incremental_responses();
            scenario.when_sync_runs().await?;
            scenario.then_last_filters_are("2026-10-07 23:59:58", "2026-10-07 23:59:58");
        } else {
            assert_eq!(phase, "resume");
            scenario
                .then_progress_times_are("2026-10-09T00:00:00Z", "2026-10-09T00:00:02Z")
                .await?;
            scenario.given_empty_incremental_responses();
            scenario.when_sync_runs().await?;
            scenario.then_last_filters_are("2026-10-08 23:59:58", "2026-10-09 00:00:00");
            scenario.then_newest_overlap_is_persisted().await?;
        }
        return Ok(());
    }
    let directory = tempfile::tempdir()?;
    let failure_directory = tempfile::tempdir()?;
    let executable = std::env::current_exe()?;
    for phase in ["initialize", "resume", "failed_initialize", "resume_failure"] {
        let path = if phase.contains("failure") || phase == "failed_initialize" {
            failure_directory.path()
        } else {
            directory.path()
        };
        let output = std::process::Command::new(&executable)
            .arg("integration::ordinary_default_assets::ordinary_catalog_process_exit_restores_persisted_watermarks")
            .arg("--exact").arg("--test-threads=1")
            .env("CATALOG_RESTART_TEST_DIR", path)
            .env("CATALOG_RESTART_TEST_PHASE", phase)
            .output()?;
        assert!(
            output.status.success(),
            "child {phase} failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        // The initialize child has exited: its Context and every OS connection
        // are gone before a fresh process opens the same SQLite files.
    }
    Ok(())
}
