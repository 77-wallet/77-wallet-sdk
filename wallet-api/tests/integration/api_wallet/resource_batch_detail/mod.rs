mod support;

use serial_test::serial;
use wallet_database::entities::bill::BillKind;

use support::{BatchResourceDetailScenario, BatchResourceFixture};

#[serial]
#[tokio::test]
async fn api_batch_delegate_success_is_queryable_from_api_bill_detail() {
    let scenario = BatchResourceDetailScenario::new();
    let fixture = BatchResourceFixture::delegate("batch_delegate_detail");

    scenario.given_successful_batch_broadcast(&fixture).await;

    let detail = scenario.when_bill_detail_is_requested(&fixture.tx_hash, &fixture.owner).await;

    assert_eq!(detail.bill.hash, fixture.tx_hash);
    assert_eq!(detail.bill.owner, fixture.owner);
    assert_eq!(detail.bill.to_addr, fixture.receiver);
    assert_eq!(detail.bill.value, fixture.amount.to_string());
    assert_eq!(detail.bill.transaction_fee, fixture.transaction_fee);
    assert_eq!(detail.bill.tx_kind, BillKind::BatchDelegateEnergy.to_i8());
}

#[serial]
#[tokio::test]
async fn api_batch_delegate_failed_broadcast_has_no_api_bill_detail() {
    let scenario = BatchResourceDetailScenario::new();
    let successful = BatchResourceFixture::delegate("batch_delegate_partial_success");
    let failed_hash = format!("{}_failed", successful.tx_hash);

    scenario.given_successful_batch_broadcast(&successful).await;

    scenario.then_bill_detail_is_missing(&failed_hash, &successful.owner).await;
    scenario.when_bill_detail_is_requested(&successful.tx_hash, &successful.owner).await;
}

#[serial]
#[tokio::test]
async fn api_batch_undelegate_success_is_queryable_from_api_bill_detail() {
    let scenario = BatchResourceDetailScenario::new();
    let fixture = BatchResourceFixture::undelegate("batch_undelegate_detail");

    scenario.given_successful_batch_undelegate_broadcast(&fixture).await;

    let detail = scenario.when_bill_detail_is_requested(&fixture.tx_hash, &fixture.owner).await;

    assert_eq!(detail.bill.hash, fixture.tx_hash);
    assert_eq!(detail.bill.to_addr, fixture.receiver);
    assert_eq!(detail.bill.value, fixture.amount.to_string());
    assert_eq!(detail.bill.tx_kind, BillKind::BatchUnDelegateBandwidth.to_i8());
}

#[serial]
#[tokio::test]
async fn api_single_delegate_success_is_queryable_from_api_bill_detail() {
    let scenario = BatchResourceDetailScenario::new();
    let fixture = BatchResourceFixture::delegate("single_delegate_detail");

    scenario.given_successful_single_delegate_broadcast(&fixture).await;

    let detail = scenario.when_bill_detail_is_requested(&fixture.tx_hash, &fixture.owner).await;
    assert_eq!(detail.bill.to_addr, fixture.receiver);
    assert_eq!(detail.bill.tx_kind, BillKind::DelegateEnergy.to_i8());
}

#[serial]
#[tokio::test]
async fn api_single_undelegate_success_is_queryable_from_api_bill_detail() {
    let scenario = BatchResourceDetailScenario::new();
    let fixture = BatchResourceFixture::undelegate("single_undelegate_detail");

    scenario.given_successful_single_undelegate_broadcast(&fixture).await;

    let detail = scenario.when_bill_detail_is_requested(&fixture.tx_hash, &fixture.owner).await;
    assert_eq!(detail.bill.to_addr, fixture.receiver);
    assert_eq!(detail.bill.tx_kind, BillKind::UnDelegateBandwidth.to_i8());
}
