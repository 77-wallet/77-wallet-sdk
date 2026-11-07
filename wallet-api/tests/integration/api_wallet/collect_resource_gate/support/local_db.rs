use wallet_api::testkit::{
    collect::scan_collect_resource_receipt_intent_labels_once, mqtt::api_resource_pool,
};
use wallet_database::ApiResourceDbPool;

use crate::harness::{WorkerTestEnv, ensure_worker_env};

use super::{
    db::insert_failed_delegation_ready_for_receipt_scan, fixtures::CollectResourceGateFixture,
};

pub(crate) struct LocalCollectResourceDb {
    env: &'static WorkerTestEnv,
    pool: ApiResourceDbPool,
}

impl LocalCollectResourceDb {
    pub(crate) async fn new() -> Self {
        let env = ensure_worker_env().await;
        env.recorder.reset();
        let pool = api_resource_pool(env.ctx()).expect("api resource pool");
        Self { env, pool }
    }

    pub(crate) async fn given_failed_delegation_ready_for_receipt_scan(
        &self,
        fixture: &CollectResourceGateFixture,
    ) {
        insert_failed_delegation_ready_for_receipt_scan(&self.pool, &fixture.resource_trade_no)
            .await;
    }

    pub(crate) async fn when_collect_scanner_runs(&self) -> Vec<String> {
        scan_collect_resource_receipt_intent_labels_once(self.env.ctx())
            .await
            .expect("scanner round should succeed")
    }

    pub(crate) fn then_scanner_emits_resource_receipt_upload(&self, labels: Vec<String>) {
        assert!(
            labels.iter().any(|label| label == "UploadResourceTxExecReceipt"),
            "failed resource delegation should emit UploadResourceTxExecReceipt"
        );
    }
}
