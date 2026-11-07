#![cfg(feature = "integration-tests")]

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use wallet_api::{
    ApiWalletBackend, dirs::Dirs, infrastructure::phrase_package::PhrasePackageCodec,
    manager::WalletManager,
};
use wallet_database::{
    ApiWalletDbPool, CoreDbPool, SqliteContext,
    entities::{
        api_chain::{ApiChainCreateVo, NodeBindType},
        api_wallet::{ApiWalletEntity, ApiWalletType},
        device::CreateDeviceEntity,
    },
    repositories::{
        api_wallet::{chain::ApiChainRepo, wallet::ApiWalletRepo},
        device::DeviceRepo,
    },
};
use wallet_transport_backend::{
    request::{
        DeviceDeleteReq, KeysInitReq,
        api_wallet::{
            address::ExpandAddressCompleteReq,
            swap::{ApiInitSwapReq, ApiInitSwapResponse},
            wallet::{
                AppIdImportRechargeWalletReq, AppIdImportReq, AppIdUidUsageReq, BindAppIdReq,
            },
        },
    },
    response_vo::api_wallet::wallet::{
        ActiveStatus, AppIdUidUsageRes, KeysUidCheckRes, QueryUidBindInfoRes,
        QueryWalletActivationInfoResp, QueryWalletActivationInfoRespItem, UidStatus,
    },
};

pub const SMOKE_WALLET_PASSWORD: &str = "q1111111";
const TEST_SN: &str = "smoke-test-sn";
const TEST_DEVICE_TYPE: &str = "ANDROID";

#[allow(dead_code)]
pub(crate) mod gwt;
#[allow(dead_code)]
pub(crate) mod worker;

pub(crate) use gwt::{
    AssertRole, CountRole, GivenRole, LoadRole, ScenarioRoles, SeedRole, ThenRole, WhenRole,
};

#[allow(unused_imports)]
pub(crate) use worker::{
    WorkerTestEnv, decrypt_captured_api_backend_body, ensure_worker_env, pop_request_with_retry,
};

static TEST_ENV: once_cell::sync::Lazy<tokio::sync::OnceCell<TestEnv>> =
    once_cell::sync::Lazy::new(tokio::sync::OnceCell::const_new);
static UNIQUE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindSnapshot {
    pub merchant_id: Option<String>,
    pub app_id: Option<String>,
    pub sn: Option<String>,
    pub binding_address: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletPair {
    pub recharge_uid: String,
    pub withdrawal_uid: String,
    pub recharge_address: String,
    pub withdrawal_address: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedBindReq {
    pub recharge_uid: String,
    pub withdrawal_uid: String,
    pub org_app_id: String,
    pub sn: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindAppIdCall {
    pub recharge_uid: String,
    pub withdrawal_uid: String,
    pub org_app_id: String,
    pub sn: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdImportCall {
    pub sn: String,
    pub recharge_uid: Option<String>,
    pub withdrawal_uid: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdImportRechargeWalletCall {
    pub sn: String,
    pub recharge_uid: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdUidUsageCall {
    pub org_app_id: String,
    pub uid: String,
    pub wallet_type: UidStatus,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandAddressCompleteCall {
    pub uid: String,
    pub batch_id: String,
    pub serial_no: String,
    pub status: bool,
    pub remark: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdWithdrawalWalletChangeCall {
    pub withdrawal_uid: String,
    pub org_app_id: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitSwapCall {
    pub sn: String,
    pub client_pub_key: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDeleteCall {
    pub sn: String,
    pub uid_list: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeysInitCall {
    pub uid: String,
    pub sn: String,
    pub client_id: Option<String>,
    pub device_type: Option<String>,
    pub name: String,
    pub invite_code: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiWalletBackendCall {
    WalletBindAppId(BindAppIdCall),
    AppIdImport(AppIdImportCall),
    AppIdImportRechargeWallet(AppIdImportRechargeWalletCall),
    KeysUidCheck { uid: String },
    QueryUidBindInfo { uid: String },
    QueryWalletActivationInfo { uid: String },
    AppIdUidUsage(AppIdUidUsageCall),
    ExpandAddressComplete(ExpandAddressCompleteCall),
    AppIdWithdrawalWalletChange(AppIdWithdrawalWalletChangeCall),
    InitSwap(InitSwapCall),
    DeviceDelete(DeviceDeleteCall),
    InitApiWallet(AppIdImportCall),
    OldKeysInit(KeysInitCall),
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BindAppIdReqView {
    recharge_uid: String,
    withdrawal_uid: String,
    org_app_id: String,
    sn: String,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppIdImportReqView {
    sn: String,
    recharge_uid: Option<String>,
    withdrawal_uid: Option<String>,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppIdImportRechargeWalletReqView {
    sn: String,
    recharge_uid: String,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppIdUidUsageReqView {
    org_app_id: String,
    uid: String,
    wallet_type: UidStatus,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpandAddressCompleteReqView {
    uid: String,
    batch_id: String,
    serial_no: String,
    status: bool,
    remark: Option<String>,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiInitSwapReqView {
    sn: String,
    client_pub_key: String,
}

#[allow(dead_code)]
#[derive(Default)]
struct FakeState {
    keys_uid_status_queue: VecDeque<UidStatus>,
    query_uid_bind_info_queue: VecDeque<QueryUidBindInfoRes>,
    query_wallet_activation_info_queue: VecDeque<Vec<(String, ActiveStatus)>>,
    appid_uid_usage_used_queue: VecDeque<bool>,
    wallet_bind_appid_error: Option<String>,
    init_api_wallet_error: Option<String>,
    old_keys_init_error: Option<String>,
    old_keys_init_gate: Option<Arc<tokio::sync::Barrier>>,
    keys_uid_check_gate: Option<Arc<tokio::sync::Barrier>>,
    appid_import_error: Option<String>,
    appid_import_delay: Option<Duration>,
    appid_import_recharge_wallet_error: Option<String>,
    query_uid_bind_info_error: Option<String>,
    query_wallet_activation_info_error: Option<String>,
    device_delete_error: Option<String>,
    device_delete_timeout: bool,
    device_delete_gate: Option<Arc<tokio::sync::Barrier>>,
    calls: Vec<ApiWalletBackendCall>,
}

#[allow(dead_code)]
#[derive(Default)]
pub struct FakeApiWalletBackend {
    state: Mutex<FakeState>,
}

#[allow(dead_code)]
impl FakeApiWalletBackend {
    pub fn gate_next_keys_uid_check(&self, gate: Arc<tokio::sync::Barrier>) {
        self.state.lock().expect("fake backend lock poisoned").keys_uid_check_gate = Some(gate);
    }

    pub fn gate_next_old_keys_init(&self, gate: Arc<tokio::sync::Barrier>) {
        self.state.lock().expect("fake backend lock poisoned").old_keys_init_gate = Some(gate);
    }

    pub fn gate_next_device_delete(&self, gate: Arc<tokio::sync::Barrier>) {
        self.state.lock().expect("fake backend lock poisoned").device_delete_gate = Some(gate);
    }
    pub fn set_device_delete_timeout(&self, timeout: bool) {
        self.state.lock().expect("fake backend lock poisoned").device_delete_timeout = timeout;
    }
    pub fn set_device_delete_error(&self, message: Option<&str>) {
        self.state.lock().expect("fake backend lock poisoned").device_delete_error =
            message.map(str::to_owned);
    }

    pub fn reset(&self) {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        *state = FakeState::default();
    }

    pub fn enqueue_keys_uid_status(&self, status: UidStatus) {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.keys_uid_status_queue.push_back(status);
    }

    pub fn enqueue_query_uid_bind_info(
        &self,
        app_id: &str,
        org_id: &str,
        bind_status: bool,
        sn: &str,
    ) {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.query_uid_bind_info_queue.push_back(QueryUidBindInfoRes {
            app_id: app_id.to_string(),
            org_id: org_id.to_string(),
            bind_status,
            sn: sn.to_string(),
        });
    }

    pub fn enqueue_appid_uid_usage_used(&self, used: bool) {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.appid_uid_usage_used_queue.push_back(used);
    }

    pub fn enqueue_wallet_activation_info(&self, items: Vec<(&str, ActiveStatus)>) {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.query_wallet_activation_info_queue.push_back(
            items.into_iter().map(|(chain, active)| (chain.to_string(), active)).collect(),
        );
    }

    pub fn set_wallet_bind_appid_error(&self, msg: Option<&str>) {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.wallet_bind_appid_error = msg.map(ToString::to_string);
    }

    pub fn set_appid_import_error(&self, msg: Option<&str>) {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.appid_import_error = msg.map(ToString::to_string);
    }

    pub fn set_appid_import_delay(&self, delay: Option<Duration>) {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.appid_import_delay = delay;
    }

    pub fn set_query_uid_bind_info_error(&self, msg: Option<&str>) {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.query_uid_bind_info_error = msg.map(ToString::to_string);
    }

    pub fn set_query_wallet_activation_info_error(&self, msg: Option<&str>) {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.query_wallet_activation_info_error = msg.map(ToString::to_string);
    }

    pub fn with_calls<R>(&self, f: impl FnOnce(&[ApiWalletBackendCall]) -> R) -> R {
        let state = self.state.lock().expect("fake backend lock poisoned");
        f(&state.calls)
    }

    fn record_appid_import_req(req: AppIdImportReq) -> AppIdImportCall {
        let view: AppIdImportReqView =
            serde_json::from_value(serde_json::to_value(req).expect("serialize appid import req"))
                .expect("deserialize appid import req");
        AppIdImportCall {
            sn: view.sn,
            recharge_uid: view.recharge_uid,
            withdrawal_uid: view.withdrawal_uid,
        }
    }

    fn service_error(msg: &str) -> wallet_api::error::service::ServiceError {
        wallet_api::error::service::ServiceError::System(
            wallet_api::error::system::SystemError::Internal(msg.to_string()),
        )
    }
}

#[async_trait]
impl ApiWalletBackend for FakeApiWalletBackend {
    async fn wallet_bind_appid(
        &self,
        req: BindAppIdReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        let view: BindAppIdReqView =
            serde_json::from_value(serde_json::to_value(req).expect("serialize bind req"))
                .expect("deserialize bind req");

        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.calls.push(ApiWalletBackendCall::WalletBindAppId(BindAppIdCall {
            recharge_uid: view.recharge_uid,
            withdrawal_uid: view.withdrawal_uid,
            org_app_id: view.org_app_id,
            sn: view.sn,
        }));

        if let Some(msg) = state.wallet_bind_appid_error.clone() {
            return Err(Self::service_error(&msg));
        }
        Ok(())
    }

    async fn init_api_wallet(
        &self,
        req: AppIdImportReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.calls.push(ApiWalletBackendCall::InitApiWallet(Self::record_appid_import_req(req)));
        if let Some(msg) = state.init_api_wallet_error.clone() {
            return Err(Self::service_error(&msg));
        }
        Ok(())
    }

    async fn old_keys_init(
        &self,
        req: KeysInitReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        let gate = {
            let mut state = self.state.lock().expect("fake backend lock poisoned");
            state.calls.push(ApiWalletBackendCall::OldKeysInit(KeysInitCall {
                uid: req.uid,
                sn: req.sn,
                client_id: req.client_id,
                device_type: req.device_type,
                name: req.name,
                invite_code: req.invite_code,
            }));
            if let Some(msg) = state.old_keys_init_error.clone() {
                return Err(Self::service_error(&msg));
            }
            state.old_keys_init_gate.take()
        };
        if let Some(gate) = gate {
            gate.wait().await;
            gate.wait().await;
        }
        Ok(())
    }

    async fn appid_import(
        &self,
        req: AppIdImportReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        let (delay, err_msg) = {
            let mut state = self.state.lock().expect("fake backend lock poisoned");
            state.calls.push(ApiWalletBackendCall::AppIdImport(Self::record_appid_import_req(req)));
            (state.appid_import_delay, state.appid_import_error.clone())
        };
        if let Some(msg) = err_msg {
            return Err(Self::service_error(&msg));
        }
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        Ok(())
    }

    async fn appid_import_recharge_wallet(
        &self,
        req: AppIdImportRechargeWalletReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        let view: AppIdImportRechargeWalletReqView = serde_json::from_value(
            serde_json::to_value(req).expect("serialize import recharge req"),
        )
        .expect("deserialize import recharge req");
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.calls.push(ApiWalletBackendCall::AppIdImportRechargeWallet(
            AppIdImportRechargeWalletCall { sn: view.sn, recharge_uid: view.recharge_uid },
        ));
        if let Some(msg) = state.appid_import_recharge_wallet_error.clone() {
            return Err(Self::service_error(&msg));
        }
        Ok(())
    }

    async fn keys_uid_check(
        &self,
        uid: &str,
    ) -> Result<KeysUidCheckRes, wallet_api::error::service::ServiceError> {
        let (status, gate) = {
            let mut state = self.state.lock().expect("fake backend lock poisoned");
            state.calls.push(ApiWalletBackendCall::KeysUidCheck { uid: uid.to_string() });
            let status = state
                .keys_uid_status_queue
                .pop_front()
                .unwrap_or_else(|| panic!("keys_uid_check response not configured for uid={uid}"));
            (status, state.keys_uid_check_gate.take())
        };
        if let Some(gate) = gate {
            gate.wait().await;
            gate.wait().await;
        }
        Ok(KeysUidCheckRes { uid: uid.to_string(), status })
    }

    async fn query_uid_bind_info(
        &self,
        uid: &str,
    ) -> Result<QueryUidBindInfoRes, wallet_api::error::service::ServiceError> {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.calls.push(ApiWalletBackendCall::QueryUidBindInfo { uid: uid.to_string() });
        if let Some(msg) = state.query_uid_bind_info_error.clone() {
            return Err(Self::service_error(&msg));
        }
        let res = state
            .query_uid_bind_info_queue
            .pop_front()
            .unwrap_or_else(|| panic!("query_uid_bind_info response not configured for uid={uid}"));
        Ok(res)
    }

    async fn query_wallet_activation_info(
        &self,
        uid: &str,
    ) -> Result<QueryWalletActivationInfoResp, wallet_api::error::service::ServiceError> {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.calls.push(ApiWalletBackendCall::QueryWalletActivationInfo { uid: uid.to_string() });
        if let Some(msg) = state.query_wallet_activation_info_error.clone() {
            return Err(Self::service_error(&msg));
        }
        let items = state.query_wallet_activation_info_queue.pop_front().unwrap_or_else(|| {
            panic!("query_wallet_activation_info response not configured for uid={uid}")
        });
        Ok(QueryWalletActivationInfoResp(
            items
                .into_iter()
                .map(|(chain, active)| QueryWalletActivationInfoRespItem { chain, active })
                .collect(),
        ))
    }

    async fn appid_uid_usage(
        &self,
        req: AppIdUidUsageReq,
    ) -> Result<AppIdUidUsageRes, wallet_api::error::service::ServiceError> {
        let view: AppIdUidUsageReqView = serde_json::from_value(
            serde_json::to_value(req).expect("serialize appid uid usage req"),
        )
        .expect("deserialize appid uid usage req");
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.calls.push(ApiWalletBackendCall::AppIdUidUsage(AppIdUidUsageCall {
            org_app_id: view.org_app_id,
            uid: view.uid,
            wallet_type: view.wallet_type,
        }));
        let used = state
            .appid_uid_usage_used_queue
            .pop_front()
            .unwrap_or_else(|| panic!("appid_uid_usage response not configured"));
        Ok(AppIdUidUsageRes { used })
    }

    async fn expand_address_complete(
        &self,
        req: ExpandAddressCompleteReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        let view: ExpandAddressCompleteReqView = serde_json::from_value(
            serde_json::to_value(req).expect("serialize expand address complete req"),
        )
        .expect("deserialize expand address complete req");
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.calls.push(ApiWalletBackendCall::ExpandAddressComplete(ExpandAddressCompleteCall {
            uid: view.uid,
            batch_id: view.batch_id,
            serial_no: view.serial_no,
            status: view.status,
            remark: view.remark,
        }));
        Ok(())
    }

    async fn appid_withdrawal_wallet_change(
        &self,
        withdrawal_uid: &str,
        org_app_id: &str,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.calls.push(ApiWalletBackendCall::AppIdWithdrawalWalletChange(
            AppIdWithdrawalWalletChangeCall {
                withdrawal_uid: withdrawal_uid.to_string(),
                org_app_id: org_app_id.to_string(),
            },
        ));
        Ok(())
    }

    async fn init_swap(
        &self,
        req: &ApiInitSwapReq,
    ) -> Result<ApiInitSwapResponse, wallet_api::error::service::ServiceError> {
        let view: ApiInitSwapReqView =
            serde_json::from_value(serde_json::to_value(req).expect("serialize init swap req"))
                .expect("deserialize init swap req");
        let mut state = self.state.lock().expect("fake backend lock poisoned");
        state.calls.push(ApiWalletBackendCall::InitSwap(InitSwapCall {
            sn: view.sn,
            client_pub_key: view.client_pub_key,
        }));
        Ok(ApiInitSwapResponse { success: true, code: None, msg: None, data: None })
    }

    async fn device_delete(
        &self,
        req: &DeviceDeleteReq,
    ) -> Result<Option<()>, wallet_api::error::service::ServiceError> {
        let (gate, timeout) = {
            let mut state = self.state.lock().expect("fake backend lock poisoned");
            state.calls.push(ApiWalletBackendCall::DeviceDelete(DeviceDeleteCall {
                sn: req.sn.clone(),
                uid_list: req.uid_list.clone(),
            }));
            if let Some(message) = &state.device_delete_error {
                return Err(wallet_api::error::service::ServiceError::Parameter(message.clone()));
            }
            (state.device_delete_gate.take(), state.device_delete_timeout)
        };
        if let Some(gate) = gate {
            gate.wait().await;
            gate.wait().await;
        }
        if timeout {
            return Err(wallet_transport_backend::Error::Timeout.into());
        }
        Ok(Some(()))
    }
}

#[allow(dead_code)]
pub struct TestEnv {
    pub manager: WalletManager,
    pub fake_backend: Arc<FakeApiWalletBackend>,
    pub sn: String,
    pub db_dir: PathBuf,
}

#[allow(dead_code)]
pub async fn ensure_env() -> &'static TestEnv {
    TEST_ENV
        .get_or_init(|| async {
            let config = wallet_api::config::Config::new(
                r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
aggregate_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
"#,
            )
            .expect("parse test config");

            let root = create_test_root_dir();
            let dirs = Dirs::new(root.to_str().expect("utf8 root dir")).expect("create dirs");
            let db_dir = dirs.db_dir.clone();
            let fake_backend = Arc::new(FakeApiWalletBackend::default());
            let sn = TEST_SN.to_string();
            unsafe {
                std::env::set_var("WALLET_TRANSPORT_NO_PROXY", "1");
            }

            let manager = WalletManager::new_for_test(
                &sn,
                TEST_DEVICE_TYPE,
                config,
                dirs,
                fake_backend.clone(),
            )
            .await
            .expect("create test wallet manager");
            prepare_minimum_data(&db_dir, &sn).await;
            manager
                .set_passwd_cache(SMOKE_WALLET_PASSWORD)
                .await
                .expect("cache smoke wallet password");

            let local_pub_key = wallet_ecdh::GLOBAL_KEY.secret_pub_key();
            wallet_ecdh::GLOBAL_KEY
                .set_shared_secret(&local_pub_key)
                .expect("set local shared secret");

            TestEnv { manager, fake_backend, sn, db_dir }
        })
        .await
}

#[allow(dead_code)]
fn create_test_root_dir() -> PathBuf {
    let id = UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("wallet_api_smoke_{id}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create test root");
    root
}

#[allow(dead_code)]
pub async fn open_core_pool(db_dir: &Path) -> CoreDbPool {
    let sqlite = SqliteContext::new(&db_dir.to_string_lossy(), Some("data.db"))
        .await
        .expect("open core sqlite");
    let pool = sqlite.get_pool().expect("core db pool");
    CoreDbPool::new(pool)
}

pub async fn open_api_wallet_pool(db_dir: &Path) -> ApiWalletDbPool {
    let sqlite = SqliteContext::new(&db_dir.to_string_lossy(), Some("api_wallet.db"))
        .await
        .expect("open api wallet sqlite");
    let pool = sqlite.get_pool().expect("api wallet db pool");
    ApiWalletDbPool::new(pool)
}

#[allow(dead_code)]
async fn prepare_minimum_data(db_dir: &Path, sn: &str) {
    let core_pool = open_core_pool(db_dir).await;
    DeviceRepo::upsert(
        core_pool,
        CreateDeviceEntity {
            device_type: TEST_DEVICE_TYPE.to_string(),
            sn: sn.to_string(),
            code: "test-code".to_string(),
            system_ver: "1.0.0".to_string(),
            iemi: None,
            meid: None,
            iccid: None,
            mem: None,
            app_id: Some("test-app".to_string()),
            is_init: 1,
            language_init: 1,
        },
    )
    .await
    .expect("upsert device");

    let api_wallet_pool = open_api_wallet_pool(db_dir).await;
    ApiChainRepo::add(
        &api_wallet_pool,
        ApiChainCreateVo::new(
            "TRON",
            "tron",
            &["m/44'/195'/0'/0".to_string()],
            NodeBindType::AutoBackend,
            "TRX",
        ),
    )
    .await
    .expect("insert chain");
}

pub fn next_tag(prefix: &str) -> String {
    let id = UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{id}")
}

#[allow(dead_code)]
pub fn next_unique_id() -> u64 {
    UNIQUE_ID.fetch_add(1, Ordering::Relaxed)
}

fn next_eth_like_address() -> String {
    let id = UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    format!("0x{id:040x}")
}

async fn encrypt_test_secret(data: &[u8]) -> Vec<u8> {
    let phrase = std::str::from_utf8(data).expect("test secret must be utf8");
    PhrasePackageCodec::encrypt_phrase(SMOKE_WALLET_PASSWORD, phrase)
        .await
        .expect("encrypt test secret")
}

async fn encrypt_test_seed(data: &[u8]) -> Vec<u8> {
    wallet_api::testkit::seed::encrypt_seed(SMOKE_WALLET_PASSWORD, data).await
}

pub async fn upsert_wallet(
    db_dir: &Path,
    sn: &str,
    uid: &str,
    wallet_type: ApiWalletType,
    binding_address: Option<&str>,
) -> String {
    upsert_wallet_with_import_stage(db_dir, sn, uid, wallet_type, binding_address, 0).await
}

pub async fn upsert_wallet_with_import_stage(
    db_dir: &Path,
    sn: &str,
    uid: &str,
    wallet_type: ApiWalletType,
    binding_address: Option<&str>,
    import_stage: u8,
) -> String {
    let address = next_eth_like_address();
    let phrase_enc = encrypt_test_secret(b"smoke-phrase").await;
    let seed_enc: Vec<u8> = encrypt_test_seed(b"smoke-seed").await;
    let pool = open_api_wallet_pool(db_dir).await;
    ApiWalletRepo::upsert(
        &pool,
        uid,
        &next_tag("wallet"),
        &address,
        &phrase_enc,
        &seed_enc,
        wallet_type,
        binding_address,
        sn,
        import_stage,
    )
    .await
    .expect("upsert wallet");
    address
}

#[allow(dead_code)]
pub async fn prepare_wallet_pair(env: &TestEnv) -> WalletPair {
    let recharge_uid = next_tag("recharge-uid");
    let withdrawal_uid = next_tag("withdrawal-uid");
    let recharge_address =
        upsert_wallet(&env.db_dir, &env.sn, &recharge_uid, ApiWalletType::SubAccount, None).await;
    let withdrawal_address =
        upsert_wallet(&env.db_dir, &env.sn, &withdrawal_uid, ApiWalletType::Withdrawal, None).await;

    env.manager
        .set_passwd_cache(SMOKE_WALLET_PASSWORD)
        .await
        .expect("refresh unlock session for prepared wallet pair");

    WalletPair { recharge_uid, withdrawal_uid, recharge_address, withdrawal_address }
}

#[allow(dead_code)]
pub fn reset_fake(env: &TestEnv) {
    env.fake_backend.reset();
}

#[allow(dead_code)]
pub async fn load_wallet_by_uid(env: &TestEnv, uid: &str) -> ApiWalletEntity {
    let pool = open_api_wallet_pool(&env.db_dir).await;
    ApiWalletRepo::find_by_uid(&pool, uid)
        .await
        .expect("query wallet by uid")
        .expect("wallet should exist")
}

#[allow(dead_code)]
pub async fn find_wallet_by_uid(env: &TestEnv, uid: &str) -> Option<ApiWalletEntity> {
    let pool = open_api_wallet_pool(&env.db_dir).await;
    ApiWalletRepo::find_by_uid(&pool, uid).await.expect("query wallet by uid")
}

#[allow(dead_code)]
pub fn snapshot_bind_fields(wallet: &ApiWalletEntity) -> BindSnapshot {
    BindSnapshot {
        merchant_id: wallet.merchant_id.clone(),
        app_id: wallet.app_id.clone(),
        sn: wallet.sn.clone(),
        binding_address: wallet.binding_address.clone(),
    }
}

#[allow(dead_code)]
pub fn assert_bind_call_once(fake: &FakeApiWalletBackend, expect: ExpectedBindReq) {
    fake.with_calls(|calls| {
        let binds: Vec<&BindAppIdCall> = calls
            .iter()
            .filter_map(|call| match call {
                ApiWalletBackendCall::WalletBindAppId(req) => Some(req),
                _ => None,
            })
            .collect();
        assert_eq!(binds.len(), 1, "expected exactly one wallet_bind_appid call");
        let bind = binds[0];
        assert_eq!(bind.recharge_uid, expect.recharge_uid);
        assert_eq!(bind.withdrawal_uid, expect.withdrawal_uid);
        assert_eq!(bind.org_app_id, expect.org_app_id);
        assert_eq!(bind.sn, expect.sn);
    });
}

#[allow(dead_code)]
pub fn derive_uid(phrase: &str, salt: &str) -> String {
    wallet_utils::pbkdf2_string(&format!("{phrase}{salt}"), salt, 100000, 32)
        .expect("derive uid from phrase and salt")
}

#[cfg(any(test, feature = "integration-tests"))]
pub mod ordinary_business;
