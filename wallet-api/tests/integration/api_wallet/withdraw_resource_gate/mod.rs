mod support;

use serial_test::serial;

use support::{
    ScenarioRoles, WithdrawResourceGateFixture, WithdrawResourceGateGiven,
    WithdrawResourceGateScenario, WithdrawResourceGateThen, WithdrawResourceGateWhen,
    install_tron_withdraw_resource_gate_adapter,
};

#[serial]
#[tokio::test]
async fn withdraw_resource_result_ack_uses_wd_rsc_dl_type() {
    let scenario = WithdrawResourceGateScenario::new().await;
    let fixture = WithdrawResourceGateFixture::result_ack_payload_case("RSC_WD_ACK");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.resource_delegation_ready_for_ack(&fixture).await;

    when.resource_result_ack_is_sent(&fixture).await;

    then.resource_result_ack_uses_withdraw_resource_type(&fixture).await;
}

#[serial]
#[tokio::test]
async fn withdraw_resource_delegation_chain_verification_blocks_build_until_energy_is_visible() {
    let scenario = WithdrawResourceGateScenario::new().await;
    let fixture = WithdrawResourceGateFixture::origin_gate_case("W_RSC_RELEASE");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.blocked_withdraw(&fixture).await;
    given.successful_withdraw_resource_delegation(&fixture).await;

    when.resource_result_ack_is_sent(&fixture).await;

    then.origin_withdraw_waits_for_delegated_energy_on_chain(&fixture).await;
    then.withdraw_cannot_build(&fixture).await;
}

#[serial]
#[tokio::test]
async fn same_address_unconsumed_platform_energy_does_not_shrink_next_withdraw_delegation() {
    let _adapter = install_tron_withdraw_resource_gate_adapter();
    let scenario = WithdrawResourceGateScenario::new().await;
    let first = WithdrawResourceGateFixture::origin_gate_case("W_RSC_OVERLAP_FIRST");
    let second = WithdrawResourceGateFixture::origin_gate_case("W_RSC_OVERLAP_SECOND");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.same_address_withdraw_with_unconsumed_platform_resource(&first, &second).await;

    when.resource_gate_is_evaluated(&second).await;

    then.platform_resource_request_uses_full_order_energy(&second).await;
    then.withdraw_cannot_build(&second).await;
}

#[serial]
#[tokio::test]
async fn withdraw_failed_original_order_resource_result_ack_bypasses_gate() {
    let scenario = WithdrawResourceGateScenario::new().await;
    let fixture = WithdrawResourceGateFixture::original_order_result_case("W_RSC_ORIGIN_FAIL");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.blocked_withdraw(&fixture).await;
    given.failed_original_order_resource_result(&fixture).await;
    then.withdraw_cannot_build(&fixture).await;

    when.resource_result_ack_is_sent(&fixture).await;

    then.origin_withdraw_gate_is_released_by_failed_bypass(&fixture).await;
    then.withdraw_can_build(&fixture).await;
}

#[serial]
#[tokio::test]
async fn withdraw_failed_resource_bypass_reopens_withdraw_build_flow() {
    let scenario = WithdrawResourceGateScenario::new().await;
    let fixture = WithdrawResourceGateFixture::origin_gate_case("W_RSC_FAIL");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.blocked_withdraw(&fixture).await;
    given.failed_withdraw_resource_delegation(&fixture).await;
    then.withdraw_cannot_build(&fixture).await;

    when.resource_receipt_upload_is_sent(&fixture).await;

    then.origin_withdraw_gate_is_released_by_failed_bypass(&fixture).await;
    then.withdraw_can_build(&fixture).await;
}

#[serial]
#[tokio::test]
async fn withdraw_resource_result_ack_without_origin_trade_no_does_not_release_gate() {
    let scenario = WithdrawResourceGateScenario::new().await;
    let fixture = WithdrawResourceGateFixture::origin_gate_case("W_RSC_NO_ORIGIN");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.blocked_withdraw(&fixture).await;
    given.resource_delegation_without_origin_trade(&fixture).await;

    when.resource_result_ack_is_sent(&fixture).await;

    then.origin_withdraw_gate_is_not_released(&fixture).await;
}

#[serial]
#[tokio::test]
async fn withdraw_resource_result_ack_for_collect_origin_does_not_release_withdraw_gate() {
    let scenario = WithdrawResourceGateScenario::new().await;
    let fixture = WithdrawResourceGateFixture::origin_gate_case("W_RSC_WRONG_ORIGIN");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.blocked_withdraw(&fixture).await;
    given.collect_origin_resource_delegation(&fixture).await;

    when.resource_result_ack_is_sent(&fixture).await;

    then.origin_withdraw_gate_is_not_released(&fixture).await;
}
