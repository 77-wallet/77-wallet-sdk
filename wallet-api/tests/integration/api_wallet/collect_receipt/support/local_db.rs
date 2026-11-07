use serde_json::Value;
use tempfile::TempDir;
use wallet_database::{
    ApiTransactionDbPool, SqliteContext,
    entities::api_collect::{ApiCollectEntity, ApiCollectStatus},
    repositories::api_wallet::collect::ApiCollectRepo,
};

use super::{
    db::{insert_collect, persist_rebuilt_execution_facts, persist_stale_build_facts},
    fixtures::CollectReceiptFixture,
    payload::collect_receipt_payload_json,
};

pub(crate) struct LocalCollectDb {
    _dir: TempDir,
    pool: ApiTransactionDbPool,
}

impl LocalCollectDb {
    pub(crate) async fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx =
            SqliteContext::new(dir.path().to_string_lossy().as_ref(), Some("api_transaction.db"))
                .await
                .expect("init api_transaction.db");
        let pool = ctx.into_transaction_db_pool().expect("transaction pool");
        Self { _dir: dir, pool }
    }

    pub(crate) async fn seed_stale_collect_build(&self, fixture: &CollectReceiptFixture) {
        insert_collect(
            &self.pool,
            &fixture.trade_no,
            &fixture.from_addr,
            &fixture.initial_to_addr,
            ApiCollectStatus::Init,
        )
        .await;
        persist_stale_build_facts(&self.pool, &fixture.trade_no).await;
    }

    pub(crate) async fn rebuild_collect_execution(&self, fixture: &CollectReceiptFixture) {
        ApiCollectRepo::invalidate_raw_tx_for_rebuild(&self.pool, &fixture.trade_no, None)
            .await
            .expect("invalidate raw tx for rebuild");
        persist_rebuilt_execution_facts(
            &self.pool,
            &fixture.trade_no,
            &fixture.receipt_to_addr,
            &fixture.tx_hash,
        )
        .await;
    }

    pub(crate) async fn receipt_payload_json(&self, fixture: &CollectReceiptFixture) -> Value {
        let rebuilt = self.load_collect(&fixture.trade_no).await;
        collect_receipt_payload_json(&rebuilt, &fixture.trade_no)
    }

    async fn load_collect(&self, trade_no: &str) -> ApiCollectEntity {
        ApiCollectRepo::get_api_collect_by_trade_no(&self.pool, trade_no)
            .await
            .expect("load collect")
    }
}
