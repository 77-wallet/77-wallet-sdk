use wallet_api::testkit::{
    collect::scan_collect_intent_labels_once,
    context::{api_trans_test_ctx, api_trans_test_pool},
};
use wallet_database::{
    ApiTransactionDbPool,
    entities::api_collect::{ApiCollectEntity, ApiCollectStatus},
    repositories::api_wallet::collect::ApiCollectRepo,
};

use super::fixtures::CollectRecoveryFixture;

pub(crate) struct LocalCollectRecoveryDb {
    pool: ApiTransactionDbPool,
}

impl LocalCollectRecoveryDb {
    pub(crate) async fn new() -> Self {
        let pool = api_trans_test_pool().await;
        Self { pool }
    }

    pub(crate) async fn given_stale_blockhash_build(&self, fixture: &CollectRecoveryFixture) {
        ApiCollectRepo::upsert_api_collect(
            &self.pool,
            "uid",
            "collect",
            "from",
            "old-to",
            "1.12",
            "digest",
            "sol",
            Some("token".to_string()),
            "USDC",
            &fixture.trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await
        .expect("insert collect");

        sqlx::query(
            r#"
            UPDATE api_collect
            SET raw_tx = $2,
                tx_hash = $3,
                status = $4,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
            "#,
        )
        .bind(&fixture.trade_no)
        .bind("{\"stale\":true}")
        .bind(&fixture.tx_hash)
        .bind(ApiCollectStatus::SendingTx)
        .execute(self.pool.as_ref())
        .await
        .expect("set stale build facts");
    }

    pub(crate) async fn when_raw_tx_is_invalidated_for_rebuild(
        &self,
        fixture: &CollectRecoveryFixture,
    ) -> u64 {
        ApiCollectRepo::invalidate_raw_tx_for_rebuild(&self.pool, &fixture.trade_no, None)
            .await
            .expect("invalidate raw tx for rebuild")
    }

    pub(crate) async fn then_stale_build_facts_are_cleared(
        &self,
        fixture: &CollectRecoveryFixture,
        invalidated: u64,
    ) {
        assert_eq!(invalidated, 1);

        let after_invalidate = self.load_collect(&fixture.trade_no).await;
        assert!(after_invalidate.raw_tx.is_none(), "stale raw_tx must be cleared");
        assert!(after_invalidate.tx_hash.is_none(), "stale tx_hash must be cleared");
        assert_eq!(
            after_invalidate.to_addr, "old-to",
            "rebuild invalidation must not invent a new execution address on its own"
        );
    }

    pub(crate) async fn when_rebuilt_to_addr_is_persisted(
        &self,
        fixture: &CollectRecoveryFixture,
        to_addr: &str,
    ) {
        ApiCollectRepo::update_api_collect_to_addr(&self.pool, &fixture.trade_no, to_addr)
            .await
            .expect("persist rebuilt to_addr");
    }

    pub(crate) async fn then_rebuilt_to_addr_is_persisted(
        &self,
        fixture: &CollectRecoveryFixture,
        to_addr: &str,
    ) {
        let rebuilt = self.load_collect(&fixture.trade_no).await;
        assert!(rebuilt.raw_tx.is_none(), "rebuild starts from cleared build facts");
        assert!(rebuilt.tx_hash.is_none(), "rebuild starts from cleared tx hash");
        assert_eq!(
            rebuilt.to_addr, to_addr,
            "next build must persist the latest strategy address before generating new raw_tx"
        );
    }

    pub(crate) async fn given_broadcast_visible_pending_collect(
        &self,
        fixture: &CollectRecoveryFixture,
    ) {
        ApiCollectRepo::upsert_api_collect(
            &self.pool,
            "uid",
            "collect",
            "from-recover",
            "to-recover",
            "1.12",
            "digest",
            "eth",
            None,
            "USDC",
            &fixture.trade_no,
            2,
            ApiCollectStatus::SendingTx,
            1,
        )
        .await
        .expect("insert collect");

        sqlx::query(
            r#"
            UPDATE api_collect
            SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                raw_tx = '{"tx":true}',
                tx_hash = $2,
                last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
            "#,
        )
        .bind(&fixture.trade_no)
        .bind(&fixture.tx_hash)
        .execute(self.pool.as_ref())
        .await
        .expect("seed recoverable collect row");
    }

    pub(crate) async fn when_collect_scanner_runs(&self) -> Vec<String> {
        scan_collect_intent_labels_once(api_trans_test_ctx().await)
            .await
            .expect("scanner round should succeed")
    }

    pub(crate) fn then_scanner_emits_recover_only(&self, labels: Vec<String>) {
        assert!(
            labels.iter().any(|label| label == "RecoverTx"),
            "broadcast-visible pending collect row must emit RecoverTx"
        );
        assert!(
            labels.iter().all(|label| label != "BuildTx"),
            "recoverable row should not re-enter build"
        );
        assert!(
            labels.iter().all(|label| label != "UploadServiceFee"),
            "recoverable row should not go back to fee upload"
        );
    }

    pub(crate) async fn then_recoverable_row_stays_pending(
        &self,
        fixture: &CollectRecoveryFixture,
    ) {
        let persisted_after = self.load_collect(&fixture.trade_no).await;
        assert_eq!(persisted_after.tx_hash.as_deref(), Some(fixture.tx_hash.as_str()));
        assert!(persisted_after.transaction_time.is_none());
    }

    async fn load_collect(&self, trade_no: &str) -> ApiCollectEntity {
        ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, trade_no)
            .await
            .expect("load collect")
    }
}
