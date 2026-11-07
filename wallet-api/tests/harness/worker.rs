use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    net::{Shutdown, TcpListener},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use wallet_api::{ApiWalletBackend, Context, dirs::Dirs, manager::WalletManager};
use wallet_ecdh::GLOBAL_KEY;
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
        AppIdUidUsageRes, KeysUidCheckRes, QueryUidBindInfoRes, QueryWalletActivationInfoResp,
    },
};

use super::next_unique_id;

const WORKER_TEST_SN: &str = "collect-worker-test-sn";
const WORKER_TEST_DEVICE_TYPE: &str = "ANDROID";
const WORKER_TEST_PUB_KEY: &str = r#"-----BEGIN PUBLIC KEY-----
MFYwEAYHKoZIzj0CAQYFK4EEAAoDQgAEWDZNP0ClbeWJey9hBr2rsjSayQEBywnv
ZXi0RberQCAp+06fOjvr+jZI5qwYGglmMkGJw49tbni6qgm4QNV6WQ==
-----END PUBLIC KEY-----"#;

static WORKER_ENV: tokio::sync::OnceCell<WorkerTestEnv> = tokio::sync::OnceCell::const_new();

#[derive(Clone, Debug)]
pub(crate) struct CapturedHttpRequest {
    pub(crate) path: String,
    pub(crate) body: String,
}

#[derive(Default)]
struct MockBackendState {
    requests: VecDeque<CapturedHttpRequest>,
    responses: VecDeque<String>,
    response_gates: VecDeque<Arc<MockResponseGate>>,
    rejected_address: Option<String>,
    first_chain_gate: Option<(Arc<MockResponseGate>, Option<String>)>,
}

pub(crate) struct MockResponseGate {
    reached: tokio::sync::Notify,
    wait_limit: std::time::Duration,
    released: (Mutex<bool>, std::sync::Condvar),
}
impl MockResponseGate {
    pub(crate) async fn wait_for_request(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.reached.notified())
            .await
            .expect("mock request did not arrive");
    }
    pub(crate) fn release(&self) {
        let mut released = self.released.0.lock().expect("response gate");
        *released = true;
        self.released.1.notify_all();
    }
}

#[derive(Clone, Default)]
pub(crate) struct MockBackendRecorder {
    state: Arc<Mutex<MockBackendState>>,
}

impl MockBackendRecorder {
    fn push(&self, req: CapturedHttpRequest) {
        let mut state = self.state.lock().expect("mock backend lock poisoned");
        state.requests.push_back(req);
    }

    fn pop(&self) -> Option<CapturedHttpRequest> {
        let mut state = self.state.lock().expect("mock backend lock poisoned");
        state.requests.pop_front()
    }

    pub(crate) fn reset(&self) {
        let mut state = self.state.lock().expect("mock backend lock poisoned");
        state.requests.clear();
        state.responses.clear();
        state.response_gates.clear();
        state.first_chain_gate = None;
        state.rejected_address = None;
    }

    pub(crate) fn snapshot(&self) -> Vec<CapturedHttpRequest> {
        let state = self.state.lock().expect("mock backend lock poisoned");
        state.requests.iter().cloned().collect()
    }

    pub(crate) fn fail_next_api_backend_call(&self, code: i64, msg: &str) {
        let mut state = self.state.lock().expect("mock backend lock poisoned");
        state.responses.push_back(
            serde_json::json!({
                "success": false,
                "code": code.to_string(),
                "msg": msg,
                "data": null,
            })
            .to_string(),
        );
    }

    pub(crate) fn disconnect_next_backend_call(&self) {
        // Empty scripted reply closes the accepted connection without HTTP bytes.
        self.state.lock().expect("mock backend lock poisoned").responses.push_back(String::new());
    }

    pub(crate) fn succeed_next_backend_call_with_data(&self, data: serde_json::Value) {
        let mut state = self.state.lock().expect("mock backend lock poisoned");
        state.responses.push_back(
            serde_json::json!({"success":true,"code":"200","msg":"ok","data":data}).to_string(),
        );
    }

    pub(crate) fn succeed_next_api_backend_call_with_data(&self, data: serde_json::Value) {
        let encrypted = GLOBAL_KEY.encrypt(data.to_string().as_bytes()).expect("encrypt response");
        let key = wallet_utils::bytes_to_base64(&encrypted.key);
        let ciphertext = wallet_utils::bytes_to_base64(&encrypted.ciphertext);
        let body_data = key.clone() + ciphertext.as_str();
        let sign = wallet_utils::bytes_to_base64(
            &GLOBAL_KEY
                .sign(&uuid::Uuid::new_v4().to_string(), body_data.as_bytes())
                .expect("sign response"),
        );
        let mut state = self.state.lock().expect("mock backend lock poisoned");
        state.responses.push_back(
            serde_json::json!({
                "success": true,
                "code": "200",
                "msg": "ok",
                "data": {
                    "sign": sign,
                    "body": {
                        "key": key,
                        "data": ciphertext,
                    },
                },
            })
            .to_string(),
        );
    }

    pub(crate) fn hold_next_response(&self) -> Arc<MockResponseGate> {
        self.hold_next_response_for(std::time::Duration::from_secs(10))
    }
    pub(crate) fn hold_next_response_for(
        &self,
        wait_limit: std::time::Duration,
    ) -> Arc<MockResponseGate> {
        let gate = Arc::new(MockResponseGate {
            reached: tokio::sync::Notify::new(),
            wait_limit,
            released: (Mutex::new(false), std::sync::Condvar::new()),
        });
        self.state
            .lock()
            .expect("mock backend lock poisoned")
            .response_gates
            .push_back(gate.clone());
        gate
    }
    pub(crate) fn hold_first_registration_chain_for(
        &self,
        wait_limit: std::time::Duration,
    ) -> Arc<MockResponseGate> {
        let gate = Arc::new(MockResponseGate {
            reached: tokio::sync::Notify::new(),
            wait_limit,
            released: (Mutex::new(false), std::sync::Condvar::new()),
        });
        self.state.lock().expect("mock backend lock poisoned").first_chain_gate =
            Some((gate.clone(), None));
        gate
    }
    pub(crate) fn reject_registration_address(&self, address: &str) {
        self.state.lock().expect("mock backend").rejected_address = Some(address.to_owned());
    }
    fn next_response_body(&self, request: &CapturedHttpRequest) -> String {
        let (body, gate) = {
            let mut state = self.state.lock().expect("mock backend lock poisoned");
            let rejected = state.rejected_address.as_ref().is_some_and(|address| {
                request.path == "/awallet/aw/address/init"
                    && decrypt_captured_api_backend_body(&request.body)["addressList"]
                        .as_array()
                        .is_some_and(|items| {
                            items
                                .iter()
                                .any(|item| item["address"].as_str() == Some(address.as_str()))
                        })
            });
            let body = if rejected {
                r#"{"success":false,"code":"8050001","msg":"synthetic address not found","data":null}"#.to_owned()
            } else {
                state.responses.pop_front().unwrap_or_else(|| {
                    r#"{"success":true,"code":"200","msg":"ok","data":null}"#.to_string()
                })
            };
            let chain_gate = if request.path == "/awallet/aw/address/init" {
                state.first_chain_gate.as_mut().and_then(|(gate, first)| {
                    let payload = decrypt_captured_api_backend_body(&request.body);
                    let chain = payload["addressList"][0]["chainCode"]
                        .as_str()
                        .expect("registration chain");
                    let first = first.get_or_insert_with(|| chain.to_owned());
                    (first == chain).then(|| gate.clone())
                })
            } else {
                None
            };
            (body, chain_gate.or_else(|| state.response_gates.pop_front()))
        };
        if let Some(gate) = gate {
            gate.reached.notify_one();
            let released = gate.released.0.lock().expect("response gate");
            let _ = gate
                .released
                .1
                .wait_timeout_while(released, gate.wait_limit, |released| !*released)
                .expect("response gate wait");
        }
        body
    }
}

pub(crate) async fn pop_request_with_retry(
    recorder: &MockBackendRecorder,
) -> Option<CapturedHttpRequest> {
    for _ in 0..20 {
        if let Some(req) = recorder.pop() {
            return Some(req);
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    None
}

#[derive(Default)]
struct NoopApiWalletBackend;

#[async_trait::async_trait]
impl ApiWalletBackend for NoopApiWalletBackend {
    async fn wallet_bind_appid(
        &self,
        _req: BindAppIdReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        Ok(())
    }

    async fn init_api_wallet(
        &self,
        _req: AppIdImportReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        Ok(())
    }

    async fn old_keys_init(
        &self,
        _req: KeysInitReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        Ok(())
    }

    async fn appid_import(
        &self,
        _req: AppIdImportReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        Ok(())
    }

    async fn appid_import_recharge_wallet(
        &self,
        _req: AppIdImportRechargeWalletReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        Ok(())
    }

    async fn keys_uid_check(
        &self,
        _uid: &str,
    ) -> Result<KeysUidCheckRes, wallet_api::error::service::ServiceError> {
        Err(wallet_api::error::service::ServiceError::System(
            wallet_api::error::system::SystemError::Internal("noop".to_string()),
        ))
    }

    async fn query_uid_bind_info(
        &self,
        _uid: &str,
    ) -> Result<QueryUidBindInfoRes, wallet_api::error::service::ServiceError> {
        Err(wallet_api::error::service::ServiceError::System(
            wallet_api::error::system::SystemError::Internal("noop".to_string()),
        ))
    }

    async fn query_wallet_activation_info(
        &self,
        _uid: &str,
    ) -> Result<QueryWalletActivationInfoResp, wallet_api::error::service::ServiceError> {
        Ok(QueryWalletActivationInfoResp(Vec::new()))
    }

    async fn appid_uid_usage(
        &self,
        _req: AppIdUidUsageReq,
    ) -> Result<AppIdUidUsageRes, wallet_api::error::service::ServiceError> {
        Err(wallet_api::error::service::ServiceError::System(
            wallet_api::error::system::SystemError::Internal("noop".to_string()),
        ))
    }

    async fn expand_address_complete(
        &self,
        _req: ExpandAddressCompleteReq,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        Ok(())
    }

    async fn appid_withdrawal_wallet_change(
        &self,
        _withdrawal_uid: &str,
        _org_app_id: &str,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        Ok(())
    }

    async fn init_swap(
        &self,
        _req: &ApiInitSwapReq,
    ) -> Result<ApiInitSwapResponse, wallet_api::error::service::ServiceError> {
        Ok(ApiInitSwapResponse { success: true, code: None, msg: None, data: None })
    }

    async fn device_delete(
        &self,
        _req: &DeviceDeleteReq,
    ) -> Result<Option<()>, wallet_api::error::service::ServiceError> {
        Ok(Some(()))
    }
}

pub(crate) struct WorkerTestEnv {
    pub(crate) _manager: WalletManager,
    pub(crate) backend_url: String,
    pub(crate) db_dir: PathBuf,
    pub(crate) recorder: MockBackendRecorder,
}

impl WorkerTestEnv {
    pub(crate) fn ctx(&self) -> &'static Context {
        self._manager.ctx()
    }
}

pub(crate) fn start_mock_backend_server() -> io::Result<(String, MockBackendRecorder)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let recorder = MockBackendRecorder::default();
    let recorder_clone = recorder.clone();

    std::thread::spawn(move || {
        loop {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let recorder = recorder_clone.clone();
            std::thread::spawn(move || {
                let mut header_buf = Vec::new();
                let mut temp = [0u8; 1024];
                let header_end;
                loop {
                    let n = match stream.read(&mut temp) {
                        Ok(0) => return,
                        Ok(n) => n,
                        Err(_) => return,
                    };
                    header_buf.extend_from_slice(&temp[..n]);
                    if let Some(pos) = header_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = pos + 4;
                        break;
                    }
                }

                let header_text = String::from_utf8_lossy(&header_buf[..header_end]);
                let path = header_text
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or_default()
                    .to_string();
                let content_length = header_text
                    .lines()
                    .find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);

                let mut body = header_buf[header_end..].to_vec();
                while body.len() < content_length {
                    let n = match stream.read(&mut temp) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => return,
                    };
                    body.extend_from_slice(&temp[..n]);
                }

                let request =
                    CapturedHttpRequest { path, body: String::from_utf8_lossy(&body).to_string() };
                recorder.push(request.clone());
                let response_body = recorder.next_response_body(&request);
                if response_body.is_empty() {
                    let _ = stream.shutdown(Shutdown::Both);
                    return;
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
                let _ = stream.shutdown(Shutdown::Both);
            });
        }
    });

    Ok((format!("http://{}", addr), recorder))
}

fn create_worker_test_root_dir() -> PathBuf {
    let pid = std::process::id();
    let id = next_unique_id();
    let root = std::env::temp_dir().join(format!("wallet_api_collect_worker_{pid}_{id}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create test root");
    root
}

pub(crate) async fn ensure_worker_env() -> &'static WorkerTestEnv {
    WORKER_ENV
        .get_or_init(|| async {
            let (backend_url, recorder) =
                start_mock_backend_server().expect("start mock backend server");
            // Match wallet-api test env setup and disable system proxy resolution for reqwest.
            unsafe {
                std::env::set_var("WALLET_TRANSPORT_NO_PROXY", "1");
            }
            let config = worker_test_config(&backend_url);

            let root = create_worker_test_root_dir();
            let dirs = Dirs::new(root.to_str().expect("utf8 root dir")).expect("create dirs");
            GLOBAL_KEY.set_shared_secret(WORKER_TEST_PUB_KEY).expect("set shared secret");
            let manager = WalletManager::new_for_test(
                WORKER_TEST_SN,
                WORKER_TEST_DEVICE_TYPE,
                config,
                dirs.clone(),
                Arc::new(NoopApiWalletBackend),
            )
            .await
            .expect("create wallet manager");
            wallet_api::infrastructure::system_ready::mark_system_ready();

            WorkerTestEnv { _manager: manager, backend_url, db_dir: dirs.db_dir.clone(), recorder }
        })
        .await
}

pub(crate) fn decrypt_captured_api_backend_body(body: &str) -> serde_json::Value {
    #[derive(serde::Deserialize)]
    struct CapturedApiBackendBody {
        key: String,
        data: String,
    }
    #[derive(serde::Deserialize)]
    struct CapturedApiBackendRequest {
        body: CapturedApiBackendBody,
    }

    let req: CapturedApiBackendRequest =
        serde_json::from_str(body).expect("deserialize captured backend request");
    let key = wallet_utils::base64_to_bytes(&req.body.key).expect("decode encrypted key");
    let data = wallet_utils::base64_to_bytes(&req.body.data).expect("decode encrypted data");
    let plain = GLOBAL_KEY.decrypt(&data, &key).expect("decrypt backend body");
    serde_json::from_slice(&plain).expect("deserialize decrypted payload")
}

fn worker_test_config(backend_url: &str) -> wallet_api::config::Config {
    wallet_api::config::Config::new(&format!(
        r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "{backend_url}"
  test_url: "{backend_url}"
  prod_url: "{backend_url}"
aggregate_api:
  dev_url: "{backend_url}"
  test_url: "{backend_url}"
  prod_url: "{backend_url}"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
"#
    ))
    .expect("parse test config")
}

/// Explicit worker-step tests must not race auto-started background actors.
pub(crate) async fn worker_context_without_actors()
-> (&'static Context, PathBuf, MockBackendRecorder) {
    let (backend_url, recorder) = start_mock_backend_server().expect("start mock backend server");
    unsafe {
        std::env::set_var("WALLET_TRANSPORT_NO_PROXY", "1");
    }
    let config = worker_test_config(&backend_url);
    let root = create_worker_test_root_dir();
    let dirs = Dirs::new(root.to_str().expect("utf8 root")).expect("test dirs");
    GLOBAL_KEY.set_shared_secret(WORKER_TEST_PUB_KEY).expect("test shared secret");
    GLOBAL_KEY.set_sn(WORKER_TEST_SN);
    let db_dir = dirs.db_dir.clone();
    let ctx = wallet_api::testkit::context::init_without_workers(
        WORKER_TEST_SN,
        WORKER_TEST_DEVICE_TYPE,
        dirs,
        config,
    )
    .await
    .expect("worker-step context");
    (ctx, db_dir, recorder)
}
