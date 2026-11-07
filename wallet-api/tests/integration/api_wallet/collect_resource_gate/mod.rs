mod support;

use serial_test::serial;

use support::{
    CollectResourceGateFixture, CollectResourceGateGiven, CollectResourceGateScenario,
    CollectResourceGateThen, CollectResourceGateWhen, LocalCollectResourceDb, ScenarioRoles,
    install_tron_insufficient_fee_funding_adapter, install_tron_resource_gate_adapter,
};

#[tokio::test]
#[serial]
async fn collect_scanner_emits_resource_receipt_upload_for_failed_delegation() {
    let db = LocalCollectResourceDb::new().await;
    let fixture = CollectResourceGateFixture::resource_scan_case("RSC_FAIL_RECEIPT_SCAN");

    db.given_failed_delegation_ready_for_receipt_scan(&fixture).await;

    let labels = db.when_collect_scanner_runs().await;

    db.then_scanner_emits_resource_receipt_upload(labels);
}

#[tokio::test]
#[serial]
async fn collect_resource_result_ack_releases_origin_collect_gate() {
    let scenario = CollectResourceGateScenario::new().await;
    let fixture = CollectResourceGateFixture::origin_case("C_RSC_RELEASE");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.blocked_collect(&fixture).await;
    given.successful_collect_resource_result(&fixture).await;

    when.resource_result_ack_is_sent(&fixture).await;

    then.origin_collect_gate_is_released_by_successful_delegation(&fixture).await;
    then.collect_can_build(&fixture).await;
}

#[tokio::test]
#[serial]
async fn same_address_unconsumed_platform_energy_does_not_shrink_next_delegation() {
    let _adapter = install_tron_resource_gate_adapter();
    let scenario = CollectResourceGateScenario::new().await;
    let first = CollectResourceGateFixture::origin_case("C_RSC_OVERLAP_FIRST");
    let second = CollectResourceGateFixture::origin_case("C_RSC_OVERLAP_SECOND");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.same_address_collect_with_unconsumed_platform_resource(&first, &second).await;

    when.resource_gate_is_evaluated(&second).await;

    then.platform_resource_request_uses_full_order_energy(&second).await;
    then.collect_cannot_build(&second).await;
}

#[tokio::test]
#[serial]
async fn tron_collect_amount_shortage_fails_before_resource_delegation() {
    let _adapter = install_tron_resource_gate_adapter();
    let scenario = CollectResourceGateScenario::new().await;
    let fixture = CollectResourceGateFixture::origin_case("C_RSC_AMOUNT_SHORT");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.tron_native_collect_with_insufficient_amount(&fixture).await;

    when.resource_gate_is_evaluated(&fixture).await;

    then.collect_fails_amount_shortage_without_resource_delegation(&fixture).await;
    then.collect_cannot_build(&fixture).await;
}

#[tokio::test]
#[serial]
async fn tron_collect_fee_funding_shortage_fails_before_resource_delegation() {
    let funding_addr = "TFundingFeeReservation111111111111111111";
    let _adapter = install_tron_insufficient_fee_funding_adapter(funding_addr);
    let scenario = CollectResourceGateScenario::new().await;
    let fixture = CollectResourceGateFixture::origin_case("C_RSC_FEE_RESERVE_SHORT");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.tron_token_collect_with_insufficient_fee_funding(&fixture, funding_addr).await;

    when.resource_gate_is_evaluated(&fixture).await;

    then.collect_fails_fee_reservation_without_resource_delegation(&fixture).await;
    then.collect_cannot_build(&fixture).await;
}

#[tokio::test]
#[serial]
async fn collect_platform_resource_result_ack_releases_gate_after_chain_success() {
    let scenario = CollectResourceGateScenario::new().await;
    let fixture = CollectResourceGateFixture::origin_case("C_RSC_CHAIN_DONE");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.blocked_collect(&fixture).await;
    given.collect_chain_success(&fixture).await;
    given.successful_collect_resource_result(&fixture).await;

    when.resource_result_ack_is_sent(&fixture).await;

    then.origin_collect_gate_is_released_by_successful_delegation(&fixture).await;
}

#[tokio::test]
#[serial]
async fn collect_failed_resource_result_ack_keeps_local_fallback_gate() {
    let scenario = CollectResourceGateScenario::new().await;
    let fixture = CollectResourceGateFixture::original_order_result_case("C_RSC_FAIL");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.blocked_collect(&fixture).await;
    given.failed_collect_resource_result(&fixture).await;

    when.resource_result_ack_is_sent(&fixture).await;

    then.origin_collect_gate_is_not_released(&fixture).await;
    then.collect_waits_for_local_delegate(&fixture).await;
    then.collect_cannot_build(&fixture).await;
}

#[tokio::test]
#[serial]
async fn withdraw_origin_resource_result_ack_does_not_release_collect_gate() {
    let scenario = CollectResourceGateScenario::new().await;
    let fixture = CollectResourceGateFixture::origin_case("C_WD_ORIGIN_SKIP");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.blocked_collect(&fixture).await;
    given.successful_withdraw_origin_resource_result(&fixture).await;

    when.resource_result_ack_is_sent(&fixture).await;

    then.origin_collect_gate_is_not_released(&fixture).await;
}

#[tokio::test]
#[serial]
async fn collect_failed_resource_receipt_switches_to_local_delegate() {
    let scenario = CollectResourceGateScenario::new().await;
    let fixture = CollectResourceGateFixture::origin_case("C_RSC_FAIL_BYPASS");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.blocked_collect(&fixture).await;
    given.failed_collect_resource_receipt(&fixture).await;
    then.collect_cannot_build(&fixture).await;

    when.resource_receipt_upload_is_sent(&fixture).await;

    then.origin_collect_gate_is_not_released(&fixture).await;
    then.collect_waits_for_local_delegate(&fixture).await;
    then.collect_cannot_build(&fixture).await;
}

#[tokio::test]
#[serial]
async fn collect_resource_tx_exec_receipt_failure_without_origin_trade_no_does_not_release_gate() {
    let scenario = CollectResourceGateScenario::new().await;
    let fixture = CollectResourceGateFixture::origin_case("C_RSC_NO_ORIGIN");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.blocked_collect(&fixture).await;
    given.failed_resource_receipt_without_origin_trade(&fixture).await;

    when.resource_receipt_upload_is_sent(&fixture).await;

    then.origin_collect_gate_is_not_released(&fixture).await;
}
