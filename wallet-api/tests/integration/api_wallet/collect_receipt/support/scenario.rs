use crate::harness::{
    AssertRole, GivenRole, LoadRole, SeedRole, ThenRole, WhenRole, WorkerTestEnv,
    decrypt_captured_api_backend_body, ensure_worker_env, pop_request_with_retry,
};
use serde_json::Value;
use wallet_api::testkit::collect::{
    scan_and_dispatch_collect_tx_exec_receipt_once, upload_collect_tx_exec_receipt_via_backend,
    upload_collect_tx_exec_receipt_via_worker,
};
use wallet_database::{
    ApiTransactionDbPool, SqliteContext,
    entities::api_collect::{ApiCollectEntity, ApiCollectStatus},
    repositories::api_wallet::collect::ApiCollectRepo,
};

use super::{
    db::{
        insert_collect, persist_rebuilt_execution_facts, persist_scanner_receipt_facts,
        persist_stale_build_facts,
    },
    fixtures::CollectReceiptFixture,
    payload::{
        assert_collect_receipt_payload, assert_collect_tx_exec_receipt_uploaded,
        collect_receipt_payload_json,
    },
};

pub(crate) struct CollectReceiptScenario {
    env: &'static WorkerTestEnv,
    collect_pool: ApiTransactionDbPool,
    ctx: &'static wallet_api::Context,
}

impl CollectReceiptScenario {
    pub(crate) async fn new() -> Self {
        let env = ensure_worker_env().await;
        env.recorder.reset();

        let root = std::env::temp_dir().join("collect-receipt").join(format!(
            "{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let dirs = wallet_api::dirs::Dirs::new(root.to_str().unwrap()).unwrap();
        let config = wallet_api::config::Config::new(&format!(
            r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "{url}"
  test_url: "{url}"
  prod_url: "{url}"
aggregate_api:
  dev_url: "{url}"
  test_url: "{url}"
  prod_url: "{url}"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
"#,
            url = env.backend_url
        ))
        .unwrap();
        let ctx = Box::leak(Box::new(
            wallet_api::Context::new_without_handles_for_test(dirs, config).await.unwrap(),
        ));
        let collect_pool = SqliteContext::new(
            &ctx.get_global_dirs().db_dir.to_string_lossy(),
            Some("api_transaction.db"),
        )
        .await
        .unwrap()
        .into_transaction_db_pool()
        .unwrap();
        Self { env, collect_pool, ctx }
    }

    pub(crate) async fn given_finalized_conflict(&self, fixture: &CollectReceiptFixture) {
        use wallet_database::repositories::api_wallet::collect::FinalizedCollectNonceConflict;
        insert_collect(
            &self.collect_pool,
            &fixture.trade_no,
            &fixture.from_addr,
            &fixture.receipt_to_addr,
            ApiCollectStatus::SendingTx,
        )
        .await;
        sqlx::query(
            "UPDATE api_collect SET chain_code='eth', nonce=1, raw_tx='original-raw',
            tx_hash=?, order_ack_sent_at=datetime('now'), last_broadcast_at=datetime('now'),
            transaction_time=NULL WHERE trade_no=?",
        )
        .bind(&fixture.tx_hash)
        .bind(&fixture.trade_no)
        .execute(self.collect_pool.as_ref())
        .await
        .unwrap();
        let evidence = FinalizedCollectNonceConflict {
            chain_code: "eth".into(),
            from_addr: fixture.from_addr.trim().into(),
            nonce: 1,
            conflicting_tx_hash: format!("0x{}", "22".repeat(32)),
            block_hash: format!("0x{}", "aa".repeat(32)),
            block_number: 99,
        };
        assert_eq!(
            ApiCollectRepo::record_finalized_nonce_conflict(
                &self.collect_pool,
                &fixture.trade_no,
                &fixture.tx_hash,
                &evidence
            )
            .await
            .unwrap(),
            1
        );
    }

    pub(crate) fn given_backend_rejects_receipt(&self) {
        self.env.recorder.fail_next_api_backend_call(500, "temporary backend failure");
    }

    pub(crate) async fn when_receipt_upload_fails(&self, fixture: &CollectReceiptFixture) {
        assert!(
            upload_collect_tx_exec_receipt_via_worker(self.ctx, &fixture.trade_no).await.is_err()
        );
    }

    pub(crate) async fn then_conflict_receipt(
        &self,
        fixture: &CollectReceiptFixture,
        uploaded: bool,
        calls: usize,
    ) {
        let row =
            ApiCollectRepo::get_api_collect_by_trade_no(&self.collect_pool, &fixture.trade_no)
                .await
                .unwrap();
        assert_eq!(row.tx_hash.as_deref(), Some(fixture.tx_hash.as_str()));
        assert_eq!(row.raw_tx.as_deref(), Some("original-raw"));
        assert_eq!(row.nonce, 1);
        assert!(row.transaction_time.is_none());
        assert!(row.err_code.is_some());
        assert_eq!(row.tx_exec_receipt_uploaded_at.is_some(), uploaded);
        assert_eq!(row.finished_at.is_some(), uploaded);
        let requests = self.env.recorder.snapshot();
        let receipts: Vec<_> = requests
            .iter()
            .filter(|r| r.path.contains("awallet/aw/trans/executeComplete"))
            .collect();
        assert_eq!(receipts.len(), calls);
        for receipt in receipts {
            let payload = decrypt_captured_api_backend_body(&receipt.body);
            assert_eq!(payload["tradeNo"], fixture.trade_no);
            assert_eq!(payload["hash"], fixture.tx_hash);
            assert_eq!(payload["status"], "FAIL");
            assert!(
                payload["remark"].as_str().unwrap().contains(&format!("0x{}", "22".repeat(32)))
            );
        }
        let needs_upload =
            ApiCollectRepo::scan_need_tx_exec_receipt_upload(&self.collect_pool, 10000)
                .await
                .unwrap()
                .iter()
                .any(|r| r.trade_no == fixture.trade_no);
        assert_eq!(needs_upload, !uploaded);
    }

    fn seed(&self) -> SeedRole<'_, Self> {
        SeedRole::new(self)
    }

    fn load(&self) -> LoadRole<'_, Self> {
        LoadRole::new(self)
    }

    fn assert(&self) -> AssertRole<'_, Self> {
        AssertRole::new(self)
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectReceiptGiven {
    async fn mock_backend_is_active(&self);

    async fn rebuilt_collect_execution(&self, fixture: &CollectReceiptFixture);

    async fn scanner_ready_collect_execution(&self, fixture: &CollectReceiptFixture);
}

#[async_trait::async_trait(?Send)]
impl CollectReceiptGiven for GivenRole<'_, CollectReceiptScenario> {
    async fn mock_backend_is_active(&self) {
        let backend_url = current_backend_url().await.expect("backend url set in app state");
        self.scenario()
            .assert()
            .mock_backend_is_active(&backend_url, &self.scenario().env.backend_url);
    }

    async fn rebuilt_collect_execution(&self, fixture: &CollectReceiptFixture) {
        self.scenario().seed().collect_order(fixture, ApiCollectStatus::Init).await;
        self.scenario().seed().stale_build_facts(&fixture.trade_no).await;
        self.scenario().seed().raw_tx_invalidated_for_rebuild(&fixture.trade_no).await;
        self.scenario().seed().rebuilt_execution_facts(fixture).await;
    }

    async fn scanner_ready_collect_execution(&self, fixture: &CollectReceiptFixture) {
        self.scenario().seed().collect_order(fixture, ApiCollectStatus::SendingTx).await;
        self.scenario().seed().rebuilt_execution_facts(fixture).await;
        self.scenario().seed().scanner_receipt_facts(fixture).await;
    }
}

#[async_trait::async_trait(?Send)]
trait CollectReceiptSeed {
    async fn collect_order(&self, fixture: &CollectReceiptFixture, status: ApiCollectStatus);

    async fn stale_build_facts(&self, trade_no: &str);

    async fn raw_tx_invalidated_for_rebuild(&self, trade_no: &str);

    async fn rebuilt_execution_facts(&self, fixture: &CollectReceiptFixture);

    async fn scanner_receipt_facts(&self, fixture: &CollectReceiptFixture);
}

#[async_trait::async_trait(?Send)]
impl CollectReceiptSeed for SeedRole<'_, CollectReceiptScenario> {
    async fn collect_order(&self, fixture: &CollectReceiptFixture, status: ApiCollectStatus) {
        insert_collect(
            &self.scenario().collect_pool,
            &fixture.trade_no,
            &fixture.from_addr,
            &fixture.initial_to_addr,
            status,
        )
        .await;
    }

    async fn stale_build_facts(&self, trade_no: &str) {
        persist_stale_build_facts(&self.scenario().collect_pool, trade_no).await;
    }

    async fn raw_tx_invalidated_for_rebuild(&self, trade_no: &str) {
        ApiCollectRepo::invalidate_raw_tx_for_rebuild(
            &self.scenario().collect_pool,
            trade_no,
            None,
        )
        .await
        .expect("invalidate raw tx for rebuild");
    }

    async fn rebuilt_execution_facts(&self, fixture: &CollectReceiptFixture) {
        persist_rebuilt_execution_facts(
            &self.scenario().collect_pool,
            &fixture.trade_no,
            &fixture.receipt_to_addr,
            &fixture.tx_hash,
        )
        .await;
    }

    async fn scanner_receipt_facts(&self, fixture: &CollectReceiptFixture) {
        persist_scanner_receipt_facts(
            &self.scenario().collect_pool,
            &fixture.trade_no,
            &fixture.tx_hash,
        )
        .await;
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectReceiptWhen {
    async fn worker_uploads_receipt(&self, fixture: &CollectReceiptFixture);

    async fn direct_backend_uploads_receipt(&self, fixture: &CollectReceiptFixture);

    async fn scanner_dispatches_receipt(&self) -> Option<String>;
}

#[async_trait::async_trait(?Send)]
impl CollectReceiptWhen for WhenRole<'_, CollectReceiptScenario> {
    async fn worker_uploads_receipt(&self, fixture: &CollectReceiptFixture) {
        upload_collect_tx_exec_receipt_via_worker(self.scenario().ctx, &fixture.trade_no)
            .await
            .expect("upload tx exec receipt should succeed");
    }

    async fn direct_backend_uploads_receipt(&self, fixture: &CollectReceiptFixture) {
        let req = fixture.receipt_entity();

        upload_collect_tx_exec_receipt_via_backend(self.scenario().ctx, &req, &req.trade_no)
            .await
            .expect("direct backend upload should succeed");
    }

    async fn scanner_dispatches_receipt(&self) -> Option<String> {
        scan_and_dispatch_collect_tx_exec_receipt_once(
            self.scenario().ctx,
            self.scenario().collect_pool.clone(),
        )
        .await
        .expect("scanner-dispatcher flow should succeed")
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectReceiptThen {
    async fn receipt_upload_is_persisted(&self, fixture: &CollectReceiptFixture);

    async fn receipt_payload_uses_execution_facts(&self, fixture: &CollectReceiptFixture);

    async fn backend_received_execute_complete(&self, fixture: &CollectReceiptFixture);

    fn scanner_selected_trade(
        &self,
        dispatched_trade_no: Option<String>,
        fixture: &CollectReceiptFixture,
    );
}

#[async_trait::async_trait(?Send)]
impl CollectReceiptThen for ThenRole<'_, CollectReceiptScenario> {
    async fn receipt_upload_is_persisted(&self, fixture: &CollectReceiptFixture) {
        let rec = self.scenario().load().collect(&fixture.trade_no).await;
        self.scenario().assert().tx_exec_receipt_uploaded(&rec);
    }

    async fn receipt_payload_uses_execution_facts(&self, fixture: &CollectReceiptFixture) {
        let rec = self.scenario().load().collect(&fixture.trade_no).await;
        self.scenario().assert().receipt_payload_uses_execution_facts(&rec, fixture);
    }

    async fn backend_received_execute_complete(&self, fixture: &CollectReceiptFixture) {
        let payload_json = self.scenario().load().execute_complete_payload().await;
        self.scenario().assert().backend_received_execute_complete(&payload_json, fixture);
    }

    fn scanner_selected_trade(
        &self,
        dispatched_trade_no: Option<String>,
        fixture: &CollectReceiptFixture,
    ) {
        self.scenario().assert().scanner_selected_trade(dispatched_trade_no, fixture);
    }
}

#[async_trait::async_trait(?Send)]
trait CollectReceiptLoad {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity;

    async fn execute_complete_payload(&self) -> Value;
}

#[async_trait::async_trait(?Send)]
impl CollectReceiptLoad for LoadRole<'_, CollectReceiptScenario> {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity {
        ApiCollectRepo::get_api_collect_by_trade_no(&self.scenario().collect_pool, trade_no)
            .await
            .expect("load collect")
    }

    async fn execute_complete_payload(&self) -> Value {
        let mut seen_paths = Vec::new();

        for _ in 0..20 {
            let captured = pop_request_with_retry(&self.scenario().env.recorder)
                .await
                .expect("captured backend request for direct upload");

            if captured.path.contains("awallet/aw/trans/executeComplete") {
                return decrypt_captured_api_backend_body(&captured.body);
            }

            seen_paths.push(captured.path);
        }

        panic!("expected execute-complete backend request, saw paths: {seen_paths:?}");
    }
}

trait CollectReceiptAssert {
    fn mock_backend_is_active(&self, backend_url: &str, expected_backend_url: &str);

    fn tx_exec_receipt_uploaded(&self, collect: &ApiCollectEntity);

    fn receipt_payload_uses_execution_facts(
        &self,
        collect: &ApiCollectEntity,
        fixture: &CollectReceiptFixture,
    );

    fn backend_received_execute_complete(
        &self,
        payload_json: &Value,
        fixture: &CollectReceiptFixture,
    );

    fn scanner_selected_trade(
        &self,
        dispatched_trade_no: Option<String>,
        fixture: &CollectReceiptFixture,
    );
}

impl CollectReceiptAssert for AssertRole<'_, CollectReceiptScenario> {
    fn mock_backend_is_active(&self, backend_url: &str, expected_backend_url: &str) {
        assert_eq!(backend_url, expected_backend_url, "worker should use the mock backend URL");
    }

    fn tx_exec_receipt_uploaded(&self, collect: &ApiCollectEntity) {
        assert_collect_tx_exec_receipt_uploaded(collect);
    }

    fn receipt_payload_uses_execution_facts(
        &self,
        collect: &ApiCollectEntity,
        fixture: &CollectReceiptFixture,
    ) {
        let payload_json = collect_receipt_payload_json(collect, &fixture.trade_no);

        assert_collect_receipt_payload(
            &payload_json,
            &fixture.trade_no,
            &fixture.receipt_to_addr,
            &fixture.tx_hash,
        );
    }

    fn backend_received_execute_complete(
        &self,
        payload_json: &Value,
        fixture: &CollectReceiptFixture,
    ) {
        assert_collect_receipt_payload(
            payload_json,
            &fixture.trade_no,
            &fixture.receipt_to_addr,
            &fixture.tx_hash,
        );
    }

    fn scanner_selected_trade(
        &self,
        dispatched_trade_no: Option<String>,
        fixture: &CollectReceiptFixture,
    ) {
        assert_eq!(dispatched_trade_no.as_deref(), Some(fixture.trade_no.as_str()));
    }
}

async fn current_backend_url() -> Option<String> {
    let app_state = wallet_api::app_state::APP_STATE.read().await;
    app_state.url().backend.clone()
}

impl CollectReceiptScenario {
    pub(crate) async fn given_signed_replacement(&self, fixture: &CollectReceiptFixture) {
        use wallet_database::repositories::api_wallet::{
            collect::SignedCollectCandidate,
            collect_replacement::{CollectReplacementRepo, CollectReplacementScope},
            nonce::ApiNonceRepo,
        };
        let old = format!("old-{}", fixture.trade_no);
        for trade in [&old, &fixture.trade_no] {
            ApiCollectRepo::upsert_api_collect(
                &self.collect_pool,
                "uid",
                "collect",
                &fixture.from_addr,
                &fixture.receipt_to_addr,
                "1.12",
                "digest",
                "scroll",
                None,
                "ETH",
                trade,
                2,
                ApiCollectStatus::Init,
                1,
            )
            .await
            .unwrap();
            ApiCollectRepo::set_order_ack_sent(&self.collect_pool, trade).await.unwrap();
            ApiCollectRepo::update_building_at(&self.collect_pool, trade).await.unwrap();
        }
        let old_hash = format!("0x{}", "33".repeat(32));
        for (trade, hash, raw) in [
            (&old, &old_hash, "old-synthetic-raw"),
            (&fixture.trade_no, &fixture.tx_hash, "new-synthetic-raw"),
        ] {
            ApiCollectRepo::reserve_signed_collect(
                &self.collect_pool,
                &SignedCollectCandidate {
                    trade_no: trade,
                    from_addr: &fixture.from_addr,
                    chain_code: "scroll",
                    tx_hash: hash,
                    raw_tx: raw,
                    transaction_fee: "0.1",
                    nonce: 6,
                    chain_next: 6,
                },
            )
            .await
            .unwrap();
            if trade == &old {
                ApiCollectRepo::fail_verified_nonce_gap(&self.collect_pool, &old, 5, 6)
                    .await
                    .unwrap();
                sqlx::query("UPDATE api_collect SET tx_exec_receipt_uploaded_at=CURRENT_TIMESTAMP WHERE trade_no=?").bind(&old).execute(self.collect_pool.write_ref()).await.unwrap();
                ApiNonceRepo::set_nonce_floor(
                    &self.collect_pool,
                    &fixture.from_addr,
                    "scroll",
                    153,
                )
                .await
                .unwrap();
                CollectReplacementRepo::prepare(
                    &self.collect_pool,
                    &fixture.trade_no,
                    &CollectReplacementScope {
                        chain_code: "scroll".into(),
                        from_addr: fixture.from_addr.clone(),
                        old_trade_no: old.clone(),
                        old_hash: old_hash.clone(),
                        nonce: 6,
                        legacy_counter: 153,
                    },
                    6,
                )
                .await
                .unwrap()
                .unwrap();
            }
        }
    }

    pub(crate) async fn when_replacement_winner_is_reconciled(
        &self,
        fixture: &CollectReceiptFixture,
    ) {
        use wallet_database::repositories::api_wallet::collect_replacement::{
            CollectReplacementRepo, ReplacementResolution,
        };
        let block = format!("0x{}", "ab".repeat(32));
        let proof = ReplacementResolution {
            transaction_fee: "0.000000000000000123",
            resource_consume: "21000",
            winner_hash: &fixture.tx_hash,
            success: true,
            block_hash: &block,
            block_number: 42,
            transaction_time: "2026-10-19T00:00:00Z",
        };
        assert!(
            CollectReplacementRepo::resolve(&self.collect_pool, &fixture.trade_no, &proof)
                .await
                .unwrap()
        );
        assert!(
            !CollectReplacementRepo::resolve(&self.collect_pool, &fixture.trade_no, &proof)
                .await
                .unwrap()
        );
    }

    pub(crate) async fn then_replacement_upload_and_counter_are_recovered(
        &self,
        fixture: &CollectReceiptFixture,
    ) {
        use wallet_database::repositories::api_wallet::{
            collect_replacement::CollectReplacementRepo, nonce::ApiNonceRepo,
        };
        let row =
            ApiCollectRepo::get_api_collect_by_trade_no(&self.collect_pool, &fixture.trade_no)
                .await
                .unwrap();
        assert_eq!(row.nonce, 6);
        assert!(row.transaction_time.is_some());
        assert!(row.tx_exec_receipt_uploaded_at.is_some());
        assert!(row.err_code.is_none());
        assert_eq!(row.transaction_fee, "0.000000000000000123");
        let relation = CollectReplacementRepo::find(&self.collect_pool, &fixture.trade_no)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(relation.winner_hash.as_deref(), Some(fixture.tx_hash.as_str()));
        assert_eq!(relation.legacy_counter, 153);
        assert_eq!(
            ApiNonceRepo::get_api_nonce(&self.collect_pool, &fixture.from_addr, "scroll")
                .await
                .unwrap(),
            6
        );
        let requests = self.env.recorder.snapshot();
        let receipts: Vec<_> = requests
            .iter()
            .filter(|r| r.path.contains("awallet/aw/trans/executeComplete"))
            .collect();
        assert_eq!(receipts.len(), 1);
        let payload = decrypt_captured_api_backend_body(&receipts[0].body);
        assert_collect_receipt_payload(
            &payload,
            &fixture.trade_no,
            &fixture.receipt_to_addr,
            &fixture.tx_hash,
        );
        let old =
            ApiCollectRepo::get_api_collect_by_trade_no(&self.collect_pool, &relation.old_trade_no)
                .await
                .unwrap();
        assert!(old.err_code.is_some());
        assert_eq!(old.tx_hash.as_deref(), Some(relation.old_hash.as_str()));
    }
}
