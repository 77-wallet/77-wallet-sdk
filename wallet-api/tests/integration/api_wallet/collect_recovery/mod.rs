mod support;

use serial_test::serial;

use support::{
    CollectRecoveryFixture, CollectRecoveryGiven, CollectRecoveryThen, CollectRecoveryWhen,
    LocalCollectRecoveryDb, ScenarioRoles, ShadowCollectRecoveryScenario,
};

#[tokio::test]
async fn collect_blockhash_rebuild_clears_stale_build_facts_and_persists_new_to_addr() {
    let db = LocalCollectRecoveryDb::new().await;
    let fixture = CollectRecoveryFixture::blockhash_rebuild();

    db.given_stale_blockhash_build(&fixture).await;

    let invalidated = db.when_raw_tx_is_invalidated_for_rebuild(&fixture).await;

    db.then_stale_build_facts_are_cleared(&fixture, invalidated).await;

    db.when_rebuilt_to_addr_is_persisted(&fixture, "new-to").await;

    db.then_rebuilt_to_addr_is_persisted(&fixture, "new-to").await;
}

#[tokio::test]
#[serial]
async fn collect_recover_queries_chain_before_any_expired_raw_rebuild_invalidation() {
    let scenario = ShadowCollectRecoveryScenario::new().await;
    let fixture = CollectRecoveryFixture::expired_tron_raw_probe();
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.chain_probe_confirms_tx(&fixture);
    given.expired_raw_tx_collect(&fixture).await;

    when.recover_runs(&fixture).await;

    then.chain_was_queried_once();
    then.expired_raw_tx_is_confirmed_without_rebuild(&fixture).await;
}

#[tokio::test]
#[serial]
async fn collect_recover_backfills_missing_tx_hash_before_receipt_upload() {
    let scenario = ShadowCollectRecoveryScenario::new().await;
    let fixture = CollectRecoveryFixture::tron_backfill();
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.recoverable_collect_with_tx_hash(&fixture).await;
    given.chain_query_clears_hash_then_confirms(&fixture);

    when.recover_runs(&fixture).await;

    then.tx_hash_is_backfilled_and_receipt_upload_needed(&fixture).await;
}

#[tokio::test]
async fn collect_scanner_recovers_broadcast_visible_pending_result() {
    let db = LocalCollectRecoveryDb::new().await;
    let fixture = CollectRecoveryFixture::broadcast_visible_pending();

    db.given_broadcast_visible_pending_collect(&fixture).await;

    let labels = db.when_collect_scanner_runs().await;

    db.then_scanner_emits_recover_only(labels);
    db.then_recoverable_row_stays_pending(&fixture).await;
}

#[tokio::test]
#[serial]
async fn collect_recover_finalized_nonce_conflict_enters_failure_receipt_without_resend() {
    let scenario = ShadowCollectRecoveryScenario::new().await;
    let fixture = CollectRecoveryFixture {
        trade_no: "evm-finalized-conflict".to_owned(),
        tx_hash: format!("0x{}", "11".repeat(32)),
    };
    scenario.given_evm_uncertain_collect(&fixture, true, Some(true)).await;

    scenario.when().recover_runs(&fixture).await;

    scenario.then_evm_recovery_facts(&fixture, true, false).await;
}

#[tokio::test]
#[serial]
async fn collect_recover_missing_or_unavailable_finalized_proof_preserves_original() {
    for proof in [Some(false), None] {
        let scenario = ShadowCollectRecoveryScenario::new().await;
        let fixture = CollectRecoveryFixture {
            trade_no: format!("evm-no-proof-{proof:?}"),
            tx_hash: format!("0x{}", "11".repeat(32)),
        };
        scenario.given_evm_uncertain_collect(&fixture, true, proof).await;

        scenario.when().recover_runs(&fixture).await;

        scenario.then_evm_recovery_facts(&fixture, false, false).await;
    }
}

#[tokio::test]
#[serial]
async fn collect_recover_confirmed_original_never_checks_nonce_conflict() {
    let scenario = ShadowCollectRecoveryScenario::new().await;
    let fixture = CollectRecoveryFixture {
        trade_no: "evm-confirmed-original".to_owned(),
        tx_hash: format!("0x{}", "11".repeat(32)),
    };
    scenario.given_evm_uncertain_collect(&fixture, false, Some(true)).await;

    scenario.when().recover_runs(&fixture).await;

    scenario.then_evm_recovery_facts(&fixture, false, true).await;
}

#[tokio::test]
#[serial]
async fn collect_recover_nonce_gap_reports_failure_and_preserves_signed_transaction() {
    let scenario = ShadowCollectRecoveryScenario::new().await;
    let fixture = CollectRecoveryFixture {
        trade_no: "evm-missing-frontier".into(),
        tx_hash: format!("0x{}", "22".repeat(32)),
    };
    scenario.given_evm_uncertain_collect(&fixture, true, None).await;
    scenario.given_future_nonce(&fixture).await;
    scenario.when().recover_runs(&fixture).await;
    scenario.then_gap_is_failed_without_releasing_signed_nonce(&fixture).await;
}

#[tokio::test]
#[serial]
async fn collect_query_uncertain_reports_verified_gap_with_legacy_counter() {
    let scenario = ShadowCollectRecoveryScenario::new().await;
    let fixture = CollectRecoveryFixture {
        trade_no: "query-uncertain-gap".into(),
        tx_hash: format!("0x{}", "33".repeat(32)),
    };
    scenario.given_query_uncertain_gap(&fixture, Some(2), false).await;
    scenario.when().recover_runs(&fixture).await;
    scenario.then_gap_is_failed_without_releasing_signed_nonce(&fixture).await;
}

#[tokio::test]
#[serial]
async fn collect_query_uncertain_does_not_fail_without_old_verified_gap() {
    for (tag, nonce, recent) in
        [("nonce-error", None, false), ("recent", Some(2), true), ("normal", Some(3), false)]
    {
        let scenario = ShadowCollectRecoveryScenario::new().await;
        let fixture = CollectRecoveryFixture {
            trade_no: format!("query-uncertain-{tag}"),
            tx_hash: format!("0x{}", "44".repeat(32)),
        };
        scenario.given_query_uncertain_gap(&fixture, nonce, recent).await;
        scenario.when().recover_runs(&fixture).await;
        scenario.then_query_uncertain_preserves_order(&fixture).await;
    }
}
