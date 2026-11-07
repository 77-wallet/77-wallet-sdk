mod support;

use serial_test::serial;

use support::{
    CollectReceiptFixture, CollectReceiptGiven, CollectReceiptScenario, CollectReceiptThen,
    CollectReceiptWhen, LocalCollectDb, ScenarioRoles, assert_collect_receipt_payload,
    base_collect_for_receipt, collect_receipt_payload_json,
};

#[tokio::test]
async fn collect_tx_exec_receipt_uses_persisted_to_addr() {
    // Arrange: payload input
    let req = base_collect_for_receipt();

    // Act
    let payload_json = collect_receipt_payload_json(&req, &req.trade_no);

    // Assert: persisted execution facts are used in the receipt payload.
    assert_collect_receipt_payload(&payload_json, &req.trade_no, "persisted-to", "hash");
}

#[tokio::test]
async fn collect_rebuild_then_receipt_upload_uses_rebuilt_to_addr() {
    // Arrange: local DB and stale build facts
    let db = LocalCollectDb::new().await;
    let fixture = CollectReceiptFixture::new("collect_rebuild_then_receipt");
    db.seed_stale_collect_build(&fixture).await;

    // Act: invalidate and persist rebuilt execution facts.
    db.rebuild_collect_execution(&fixture).await;

    // Assert: receipt payload uses rebuilt facts, not stale build facts.
    let payload_json = db.receipt_payload_json(&fixture).await;
    assert_collect_receipt_payload(
        &payload_json,
        &fixture.trade_no,
        &fixture.receipt_to_addr,
        &fixture.tx_hash,
    );
}

#[serial]
#[tokio::test]
async fn collect_side_effect_worker_marks_tx_exec_receipt_uploaded_after_rebuild() {
    let scenario = CollectReceiptScenario::new().await;
    let fixture = CollectReceiptFixture::new("collect_worker_receipt");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.mock_backend_is_active().await;
    given.rebuilt_collect_execution(&fixture).await;

    when.worker_uploads_receipt(&fixture).await;

    then.receipt_upload_is_persisted(&fixture).await;
    then.receipt_payload_uses_execution_facts(&fixture).await;
}

#[serial]
#[tokio::test]
async fn collect_backend_api_direct_upload_hits_mock_server() {
    let scenario = CollectReceiptScenario::new().await;
    let fixture = CollectReceiptFixture::new("collect_direct_backend");
    let when = scenario.when();
    let then = scenario.then();

    when.direct_backend_uploads_receipt(&fixture).await;

    then.backend_received_execute_complete(&fixture).await;
}

#[serial]
#[tokio::test]
async fn collect_scanner_dispatcher_uploads_rebuilt_tx_exec_receipt() {
    let scenario = CollectReceiptScenario::new().await;
    let fixture = CollectReceiptFixture::new("collect_scan_dispatch");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.scanner_ready_collect_execution(&fixture).await;

    let dispatched_trade_no = when.scanner_dispatches_receipt().await;

    then.scanner_selected_trade(dispatched_trade_no, &fixture);
    then.receipt_upload_is_persisted(&fixture).await;
    then.receipt_payload_uses_execution_facts(&fixture).await;
}

fn finalized_conflict_fixture(prefix: &str) -> CollectReceiptFixture {
    let mut fixture = CollectReceiptFixture::new(prefix);
    fixture.from_addr = format!(" 0x{} ", "ab".repeat(20));
    fixture.tx_hash = format!("0x{}", "11".repeat(32));
    fixture
}

#[serial]
#[tokio::test]
async fn collect_finalized_conflict_receipt_finishes_once_without_replacing_hash() {
    let scenario = CollectReceiptScenario::new().await;
    let fixture = finalized_conflict_fixture("conflict_receipt");
    scenario.given_finalized_conflict(&fixture).await;

    scenario.when().worker_uploads_receipt(&fixture).await;
    scenario.when().worker_uploads_receipt(&fixture).await;

    scenario.then_conflict_receipt(&fixture, true, 1).await;
}

#[serial]
#[tokio::test]
async fn collect_finalized_conflict_receipt_retries_backend_failure_before_finishing() {
    let scenario = CollectReceiptScenario::new().await;
    let fixture = finalized_conflict_fixture("conflict_receipt_retry");
    scenario.given_finalized_conflict(&fixture).await;
    scenario.given_backend_rejects_receipt();

    scenario.when_receipt_upload_fails(&fixture).await;

    scenario.then_conflict_receipt(&fixture, false, 1).await;

    scenario.when().worker_uploads_receipt(&fixture).await;

    scenario.then_conflict_receipt(&fixture, true, 2).await;
}

#[serial]
#[tokio::test]
async fn collect_replacement_included_winner_reports_once_and_restores_next_seven() {
    // Given immutable old/new6 and the captured legacy153, without a live signer.
    let scenario = CollectReceiptScenario::new().await;
    let fixture = CollectReceiptFixture::new("replacement_finality");
    scenario.given_signed_replacement(&fixture).await;
    // When included receipt evidence is reconciled twice and the real scanner dispatches upload.
    scenario.when_replacement_winner_is_reconciled(&fixture).await;
    let selected = scenario.when().scanner_dispatches_receipt().await;
    scenario.then().scanner_selected_trade(selected, &fixture);
    scenario.when().worker_uploads_receipt(&fixture).await;
    // Then one success report uses the new identity, audit remains, next usable counter is7.
    scenario.then_replacement_upload_and_counter_are_recovered(&fixture).await;
}
