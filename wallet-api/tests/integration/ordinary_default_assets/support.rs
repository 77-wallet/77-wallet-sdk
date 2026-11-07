use crate::harness::worker::{MockBackendRecorder, MockResponseGate, start_mock_backend_server};
use std::sync::Arc;
use wallet_api::{
    Context,
    testkit::{chain_sync, mqtt},
};
use wallet_database::CoreDbPool;

pub(super) struct OrdinaryDefaultAssetsScenario {
    ctx: &'static Context,
    pool: CoreDbPool,
    recorder: MockBackendRecorder,
    _dir: tempfile::TempDir,
    api_asset_count: i64,
    config_text: String,
    events: std::sync::Mutex<
        tokio::sync::mpsc::UnboundedReceiver<wallet_api::messaging::notify::FrontendNotifyEvent>,
    >,
}
impl OrdinaryDefaultAssetsScenario {
    pub async fn new() -> anyhow::Result<Self> {
        let (url, recorder) = start_mock_backend_server()?;
        unsafe {
            std::env::set_var("WALLET_TRANSPORT_NO_PROXY", "1");
        }
        let config_text = format!(
            r#"
app_code: "test"
crypto: {{ aes_key: "1234567890abcdef", aes_iv: "abcdef1234567890" }}
backend_api: {{ dev_url: "{url}", test_url: "{url}", prod_url: "{url}" }}
aggregate_api: {{ dev_url: "{url}", test_url: "{url}", prod_url: "{url}" }}
oss: {{ access_key_id: "test", access_key_secret: "test", bucket_name: "test", endpoint: "test" }}
"#
        );
        let config = wallet_api::config::Config::new(&config_text)?;
        let dir = tempfile::tempdir()?;
        let ctx = Box::leak(Box::new(
            Context::new_without_handles_for_test(
                wallet_api::dirs::Dirs::new(
                    &std::env::var("CATALOG_RESTART_TEST_DIR")
                        .unwrap_or_else(|_| dir.path().to_str().unwrap().to_owned()),
                )?,
                config,
            )
            .await?,
        ));
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        ctx.set_frontend_notify_sender(Some(sender)).await?;
        let pool = chain_sync::core_pool(ctx)?;
        if !std::env::var("CATALOG_RESTART_TEST_PHASE").unwrap_or_default().starts_with("resume") {
            sqlx::query("INSERT INTO wallet(name,uid,address,status,is_init,created_at) VALUES ('fixture','fixture','wallet',1,1,'2026-01-01T00:00:00Z')").execute(pool.write_ref()).await?;
            sqlx::query("INSERT INTO chain(name,chain_code,protocols,main_symbol,status,created_at) VALUES ('Scroll','scroll','[]','ETH',1,'2026-01-01T00:00:00Z')").execute(pool.write_ref()).await?;
            sqlx::query("INSERT INTO account(account_id,name,address,wallet_address,chain_code,status,is_init,created_at) VALUES (1,'fixture','owner','wallet','scroll',1,1,'2026-01-01T00:00:00Z')").execute(pool.write_ref()).await?;
        }
        let api_asset_count = sqlx::query_scalar("SELECT COUNT(*) FROM api_assets")
            .fetch_one(mqtt::api_wallet_pool(ctx)?.read_ref())
            .await?;
        Ok(Self {
            ctx,
            pool,
            recorder,
            _dir: dir,
            api_asset_count,
            config_text,
            events: std::sync::Mutex::new(receiver),
        })
    }
    /// Only used after directly awaited syncs, with no spawned work or external Context references.
    pub async fn when_context_is_rebuilt(self) -> anyhow::Result<Self> {
        let Self { ctx, pool, recorder, _dir, api_asset_count, config_text, events } = self;
        pool.read_ref().close().await;
        pool.write_ref().close().await;
        macro_rules! close_pool {
            ($pool:expr) => {{
                let pool = $pool?;
                pool.read_ref().close().await;
                pool.write_ref().close().await;
            }};
        }
        close_pool!(mqtt::api_wallet_pool(ctx));
        close_pool!(mqtt::api_transaction_pool(ctx));
        close_pool!(mqtt::api_resource_pool(ctx));
        close_pool!(mqtt::task_pool(ctx));
        assert!(pool.read_ref().is_closed());
        assert!(pool.write_ref().is_closed());
        drop(pool);
        drop(events);
        // The existing testkit requires &'static Context. Keep its original allocation
        // leaked rather than reclaiming a potentially referenced object. All old DB
        // connections are closed; the replacement must reopen the same disk files.
        let ctx = Box::leak(Box::new(
            Context::new_without_handles_for_test(
                wallet_api::dirs::Dirs::new(_dir.path().to_str().unwrap())?,
                wallet_api::config::Config::new(&config_text)?,
            )
            .await?,
        ));
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        ctx.set_frontend_notify_sender(Some(sender)).await?;
        let pool = chain_sync::core_pool(ctx)?;
        assert!(!pool.read_ref().is_closed());
        Ok(Self {
            ctx,
            pool,
            recorder,
            _dir,
            api_asset_count,
            config_text,
            events: std::sync::Mutex::new(receiver),
        })
    }
    pub fn then_last_filters_are(&self, created: &str, updated: &str) {
        let requests: Vec<serde_json::Value> = self
            .recorder
            .snapshot()
            .iter()
            .filter(|call| call.path.ends_with("token/queryAllExcludeCustomByPage"))
            .map(|call| serde_json::from_str(&call.body).unwrap())
            .collect();
        let last = &requests[requests.len() - 2..];
        assert_eq!(last[0]["updateTime"], updated);
        assert!(last[0].get("createTime").is_none());
        assert_eq!(last[1]["createTime"], created);
        assert!(last[1].get("updateTime").is_none());
    }
    fn page(&self, symbol: &str, token: &str) {
        self.page_total(symbol, token, 2);
    }
    fn page_total(&self, symbol: &str, token: &str, total_page: i32) {
        let created = if symbol == "USDT" { "2026-10-01 00:00:00" } else { "2026-10-09 00:00:00" };
        self.recorder.succeed_next_backend_call_with_data(serde_json::json!({
            "totalPage":total_page,"pageSize":500,"totalCount":2,"list":[{
                "id":symbol,"code":symbol,"name":symbol,"chainCode":"scroll","contractAddress":token,
                "unit":6,"enable":true,"defaultToken":true,"popularToken":true,"protocol":"ERC20","price":1.0,
                "createTime":created,"updateTime":"2026-10-09 00:00:00"
            }]
        }));
    }
    pub async fn given_large_legacy_catalog(&self) -> anyhow::Result<()> {
        use wallet_database::{
            entities::{asset_token_key::AssetTokenKey, coin::CoinData},
            repositories::coin::CoinRepo,
        };
        let created = "2026-10-01T00:00:00Z".parse()?;
        let mut rows = vec![CoinData::new(
            Some("USDT".into()),
            "USDT",
            "scroll",
            AssetTokenKey::from_raw(Some("0xtoken-a")),
            Some("1".into()),
            Some("ERC20".into()),
            6,
            0,
            0,
            1,
            false,
            created,
            created,
        )];
        let latest = "2026-10-08T00:00:00Z".parse()?;
        for i in 0..500 {
            rows.push(CoinData::new(
                Some("fixture".into()),
                &format!("FIXTURE{i}"),
                "scroll",
                AssetTokenKey::from_raw(Some(&format!("0xfixture{i}"))),
                Some("1".into()),
                Some("ERC20".into()),
                6,
                0,
                0,
                1,
                false,
                latest,
                latest,
            ));
        }
        CoinRepo::upsert_multi_coin(&self.pool, rows).await?;
        Ok(())
    }
    pub async fn then_legacy_metadata_is_unchanged(&self) -> anyhow::Result<()> {
        let row: (i64, String) = sqlx::query_as(
            "SELECT is_default, updated_at FROM coin WHERE symbol='USDT' AND token_address='0xtoken-a'",
        ).fetch_one(self.pool.read_ref()).await?;
        assert_eq!(row.0, 0);
        assert!(row.1.starts_with("2026-10-01"));
        Ok(())
    }
    pub async fn then_legacy_repair_and_new_coin_are_persisted(&self) -> anyhow::Result<()> {
        let rows: Vec<(String, i64, String, String)> = sqlx::query_as(
            "SELECT symbol,is_default,created_at,updated_at FROM coin WHERE token_address IN ('0xtoken-a','0xtoken-b') ORDER BY symbol",
        ).fetch_all(self.pool.read_ref()).await?;
        assert_eq!(rows.len(), 2);
        for (symbol, default, created, updated) in rows {
            assert_eq!(default, 1);
            assert!(updated.starts_with("2026-10-09"));
            let day = if symbol == "USDT" { "2026-10-01" } else { "2026-10-09" };
            assert!(created.starts_with(day));
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM coin WHERE token_address LIKE '0xfixture%' OR token_address IN ('0xtoken-a','0xtoken-b')").fetch_one(self.pool.read_ref()).await?;
        assert_eq!(count, 502);
        Ok(())
    }
    pub fn then_only_separate_incremental_queries(&self) {
        let requests = self.recorder.snapshot();
        let mut expected_page = 0;
        for call in
            requests.iter().filter(|c| c.path.ends_with("token/queryAllExcludeCustomByPage"))
        {
            let body: serde_json::Value = serde_json::from_str(&call.body).unwrap();
            let created = body.get("createTime");
            let updated = body.get("updateTime");
            assert_ne!(
                created.is_some(),
                updated.is_some(),
                "never combine AND filters or silently request history"
            );
            let cutoff = created.or(updated).unwrap().as_str().unwrap();
            assert!(cutoff >= "2026-10-07 23:59:58", "do not derive from legacy wrong update time");
            assert_eq!(body["pageSize"], 500);
            let page = body["pageNum"].as_i64().unwrap();
            if page == 0 {
                expected_page = 0;
            }
            assert_eq!(page, expected_page);
            expected_page += 1;
        }
    }
    async fn incremental_mode(&self) -> bool {
        use wallet_database::repositories::{coin::CoinRepo, config::ConfigRepo};
        if let Some(saved) =
            ConfigRepo::find_by_key("ordinary_catalog_progress_v1", &self.pool).await.unwrap()
        {
            let state: serde_json::Value = serde_json::from_str(&saved.value).unwrap();
            !state["cursor"].is_null()
        } else {
            CoinRepo::last_coin(&self.pool, true)
                .await
                .unwrap()
                .is_some_and(|coin| coin.created_at.timestamp() > 0)
        }
    }
    async fn queue_catalog(&self) {
        self.page("USDT", "0xtoken-a");
        self.page("USDC", "0xtoken-b");
        if self.incremental_mode().await {
            self.page_total("USDC", "0xtoken-b", 1);
        }
    }
    pub async fn given_two_successful_pages(&self) {
        self.queue_catalog().await;
        self.recorder.succeed_next_backend_call_with_data(serde_json::json!({"list":[]}));
    }
    pub async fn given_catalog_pages_without_completion(&self) {
        self.page("USDT", "0xtoken-a");
        self.page("USDC", "0xtoken-b");
    }
    pub async fn given_failed_last_page(&self) {
        self.page("USDT", "0xtoken-a");
        self.recorder.fail_next_api_backend_call(500, "controlled last-page failure");
    }
    pub async fn given_failed_catalog_completion(&self) {
        self.queue_catalog().await;
        self.recorder.fail_next_api_backend_call(500, "controlled completion failure");
    }
    pub fn given_last_page_paused(&self) -> Arc<MockResponseGate> {
        self.recorder.hold_next_response().release();
        self.recorder.hold_next_response()
    }
    pub fn when_sync_starts(
        &self,
    ) -> tokio::task::JoinHandle<Result<(), wallet_api::error::service::ServiceError>> {
        let ctx = self.ctx;
        tokio::spawn(async move { chain_sync::pull_ordinary_coins(ctx).await })
    }
    pub async fn when_sync_runs(&self) -> anyhow::Result<()> {
        chain_sync::pull_ordinary_coins(self.ctx).await?;
        Ok(())
    }
    pub async fn then_progress_is(&self, expected: Option<&str>) -> anyhow::Result<()> {
        let value: String =
            sqlx::query_scalar("SELECT value FROM config WHERE key='ordinary_catalog_progress_v1'")
                .fetch_one(self.pool.read_ref())
                .await?;
        let state: serde_json::Value = serde_json::from_str(&value)?;
        if let Some(expected) = expected {
            assert_eq!(state["cursor"]["created"], expected);
            assert_eq!(state["cursor"]["updated"], expected);
        } else {
            assert!(state["cursor"].is_null());
        }
        Ok(())
    }
    pub async fn then_progress_times_are(
        &self,
        created: &str,
        updated: &str,
    ) -> anyhow::Result<()> {
        let value: String =
            sqlx::query_scalar("SELECT value FROM config WHERE key='ordinary_catalog_progress_v1'")
                .fetch_one(self.pool.read_ref())
                .await?;
        let state: serde_json::Value = serde_json::from_str(&value)?;
        assert_eq!(state["cursor"]["created"], created);
        assert_eq!(state["cursor"]["updated"], updated);
        Ok(())
    }
    pub async fn given_untrusted_local_update_time(&self) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE coin SET updated_at='2099-01-01T00:00:00Z' WHERE token_address='0xfixture0'",
        )
        .execute(self.pool.write_ref())
        .await?;
        Ok(())
    }
    fn version_page(&self, updated: &str, default: bool) {
        self.recorder.succeed_next_backend_call_with_data(serde_json::json!({
            "totalPage":1,"pageSize":500,"totalCount":1,"list":[{
                "id":"USDC","code":"USDC","chainCode":"scroll","contractAddress":"0xtoken-b",
                "unit":6,"enable":true,"defaultToken":default,"popularToken":true,"protocol":"ERC20",
                "createTime":"2026-10-09 00:00:00","updateTime":updated
            }]
        }));
    }
    pub fn given_overlapping_different_versions(&self) {
        self.version_page("2026-10-09 00:00:02", true);
        self.version_page("2026-10-09 00:00:01", false);
        self.recorder.succeed_next_backend_call_with_data(serde_json::json!({"list":[]}));
    }
    pub fn given_invalid_server_timestamp(&self) {
        self.version_page("invalid time", true);
        self.recorder.succeed_next_backend_call_with_data(serde_json::json!({
            "totalPage":0,"pageSize":500,"totalCount":0,"list":[]
        }));
    }
    pub async fn then_newest_overlap_is_persisted(&self) -> anyhow::Result<()> {
        let row: (i64, String) = sqlx::query_as(
            "SELECT is_default,updated_at FROM coin WHERE token_address='0xtoken-b'",
        )
        .fetch_one(self.pool.read_ref())
        .await?;
        assert_eq!(row, (1, "2026-10-09T00:00:02Z".to_owned()));
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM coin WHERE token_address='0xtoken-b'")
                .fetch_one(self.pool.read_ref())
                .await?;
        assert_eq!(count, 1);
        Ok(())
    }
    pub fn given_empty_incremental_responses(&self) {
        for _ in 0..2 {
            self.recorder.succeed_next_backend_call_with_data(serde_json::json!({
                "totalPage":0,"pageSize":500,"totalCount":0,"list":[]
            }));
        }
        self.recorder.succeed_next_backend_call_with_data(serde_json::json!({"list":[]}));
    }
    pub fn then_initial_filters_use_creation_baseline(&self) {
        let calls = self.recorder.snapshot();
        let requests: Vec<_> = calls
            .iter()
            .filter(|call| call.path.ends_with("token/queryAllExcludeCustomByPage"))
            .map(|call| serde_json::from_str::<serde_json::Value>(&call.body).unwrap())
            .collect();
        assert_eq!(requests[0]["updateTime"], "2026-10-07 23:59:58");
        assert!(requests[0].get("createTime").is_none());
        assert!(requests.iter().any(|r| r["createTime"] == "2026-10-07 23:59:58"));
    }
    pub fn then_successful_boundary_is_replayed(&self) {
        let calls = self.recorder.snapshot();
        assert!(calls.iter().any(|call| {
            if !call.path.ends_with("token/queryAllExcludeCustomByPage") {
                return false;
            }
            let body: serde_json::Value = serde_json::from_str(&call.body).unwrap();
            body["updateTime"] == "2026-10-08 23:59:58"
        }));
    }
    pub fn then_directory_request_count_is(&self, expected: usize) {
        assert_eq!(
            self.recorder
                .snapshot()
                .iter()
                .filter(|call| call.path.ends_with("token/queryAllExcludeCustomByPage"))
                .count(),
            expected
        );
    }
    pub fn given_first_request_failure(&self) {
        self.recorder.fail_next_api_backend_call(500, "controlled initial request failure");
    }
    pub fn given_creation_route_failure(&self) {
        self.page("USDT", "0xtoken-a");
        self.page("USDC", "0xtoken-b");
        self.recorder.fail_next_api_backend_call(500, "controlled creation route failure");
    }
    pub async fn given_metadata_write_failure(&self) -> anyhow::Result<()> {
        sqlx::query("CREATE TRIGGER catalog_metadata_fail BEFORE UPDATE ON coin WHEN NEW.token_address='0xtoken-a' BEGIN SELECT RAISE(ABORT,'controlled metadata failure'); END")
            .execute(self.pool.write_ref()).await?;
        Ok(())
    }
    pub async fn given_progress_save_failure(&self) -> anyhow::Result<()> {
        sqlx::query("CREATE TRIGGER catalog_progress_fail BEFORE UPDATE ON config WHEN NEW.key='ordinary_catalog_progress_v1' BEGIN SELECT RAISE(ABORT,'controlled progress failure'); END")
            .execute(self.pool.write_ref()).await?;
        Ok(())
    }
    pub async fn given_write_failures_removed(&self) -> anyhow::Result<()> {
        for name in ["catalog_metadata_fail", "catalog_progress_fail"] {
            sqlx::query(&format!("DROP TRIGGER IF EXISTS {name}"))
                .execute(self.pool.write_ref())
                .await?;
        }
        // Failed metadata writes leave the previously queued completion response unused.
        self.recorder.reset();
        Ok(())
    }
    pub async fn then_contract_assets_are(&self, expected: i64) -> anyhow::Result<()> {
        let actual: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM assets WHERE chain_code='scroll' AND token_address IN ('0xtoken-a','0xtoken-b') AND is_multisig=0 AND status=1 AND balance='0'").fetch_one(self.pool.read_ref()).await?;
        assert_eq!(actual, expected);
        Ok(())
    }
    pub async fn then_contract_coins_are(&self, expected: i64) -> anyhow::Result<()> {
        let actual: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM coin WHERE chain_code='scroll' AND token_address IN ('0xtoken-a','0xtoken-b') AND is_default=1").fetch_one(self.pool.read_ref()).await?;
        assert_eq!(actual, expected);
        Ok(())
    }
    pub async fn then_api_assets_are_unchanged(&self) -> anyhow::Result<()> {
        let actual: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_assets")
            .fetch_one(mqtt::api_wallet_pool(self.ctx)?.read_ref())
            .await?;
        assert_eq!(actual, self.api_asset_count);
        Ok(())
    }
    pub fn then_refresh_signals_are(&self, expected: usize) {
        let mut receiver = self.events.lock().unwrap();
        let mut count = 0;
        while let Ok(event) = receiver.try_recv() {
            assert_eq!(event.event, "SYNC_ASSETS");
            count += 1;
        }
        assert_eq!(count, expected);
    }
    pub fn then_only_catalog_endpoints_were_called(&self) {
        let calls = self.recorder.snapshot();
        assert_eq!(calls.len(), 7);
        assert_eq!(
            calls.iter().filter(|c| c.path.ends_with("token/queryAllExcludeCustomByPage")).count(),
            5
        );
        assert!(calls.iter().all(|c| c.path.contains("token/")));
    }
}
