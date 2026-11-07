mod support;

use support::{CollectFeeCycleFixture, LocalCollectFeeCycleDb};

#[tokio::test]
async fn collect_scanner_skips_stale_fee_cycle_rows() {
    let db = LocalCollectFeeCycleDb::new().await;
    let fixture = CollectFeeCycleFixture::stale_uploaded_fee();

    db.given_stale_fee_cycle_row(&fixture).await;

    let labels = db.when_collect_scanner_runs().await;

    db.then_stale_fee_cycle_row_is_skipped(&fixture, labels).await;
}

#[tokio::test]
async fn collect_scanner_emits_upload_service_fee_when_need_service_fee_is_true() {
    let db = LocalCollectFeeCycleDb::new().await;
    let fixture = CollectFeeCycleFixture::waiting_service_fee();

    db.given_waiting_service_fee_row(&fixture).await;

    let labels = db.when_collect_scanner_runs().await;

    db.then_upload_service_fee_is_selected_before_build(&fixture, labels).await;
}

#[tokio::test]
async fn collect_scanner_builds_after_fee_cycle_reopen_without_service_fee_upload() {
    let db = LocalCollectFeeCycleDb::new().await;
    let fixture = CollectFeeCycleFixture::reopened_without_fee_upload();

    db.given_reopened_without_service_fee_upload(&fixture).await;

    let labels = db.when_collect_scanner_runs().await;

    db.then_reopened_fee_cycle_continues_to_build(&fixture, labels).await;
}

#[tokio::test]
async fn collect_scanner_emits_tx_fee_res_ack_before_build_after_fee_result() {
    let db = LocalCollectFeeCycleDb::new().await;
    let fixture = CollectFeeCycleFixture::completed_fee_result();

    db.given_completed_fee_cycle_row(&fixture).await;

    let labels = db.when_collect_scanner_runs().await;

    db.then_tx_fee_res_ack_is_selected_before_build(&fixture, labels).await;
}
