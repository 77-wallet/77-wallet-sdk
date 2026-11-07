use wallet_api::testkit::{
    collect::scan_collect_intent_labels_once,
    context::{api_resource_test_pool, api_trans_test_ctx},
    resource_reclaim::scan_local_reclaim_intent_labels_once,
};
use wallet_database::ApiResourceDbPool;

use super::db::{insert_local_undelegation, mark_local_undelegation_broadcasted};

pub(crate) struct LocalReclaimScannerDb {
    pool: ApiResourceDbPool,
}

impl LocalReclaimScannerDb {
    pub(crate) async fn new() -> Self {
        let pool = api_resource_test_pool().await;

        Self { pool }
    }

    pub(crate) async fn given_local_undelegation_task(
        &self,
        resource_trade_no: &str,
        origin_trade_no: &str,
    ) {
        insert_local_undelegation(&self.pool, resource_trade_no, origin_trade_no).await;
    }

    pub(crate) async fn given_broadcasted_local_undelegation_task(
        &self,
        resource_trade_no: &str,
        origin_trade_no: &str,
        tx_hash: &str,
    ) {
        insert_local_undelegation(&self.pool, resource_trade_no, origin_trade_no).await;
        mark_local_undelegation_broadcasted(&self.pool, resource_trade_no, tx_hash).await;
    }

    pub(crate) async fn when_collect_scanner_runs(&self) -> Vec<String> {
        scan_collect_intent_labels_once(api_trans_test_ctx().await)
            .await
            .expect("scan collect labels")
    }

    pub(crate) async fn when_local_reclaim_scanner_runs(&self) -> Vec<String> {
        scan_local_reclaim_intent_labels_once(self.pool.clone()).await.expect("scan reclaim labels")
    }

    pub(crate) fn then_collect_scanner_does_not_own_local_undelegation(&self, labels: Vec<String>) {
        assert!(
            labels.iter().all(|label| label != "ExecuteLocalUndelegation"),
            "collect shadow should no longer own local undelegation execute"
        );
        assert!(
            labels.iter().all(|label| label != "RecoverLocalUndelegation"),
            "collect shadow should no longer own local undelegation recover"
        );
    }

    pub(crate) fn then_local_reclaim_scanner_owns_execute_and_recover(&self, labels: Vec<String>) {
        assert!(labels.iter().any(|label| label == "ExecuteLocalUndelegation"));
        assert!(labels.iter().any(|label| label == "RecoverLocalUndelegation"));
    }
}
