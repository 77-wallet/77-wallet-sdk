mod support;

use serial_test::serial;

use support::{
    CollectServiceFeeUploadGiven, CollectServiceFeeUploadScenario, CollectServiceFeeUploadThen,
    CollectServiceFeeUploadWhen, ScenarioRoles,
};

#[serial]
#[tokio::test]
async fn collect_service_fee_upload_bypasses_local_sol_fee_gate() {
    let scenario = CollectServiceFeeUploadScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let _guard = given.sol_fee_shortage_adapter(false);
    let upload = given.sol_service_fee_upload_waiting("T_collect_service_fee_upload").await;

    when.service_fee_is_uploaded(&upload, "service fee upload should bypass local balance gate")
        .await;

    then.payload_routes_reverse_transfer(&upload);
    then.payload_amount_is(&upload, 0.00100588, "base Solana shortfall");
}

#[serial]
#[tokio::test]
async fn collect_service_fee_upload_includes_solana_recipient_ata_rent_when_missing() {
    let scenario = CollectServiceFeeUploadScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let _guard = given.sol_fee_shortage_adapter(true);
    let upload = given.sol_service_fee_upload_waiting("T_collect_service_fee_upload_ata").await;

    when.service_fee_is_uploaded(&upload, "service fee upload should include recipient ATA rent")
        .await;

    then.payload_trade_no_matches(&upload);
    then.payload_amount_is(&upload, 0.00350588, "Solana recipient ATA rent");
}

#[serial]
#[tokio::test]
async fn collect_eth_service_fee_upload_uses_exact_propose_maximum() {
    let scenario = CollectServiceFeeUploadScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let _guard = given.eth_fee_adapter(100_000_000_000_000_000u128, 0.000015);
    let upload = given.eth_service_fee_upload_waiting().await;

    when.service_fee_is_uploaded(
        &upload,
        "eth service fee upload should use the exact propose maximum",
    )
    .await;

    then.payload_routes_reverse_transfer(&upload);
    // Fixture: 23,100 gas × 2 gwei = 0.0000462 ETH; estimate/display amounts differ.
    then.payload_amount_is(&upload, 0.00005544, "exact propose maximum plus 20% funding headroom");
}
