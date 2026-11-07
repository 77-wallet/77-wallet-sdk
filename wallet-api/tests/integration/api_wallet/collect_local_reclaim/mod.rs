mod support;

use serial_test::serial;

use crate::harness::next_unique_id;

use support::LocalReclaimScannerDb;

#[tokio::test]
#[serial]
async fn collect_shadow_scanner_no_longer_owns_local_undelegation_intents() {
    let db = LocalReclaimScannerDb::new().await;
    let id = next_unique_id();

    db.given_local_undelegation_task(
        &format!("rsc_local_undelegate_scan_{id}"),
        &format!("C_SCAN_{id}"),
    )
    .await;

    let labels = db.when_collect_scanner_runs().await;

    db.then_collect_scanner_does_not_own_local_undelegation(labels);
}

#[tokio::test]
#[serial]
async fn local_reclaim_shadow_scanner_owns_local_undelegation_intents() {
    let db = LocalReclaimScannerDb::new().await;
    let id = next_unique_id();

    db.given_local_undelegation_task(
        &format!("rsc_local_undelegate_execute_scan_{id}"),
        &format!("C_EXECUTE_SCAN_{id}"),
    )
    .await;
    db.given_broadcasted_local_undelegation_task(
        &format!("rsc_local_undelegate_recover_scan_{id}"),
        &format!("C_RECOVER_SCAN_{id}"),
        &format!("tx_hash_recover_scan_{id}"),
    )
    .await;

    let labels = db.when_local_reclaim_scanner_runs().await;

    db.then_local_reclaim_scanner_owns_execute_and_recover(labels);
}
