mod support;

use serial_test::serial;

use support::{
    ScenarioRoles, TransferNonceGiven, TransferNonceScenario, TransferNonceThen, TransferNonceWhen,
};

#[tokio::test]
#[serial]
async fn api_wallet_transfer_nonce_lock_keeps_same_address_requests_serial() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.bnb_transfer_fixture().await?;
    given.first_transfer_blocks();
    given.fake_chain_adapter();
    given.wallet_password_cached().await;

    let first = when.transfer_starts();
    then.first_transfer_has_entered().await;
    scenario.then_durable_transfer_is(1, false).await?;

    let second = when.transfer_starts();
    then.only_first_nonce_is_recorded_while_second_waits().await;

    when.first_transfer_is_released();

    then.serial_transfer_results_are(first, second).await?;
    scenario.then_durable_transfer_is(1, true).await?;
    scenario.then_durable_transfer_is(2, true).await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn api_wallet_transfer_nonce_failure_keeps_reserved_nonce() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.bnb_transfer_fixture().await?;
    given.transfer_fails();
    given.fake_chain_adapter();
    given.wallet_password_cached().await;

    let err = when.transfer_fails().await;

    then.failure_keeps_reserved_nonce(err).await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn shared_candidate_nonce_skips_consumed_without_reserving() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(2));

    let nonce = scenario.when_shared_nonce_is_observed().await?;

    assert_eq!(nonce, 2);
    scenario.then_nonce_floor_and_queries_are(0, 1).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn shared_candidate_nonce_stale_chain_response_does_not_advance_counter() -> anyhow::Result<()>
{
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(2));
    assert_eq!(scenario.when_shared_nonce_is_observed().await?, 2);
    scenario.given_chain_next_nonce(Some(0));
    assert!(
        scenario
            .when_shared_nonce_is_observed()
            .await
            .unwrap_err()
            .to_string()
            .contains("nonce_gap_requires_recovery")
    );
    scenario.then_nonce_floor_and_queries_are(0, 2).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_nonce_rpc_failure_does_not_reserve_nonce() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(None);

    let error = scenario.when_shared_nonce_is_observed().await.unwrap_err();

    assert!(error.to_string().contains("simulated nonce RPC failure"));
    scenario.then_nonce_floor_and_queries_are(0, 1).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn shared_candidate_nonce_concurrent_observations_do_not_reserve() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(2));

    let (first, second) = tokio::join!(
        scenario.when_shared_nonce_is_observed(),
        scenario.when_shared_nonce_is_observed(),
    );

    let mut nonces = [first?, second?];
    nonces.sort();
    assert_eq!(nonces, [2, 2]);
    scenario.then_nonce_floor_and_queries_are(0, 2).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn nonce_alignment_preserves_reserved_nonces() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(9));
    scenario.given_nonce_floor(11).await?;
    let alignment = scenario.when_nonce_alignment_runs().await;
    let next = scenario.when_shared_nonce_is_observed().await;
    assert!(alignment.unwrap_err().to_string().contains("nonce_gap_requires_recovery"));
    assert!(next.unwrap_err().to_string().contains("nonce_gap_requires_recovery"));
    scenario.then_nonce_floor_and_queries_are(11, 2).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn nonce_alignment_and_observation_preserve_reservations() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(9));
    scenario.given_nonce_floor(9).await?;
    let (alignment, observation) = tokio::join!(
        scenario.when_nonce_alignment_runs(),
        scenario.when_shared_nonce_is_observed(),
    );
    assert!(alignment.unwrap_err().to_string().contains("nonce_gap_requires_recovery"));
    assert!(observation.unwrap_err().to_string().contains("nonce_gap_requires_recovery"));
    scenario.then_nonce_floor_and_queries_are(9, 2).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn nonce_alignment_advances_floor_without_reserving() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(9));

    assert_eq!(scenario.when_nonce_alignment_runs().await?, 9);

    scenario.then_nonce_floor_and_queries_are(8, 1).await;
    assert_eq!(scenario.when_shared_nonce_is_observed().await?, 9);
    Ok(())
}

#[tokio::test]
#[serial]
async fn nonce_alignment_rpc_failure_preserves_floor() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(None);

    let error = scenario.when_nonce_alignment_runs().await.unwrap_err();

    assert!(error.to_string().contains("simulated nonce RPC failure"));
    scenario.then_nonce_floor_and_queries_are(0, 1).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn nonce_alignment_frozen_address_does_not_query_or_mutate() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(9));
    scenario.given_nonce_allocation_frozen(true);

    let result = scenario.when_nonce_alignment_runs().await;
    scenario.given_nonce_allocation_frozen(false);

    assert!(result.unwrap_err().to_string().contains("frozen"));
    scenario.then_nonce_floor_and_queries_are(0, 0).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_nonce_syncs_external_chain_progress() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(Some(9));

    let first = scenario.when().transfer_starts().await??;
    scenario.given_chain_next_nonce(Some(15));
    let second = scenario.when().transfer_starts().await??;

    assert_eq!(first, format!("0x{:064x}", 9));
    assert_eq!(second, format!("0x{:064x}", 15));
    scenario.then_transfer_nonces_and_floor_are(&[9, 15], 15).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_nonce_rpc_failure_recovers_without_sending() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(None);

    let result = scenario.when().transfer_starts().await?;

    assert!(result.unwrap_err().to_string().contains("simulated nonce RPC failure"));
    scenario.then_nonce_floor_and_queries_are(0, 1).await;

    scenario.given_chain_next_nonce(Some(9));
    let hash = scenario.when().transfer_starts().await??;
    assert_eq!(hash, format!("0x{:064x}", 9));
    scenario.then_transfer_nonces_and_floor_are(&[9], 9).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_nonce_syncs_chain_progress() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(9));

    let first = scenario.when_withdrawal_nonce_is_observed().await?;
    scenario.given_chain_next_nonce(Some(15));
    let second = scenario.when_withdrawal_nonce_is_observed().await?;

    assert_eq!([first, second], [9, 15]);
    scenario.then_nonce_floor_and_queries_are(0, 2).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_nonce_rpc_failure_recovers_without_reservation() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(None);

    let error = scenario.when_withdrawal_nonce_is_observed().await.unwrap_err();

    assert!(error.to_string().contains("simulated nonce RPC failure"));
    scenario.then_nonce_floor_and_queries_are(0, 1).await;
    scenario.given_chain_next_nonce(Some(9));
    assert_eq!(scenario.when_withdrawal_nonce_is_observed().await?, 9);
    scenario.then_nonce_floor_and_queries_are(0, 2).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_and_shared_observations_do_not_reserve() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(9));

    assert_eq!(scenario.when_withdrawal_nonce_is_observed().await?, 9);
    assert_eq!(scenario.when_shared_nonce_is_observed().await?, 9);
    scenario.then_nonce_floor_and_queries_are(0, 2).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_fee_nonce_syncs_chain_progress() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(9));

    let first = scenario.when_collect_fee_nonce_is_allocated().await?;
    scenario.given_chain_next_nonce(Some(15));
    let second = scenario.when_collect_fee_nonce_is_allocated().await?;

    assert_eq!([first, second], [9, 15]);
    scenario.then_nonce_floor_and_queries_are(0, 2).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_fee_nonce_rpc_failure_recovers_without_reservation() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(None);

    let error = scenario.when_collect_fee_nonce_is_allocated().await.unwrap_err();

    assert!(error.to_string().contains("simulated nonce RPC failure"));
    scenario.then_nonce_floor_and_queries_are(0, 1).await;
    scenario.given_chain_next_nonce(Some(9));
    assert_eq!(scenario.when_collect_fee_nonce_is_allocated().await?, 9);
    scenario.then_nonce_floor_and_queries_are(0, 2).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_fee_nonce_shared_observations_require_committed_ownership() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(9));

    // Observing an unsigned fee attempt twice must not consume either candidate.
    assert_eq!(scenario.when_collect_fee_nonce_is_allocated().await?, 9);
    assert_eq!(scenario.when_collect_fee_nonce_is_allocated().await?, 9);
    scenario.then_nonce_floor_and_queries_are(0, 2).await;
    assert_eq!(scenario.when_shared_nonce_is_observed().await?, 9);
    scenario.given_nonce_floor(9).await?;
    let error = scenario.when_collect_fee_nonce_is_allocated().await.unwrap_err();
    assert!(error.to_string().contains("nonce_gap_requires_recovery"));
    scenario.then_nonce_floor_and_queries_are(9, 4).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn nonce_fee_rejections_preserve_reservation_without_chain_repair() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(9));
    assert_eq!(scenario.when_shared_nonce_is_observed().await?, 9);
    scenario.given_nonce_floor(9).await?;
    scenario.given_chain_next_nonce(Some(50));

    for error in ["replacement transaction underpriced", "transaction underpriced"] {
        let result = scenario.when_nonce_error_is_handled(error).await;
        assert!(result.is_err(), "fee rejection must remain an error");
        scenario.then_nonce_floor_and_queries_are(9, 1).await;
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn nonce_known_transaction_preserves_reservation_without_chain_repair() -> anyhow::Result<()>
{
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given_chain_next_nonce(Some(9));
    assert_eq!(scenario.when_shared_nonce_is_observed().await?, 9);
    scenario.given_nonce_floor(9).await?;
    scenario.given_chain_next_nonce(Some(50));

    scenario.when_nonce_error_is_handled("already known").await?;

    scenario.then_nonce_floor_and_queries_are(9, 1).await;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_uncertain_nonce_blocks_next_allocation() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().transfer_fails();
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(Some(4));

    let first = scenario.when().transfer_fails().await;
    let second = scenario.when().transfer_fails().await;

    assert!(first.to_string().contains("simulated transfer failure"));
    assert!(second.to_string().contains("nonce_gap_requires_recovery"));
    scenario.then_transfer_nonces_and_floor_are(&[4], 4).await;
    scenario.then_durable_transfer_is(4, false).await?;

    scenario.given_transfer_can_succeed();
    scenario.given_chain_next_nonce(Some(5));
    assert_eq!(scenario.when().transfer_starts().await??, format!("0x{:064x}", 5));
    scenario.then_transfer_nonces_and_floor_are(&[4, 5], 5).await;
    scenario.then_durable_transfer_is(4, false).await?;
    scenario.then_durable_transfer_is(5, true).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_timeout_blocks_next_allocation() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(Some(4));
    scenario.given_transfer_times_out();

    let first = scenario.when().transfer_fails().await;
    let second = scenario.when().transfer_fails().await;

    assert!(first.to_string().contains("timeout"));
    assert!(second.to_string().contains("nonce_gap_requires_recovery"));
    scenario.then_transfer_nonces_and_floor_are(&[4], 4).await;
    scenario.then_durable_transfer_is(4, false).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_signed_persistence_failure_never_sends() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(Some(4));
    scenario.given_signed_persistence_fails().await?;

    let error = scenario.when().transfer_fails().await;
    let retry = scenario.when().transfer_fails().await;

    assert!(error.to_string().contains("simulated signed persistence failure"));
    assert!(retry.to_string().contains("simulated signed persistence failure"));
    scenario.then_transfer_nonces_and_floor_are(&[], 0).await;
    scenario.then_no_signed_transfer_exists().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_invisible_submission_returns_accepted_hash() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(Some(4));
    scenario.given_transfer_is_invisible();

    let hash = scenario.when().transfer_starts().await??;

    assert_eq!(hash, format!("0x{:064x}", 4));
    scenario.then_transfer_nonces_and_floor_are(&[4], 4).await;
    scenario.then_durable_transfer_is(4, true).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_wrong_response_hash_cannot_mark_accepted() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(Some(4));
    scenario.given_returned_hash_mismatches();

    let error = scenario.when().transfer_fails().await;

    assert!(error.to_string().contains("transfer_broadcast_hash_mismatch"));
    scenario.then_transfer_nonces_and_floor_are(&[4], 4).await;
    scenario.then_durable_transfer_is(4, false).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_acceptance_write_failure_preserves_signed_transaction()
-> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(Some(4));
    scenario.given_acceptance_persistence_fails().await?;

    let error = scenario.when().transfer_fails().await;

    assert!(error.to_string().contains("simulated acceptance persistence failure"));
    scenario.then_transfer_nonces_and_floor_are(&[4], 4).await;
    scenario.then_durable_transfer_is(4, false).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_observer_preserves_uncertainty_until_valid_receipt() -> anyhow::Result<()>
{
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(Some(20001));
    scenario.given_transfer_times_out();
    scenario.when().transfer_fails().await;

    for mode in [0, -1, 4, 5] {
        scenario.given_receipt_behavior(20001, mode);
        scenario.when_receipts_are_observed().await?;
        scenario.then_durable_transfer_is(20001, false).await?;
        scenario.then_transfer_nonces_and_floor_are(&[20001], 20001).await;
    }
    scenario.given_receipt_behavior(20001, 2);
    scenario.when_receipts_are_observed().await?;
    scenario.then_receipt_is_persisted(20001, true).await?;
    scenario.when_receipts_are_observed().await?;
    scenario.then_receipt_is_persisted(20001, true).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_observer_records_onchain_failure_without_resending() -> anyhow::Result<()>
{
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(Some(20002));
    scenario.when().transfer_starts().await??;
    scenario.given_receipt_behavior(20002, 3);

    scenario.when_receipts_are_observed().await?;

    scenario.then_receipt_is_persisted(20002, false).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_background_observer_cancels_restarts_and_retries() -> anyhow::Result<()>
{
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(Some(20003));
    scenario.given_transfer_times_out();
    scenario.when().transfer_fails().await;
    scenario.given_receipt_behavior(20003, -2);

    let blocked = scenario.when_observer_starts();
    scenario.then_observer_has_queried().await;
    scenario.when_observer_stops(blocked).await?;
    scenario.then_durable_transfer_is(20003, false).await?;

    scenario.given_receipt_behavior(20003, 0);
    let restarted = scenario.when_observer_starts();
    scenario.then_observer_has_queried().await;
    scenario.then_durable_transfer_is(20003, false).await?;
    scenario.given_receipt_behavior(20003, 2);
    scenario.then_background_receipt_finishes(20003).await?;
    scenario.when_observer_stops(restarted).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_rebroadcast_uses_saved_bytes_after_cooldown() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_rebroadcast_fixture();
    scenario.given_chain_next_nonce(Some(30001));
    scenario.given_transfer_times_out();
    scenario.when().transfer_fails().await;
    scenario.given_transfer_can_succeed();

    scenario.when_receipts_are_observed().await?;
    scenario.then_rebroadcast_counts_are(30001, 1, 0).await?;
    scenario.given_rebroadcast_cooldown_elapsed().await?;
    scenario.when_receipts_are_observed().await?;

    scenario.then_rebroadcast_counts_are(30001, 2, 1).await?;
    scenario.then_durable_transfer_is(30001, true).await?;
    scenario.when_receipts_are_observed().await?;
    scenario.then_rebroadcast_counts_are(30001, 2, 1).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_rebroadcast_waits_on_visibility_nonce_and_freeze() -> anyhow::Result<()>
{
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_rebroadcast_fixture();
    scenario.given_chain_next_nonce(Some(30002));
    scenario.given_transfer_times_out();
    scenario.when().transfer_fails().await;
    scenario.given_transfer_can_succeed();
    scenario.given_rebroadcast_cooldown_elapsed().await?;

    scenario.given_node_visibility(true, false);
    scenario.when_receipts_are_observed().await?;
    scenario.then_rebroadcast_counts_are(30002, 1, 0).await?;
    scenario.given_node_visibility(false, false);
    for next in [Some(30003), Some(30001), None] {
        scenario.given_chain_next_nonce(next);
        scenario.when_receipts_are_observed().await?;
        scenario.then_rebroadcast_counts_are(30002, 1, 0).await?;
    }
    scenario.given_chain_next_nonce(Some(30002));
    scenario.given_node_visibility(false, true);
    scenario.when_receipts_are_observed().await?;
    scenario.then_rebroadcast_counts_are(30002, 1, 0).await?;
    scenario.given_node_visibility(false, false);
    scenario.given_nonce_allocation_frozen(true);
    scenario.when_receipts_are_observed().await?;
    scenario.given_nonce_allocation_frozen(false);
    scenario.then_rebroadcast_counts_are(30002, 1, 0).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_rebroadcast_timeouts_consume_bounded_budget() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_rebroadcast_fixture();
    scenario.given_chain_next_nonce(Some(30003));
    scenario.given_transfer_times_out();
    scenario.when().transfer_fails().await;

    for retries in 1..=3 {
        scenario.given_rebroadcast_cooldown_elapsed().await?;
        scenario.when_receipts_are_observed().await?;
        scenario.then_rebroadcast_counts_are(30003, retries as usize + 1, retries).await?;
        scenario.then_durable_transfer_is(30003, false).await?;
    }
    scenario.given_rebroadcast_cooldown_elapsed().await?;
    scenario.when_receipts_are_observed().await?;
    scenario.then_rebroadcast_counts_are(30003, 4, 3).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_rebroadcast_rejects_corrupted_saved_raw() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_rebroadcast_fixture();
    scenario.given_chain_next_nonce(Some(30004));
    scenario.given_transfer_times_out();
    scenario.when().transfer_fails().await;
    scenario.given_transfer_can_succeed();
    scenario.given_rebroadcast_cooldown_elapsed().await?;
    scenario.given_saved_raw_corrupted().await?;

    scenario.when_receipts_are_observed().await?;

    scenario.then_rebroadcast_counts_are(30004, 1, 0).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_rebroadcast_cancellation_preserves_claim_on_restart()
-> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_rebroadcast_fixture();
    scenario.given_chain_next_nonce(Some(30005));
    scenario.given_transfer_times_out();
    scenario.when().transfer_fails().await;
    scenario.given_rebroadcast_cooldown_elapsed().await?;
    scenario.given_broadcast_hangs();

    let observer = scenario.when_observer_starts();
    scenario.then_send_count_reaches(2).await;
    scenario.then_rebroadcast_counts_are(30005, 2, 1).await?;
    scenario.when_observer_stops(observer).await?;
    scenario.then_durable_transfer_is(30005, false).await?;

    scenario.given_transfer_can_succeed();
    scenario.given_receipt_behavior(30005, 0);
    let restarted = scenario.when_observer_starts();
    scenario.then_observer_has_queried().await;
    scenario.when_observer_stops(restarted).await?;
    // Also complete a full observation round: cooldown must still suppress send.
    scenario.when_receipts_are_observed().await?;
    scenario.then_rebroadcast_counts_are(30005, 2, 1).await?;

    scenario.given_rebroadcast_cooldown_elapsed().await?;
    scenario.when_receipts_are_observed().await?;
    scenario.then_rebroadcast_counts_are(30005, 3, 2).await?;
    scenario.then_durable_transfer_is(30005, true).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_observer_failure_survives_legacy_success_projection()
-> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.given_chain_next_nonce(Some(20006));
    scenario.given_transfer_times_out();
    scenario.when().transfer_fails().await;
    scenario.given_receipt_behavior(20006, 3);
    scenario.when_receipts_are_observed().await?;
    scenario.then_receipt_is_persisted(20006, false).await?;

    scenario.given_legacy_success_overwrite().await?;

    scenario.then_bill_status_is(20006, 3).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn api_wallet_transfer_nonce_all_evm_chains_persist_send_and_confirm() -> anyhow::Result<()> {
    for chain in support::evm_chain_codes() {
        let scenario = support::EvmTransferScenario::given_chain(&chain).await?;
        scenario.when_transfer_runs().await?;
        scenario.then_signed_fact_and_send_are(true).await?;
        scenario.when_receipt_is_observed(true).await?;
        scenario.then_confirmed_without_resend(true).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn api_wallet_transfer_nonce_polygon_persistence_failure_never_broadcasts()
-> anyhow::Result<()> {
    let scenario = support::EvmTransferScenario::given_chain("polygon").await?;
    scenario.given_persistence_fails().await?;
    let err = scenario.when_transfer_runs().await.unwrap_err();
    assert!(err.to_string().contains("simulated signed persistence failure"));
    scenario.then_nothing_was_sent_or_saved().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn api_wallet_transfer_nonce_new_evm_timeout_recovers_from_failed_receipt_without_resend()
-> anyhow::Result<()> {
    for chain in ["polygon", "avalanche", "fantom", "arbitrum", "base", "scroll"] {
        let scenario = support::EvmTransferScenario::given_chain(chain).await?;
        scenario.given_timeout();
        assert!(scenario.when_transfer_runs().await.is_err());
        scenario.then_signed_fact_and_send_are(false).await?;
        scenario.when_receipt_is_observed(false).await?;
        scenario.then_confirmed_without_resend(false).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn candidate_nonce_all_evm_build_failure_can_retry_without_gap() -> anyhow::Result<()> {
    for chain in support::evm_chain_codes() {
        let scenario = support::EvmTransferScenario::given_chain(&chain).await?;
        scenario.given_build_fails(true);
        assert!(
            scenario
                .when_transfer_runs()
                .await
                .unwrap_err()
                .to_string()
                .contains("pre-send build failure")
        );
        scenario.then_pre_send_failure_has_no_reservation().await?;
        scenario.given_build_fails(false);
        scenario.when_transfer_runs().await?;
        scenario.then_signed_fact_and_send_are(true).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn candidate_nonce_all_evm_persistence_failure_can_retry_without_gap() -> anyhow::Result<()> {
    for chain in support::evm_chain_codes() {
        let scenario = support::EvmTransferScenario::given_chain(&chain).await?;
        scenario.given_persistence_fails().await?;
        assert!(
            scenario
                .when_transfer_runs()
                .await
                .unwrap_err()
                .to_string()
                .contains("signed persistence failure")
        );
        scenario.then_pre_send_failure_has_no_reservation().await?;
        scenario.given_persistence_recovers().await?;
        scenario.when_transfer_runs().await?;
        scenario.then_signed_fact_and_send_are(true).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn candidate_nonce_cancelled_build_remains_available() -> anyhow::Result<()> {
    let scenario = support::EvmTransferScenario::given_chain("polygon").await?;
    scenario.given_build_blocks();
    let attempt = scenario.when_transfer_starts();
    scenario.then_build_has_entered().await;
    attempt.abort();
    assert!(attempt.await.unwrap_err().is_cancelled());
    scenario.when_build_is_released();
    scenario.then_nonce_floor_is(0).await?;
    scenario.then_nothing_was_sent_or_saved().await?;
    scenario.when_transfer_runs().await?;
    scenario.then_signed_fact_and_send_are(true).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn candidate_nonce_foreign_allocation_during_build_never_sends() -> anyhow::Result<()> {
    let scenario = support::EvmTransferScenario::given_chain("polygon").await?;
    scenario.given_build_blocks();
    let attempt = scenario.when_transfer_starts();
    scenario.then_build_has_entered().await;
    assert_eq!(scenario.when_foreign_counter_advances().await?, 1);
    scenario.when_build_is_released();
    assert!(attempt.await?.unwrap_err().to_string().contains("nonce candidate conflict"));
    scenario.then_nonce_floor_is(1).await?;
    scenario.then_nothing_was_sent_or_saved().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_avalanche_ack_survives_visibility_error() -> anyhow::Result<()> {
    let scenario = support::EvmTransferScenario::given_chain("avalanche").await?;
    scenario.given_visibility(false, true);

    let hash = scenario.when_transfer_runs().await?;

    assert_eq!(hash, scenario.expected_hash());
    scenario.then_signed_fact_and_send_are(true).await?;
    scenario.when_receipt_is_observed(true).await?;
    scenario.then_confirmed_without_resend(true).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_avalanche_ack_survives_not_yet_visible() -> anyhow::Result<()> {
    let scenario = support::EvmTransferScenario::given_chain("avalanche").await?;
    scenario.given_visibility(false, false);

    let hash = scenario.when_transfer_runs().await?;

    assert_eq!(hash, scenario.expected_hash());
    scenario.then_signed_fact_and_send_are(true).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_avalanche_wrong_hash_with_visibility_error_stays_unaccepted()
-> anyhow::Result<()> {
    let scenario = support::EvmTransferScenario::given_chain("avalanche").await?;
    scenario.given_hash_mismatch();
    scenario.given_visibility(false, true);

    let error = scenario.when_transfer_runs().await.unwrap_err();

    assert!(error.to_string().contains("transfer_broadcast_hash_mismatch"));
    scenario.then_signed_fact_and_send_are(false).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_avalanche_ack_survives_visibility_tls_error() -> anyhow::Result<()> {
    let scenario = support::EvmTransferScenario::given_chain("avalanche").await?;
    scenario.given_visibility_tls_error();

    let hash = scenario.when_transfer_runs().await?;

    assert_eq!(hash, scenario.expected_hash());
    scenario.then_signed_fact_and_send_are(true).await?;
    scenario.then_no_visibility_query_was_needed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_transfer_avalanche_duplicate_without_visibility_stays_uncertain()
-> anyhow::Result<()> {
    let scenario = support::EvmTransferScenario::given_chain("avalanche").await?;
    scenario.given_duplicate_broadcast();
    scenario.given_visibility(false, false);

    let error = scenario.when_transfer_runs().await.unwrap_err();

    assert!(error.to_string().contains("transfer_broadcast_uncertain"));
    scenario.then_signed_fact_and_send_are(false).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn shared_broadcast_matching_hash_without_visibility_preserves_guard() -> anyhow::Result<()> {
    let scenario = support::EvmTransferScenario::given_chain("avalanche").await?;
    scenario.given_visibility(false, false);

    let accepted = scenario.when_shared_broadcast_runs().await?;

    assert!(!accepted);
    scenario.then_visibility_queries_are(3);
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_fee_build_failure_retries_same_nonce_then_commits() -> anyhow::Result<()> {
    let scenario = support::FeeBuildScenario::given().await?;
    scenario.adapter.fail_build(true);

    scenario.when_build().await?;
    scenario.when_build().await?;

    scenario.then_facts(0, false).await;
    scenario.then_slot_released().await;
    assert_eq!(scenario.adapter.build_calls(), 2);
    scenario.adapter.fail_build(false);
    scenario.when_build().await?;
    scenario.then_facts(6, true).await;
    assert_eq!(scenario.adapter.nonce_queries(), 3);
    scenario.when_build().await?;
    scenario.then_facts(6, true).await;
    assert_eq!(scenario.adapter.build_calls(), 3);
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_fee_payer_shortage_stops_build_after_later_funding() -> anyhow::Result<()> {
    let scenario = support::FeeBuildScenario::given().await?;
    scenario.adapter.build_payer_shortage(true);

    scenario.when_build().await?;

    scenario.then_failed_for_balance().await;
    scenario.then_slot_released().await;
    scenario.then_facts(0, false).await;
    scenario.adapter.build_payer_shortage(false);
    scenario.when_build().await?;
    scenario.when_build().await?;
    scenario.then_failed_for_balance().await;
    scenario.then_facts(0, false).await;
    assert_eq!(scenario.adapter.build_calls(), 1);
    assert_eq!(scenario.adapter.nonce_queries(), 1);
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_fee_build_timeout_retries_without_terminal_failure() -> anyhow::Result<()> {
    let scenario = support::FeeBuildScenario::given().await?;
    scenario.adapter.build_timeout(true);

    scenario.when_build().await?;

    scenario.then_retryable().await;
    scenario.then_facts(0, false).await;
    scenario.adapter.build_timeout(false);
    scenario.when_build().await?;
    scenario.then_facts(6, true).await;
    assert_eq!(scenario.adapter.build_calls(), 2);
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_fee_shortage_reports_failure_and_never_executes_again() {
    let scenario = support::FeeFailureScenario::given().await;

    scenario.when_gas_balance_shortage().await;
    scenario.when_upload_receipt().await;
    scenario.when_stale_execution_is_queued().await;
    scenario.when_upload_receipt().await;

    scenario.then_failure_state(true).await;
    scenario.then_failure_receipts(1);
}

#[tokio::test]
#[serial]
async fn collect_fee_shortage_retries_only_failed_receipt_upload() {
    let scenario = support::FeeFailureScenario::given().await;
    scenario.when_gas_balance_shortage().await;
    scenario.given_receipt_upload_failure();

    scenario.when_upload_receipt().await;

    scenario.then_failure_state(false).await;
    scenario.then_failure_receipts(1);
    scenario.when_restarted_execution_is_queued().await;
    scenario.when_upload_receipt().await;
    scenario.then_failure_state(true).await;
    scenario.then_failure_receipts(2);
}

#[tokio::test]
#[serial]
async fn collect_fee_shortage_freezes_inflight_build_before_nonce_commit() -> anyhow::Result<()> {
    let scenario = support::FeeBuildScenario::given().await?;

    scenario.when_inflight_build_loses_to_shortage().await?;

    scenario.then_failed_for_balance().await;
    scenario.then_slot_released().await;
    scenario.then_facts(0, false).await;
    assert_eq!(scenario.adapter.build_calls(), 1);
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_fee_recipient_rent_shortage_keeps_existing_retry() -> anyhow::Result<()> {
    let scenario = support::FeeBuildScenario::given().await?;

    scenario.when_recipient_rent_error_occurs().await?;

    scenario.then_retryable().await;
    scenario.then_facts(0, false).await;
    assert_eq!(scenario.adapter.build_calls(), 0);
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_fee_build_occupied_slot_skips_nonce_and_build() -> anyhow::Result<()> {
    let scenario = support::FeeBuildScenario::given().await?;
    scenario.given_occupied_slot().await;

    scenario.when_build().await?;

    scenario.then_facts(0, false).await;
    assert_eq!(scenario.adapter.nonce_queries(), 0);
    assert_eq!(scenario.adapter.build_calls(), 0);
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_fee_build_cancelled_before_commit_reserves_nothing() -> anyhow::Result<()> {
    let scenario = support::FeeBuildScenario::given().await?;
    scenario.adapter.block_build(true);

    {
        let build = scenario.when_build();
        tokio::pin!(build);
        tokio::select! {
            result = &mut build => panic!("build unexpectedly completed: {result:?}"),
            _ = scenario.adapter.wait_for_first_entry() => {}
        }
    } // Dropping the blocked build future leaves only a reclaimable build slot.

    scenario.then_facts(0, false).await;
    assert_eq!(scenario.adapter.build_calls(), 1);
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_fee_build_competing_allocator_discards_candidate() -> anyhow::Result<()> {
    let scenario = support::FeeBuildScenario::given().await?;
    scenario.adapter.block_build(true);

    let build = scenario.when_build();
    tokio::pin!(build);
    tokio::select! {
        result = &mut build => panic!("build unexpectedly completed: {result:?}"),
        _ = scenario.adapter.wait_for_first_entry() => {}
    }
    scenario.given_competing_reservation().await;
    scenario.adapter.release_first();
    build.await?;

    scenario.then_facts(6, false).await;
    scenario.then_slot_released().await;
    assert_eq!(scenario.adapter.build_calls(), 1);
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_broadcast_order_defers_16_until_14_and_15_are_sent() -> anyhow::Result<()> {
    let scenario = support::WithdrawBroadcastScenario::given_signed_batch().await?;

    scenario.when_broadcast(16).await?;
    scenario.then_order_is(16, false).await?;
    assert!(scenario.adapter.recorded_nonces().is_empty());

    scenario.when_broadcast(14).await?;
    scenario.then_order_is(14, true).await?;
    scenario.when_broadcast(14).await?; // Duplicate queued command must not resend.
    scenario.when_broadcast(16).await?;
    scenario.then_order_is(16, false).await?;
    scenario.when_broadcast(15).await?;
    scenario.when_broadcast(16).await?;

    scenario.then_order_is(15, true).await?;
    scenario.then_order_is(16, true).await?;
    assert_eq!(scenario.adapter.recorded_nonces(), vec![14, 15, 16]);
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_broadcast_order_waits_for_inflight_predecessor() -> anyhow::Result<()> {
    let scenario = support::WithdrawBroadcastScenario::given_signed_batch().await?;
    scenario.adapter.block_first_transfer();
    let first = scenario.when_broadcast_starts(14);
    scenario.adapter.wait_for_first_entry().await;
    let second = scenario.when_broadcast(15);
    tokio::pin!(second);
    // Poll the actual second step while the predecessor is blocked, instead of
    // assuming a spawned task ran during a sleep.
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut second).await.is_err()
    );
    assert_eq!(scenario.adapter.recorded_nonces(), vec![14]);
    scenario.adapter.release_first();
    first.await??;
    second.await?;
    scenario.then_order_is(14, true).await?;
    scenario.then_order_is(15, true).await?;
    assert_eq!(scenario.adapter.recorded_nonces(), vec![14, 15]);
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_broadcast_order_cancelled_waiter_preserves_signed_transaction()
-> anyhow::Result<()> {
    let scenario = support::WithdrawBroadcastScenario::given_signed_batch().await?;
    scenario.adapter.block_first_transfer();
    let first = scenario.when_broadcast_starts(14);
    scenario.adapter.wait_for_first_entry().await;
    // Timeout polls and then drops the blocked worker future: cancellation must
    // preserve the signed order and leave no gate reservation behind.
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), scenario.when_broadcast(15))
            .await
            .is_err()
    );
    scenario.then_order_is(15, false).await?;
    scenario.adapter.release_first();
    first.await??;

    scenario.when_broadcast(15).await?;
    scenario.then_order_is(15, true).await?;
    assert_eq!(scenario.adapter.recorded_nonces(), vec![14, 15]);
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_broadcast_order_nonce_query_failure_waits_without_send() -> anyhow::Result<()> {
    let scenario = support::WithdrawBroadcastScenario::given_signed_batch().await?;
    scenario.adapter.set_nonce_response(None);

    scenario.when_broadcast(14).await?;

    scenario.then_order_is(14, false).await?;
    assert!(scenario.adapter.recorded_nonces().is_empty());
    scenario.adapter.set_nonce_response(Some(14));
    scenario.when_broadcast(14).await?;
    scenario.then_order_is(14, true).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_broadcast_order_node_nonce_lag_keeps_order_retryable() -> anyhow::Result<()> {
    let scenario = support::WithdrawBroadcastScenario::given_signed_batch().await?;
    scenario.adapter.reject_next_nonce();

    scenario.when_broadcast(14).await?;

    scenario.then_order_is(14, false).await?;
    scenario.when_broadcast(14).await?;
    scenario.then_order_is(14, true).await?;
    assert_eq!(scenario.adapter.recorded_nonces(), vec![14, 14]);
    let raw = scenario.adapter.recorded_raw();
    assert_eq!(raw[0], raw[1]);
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_broadcast_terminal_gap_fails_unsent_followers_and_reuses_nonce()
-> anyhow::Result<()> {
    let scenario = support::WithdrawBroadcastScenario::given_signed_batch().await?;
    scenario.given_predecessor_failed().await?;
    scenario.when_broadcast(15).await?;
    scenario.when_broadcast(16).await?;
    scenario.when_broadcast(15).await?;
    scenario.then_gap_failed(15).await?;
    scenario.then_gap_failed(16).await?;
    scenario.then_next_nonce_reusable().await?;
    assert!(scenario.adapter.recorded_nonces().is_empty());
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_broadcast_cancel_during_send_keeps_durable_uncertainty() -> anyhow::Result<()> {
    let scenario = support::WithdrawBroadcastScenario::given_signed_batch().await?;
    scenario.adapter.block_first_transfer();
    let sending = scenario.when_broadcast_starts(14);
    scenario.adapter.wait_for_first_entry().await;
    scenario.then_send_is_uncertain(14).await?;
    sending.abort();
    assert!(sending.await.unwrap_err().is_cancelled());
    scenario.when_broadcast(14).await?;
    scenario.then_send_is_uncertain(14).await?;
    assert_eq!(scenario.adapter.recorded_nonces(), vec![14]);
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_build_failure_retries_same_nonce_then_commits_signed_facts()
-> anyhow::Result<()> {
    let scenario = support::WithdrawBuildScenario::given().await?;
    scenario.adapter.fail_build(true);
    scenario.when_build().await?;
    scenario.then_facts(false).await?;
    scenario.adapter.fail_build(false);
    scenario.when_build().await?;
    scenario.then_facts(true).await?;
    scenario.when_build().await?;
    assert_eq!(scenario.adapter.build_calls(), 2);
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_build_frozen_sender_does_not_observe_sign_or_reserve() -> anyhow::Result<()> {
    let scenario = support::WithdrawBuildScenario::given().await?;
    scenario.given_frozen(true)?;
    scenario.when_build().await?;
    scenario.then_facts(false).await?;
    scenario.then_no_nonce_query_or_build();
    scenario.given_frozen(false)?;
    scenario.when_build().await?;
    scenario.then_facts(true).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_broadcast_mismatching_ack_keeps_nonce_uncertain() -> anyhow::Result<()> {
    let scenario = support::WithdrawBroadcastScenario::given_signed_batch().await?;
    scenario.adapter.mismatch_hash();
    scenario.when_broadcast(14).await?;
    scenario.then_send_is_uncertain(14).await?;
    scenario.when_broadcast(14).await?;
    assert_eq!(scenario.adapter.recorded_nonces(), vec![14]);
    Ok(())
}

#[tokio::test]
#[serial]
async fn withdrawal_broadcast_storage_failure_after_send_keeps_nonce_uncertain()
-> anyhow::Result<()> {
    let scenario = support::WithdrawBroadcastScenario::given_signed_batch().await?;
    scenario.given_broadcast_persistence_fails().await?;
    scenario.when_broadcast(14).await?;
    scenario.then_send_is_uncertain(14).await?;
    scenario.when_broadcast(14).await?;
    assert_eq!(scenario.adapter.recorded_nonces(), vec![14]);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn collect_main_failed_build_retries_same_nonce() -> anyhow::Result<()> {
    let scenario = support::CollectBuildScenario::given().await?;
    scenario.adapter.fail_build(true);
    assert!(scenario.when_build().await.is_err());
    scenario.then_no_signed_reservation().await;
    scenario.adapter.fail_build(false);
    scenario.when_build().await?;
    scenario.then_signed_once().await;
    scenario.when_build().await?;
    scenario.then_signed_once().await;
    assert_eq!(scenario.adapter.build_calls(), 2);
    Ok(())
}
#[tokio::test]
#[serial_test::serial]
async fn collect_main_cancelled_build_retries_same_nonce() -> anyhow::Result<()> {
    let scenario = support::CollectBuildScenario::given().await?;
    scenario.when_cancel_inflight().await;
    scenario.then_no_signed_reservation().await;
    scenario.given_stale_build_slot().await;
    scenario.when_build().await?;
    scenario.then_signed_once().await;
    Ok(())
}
#[tokio::test]
#[serial_test::serial]
async fn collect_main_timeout_build_retries_same_nonce() -> anyhow::Result<()> {
    let scenario = support::CollectBuildScenario::given().await?;
    scenario.adapter.build_timeout(true);
    assert!(scenario.when_build().await.is_err());
    scenario.then_no_signed_reservation().await;
    scenario.adapter.build_timeout(false);
    scenario.when_build().await?;
    scenario.then_signed_once().await;
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn collect_main_competing_reservation_discards_built_candidate() -> anyhow::Result<()> {
    let scenario = support::CollectBuildScenario::given().await?;
    scenario.when_build_loses_reservation_race().await;
    Ok(())
}
#[tokio::test]
#[serial_test::serial]
async fn collect_main_existing_gap_never_allocates_another_nonce() -> anyhow::Result<()> {
    let scenario = support::CollectBuildScenario::given().await?;
    scenario.given_competing_reservation().await;
    assert!(scenario.when_build().await.is_err());
    scenario.then_no_raw().await;
    assert_eq!(scenario.adapter.build_calls(), 0);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn collect_main_new_order_uses_unsigned_five_with_saved_six() -> anyhow::Result<()> {
    // Given an old signed/broadcast6 and a different, unsigned new order.
    let scenario = support::CollectBuildScenario::given().await?;
    scenario.given_saved_six().await?;
    // When the new order builds through the real worker with a recording adapter.
    scenario.when_build().await?;
    // Then5 is committed,6 is retained and duplicate execution cannot rebuild5.
    scenario.then_hole_filled_and_six_preserved().await?;
    scenario.when_build().await?;
    assert_eq!(scenario.adapter.build_calls(), 1);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn fee_new_order_uses_unsigned_five_with_saved_six() -> anyhow::Result<()> {
    // Given an old signed collect6 on the same sender and an unsigned new order.
    let scenario = support::FeeBuildScenario::given().await?;
    let old = scenario.given_saved_six().await?;
    // When the real worker builds with a recording adapter.
    scenario.when_build().await?;
    // Then the new order owns5, the old6 remains, and duplicate work builds once.
    scenario.then_filled_five_with_saved_six(&old).await?;
    scenario.when_build().await?;
    assert_eq!(scenario.adapter.build_calls(), 1);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn withdraw_new_order_uses_unsigned_five_with_saved_six() -> anyhow::Result<()> {
    // Given an old signed collect6 on the same sender and an unsigned new order.
    let scenario = support::WithdrawBuildScenario::given().await?;
    let old = scenario.given_saved_six().await?;
    // When the real worker builds with a recording adapter.
    scenario.when_build().await?;
    // Then the new order owns5, the old6 remains, and duplicate work builds once.
    scenario.then_filled_five_with_saved_six(&old).await?;
    scenario.when_build().await?;
    assert_eq!(scenario.adapter.build_calls(), 1);
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_replacement_uses_scoped_old_nonce_and_keeps_legacy_audit() -> anyhow::Result<()> {
    let scenario = support::CollectBuildScenario::given().await?;
    scenario.given_replacement_at_frontier().await?;
    scenario.when_build().await?;
    scenario.then_replacement_is_signed_once().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_replacement_reconciles_each_winner_and_continues_from_seven() -> anyhow::Result<()>
{
    for (old, success) in [(false, true), (false, false), (true, true), (true, false)] {
        let scenario = support::CollectBuildScenario::given().await?;
        scenario.given_replacement_at_frontier().await?;
        scenario.when_build().await?;
        scenario.given_replacement_winner(old, success).await?;
        scenario.when_replacement_recovers().await?;
        scenario.when_replacement_recovers().await?;
        scenario.then_replacement_reconciled(old, success).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_replacement_pending_seven_latest_six_still_replaces_six() -> anyhow::Result<()> {
    // Given old6 is pending at the node while latest confirmed frontier is6.
    let scenario = support::CollectBuildScenario::given().await?;
    scenario.given_replacement_at_frontier().await?;
    scenario.adapter.set_nonce_response(Some(7));
    scenario.adapter.require_replacement_quote();
    // When the new authorized collect builds, then it replaces6, never allocates7.
    scenario.when_build().await?;
    scenario.then_replacement_is_signed_once().await?;
    Ok(())
}
#[tokio::test]
#[serial]
async fn collect_replacement_fee_preflight_receives_replacement_before_quote() -> anyhow::Result<()>
{
    // Given ordinary pending-state estimation rejects the already-reserved funds.
    let scenario = support::CollectBuildScenario::given().await?;
    scenario.given_replacement_at_frontier().await?;
    scenario.adapter.require_replacement_quote();
    // When preflight runs, then the replacement context is present before estimating.
    scenario.when_build().await?;
    scenario.then_replacement_is_signed_once().await?;
    assert_eq!(scenario.adapter.replacement_quotes(), 1);
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_replacement_unknown_latest_or_unproven_consumption_never_signs()
-> anyhow::Result<()> {
    for latest in [None, Some(7)] {
        // Given latest is unavailable, or moved beyond6 without canonical winner evidence.
        let scenario = support::CollectBuildScenario::given().await?;
        scenario.given_replacement_at_frontier().await?;
        scenario.adapter.set_nonce_response(Some(7));
        scenario.adapter.set_latest_nonce_response(latest);
        // When building, then neither pending7 nor uncertainty authorizes a signature.
        assert!(scenario.when_build().await.is_err());
        scenario.then_replacement_waits_unsigned().await?;
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_replacement_true_fee_shortage_reports_failure_before_signing() -> anyhow::Result<()>
{
    // Given the latest-state replacement quote genuinely exceeds available funds.
    let scenario = support::CollectBuildScenario::given().await?;
    scenario.given_replacement_at_frontier().await?;
    scenario.adapter.require_replacement_quote();
    scenario.adapter.unfund_replacement_quote();
    // When the worker handles BuildTx, then fail/report without signing or a fee top-up.
    scenario.when_replacement_build_command().await?;
    scenario.then_replacement_fee_failure_does_not_sign_or_top_up().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_replacement_restart_uses_durable_relation_then_normal_next_nonce()
-> anyhow::Result<()> {
    // Given only the durable replacement relation survives a context restart.
    let mut scenario = support::CollectBuildScenario::given().await?;
    scenario.given_replacement_at_frontier().await?;
    scenario.given_restarted_context_without_recovery_override().await?;
    scenario.when_build().await?;
    scenario.given_replacement_winner(false, true).await?;
    scenario.when_replacement_recovers().await?;
    // When the next collect arrives, it commits the next nonce through the normal flow.
    scenario.given_next_collect_after_replacement().await?;
    scenario.when_build().await?;
    scenario.then_next_collect_owns_seven_without_replacement().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_replacement_automatically_detects_failed_old_slot_without_configuration()
-> anyhow::Result<()> {
    // Given an unsigned new collect and a reported old6, latest6/pending7, no configuration/relation.
    let mut scenario = support::CollectBuildScenario::given().await?;
    scenario.given_automatic_replacement_without_configuration().await?;
    // When ordinary BuildTx commands retry and the process resumes from persisted state.
    scenario.when_build().await?;
    scenario.when_replacement_build_command().await?;
    // Then preflight used replacement context, exactly one signature owns6, old signature retained.
    scenario.then_replacement_is_signed_once().await?;
    scenario.given_replacement_winner(false, true).await?;
    scenario.when_replacement_recovers().await?;
    scenario.given_next_collect_after_replacement().await?;
    scenario.when_build().await?;
    scenario.then_next_collect_owns_seven_without_replacement().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_replacement_automatic_discovery_never_signs_on_unknown_latest_or_winner()
-> anyhow::Result<()> {
    for latest in [None, Some(7)] {
        // Given a discovered failed6, but chain observation cannot prove the slot is available.
        let scenario = support::CollectBuildScenario::given().await?;
        scenario.given_automatic_replacement_without_configuration().await?;
        scenario.adapter.set_latest_nonce_response(latest);
        // When the normal worker retries, then it keeps both the old signature and high-water.
        scenario.when_replacement_build_command().await?;
        scenario.then_automatic_replacement_waits_unsigned().await?;
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn collect_replacement_releases_ordinary_seven_without_virtual_reservation()
-> anyhow::Result<()> {
    // Given a confirmed included new6 winner and the captured legacy153.
    let scenario = support::CollectBuildScenario::given().await?;
    scenario.given_replacement_at_frontier().await?;
    scenario.when_build().await?;
    scenario.given_replacement_winner(false, true).await?;
    scenario.when_replacement_recovers().await?;
    // Then the actual ordinary observer returns7; only signed commit occupies it.
    scenario.then_ordinary_seven_is_free_until_signed_commit().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn nonce_identity_gate_and_freeze_share_evm_case_alias() -> anyhow::Result<()> {
    // Given one EVM sender represented with two case spellings.
    let scenario = TransferNonceScenario::new().await;
    // When gates/freeze are accessed through either spelling, then state is shared.
    scenario.then_case_alias_shares_nonce_gate_and_freeze().await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn receipt_refresh_publishes_only_after_terminal_commit_and_retries_closed_frontend()
-> anyhow::Result<()> {
    for (nonce, status) in [(20101, 2), (20102, 3)] {
        let scenario = TransferNonceScenario::new().await;
        scenario.given().bnb_transfer_fixture().await?;
        scenario.given().fake_chain_adapter();
        scenario.given().wallet_password_cached().await;
        scenario.seed_receipt_native_asset().await?;
        scenario.given_chain_next_nonce(Some(nonce));
        scenario.when().transfer_starts().await??;
        scenario.given_receipt_behavior(nonce, status);
        // A closed frontend must retain durable debt, while the chain fact commits.
        scenario.given_frontend_notification_closed().await;
        scenario.when_receipts_are_observed().await?;
        scenario.then_receipt_is_persisted(nonce, status == 2).await?;
        scenario.then_receipt_refresh_pending(true).await?;
        let mut notifications = scenario.collect_receipt_notifications().await;
        scenario.when_receipts_are_observed().await?;
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), notifications.recv())
            .await?
            .expect("asset notification");
        assert_eq!(event.event, "API_WALLET_SYNC_ASSETS");
        // This is the database snapshot available to the page when it gets the event.
        scenario.then_receipt_is_persisted(nonce, status == 2).await?;
        scenario.then_receipt_refresh_pending(false).await?;
        scenario.when_receipts_are_observed().await?;
        assert!(notifications.try_recv().is_err(), "terminal receipt must not publish twice");
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn receipt_refresh_defers_mqtt_and_survives_balance_query_failure() -> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.seed_receipt_native_asset().await?;
    scenario.given_chain_next_nonce(Some(20103));
    scenario.when().transfer_starts().await??;
    let mut notifications = scenario.collect_receipt_notifications().await;
    scenario.when_self_transfer_account_change_arrives(20103).await?;
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), notifications.recv())
            .await?
            .expect("account notification")
            .event,
        "API_WALLET_ACCT_CHANGE"
    );
    assert!(notifications.try_recv().is_err());
    scenario.given_receipt_behavior(20103, 2);
    scenario.given_receipt_balance_fails(true);
    scenario.when_receipts_are_observed().await?;
    scenario.then_receipt_is_persisted(20103, true).await?;
    scenario.then_receipt_refresh_pending(true).await?;
    assert!(notifications.try_recv().is_err());
    scenario.given_receipt_balance_fails(false);
    scenario.when_receipts_are_observed().await?;
    scenario.then_receipt_refresh_pending(false).await?;
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), notifications.recv())
            .await?
            .expect("account notification")
            .event,
        "API_WALLET_SYNC_ASSETS"
    );
    // Once acknowledged, later account-change messages retain their legacy path.
    Ok(())
}

#[tokio::test]
#[serial]
async fn receipt_refresh_partial_token_query_failure_does_not_publish_native_only()
-> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.seed_receipt_native_asset().await?;
    scenario.given_receipt_token_asset().await?;
    scenario.given_chain_next_nonce(Some(20104));
    scenario.when_token_transfer_starts().await?;
    scenario.given_receipt_behavior(20104, 2);
    let mut notifications = scenario.collect_receipt_notifications().await;
    scenario.given_only_token_balance_fails(true);
    scenario.when_receipts_are_observed().await?;
    scenario.then_receipt_refresh_pending(true).await?;
    assert!(notifications.try_recv().is_err());
    scenario.given_only_token_balance_fails(false);
    scenario.when_receipts_are_observed().await?;
    scenario.then_receipt_refresh_pending(false).await?;
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), notifications.recv())
            .await?
            .expect("account notification")
            .event,
        "API_WALLET_SYNC_ASSETS"
    );
    assert!(notifications.try_recv().is_err());
    Ok(())
}

#[tokio::test]
#[serial]
async fn receipt_refresh_hung_first_balance_does_not_block_same_sender_next_transfer()
-> anyhow::Result<()> {
    let scenario = TransferNonceScenario::new().await;
    scenario.given().bnb_transfer_fixture().await?;
    scenario.given().fake_chain_adapter();
    scenario.given().wallet_password_cached().await;
    scenario.seed_receipt_native_asset().await?;
    scenario.given_chain_next_nonce(Some(20105));
    scenario.when().transfer_starts().await??;
    scenario.given_receipt_behavior(20105, 2);
    scenario.given_other_receipt_balance_hangs(&scenario);
    let (shutdown, observer) = scenario.when_observer_starts();
    let result = async {
        scenario.then_old_receipt_balance_has_started().await?;
        scenario.given_chain_next_nonce(Some(20106));
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario.when().transfer_starts())
            .await???;
        scenario.given_receipt_behavior(20106, 2);
        scenario.then_background_receipt_commits(20106).await?;
        scenario.then_receipt_refresh_pending_for(20105).await?;
        anyhow::Ok(())
    }
    .await;
    let _ = shutdown.send(());
    tokio::time::timeout(std::time::Duration::from_secs(2), observer).await??;
    result?;
    scenario.then_receipt_refresh_pending_for(20105).await?;
    Ok(())
}
