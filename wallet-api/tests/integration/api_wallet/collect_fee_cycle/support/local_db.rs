use wallet_api::testkit::{
    collect::scan_collect_intent_labels_once,
    context::{api_trans_test_ctx, api_trans_test_pool},
};
use wallet_database::ApiTransactionDbPool;

use super::{
    db::{
        insert_collect, load_collect, mark_completed_fee_cycle_row,
        mark_reopened_without_fee_upload, mark_stale_fee_cycle_row, mark_waiting_service_fee_row,
    },
    fixtures::CollectFeeCycleFixture,
};

pub(crate) struct LocalCollectFeeCycleDb {
    pool: ApiTransactionDbPool,
}

impl LocalCollectFeeCycleDb {
    pub(crate) async fn new() -> Self {
        let pool = api_trans_test_pool().await;
        Self { pool }
    }

    pub(crate) async fn given_stale_fee_cycle_row(&self, fixture: &CollectFeeCycleFixture) {
        insert_collect(&self.pool, fixture).await;
        mark_stale_fee_cycle_row(&self.pool, fixture).await;
    }

    pub(crate) async fn given_waiting_service_fee_row(&self, fixture: &CollectFeeCycleFixture) {
        insert_collect(&self.pool, fixture).await;
        mark_waiting_service_fee_row(&self.pool, fixture).await;
    }

    pub(crate) async fn given_reopened_without_service_fee_upload(
        &self,
        fixture: &CollectFeeCycleFixture,
    ) {
        insert_collect(&self.pool, fixture).await;
        mark_reopened_without_fee_upload(&self.pool, fixture).await;
    }

    pub(crate) async fn given_completed_fee_cycle_row(&self, fixture: &CollectFeeCycleFixture) {
        insert_collect(&self.pool, fixture).await;
        mark_completed_fee_cycle_row(&self.pool, fixture).await;
    }

    pub(crate) async fn when_collect_scanner_runs(&self) -> Vec<String> {
        scan_collect_intent_labels_once(api_trans_test_ctx().await)
            .await
            .expect("scanner round should succeed")
    }

    pub(crate) async fn then_stale_fee_cycle_row_is_skipped(
        &self,
        fixture: &CollectFeeCycleFixture,
        labels: Vec<String>,
    ) {
        assert!(
            labels.is_empty(),
            "stale fee-cycle row must not re-enter build / fee-ack scanning"
        );

        let persisted = load_collect(&self.pool, fixture).await;
        assert_eq!(persisted.need_service_fee, Some(true));
        assert!(persisted.service_fee_uploaded_at.is_some());
        assert!(persisted.raw_tx.is_none());
        assert!(persisted.tx_hash.is_none());
    }

    pub(crate) async fn then_upload_service_fee_is_selected_before_build(
        &self,
        fixture: &CollectFeeCycleFixture,
        labels: Vec<String>,
    ) {
        assert!(
            labels.iter().any(|label| label == "UploadServiceFee"),
            "active fee-wait row must emit UploadServiceFee immediately"
        );
        assert!(
            labels.iter().all(|label| label != "BuildTx"),
            "fee upload should not bypass fee-cycle gating into build"
        );

        let persisted_after = load_collect(&self.pool, fixture).await;
        assert_eq!(persisted_after.need_service_fee, Some(true));
        assert!(persisted_after.service_fee_order_received_at.is_none());
        assert!(persisted_after.service_fee_uploaded_at.is_none());
        assert!(persisted_after.raw_tx.is_none());
        assert!(persisted_after.tx_hash.is_none());
    }

    pub(crate) async fn then_reopened_fee_cycle_continues_to_build(
        &self,
        fixture: &CollectFeeCycleFixture,
        labels: Vec<String>,
    ) {
        assert!(
            labels.iter().any(|label| label == "BuildTx"),
            "reopened row without a real fee upload must continue to BuildTx"
        );
        assert!(
            labels.iter().all(|label| label != "SendTxFeeResAck"),
            "reopened row without service_fee_uploaded_at must not ask for TxFeeResAck"
        );

        let persisted_after = load_collect(&self.pool, fixture).await;
        assert_eq!(persisted_after.need_service_fee, Some(false));
        assert!(persisted_after.service_fee_uploaded_at.is_none());
        assert!(persisted_after.tx_fee_res_ack_sent_at.is_none());
        assert!(persisted_after.raw_tx.is_none());
        assert!(persisted_after.tx_hash.is_none());
    }

    pub(crate) async fn then_tx_fee_res_ack_is_selected_before_build(
        &self,
        fixture: &CollectFeeCycleFixture,
        labels: Vec<String>,
    ) {
        assert!(
            labels.iter().any(|label| label == "SendTxFeeResAck"),
            "fee-result row must emit TxFeeResAck"
        );
        assert!(
            labels.iter().all(|label| label != "BuildTx"),
            "fee-result ACK must be sent before build is allowed again"
        );

        let persisted_after = load_collect(&self.pool, fixture).await;
        assert_eq!(persisted_after.need_service_fee, Some(false));
        assert!(persisted_after.tx_fee_res_ack_sent_at.is_none());
        assert!(persisted_after.raw_tx.is_none());
    }
}
