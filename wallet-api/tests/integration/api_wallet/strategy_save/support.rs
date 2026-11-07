use crate::harness::worker::{
    MockBackendRecorder, decrypt_captured_api_backend_body, worker_context_without_actors,
};
use wallet_transport_backend::request::api_wallet::strategy::{
    ChainConfig, IndexAndAddress, Strategy,
};

static ENV: tokio::sync::OnceCell<(
    &'static wallet_api::Context,
    std::path::PathBuf,
    MockBackendRecorder,
)> = tokio::sync::OnceCell::const_new();

#[derive(Clone, Copy)]
pub enum Kind {
    Collect,
    Withdraw,
}

pub struct Scenario {
    manager: wallet_api::manager::WalletManager,
    _guard: wallet_api::testkit::ordinary_business::TaskGuard,
    recorder: MockBackendRecorder,
    pool: wallet_database::ApiWalletDbPool,
    db_dir: std::path::PathBuf,
    uid: String,
    kind: Kind,
}

impl Scenario {
    pub async fn given(kind: Kind) -> Self {
        let (ctx, db_dir, recorder) = ENV.get_or_init(worker_context_without_actors).await;
        let ctx = *ctx;
        let db_dir = db_dir.clone();
        let recorder = recorder.clone();
        recorder.reset();
        let pool = crate::harness::open_api_wallet_pool(&db_dir).await;
        sqlx::query("DROP TRIGGER IF EXISTS strategy_save_fail")
            .execute(pool.write_ref())
            .await
            .unwrap();
        let (manager, guard) = wallet_api::testkit::ordinary_business::manager(ctx);
        Self {
            manager,
            _guard: guard,
            recorder,
            pool,
            db_dir,
            uid: format!("strategy-{}", uuid::Uuid::new_v4().simple()),
            kind,
        }
    }
    fn table(&self) -> &'static str {
        match self.kind {
            Kind::Collect => "api_collect_strategy_chain_config",
            Kind::Withdraw => "api_withdraw_strategy_chain_config",
        }
    }
    fn strategy(&self, threshold: u32, chains: &[&str]) -> Strategy {
        Strategy::new(
            &self.uid,
            threshold,
            chains
                .iter()
                .map(|chain| ChainConfig {
                    chain_code: (*chain).to_owned(),
                    chain_address_type: None,
                    normal_address: IndexAndAddress {
                        index: Some(0),
                        address: format!("0x{:040x}", threshold),
                    },
                    risk_address: IndexAndAddress {
                        index: Some(1),
                        address: format!("0x{:040x}", threshold + 1),
                    },
                })
                .collect(),
        )
    }
    pub async fn save(
        &self,
        threshold: u32,
        chains: &[&str],
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        let s = self.strategy(threshold, chains);
        match self.kind {
            Kind::Collect => {
                self.manager.update_collect_strategy(&self.uid, threshold, s.chain_configs).await
            }
            Kind::Withdraw => {
                self.manager.update_withdrawal_strategy(&self.uid, threshold, s.chain_configs).await
            }
        }
    }
    async fn query(&self) -> Result<Strategy, wallet_api::error::service::ServiceError> {
        match self.kind {
            Kind::Collect => self.manager.get_collect_strategy(&self.uid).await,
            Kind::Withdraw => self.manager.get_withdrawal_strategy(&self.uid).await,
        }
    }
    pub async fn given_second_chain_write_fails(&self) {
        sqlx::query(&format!("CREATE TRIGGER strategy_save_fail BEFORE INSERT ON {} WHEN NEW.chain_code='arbitrum' BEGIN SELECT RAISE(FAIL, 'injected strategy disk write failure'); END", self.table()))
            .execute(self.pool.write_ref()).await.unwrap();
    }
    pub async fn given_writes_recover(&self) {
        sqlx::query("DROP TRIGGER strategy_save_fail")
            .execute(self.pool.write_ref())
            .await
            .unwrap();
    }
    pub fn given_backend_fails(&self) {
        self.recorder.fail_next_api_backend_call(999, "injected backend save failure");
    }
    pub fn given_backend_strategy(&self, threshold: u32, chains: &[&str]) {
        self.recorder.succeed_next_api_backend_call_with_data(
            serde_json::to_value(self.strategy(threshold, chains)).unwrap(),
        );
    }
    pub async fn then_local_snapshot(&self, threshold: u32, chains: &[&str]) {
        let before = self.recorder.snapshot().len();
        let actual = self.query().await.unwrap();
        assert_eq!(actual.threshold, threshold, "failed replacement must preserve old threshold");
        let mut actual_chains: Vec<_> =
            actual.chain_configs.iter().map(|c| c.chain_code.as_str()).collect();
        actual_chains.sort();
        let mut expected = chains.to_vec();
        expected.sort();
        assert_eq!(actual_chains, expected, "strategy must retain complete chain configuration");
        for c in actual.chain_configs {
            assert_eq!(c.normal_address.address, format!("0x{:040x}", threshold));
        }
        assert_eq!(
            self.recorder.snapshot().len(),
            before,
            "local snapshot query must not hit backend"
        );
        let reopened = crate::harness::open_api_wallet_pool(&self.db_dir).await;
        let rows: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM {} c JOIN {} s ON c.strategy_id=s.id WHERE s.uid=?",
            self.table(),
            self.table().trim_end_matches("_chain_config")
        ))
        .bind(&self.uid)
        .fetch_one(reopened.read_ref())
        .await
        .unwrap();
        assert_eq!(rows, chains.len() as i64, "persisted list must survive pool reopen");
    }
    pub async fn then_no_local_strategy(&self) {
        let table = self.table().trim_end_matches("_chain_config");
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE uid=?"))
            .bind(&self.uid)
            .fetch_one(self.pool.read_ref())
            .await
            .unwrap();
        assert_eq!(
            count, 0,
            "failed first cache write must leave no header that hides backend retry"
        );
    }
    pub async fn query_for_cache(&self) -> bool {
        self.query().await.is_ok()
    }
    pub fn then_backend_calls(&self, expected: usize) {
        let requests = self.recorder.snapshot();
        assert_eq!(requests.len(), expected);
        for req in requests {
            let body = decrypt_captured_api_backend_body(&req.body);
            assert_eq!(body["uid"], self.uid);
            assert!(req.path.contains("strategy/"));
        }
    }
}
