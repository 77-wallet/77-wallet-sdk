use crate::harness::worker::{MockBackendRecorder, start_mock_backend_server};
use wallet_api::testkit::{chain_sync, mqtt};
use wallet_database::{
    entities::{
        api_account::CreateApiAccountVo,
        api_wallet::ApiWalletType,
        config::{
            AppVersion,
            config_key::{APP_VERSION, KEYS_RESET_EPOCH},
        },
    },
    repositories::{
        api_wallet::{account::ApiAccountRepo, chain::ApiChainRepo, coin::ApiCoinRepo},
        config::ConfigRepo,
    },
};
use wallet_transport_backend::{
    consts::endpoint::api_wallet::{ADDRESS_INIT, API_WALLET_CHAIN_LIST},
    request::{AddressInitReq, api_wallet::address::ApiAddressInitReq},
};

const CODES: [&str; 5] = ["polygon", "avalanche", "arbitrum", "base", "scroll"];
pub(super) struct ChainSyncScenario {
    ctx: &'static wallet_api::Context,
    recorder: MockBackendRecorder,
    _dir: tempfile::TempDir,
    config_text: String,
}

impl ChainSyncScenario {
    pub async fn new() -> anyhow::Result<Self> {
        // Reuse the shared HTTP fake without constructing background worker handles.
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
            wallet_api::Context::new_without_handles_for_test(
                wallet_api::dirs::Dirs::new(dir.path().to_str().unwrap())?,
                config,
            )
            .await?,
        ));
        wallet_ecdh::GLOBAL_KEY.set_shared_secret(
            r#"-----BEGIN PUBLIC KEY-----
MFYwEAYHKoZIzj0CAQYFK4EEAAoDQgAEWDZNP0ClbeWJey9hBr2rsjSayQEBywnv
ZXi0RberQCAp+06fOjvr+jZI5qwYGglmMkGJw49tbni6qgm4QNV6WQ==
-----END PUBLIC KEY-----"#,
        )?;
        wallet_ecdh::GLOBAL_KEY.set_sn("chain-sync-test");
        let api = mqtt::api_wallet_pool(ctx)?;
        // Every scenario owns a fresh migrated DB; cleanup only its five-chain fixtures.
        for table in ["api_chain", "api_coin", "api_account"] {
            sqlx::query(&format!("DELETE FROM {table} WHERE chain_code IN ('polygon','avalanche','arbitrum','base','scroll')")).execute(api.write_ref()).await?;
        }
        let core = chain_sync::core_pool(ctx)?;
        ConfigRepo::upsert(
            APP_VERSION,
            &AppVersion { app_version: "2.2.0".into() }.to_json_str()?,
            Some(1),
            &core,
        )
        .await?;
        ConfigRepo::upsert(KEYS_RESET_EPOCH, "777", None, &core).await?;
        Self::configuration_without_background_dispatch(chain_sync::initialize(ctx).await)?;
        Ok(Self { ctx, recorder, _dir: dir, config_text })
    }
    fn configuration_without_background_dispatch(
        result: Result<(), wallet_api::error::service::ServiceError>,
    ) -> anyhow::Result<()> {
        // The real domain persists configuration/tasks before reporting absent handles.
        // This fixture deliberately starts no background dispatch; other errors propagate.
        match result {
            Ok(()) => Ok(()),
            Err(error) if error.to_string().contains("global handles not ready") => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn given_backend_disables_polygon_and_gates_base(&self) {
        self.recorder.succeed_next_api_backend_call_with_data(serde_json::json!([
            {"name":"Polygon", "code":"polygon", "enable":false, "masterTokenCode":"POL", "appVersionCode":"2.2.0"},
            {"name":"Base", "code":"base", "enable":true, "masterTokenCode":"ETH", "appVersionCode":"9.0.0"}
        ]));
    }
    pub async fn when_chain_sync_runs(&self) -> anyhow::Result<()> {
        chain_sync::sync_chains(self.ctx).await?;
        Ok(())
    }
    pub async fn then_disabled_chains_stay_disabled_after_reinitialize(
        &self,
    ) -> anyhow::Result<()> {
        Self::configuration_without_background_dispatch(chain_sync::initialize(self.ctx).await)?;
        let api = mqtt::api_wallet_pool(self.ctx)?;
        let enabled = ApiChainRepo::get_chain_list(&api).await?;
        assert!(!enabled.iter().any(|row| matches!(row.chain_code.as_str(), "polygon" | "base")));
        assert!(enabled.iter().any(|row| row.chain_code == "scroll"));
        Ok(())
    }
    pub fn then_one_chain_request_was_sent(&self) {
        let calls = self.recorder.snapshot();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].path.ends_with(API_WALLET_CHAIN_LIST));
    }
    pub async fn given_no_coins_and_invalid_coin_response(&self) -> anyhow::Result<()> {
        let api = mqtt::api_wallet_pool(self.ctx)?;
        sqlx::query("DELETE FROM api_coin WHERE chain_code IN ('polygon','avalanche','arbitrum','base','scroll')").execute(api.write_ref()).await?;
        self.recorder
            .succeed_next_api_backend_call_with_data(serde_json::json!({"content": "invalid"}));
        Ok(())
    }
    pub async fn when_coin_sync_runs(&self) -> anyhow::Result<()> {
        Ok(chain_sync::pull_coins(self.ctx).await?)
    }
    pub async fn then_coin_failure_has_no_default_write(
        &self,
        error: anyhow::Error,
    ) -> anyhow::Result<()> {
        assert!(!error.to_string().is_empty());
        let coins = ApiCoinRepo::coin_list(&mqtt::api_wallet_pool(self.ctx)?).await?;
        assert!(!coins.iter().any(|coin| CODES.contains(&coin.chain_code.as_str())));
        assert_eq!(self.recorder.snapshot().len(), 1);
        Ok(())
    }
    pub fn given_valid_empty_coin_response(&self) {
        let sort = serde_json::json!({"empty":true,"unsorted":true,"sorted":false});
        self.recorder.succeed_next_api_backend_call_with_data(serde_json::json!({
            "totalElements":0,"totalPages":1,"first":true,"last":true,"size":500,"number":0,
            "sort":sort,"pageable":{"offset":0,"sort":sort,"pageNumber":0,"pageSize":500,"unpaged":false,"paged":true},
            "numberOfElements":0,"empty":true,"content":[]
        }));
    }
    pub async fn then_five_native_coins_are_present(&self) -> anyhow::Result<()> {
        let coins: Vec<_> = ApiCoinRepo::coin_list(&mqtt::api_wallet_pool(self.ctx)?)
            .await?
            .into_iter()
            .filter(|coin| CODES.contains(&coin.chain_code.as_str()))
            .collect();
        assert_eq!(coins.len(), 5);
        assert!(coins.iter().all(|coin| CODES.contains(&coin.chain_code.as_str())
            && coin.token_address.is_native()
            && coin.decimals == 18));
        Ok(())
    }
    pub async fn given_five_unregistered_addresses_and_backend_failure(
        &self,
    ) -> anyhow::Result<()> {
        let api = mqtt::api_wallet_pool(self.ctx)?;
        for code in CODES {
            ApiAccountRepo::upsert_account_multi(
                &api,
                vec![CreateApiAccountVo::new(
                    0,
                    "test-address",
                    "test-pub",
                    "chain-sync-wallet",
                    "test-uid",
                    "m/44'/60'/0'/0/0",
                    0,
                    code,
                    "test",
                    ApiWalletType::SubAccount,
                )],
            )
            .await?;
        }
        self.recorder.fail_next_api_backend_call(503, "test unavailable");
        Ok(())
    }
    pub fn given_backend_registration_success(&self) {
        self.recorder.succeed_next_api_backend_call_with_data(serde_json::Value::Null);
    }
    pub async fn when_address_registration_runs(&self, epoch: u64) -> anyhow::Result<()> {
        let mut req = ApiAddressInitReq::new(epoch);
        for code in CODES {
            req.address_list.add_address(AddressInitReq::new(
                "test-uid",
                "test-address",
                0,
                code,
                "test-sn",
                vec!["".into()],
                "test",
            ));
        }
        Ok(chain_sync::register_addresses(self.ctx, req).await?)
    }
    pub async fn then_all_addresses_have_init_state(&self, state: i32) -> anyhow::Result<()> {
        let rows: Vec<_> = ApiAccountRepo::list(&mqtt::api_wallet_pool(self.ctx)?)
            .await?
            .into_iter()
            .filter(|row| {
                CODES.contains(&row.chain_code.as_str())
                    && row.wallet_address == "chain-sync-wallet"
            })
            .collect();
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().all(|row| row.is_init == state));
        Ok(())
    }
    pub fn then_registration_call_count_is(&self, count: usize) {
        let calls = self.recorder.snapshot();
        assert_eq!(
            calls.len(),
            count,
            "recorded paths: {:?}",
            calls.iter().map(|call| call.path.as_str()).collect::<Vec<_>>()
        );
        assert!(calls.iter().all(|call| call.path.ends_with(ADDRESS_INIT)));
    }
    pub async fn given_many_unregistered_addresses(
        &self,
        count: usize,
        name_bytes: usize,
    ) -> anyhow::Result<()> {
        let pool = mqtt::api_wallet_pool(self.ctx)?;
        for chunk in (0..count).collect::<Vec<_>>().chunks(200) {
            let rows = chunk
                .iter()
                .map(|i| {
                    CreateApiAccountVo::new(
                        *i as u32,
                        &format!("bounded-address-{i}"),
                        "pub",
                        "bounded-wallet",
                        "bounded-uid",
                        "m/44'/60'/0'/0/0",
                        *i as i32,
                        "polygon",
                        &"n".repeat(name_bytes),
                        if i % 2 == 0 {
                            ApiWalletType::SubAccount
                        } else {
                            ApiWalletType::Withdrawal
                        },
                    )
                })
                .collect();
            ApiAccountRepo::upsert_account_multi(&pool, rows).await?;
        }
        for _ in 0..count {
            self.given_backend_registration_success();
        }
        Ok(())
    }
    pub async fn when_bounded_registration_runs(&self) -> anyhow::Result<()> {
        let pool = mqtt::api_wallet_pool(self.ctx)?;
        let rows = ApiAccountRepo::list(&pool).await?;
        let mut req = ApiAddressInitReq::new(777);
        for row in
            rows.into_iter().filter(|r| r.wallet_address == "bounded-wallet" && r.is_init == 0)
        {
            req.address_list.add_address(AddressInitReq::new(
                &row.uid,
                &row.address,
                row.derivation_path_index,
                &row.chain_code,
                "test-sn",
                vec!["".into()],
                &row.name,
            ));
        }
        // Exercise bounded admission with backpressure between input chunks.
        for addresses in req.address_list.0.chunks(40) {
            let mut chunk = ApiAddressInitReq::new(req.epoch);
            chunk.address_list.0 = addresses.to_vec();
            chain_sync::register_bounded(self.ctx, chunk).await?;
            chain_sync::drain_saved_recovery(self.ctx).await?;
        }
        Ok(())
    }
    pub async fn then_registration_is_bounded_and_complete(
        &self,
        count: usize,
    ) -> anyhow::Result<()> {
        let calls = self.recorder.snapshot();
        assert!(calls.len() > 1, "large registration must be split");
        let mut registered = 0;
        let mut max_items = 0;
        let mut max_payload = 0;
        let mut max_wire = 0;
        let call_count = calls.len();
        for call in calls {
            let value = crate::harness::worker::decrypt_captured_api_backend_body(&call.body);
            let addresses = value["addressList"].as_array().expect("registration address list");
            assert!(addresses.len() <= 40, "registration item bound exceeded: {}", addresses.len());
            assert!(
                serde_json::to_vec(&value)?.len() <= 32 * 1024,
                "registration byte bound exceeded"
            );
            max_items = max_items.max(addresses.len());
            max_payload = max_payload.max(serde_json::to_vec(&value)?.len());
            max_wire = max_wire.max(call.body.len());
            registered += addresses.len();
        }
        assert_eq!(registered, count);
        eprintln!(
            "registration scale: {count} addresses, {call_count} requests, max {max_items} items / {max_payload} plaintext bytes / {max_wire} encrypted envelope bytes"
        );
        let pool = mqtt::api_wallet_pool(self.ctx)?;
        let rows = ApiAccountRepo::list(&pool).await?;
        assert_eq!(
            rows.iter().filter(|r| r.wallet_address == "bounded-wallet" && r.is_init == 1).count(),
            count
        );
        Ok(())
    }

    pub async fn given_existing_accounts_of_both_types(
        &self,
        count_per_type: usize,
    ) -> anyhow::Result<()> {
        chain_sync::prepare_sync_wallets(self.ctx).await?;
        let pool = mqtt::api_wallet_pool(self.ctx)?;
        for (wallet, uid, kind) in [
            ("sync-sub-wallet", "sync-sub-uid", ApiWalletType::SubAccount),
            ("sync-withdraw-wallet", "sync-withdraw-uid", ApiWalletType::Withdrawal),
        ] {
            let rows = (0..count_per_type)
                .map(|i| {
                    let map =
                        wallet_utils::address::AccountIndexMap::from_input_index(i as i32).unwrap();
                    CreateApiAccountVo::new(
                        map.account_id,
                        &format!("source-{wallet}-{i}"),
                        "public",
                        wallet,
                        uid,
                        "m/44'/60'/0'/0/0",
                        i as i32,
                        "eth",
                        &format!("custom name {i}"),
                        kind,
                    )
                })
                .collect();
            ApiAccountRepo::upsert_account_multi(&pool, rows).await?;
        }
        // Only the five rollout chains participate in this synthetic scenario.
        sqlx::query("UPDATE api_chain SET status=0 WHERE chain_code NOT IN ('polygon','avalanche','arbitrum','base','scroll')")
            .execute(pool.write_ref()).await?;
        self.recorder.reset();
        for _ in 0..count_per_type * 10 {
            self.given_backend_registration_success();
        }
        Ok(())
    }
    pub async fn when_existing_accounts_sync_runs(&self) -> anyhow::Result<()> {
        Self::configuration_without_background_dispatch(chain_sync::sync_accounts(self.ctx).await)?;
        chain_sync::drain_saved_recovery(self.ctx).await?;
        Ok(())
    }
    pub fn given_registration_succeeds_once_then_fails(&self) {
        self.recorder.reset();
        self.given_backend_registration_success();
        self.recorder.fail_next_api_backend_call(503, "synthetic transient failure");
    }
    pub fn given_many_registration_successes(&self, count: usize) {
        for _ in 0..count {
            self.given_backend_registration_success();
        }
    }
    pub async fn then_existing_indices_types_and_names_are_complete(
        &self,
        count_per_type: usize,
    ) -> anyhow::Result<()> {
        let pool = mqtt::api_wallet_pool(self.ctx)?;
        let rows = ApiAccountRepo::list(&pool).await?;
        for (wallet, kind) in [
            ("sync-sub-wallet", ApiWalletType::SubAccount),
            ("sync-withdraw-wallet", ApiWalletType::Withdrawal),
        ] {
            for code in CODES {
                let targets: Vec<_> = rows
                    .iter()
                    .filter(|r| r.wallet_address == wallet && r.chain_code == code)
                    .collect();
                assert_eq!(targets.len(), count_per_type);
                for target in targets {
                    assert_eq!(target.api_wallet_type, kind);
                    assert_eq!(target.is_init, 1);
                    assert_eq!(
                        target.name,
                        format!("custom name {}", target.derivation_path_index)
                    );
                }
            }
        }
        for call in self.recorder.snapshot() {
            let value = crate::harness::worker::decrypt_captured_api_backend_body(&call.body);
            assert!(
                value["batchId"].is_null(),
                "supplementation must not invent expansion batch IDs"
            );
            assert!(value["addressList"].as_array().unwrap().len() <= 40);
            assert!(serde_json::to_vec(&value)?.len() <= 32 * 1024);
        }
        Ok(())
    }
    pub fn registration_call_count(&self) -> usize {
        self.recorder.snapshot().len()
    }
    pub async fn then_failure_preserves_partial_completion(&self) -> anyhow::Result<()> {
        let pool = mqtt::api_wallet_pool(self.ctx)?;
        let rows = ApiAccountRepo::list(&pool).await?;
        let completed = rows
            .iter()
            .filter(|r| CODES.contains(&r.chain_code.as_str()) && r.is_init == 1)
            .count();
        let pending = rows
            .iter()
            .filter(|r| CODES.contains(&r.chain_code.as_str()) && r.is_init == 0)
            .count();
        let first = self.first_called_chain();
        assert_eq!(
            rows.iter().filter(|r| r.chain_code == first && r.is_init == 1).count(),
            rows.iter().filter(|r| r.chain_code == first).count() - 40
        );
        assert!(pending > 0);
        assert!(completed > 40, "independent later chains must also complete");
        let source_count = rows.iter().filter(|row| row.chain_code == "eth").count();
        for code in CODES.into_iter().filter(|code| *code != first) {
            let targets: Vec<_> = rows.iter().filter(|row| row.chain_code == code).collect();
            assert_eq!(targets.len(), source_count);
            assert!(targets.iter().all(|row| row.is_init == 1));
        }
        assert_eq!(completed, 5 * source_count - 40);
        Ok(())
    }
    pub async fn when_reset_epoch_changes_during_registration(&self) -> anyhow::Result<()> {
        self.given_backend_registration_success();
        let gate = self.recorder.hold_next_response();
        let registration = self.when_address_registration_runs(777);
        let change_epoch = async {
            gate.wait_for_request().await;
            let pool = chain_sync::core_pool(self.ctx)?;
            ConfigRepo::upsert(KEYS_RESET_EPOCH, "778", None, &pool).await?;
            gate.release();
            anyhow::Ok(())
        };
        let (result, changed) = tokio::join!(registration, change_epoch);
        changed?;
        result?;
        Ok(())
    }
    pub async fn when_context_is_reopened(&mut self) -> anyhow::Result<()> {
        let api = mqtt::api_wallet_pool(self.ctx)?;
        api.read_ref().close().await;
        api.write_ref().close().await;
        let core = chain_sync::core_pool(self.ctx)?;
        core.read_ref().close().await;
        core.write_ref().close().await;
        self.ctx = Box::leak(Box::new(
            wallet_api::Context::new_without_handles_for_test(
                wallet_api::dirs::Dirs::new(self._dir.path().to_str().unwrap())?,
                wallet_api::config::Config::new(&self.config_text)?,
            )
            .await?,
        ));
        chain_sync::restore_sync_wallet_session(self.ctx).await?;
        Ok(())
    }
    pub async fn when_in_flight_registration_is_cancelled(&self) -> anyhow::Result<()> {
        self.given_backend_registration_success();
        let gate = self.recorder.hold_next_response();
        let ctx = self.ctx;
        Self::configuration_without_background_dispatch(chain_sync::sync_accounts(ctx).await)?;
        let task = tokio::spawn(async move { chain_sync::drain_saved_recovery(ctx).await });
        gate.wait_for_request().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        gate.release();
        let rows = ApiAccountRepo::list(&mqtt::api_wallet_pool(self.ctx)?).await?;
        assert!(
            rows.iter().filter(|r| CODES.contains(&r.chain_code.as_str())).all(|r| r.is_init == 0)
        );
        Ok(())
    }
    pub async fn when_concurrent_account_syncs_run(&self) -> anyhow::Result<()> {
        let (first, second) =
            tokio::join!(chain_sync::sync_accounts(self.ctx), chain_sync::sync_accounts(self.ctx));
        Self::configuration_without_background_dispatch(first)?;
        Self::configuration_without_background_dispatch(second)?;
        chain_sync::drain_saved_recovery(self.ctx).await?;
        Ok(())
    }
    pub async fn when_wallet_unlock_starts_recovery(
        &self,
        count_per_type: usize,
    ) -> anyhow::Result<()> {
        let gate = self.recorder.hold_next_response();
        let began = std::time::Instant::now();
        Self::configuration_without_background_dispatch(
            chain_sync::unlock_and_start_recovery(self.ctx).await,
        )?;
        let _coordinator = chain_sync::start_recovery_coordinator(self.ctx).await?;
        gate.wait_for_request().await;
        gate.release();
        let pool = mqtt::api_wallet_pool(self.ctx)?;
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let completed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_account WHERE wallet_address IN ('sync-sub-wallet','sync-withdraw-wallet') AND chain_code IN ('polygon','avalanche','arbitrum','base','scroll') AND status=1 AND is_init=1")
                    .fetch_one(pool.read_ref()).await?;
                if completed == (count_per_type * 10) as i64 { return anyhow::Ok(()); }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }).await??;
        // Stop future reconciliation for this test Context before its directory is removed.
        chain_sync::lock_sync_wallet(self.ctx).await?;
        eprintln!(
            "actual unlock recovery: {} derived addresses, elapsed {:?}",
            count_per_type * 10,
            began.elapsed()
        );
        Ok(())
    }
    pub async fn when_same_context_recovery_lifecycle_is_exercised(&self) -> anyhow::Result<()> {
        let first = chain_sync::recovery_task_ids(self.ctx).await?;
        Self::configuration_without_background_dispatch(
            chain_sync::unlock_and_start_recovery(self.ctx).await,
        )?;
        Self::configuration_without_background_dispatch(
            chain_sync::unlock_and_start_recovery(self.ctx).await,
        )?;
        assert_eq!(
            chain_sync::recovery_task_ids(self.ctx).await?,
            first,
            "repeated unlock reuses durable chain jobs"
        );
        chain_sync::drain_saved_recovery(self.ctx).await?;
        Ok(())
    }

    pub fn given_first_chain_registration_failure(&self) {
        self.recorder.reset();
        self.recorder.fail_next_api_backend_call(503, "synthetic first chain failure");
        self.given_many_registration_successes(100);
    }
    pub fn given_each_chain_registration_failure(&self) {
        self.recorder.reset();
        self.recorder.fail_next_api_backend_call(503, "synthetic first chain failure");
        for code in 504..508 {
            self.recorder.fail_next_api_backend_call(code, "synthetic later chain failure");
        }
    }
    pub fn given_only_registration_successes(&self) {
        self.recorder.reset();
        self.given_many_registration_successes(100);
    }
    fn first_called_chain(&self) -> String {
        let calls = self.recorder.snapshot();
        let body = crate::harness::worker::decrypt_captured_api_backend_body(&calls[0].body);
        body["addressList"][0]["chainCode"].as_str().unwrap().to_owned()
    }
    pub async fn then_first_chain_pending_and_other_chains_complete(&self) -> anyhow::Result<()> {
        let failed = self.first_called_chain();
        let pool = mqtt::api_wallet_pool(self.ctx)?;
        let rows = ApiAccountRepo::list(&pool).await?;
        for code in CODES {
            let targets: Vec<_> = rows.iter().filter(|r| r.chain_code == code).collect();
            assert_eq!(targets.len(), 2);
            assert!(targets.iter().all(|r| r.is_init == if code == failed { 0 } else { 1 }));
        }
        let calls = self.recorder.snapshot();
        let called_chains: Vec<_> = calls
            .iter()
            .map(|call| {
                let body = crate::harness::worker::decrypt_captured_api_backend_body(&call.body);
                body["addressList"][0]["chainCode"].as_str().unwrap().to_owned()
            })
            .collect();
        // Backend transport can retry the failed request internally. Successful
        // independent chains must be called once on pass1 and never on pass2.
        for code in CODES.into_iter().filter(|code| *code != failed) {
            assert!(called_chains.iter().filter(|called| called.as_str() == code).count() <= 1);
        }
        if called_chains.iter().any(|code| code != &failed) {
            assert_eq!(called_chains.iter().collect::<std::collections::HashSet<_>>().len(), 5);
        }

        Ok(())
    }
    pub async fn then_all_rollout_chains_pending(&self) -> anyhow::Result<()> {
        let rows = ApiAccountRepo::list(&mqtt::api_wallet_pool(self.ctx)?).await?;
        let targets: Vec<_> =
            rows.iter().filter(|r| CODES.contains(&r.chain_code.as_str())).collect();
        assert_eq!(targets.len(), 10);
        assert!(targets.iter().all(|r| r.is_init == 0));
        Ok(())
    }
    pub async fn when_epoch_changes_during_existing_account_sync(&self) -> anyhow::Result<()> {
        let gate = self.recorder.hold_next_response();
        let sync = self.when_existing_accounts_sync_runs();
        let reset = async {
            gate.wait_for_request().await;
            ConfigRepo::upsert(KEYS_RESET_EPOCH, "778", None, &chain_sync::core_pool(self.ctx)?)
                .await?;
            gate.release();
            anyhow::Ok(())
        };
        let (result, changed) = tokio::join!(sync, reset);
        changed?;
        result?;
        Ok(())
    }
    pub async fn given_registration_marker_database_failure(&self) -> anyhow::Result<()> {
        sqlx::raw_sql("CREATE TRIGGER reject_init_marker BEFORE UPDATE OF is_init ON api_account WHEN NEW.is_init = 1 BEGIN SELECT RAISE(ABORT, 'synthetic init marker failure'); END;")
            .execute(mqtt::api_wallet_pool(self.ctx)?.write_ref()).await?;
        Ok(())
    }
    pub async fn when_first_registration_times_out(&self) -> anyhow::Result<anyhow::Error> {
        // Hold only the first chain and any retries of that chain. Backend
        // retry counts vary; later independent chains must never inherit a gate.
        let gate =
            self.recorder.hold_first_registration_chain_for(std::time::Duration::from_secs(60));
        let ctx = self.ctx;
        Self::configuration_without_background_dispatch(chain_sync::sync_accounts(ctx).await)?;
        let task = tokio::spawn(async move { chain_sync::drain_saved_recovery(ctx).await });
        gate.wait_for_request().await;
        let result = tokio::time::timeout(std::time::Duration::from_secs(40), task).await;
        gate.release();
        result???;
        let pool = mqtt::task_pool(self.ctx)?;
        let message: String = sqlx::query_scalar(
            "SELECT err_msg FROM task_queue WHERE status=3 AND err_msg LIKE '%timed out%' LIMIT 1",
        )
        .fetch_one(pool.read_ref())
        .await?;
        let error = wallet_api::error::service::ServiceError::System(
            wallet_api::error::system::SystemError::Service(message),
        );
        assert!(
            matches!(&error,
            wallet_api::error::service::ServiceError::System(
                wallet_api::error::system::SystemError::Service(message)
            ) if message.contains("timed out"))
                || matches!(
                    &error,
                    wallet_api::error::service::ServiceError::TransportBackend(
                        wallet_transport_backend::Error::Timeout
                    )
                )
                || matches!(
                    &error,
                    wallet_api::error::service::ServiceError::TransportBackend(
                        wallet_transport_backend::Error::Transport(
                            wallet_transport::TransportError::Utils(wallet_utils::Error::Http(
                                wallet_utils::error::http::HttpError::ReqError(request_error)
                            ))
                        )
                    ) if request_error.is_timeout()
                ),
            "expected bounded registration timeout: {error}"
        );
        Ok(error.into())
    }
}

impl ChainSyncScenario {
    pub async fn when_account_sync_attempts_registration_dispatch(&self) -> anyhow::Result<()> {
        // Absent handles stop dispatch, while queue persistence must already be durable.
        Self::configuration_without_background_dispatch(chain_sync::sync_accounts(self.ctx).await)?;
        let rows = wallet_database::repositories::task_queue::TaskQueueRepo::all_tasks_queue(
            &mqtt::task_pool(self.ctx)?,
        )
        .await?;
        for row in rows.into_iter().filter(|row| row.id.starts_with("api_chain_reconcile:")) {
            chain_sync::run_saved_job(self.ctx, &row.id).await?;
        }
        Ok(())
    }

    pub async fn then_registration_is_durable_without_transport(&self) -> anyhow::Result<()> {
        let tasks = wallet_database::repositories::task_queue::TaskQueueRepo::all_tasks_queue(
            &mqtt::task_pool(self.ctx)?,
        )
        .await?;
        let registrations: Vec<_> = tasks
            .iter()
            .filter(|task| {
                serde_json::from_str::<serde_json::Value>(&task.request_body)
                    .ok()
                    .is_some_and(|body| body["endpoint"].as_str() == Some(ADDRESS_INIT))
            })
            .collect();
        assert!(
            !registrations.is_empty(),
            "missing-chain registration must be persisted before dispatch"
        );
        for task in registrations {
            assert_eq!(task.r#type, 1, "local registration is a backend task, not MQTT");
            assert_eq!(task.status, 0, "missing handles must leave registration pending");
        }
        assert!(
            self.recorder.snapshot().is_empty(),
            "planning must not call registration transport directly"
        );
        let rows = ApiAccountRepo::list(&mqtt::api_wallet_pool(self.ctx)?).await?;
        let targets: Vec<_> =
            rows.iter().filter(|row| CODES.contains(&row.chain_code.as_str())).collect();
        assert!(!targets.is_empty(), "derivation must precede persisted registration");
        assert!(targets.iter().all(|row| row.is_init == 0));
        Ok(())
    }
}

impl ChainSyncScenario {
    pub async fn when_retry_deadlines_arrive(&self) -> anyhow::Result<()> {
        sqlx::query("UPDATE task_queue SET remark=json_set(remark,'$.next_retry_at_ms',0) WHERE status IN (0,3) AND json_valid(remark) AND json_extract(remark,'$.owner')='api_missing_chain'").execute(mqtt::task_pool(self.ctx)?.write_ref()).await?;
        chain_sync::drain_saved_recovery(self.ctx).await?;
        Ok(())
    }
    pub async fn then_failed_queue_contains(&self, expected: &str) -> anyhow::Result<()> {
        let rows = wallet_database::repositories::task_queue::TaskQueueRepo::all_tasks_queue(
            &mqtt::task_pool(self.ctx)?,
        )
        .await?;
        assert!(rows.iter().any(|row| row.status == 3
            && row.err_msg.as_deref().is_some_and(|message| message.contains(expected))));
        Ok(())
    }
    pub async fn then_derived_addresses_stay_pending(&self) -> anyhow::Result<()> {
        let rows = ApiAccountRepo::list(&mqtt::api_wallet_pool(self.ctx)?).await?;
        let targets: Vec<_> =
            rows.iter().filter(|row| CODES.contains(&row.chain_code.as_str())).collect();
        assert_eq!(targets.len(), 10);
        assert!(targets.iter().all(|row| row.is_init == 0));
        Ok(())
    }
}

impl ChainSyncScenario {
    pub async fn given_one_rejected_address(&self) -> anyhow::Result<()> {
        self.given_many_unregistered_addresses(8, 10).await?;
        self.recorder.reject_registration_address("bounded-address-0");
        Ok(())
    }
    pub async fn when_registration_is_planned(&self) -> anyhow::Result<()> {
        let rows = ApiAccountRepo::list(&mqtt::api_wallet_pool(self.ctx)?).await?;
        let mut request = ApiAddressInitReq::new(777);
        for row in rows.into_iter().filter(|row| row.wallet_address == "bounded-wallet") {
            request.address_list.add_address(AddressInitReq::new(
                &row.uid,
                &row.address,
                row.derivation_path_index,
                &row.chain_code,
                "test-sn",
                vec!["".into()],
                &row.name,
            ));
        }
        chain_sync::register_bounded(self.ctx, request).await?;
        Ok(())
    }
    pub async fn then_only_rejected_address_is_pending(&self) -> anyhow::Result<()> {
        let rows = ApiAccountRepo::list(&mqtt::api_wallet_pool(self.ctx)?).await?;
        let targets: Vec<_> =
            rows.iter().filter(|row| row.wallet_address == "bounded-wallet").collect();
        assert_eq!(targets.len(), 8);
        for row in targets {
            assert_eq!(row.is_init, if row.address == "bounded-address-0" { 0 } else { 1 });
        }
        assert!(self.recorder.snapshot().len() <= 15, "binary localization stays bounded");
        self.then_failed_queue_contains("synthetic address not found").await?;
        Ok(())
    }
    pub async fn when_saved_tasks_are_consumed(&self) -> anyhow::Result<()> {
        chain_sync::drain_saved_recovery(self.ctx).await?;
        Ok(())
    }
    pub async fn when_same_coordinator_recovers_after_twelve_failures(&self) -> anyhow::Result<()> {
        let pool = mqtt::task_pool(self.ctx)?;
        sqlx::query("UPDATE task_queue SET remark=json_set(remark,'$.next_retry_at_ms',?) WHERE id LIKE 'api_chain_address:%'").bind(chrono::Utc::now().timestamp_millis()+600_000).execute(pool.write_ref()).await?;
        let _coordinator = chain_sync::start_recovery_coordinator(self.ctx).await?;
        for attempt in 1..=12_i64 {
            self.recorder.reset();
            self.recorder.fail_next_api_backend_call(503, "synthetic unavailable");
            sqlx::query("UPDATE task_queue SET remark=json_set(remark,'$.next_retry_at_ms',0) WHERE status IN (0,3) AND id LIKE 'api_chain_address:%'").execute(pool.write_ref()).await?;
            // An accidental delivery to the generic consumer must never execute this owned row.
            _coordinator.deliver_owned_to_default_consumer().await?;
            // All twelve attempts run through the same production chain coordinator.
            tokio::time::timeout(std::time::Duration::from_secs(8), async {
                loop {
                    let (status,attempts,message):(i64,i64,Option<String>)=sqlx::query_as("SELECT status,json_extract(remark,'$.attempts'),err_msg FROM task_queue WHERE id LIKE 'api_chain_address:%' LIMIT 1").fetch_one(pool.read_ref()).await?;
                    assert_ne!(status,2,"failure script cannot silently initialize addresses");
                    if status==3 && attempts==attempt {
                        assert!(message.as_deref().is_some_and(|text|text.contains("synthetic unavailable")));
                        assert_eq!(self.recorder.snapshot().len(),1,"one reservation permits one backend attempt");
                        return anyhow::Ok(());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            }).await??;
        }
        let row: (i64,i64,i64) = sqlx::query_as("SELECT status,json_extract(remark,'$.attempts'),json_extract(remark,'$.next_retry_at_ms') FROM task_queue WHERE status=3 AND id LIKE 'api_chain_address:%' LIMIT 1").fetch_one(pool.read_ref()).await?;
        assert_eq!(row.0, 3);
        assert!(row.1 >= 12);
        assert!(row.2 > chrono::Utc::now().timestamp_millis());
        let before = self.recorder.snapshot().len();
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        assert_eq!(
            self.recorder.snapshot().len(),
            before,
            "same checker must honor persisted future deadline"
        );
        self.given_only_registration_successes();
        // Deadline injection simulates time passage; no restart, unlock or manual sync follows.
        sqlx::query("UPDATE task_queue SET remark=json_set(remark,'$.next_retry_at_ms',?) WHERE status=3 AND id LIKE 'api_chain_address:%'").bind(chrono::Utc::now().timestamp_millis()+100).execute(pool.write_ref()).await?;
        tokio::time::timeout(std::time::Duration::from_secs(12), async {
            loop {
                let pending:i64=sqlx::query_scalar("SELECT COUNT(*) FROM api_account WHERE wallet_address='bounded-wallet' AND is_init=0").fetch_one(mqtt::api_wallet_pool(self.ctx)?.read_ref()).await?;
                if pending==0 {return anyhow::Ok(());}
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }).await??;
        self.then_registration_call_count_is(1);
        Ok(())
    }
    pub async fn when_finished_stable_tasks_are_reactivated(&self) -> anyhow::Result<()> {
        let pool = mqtt::api_wallet_pool(self.ctx)?;
        sqlx::query("UPDATE api_account SET is_init=0 WHERE wallet_address='bounded-wallet'")
            .execute(pool.write_ref())
            .await?;
        self.when_registration_is_planned().await?;
        self.when_saved_tasks_are_consumed().await?;
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_account WHERE wallet_address='bounded-wallet' AND is_init=0",
        )
        .fetch_one(pool.read_ref())
        .await?;
        assert_eq!(pending, 0);
        Ok(())
    }
}

impl ChainSyncScenario {
    pub async fn when_registration_insert_gap_then_later_pages_resume(&self) -> anyhow::Result<()> {
        let task_pool = mqtt::task_pool(self.ctx)?;
        sqlx::raw_sql("CREATE TRIGGER stop_registration BEFORE INSERT ON task_queue WHEN NEW.id LIKE 'api_chain_address:%' BEGIN SELECT RAISE(ABORT,'synthetic enqueue gap'); END;").execute(task_pool.write_ref()).await?;
        self.when_existing_accounts_sync_runs().await?;
        self.then_failed_queue_contains("synthetic enqueue gap").await?;
        assert!(self.recorder.snapshot().is_empty());
        let api = mqtt::api_wallet_pool(self.ctx)?;
        let rows = ApiAccountRepo::list(&api).await?;
        let targets = rows.iter().filter(|row| CODES.contains(&row.chain_code.as_str())).count();
        assert!(
            targets > 0 && targets < 201 * 10,
            "only the first page of each chain was generated"
        );
        let asset_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM api_assets").fetch_one(api.read_ref()).await?;
        assert!(asset_count > 0, "addresses and default assets survive pre-enqueue gap");
        sqlx::query("DROP TRIGGER stop_registration").execute(task_pool.write_ref()).await?;
        // Same database facts and durable parent, no unlock or reinitialization.
        for _ in 0..20 {
            self.when_retry_deadlines_arrive().await?;
            let pending:i64=sqlx::query_scalar("SELECT COUNT(*) FROM task_queue WHERE status != 2 AND id LIKE 'api_chain_reconcile:%'").fetch_one(task_pool.read_ref()).await?;
            if pending == 0 {
                break;
            }
        }
        self.then_existing_indices_types_and_names_are_complete(201).await?;
        Ok(())
    }
    pub async fn when_completed_delivery_is_duplicated(&self) -> anyhow::Result<()> {
        let rows = wallet_database::repositories::task_queue::TaskQueueRepo::all_tasks_queue(
            &mqtt::task_pool(self.ctx)?,
        )
        .await?;
        let before = self.recorder.snapshot().len();
        for row in rows.into_iter().filter(|row| row.id.starts_with("api_chain_address:")) {
            chain_sync::run_saved_job(self.ctx, &row.id).await?;
        }
        assert_eq!(
            self.recorder.snapshot().len(),
            before,
            "completed redelivery must not contact backend"
        );
        Ok(())
    }
}

impl ChainSyncScenario {
    pub async fn given_oversized_source_name(&self) -> anyhow::Result<()> {
        sqlx::query("UPDATE api_account SET name=? WHERE wallet_address='sync-sub-wallet' AND chain_code='eth' AND derivation_path_index=20")
            .bind("x".repeat(40_000)).execute(mqtt::api_wallet_pool(self.ctx)?.write_ref()).await?;
        Ok(())
    }
    pub async fn then_oversized_item_does_not_block_healthy_chunks_or_future_pages(
        &self,
    ) -> anyhow::Result<()> {
        self.then_failed_queue_contains("exceeds payload limit").await?;
        let rows = ApiAccountRepo::list(&mqtt::api_wallet_pool(self.ctx)?).await?;
        let targets: Vec<_> =
            rows.iter().filter(|row| CODES.contains(&row.chain_code.as_str())).collect();
        assert_eq!(targets.len(), 101 * 10, "future page must be generated despite oversized item");
        for row in targets {
            let oversized =
                row.wallet_address == "sync-sub-wallet" && row.derivation_path_index == 20;
            assert_eq!(
                row.is_init,
                if oversized { 0 } else { 1 },
                "healthy preceding/following items must register"
            );
        }
        let calls = self.recorder.snapshot();
        assert!(!calls.is_empty());
        for call in calls {
            let body = crate::harness::worker::decrypt_captured_api_backend_body(&call.body);
            assert!(body["addressList"].as_array().unwrap().len() <= 40);
            assert!(
                serde_json::to_vec(&body)?.len() <= 32 * 1024,
                "oversized item must never reach transport"
            );
        }
        Ok(())
    }
}

impl ChainSyncScenario {
    pub async fn when_unknown_external_owner_is_delivered(&self) -> anyhow::Result<()> {
        use wallet_database::repositories::task_queue::TaskQueueRepo;
        let pool = mqtt::task_pool(self.ctx)?;
        let deep =
            format!(r#"{{"owner":"external","unused":{}0{}}}"#, "[".repeat(140), "]".repeat(140));
        for (case, remark) in [
            r#"{"owner":"future-consumer"}"#,
            r#"{"owner":"external","owner":""}"#,
            r#"{"\u006fwner":"external","owner":""}"#,
            deep.as_str(),
            r#"{"owner":"external","unused":1e999}"#,
        ]
        .into_iter()
        .enumerate()
        {
            for status in [0, 1, 3, 4] {
                sqlx::query("INSERT INTO task_queue(id,task_name,request_body,type,status,remark,created_at) VALUES(?,'UnknownRecovery','{}',1,?,?,'1970-01-01T00:00:00Z')").bind(format!("external-{case}-{status}")).bind(status).bind(remark).execute(pool.write_ref()).await?;
            }
        }
        chain_sync::deliver_saved_owned_through_default_entry(self.ctx).await?;
        assert!(
            TaskQueueRepo::recoverable_default(&pool)
                .await?
                .iter()
                .all(|r| !r.id.starts_with("external-")),
            "startup recovery must not reach unknown-name deletion for external records"
        );
        TaskQueueRepo::delete_old(&pool, 15).await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_queue WHERE id LIKE 'external-%' AND status IN(0,1,3,4)",
        )
        .fetch_one(pool.read_ref())
        .await?;
        assert_eq!(
            count, 20,
            "dispatch, startup selection and age cleanup must retain all external records"
        );
        Ok(())
    }
}
