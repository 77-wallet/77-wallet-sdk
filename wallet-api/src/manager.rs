use crate::domain::task_queue::{BackendApiTask, InitializationTask, Task};
use crate::notify::FrontendNotifyEvent;
use crate::service::coin::CoinService;
use crate::service::node::NodeService;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::RwLock;
use wallet_database::entities::task_queue::TaskQueueEntity;
use wallet_database::factory::RepositoryFactory;
use wallet_database::SqliteContext;

pub const ACCESS_KEY_ID: &str = "access_key_id";
pub const ACCESS_KEY_SECRET: &str = "access_key_secret";
pub const BUCKET_NAME: &str = "bucket_name";
pub const ENDPOINT: &str = "oss_endpoint";

pub(crate) static INIT_DATA: once_cell::sync::Lazy<tokio::sync::OnceCell<()>> =
    once_cell::sync::Lazy::new(tokio::sync::OnceCell::new);

async fn do_some_init<'a>() -> Result<&'a (), crate::ServiceError> {
    INIT_DATA
        .get_or_try_init(async || init_some_data().await)
        .await
}

pub async fn init_some_data() -> Result<(), crate::ServiceError> {
    let pool = Context::get_global_sqlite_pool()?;

    let repo = RepositoryFactory::repo(pool.clone());
    let mut node_service = NodeService::new(repo);
    node_service.init_node_info().await?;

    crate::domain::app::config::ConfigDomain::init_url().await?;
    let repo = RepositoryFactory::repo(pool.clone());
    let mut coin_service = CoinService::new(repo);

    let list: Vec<wallet_transport_backend::CoinInfo> =
        crate::default_data::coins::init_default_coins_list()?
            .iter()
            .map(|coin| coin.to_owned().into())
            .collect();
    coin_service.upsert_hot_coin_list(list, 1, 1).await?;

    let token_query_rates_req = crate::domain::task_queue::BackendApiTaskData::new(
        wallet_transport_backend::consts::endpoint::TOKEN_QUERY_RATES,
        &(),
    )?;

    let set_official_website_req = crate::domain::task_queue::BackendApiTaskData::new(
        wallet_transport_backend::consts::endpoint::SYS_CONFIG_FIND_CONFIG_BY_KEY,
        &wallet_transport_backend::request::FindConfigByKey::new("OFFICIAL:WEBSITE"),
    )?;

    let set_app_install_download_req = crate::domain::task_queue::BackendApiTaskData::new(
        wallet_transport_backend::consts::endpoint::APP_INSTALL_DOWNLOAD,
        &(),
    )?;

    let mqtt_init_req = crate::domain::task_queue::BackendApiTaskData::new(
        wallet_transport_backend::consts::endpoint::MQTT_INIT,
        &(),
    )?;

    crate::domain::task_queue::Tasks::new()
        .push(Task::Initialization(InitializationTask::PullAnnouncement))
        .push(Task::Initialization(InitializationTask::PullHotCoins))
        .push(Task::Initialization(
            InitializationTask::ProcessUnconfirmMsg,
        ))
        .push(Task::Initialization(InitializationTask::SetBlockBrowserUrl))
        .push(Task::Initialization(InitializationTask::SetFiat))
        .push(Task::Initialization(InitializationTask::RecoverQueueData))
        .push(Task::BackendApi(BackendApiTask::BackendApi(
            token_query_rates_req,
        )))
        .push(Task::BackendApi(BackendApiTask::BackendApi(
            set_official_website_req,
        )))
        .push(Task::BackendApi(BackendApiTask::BackendApi(
            set_app_install_download_req,
        )))
        .push(Task::BackendApi(BackendApiTask::BackendApi(mqtt_init_req)))
        .send()
        .await?;

    Ok(())
}

pub(crate) static CONTEXT: once_cell::sync::Lazy<tokio::sync::OnceCell<Context>> =
    once_cell::sync::Lazy::new(tokio::sync::OnceCell::new);

pub(crate) async fn init_context<'a>(
    sn: &str,
    device_type: &str,
    dirs: Dirs,
    mqtt_url: &str,
    frontend_notify: Option<tokio::sync::mpsc::UnboundedSender<FrontendNotifyEvent>>,
) -> Result<&'a Context, crate::ServiceError> {
    let context = CONTEXT
        .get_or_try_init::<crate::ServiceError, _, _>(async || {
            let context = Context::new(sn, device_type, dirs, mqtt_url, frontend_notify).await?;
            Ok(context)
        })
        .await?;
    Ok(context)
}

#[derive(Debug, Clone)]
pub struct Context {
    pub(crate) dirs: Dirs,
    pub(crate) mqtt_url: String,
    pub(crate) backend_api: wallet_transport_backend::api::BackendApi,
    pub(crate) sqlite_context: wallet_database::SqliteContext,
    pub(crate) oss_client: wallet_oss::oss_client::OssClient,
    pub(crate) frontend_notify: Arc<RwLock<FrontendNotifySender>>,
    pub(crate) task_manager: crate::domain::task_queue::task_manager::TaskManager,
    pub(crate) mqtt_topics: Arc<RwLock<crate::mqtt::topic::Topics>>,
    pub(crate) rpc_token: Arc<RwLock<RpcToken>>,
    pub(crate) device: Arc<DeviceInfo>,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub(crate) sn: String,
    pub(crate) client_id: String,
}
impl DeviceInfo {
    pub fn new(sn: &str, client_id: &str) -> Self {
        Self {
            sn: sn.to_owned(),
            client_id: client_id.to_owned(),
        }
    }
}

pub type FrontendNotifySender = Option<tokio::sync::mpsc::UnboundedSender<FrontendNotifyEvent>>;
pub type TaskSender = tokio::sync::mpsc::UnboundedSender<Vec<TaskQueueEntity>>;

impl Context {
    async fn new(
        sn: &str,
        device_type: &str,
        dirs: Dirs,
        mqtt_url: &str,
        frontend_notify: FrontendNotifySender,
    ) -> Result<Context, crate::ServiceError> {
        let sqlite_context = SqliteContext::new(&dirs.db_dir.to_string_lossy()).await?;

        let client_id = crate::domain::app::DeviceDomain::client_device_by_sn(sn, device_type);
        let header_opt = Some(HashMap::from([("clientId".to_string(), client_id.clone())]));
        let backend_api = wallet_transport_backend::api::BackendApi::new(None, header_opt)?;

        let frontend_notify = Arc::new(RwLock::new(frontend_notify));

        {
            let mut app_state = crate::app_state::APP_STATE.write().await;
            app_state.set_backend_url(Some(backend_api.base_url.clone()));
            app_state.set_mqtt_url(Some(mqtt_url.to_string()));
        }

        let oss_client = wallet_oss::oss_client::OssClient::new(
            ACCESS_KEY_ID,
            ACCESS_KEY_SECRET,
            ENDPOINT,
            BUCKET_NAME,
        );

        // 创建 TaskManager 实例
        let task_manager = crate::domain::task_queue::task_manager::TaskManager::new();

        Ok(Context {
            dirs,
            mqtt_url: mqtt_url.to_string(),
            backend_api,
            sqlite_context,
            frontend_notify,
            oss_client,
            task_manager,
            mqtt_topics: Arc::new(RwLock::new(crate::mqtt::topic::Topics::new())),
            rpc_token: Arc::new(RwLock::new(RpcToken::default())),
            device: Arc::new(DeviceInfo::new(sn, &client_id)),
        })
    }

    pub async fn set_frontend_notify_sender(
        frontend_notify: FrontendNotifySender,
    ) -> Result<(), crate::ServiceError> {
        let cx = Context::get_context()?;
        let mut lock = cx.frontend_notify.write().await;
        *lock = frontend_notify;
        Ok(())
    }

    pub(crate) fn get_context() -> Result<&'static Context, crate::SystemError> {
        CONTEXT.get().ok_or(crate::SystemError::ContextNotInit)
    }

    pub fn get_global_sqlite_pool() -> Result<std::sync::Arc<sqlx::SqlitePool>, crate::ServiceError>
    {
        Ok(Context::get_context()?.sqlite_context.get_pool()?.clone())
    }

    pub(crate) fn get_global_backend_api(
    ) -> Result<&'static wallet_transport_backend::api::BackendApi, crate::ServiceError> {
        Ok(&Context::get_context()?.backend_api)
    }

    pub(crate) fn get_global_dirs() -> Result<&'static crate::manager::Dirs, crate::SystemError> {
        Ok(&Context::get_context()?.dirs)
    }

    pub(crate) fn get_global_mqtt_url() -> Result<&'static String, crate::SystemError> {
        Ok(&Context::get_context()?.mqtt_url)
    }

    pub(crate) fn get_global_oss_client(
    ) -> Result<&'static wallet_oss::oss_client::OssClient, crate::SystemError> {
        Ok(&Context::get_context()?.oss_client)
    }

    pub(crate) fn get_global_task_manager(
    ) -> Result<&'static crate::domain::task_queue::task_manager::TaskManager, crate::SystemError>
    {
        Ok(&Context::get_context()?.task_manager)
    }

    pub(crate) fn get_global_mqtt_topics(
    ) -> Result<&'static std::sync::Arc<RwLock<crate::mqtt::topic::Topics>>, crate::SystemError>
    {
        Ok(&Context::get_context()?.mqtt_topics)
    }

    pub(crate) fn get_global_frontend_notify_sender() -> Result<
        &'static std::sync::Arc<RwLock<crate::manager::FrontendNotifySender>>,
        crate::SystemError,
    > {
        Ok(&Context::get_context()?.frontend_notify)
    }

    pub(crate) async fn get_rpc_header(
    ) -> Result<std::collections::HashMap<String, String>, crate::ServiceError> {
        let cx = Context::get_context()?;

        let token_expired = {
            let token_guard = cx.rpc_token.read().await;
            token_guard.instance < tokio::time::Instant::now()
        };

        if token_expired {
            let backend_api = cx.backend_api.clone();

            let new_token_response =
                backend_api
                    .rpc_token(&cx.device.client_id)
                    .await
                    .map_err(|e| {
                        crate::BusinessError::Chain(crate::ChainError::NodeToken(e.to_string()))
                    })?;

            let new_token = RpcToken {
                token: new_token_response,
                instance: tokio::time::Instant::now() + tokio::time::Duration::from_secs(8 * 60),
            };

            {
                let mut token_guard = cx.rpc_token.write().await;
                *token_guard = new_token.clone();
            }

            Ok(HashMap::from([("token".to_string(), new_token.token)]))
        } else {
            let token_guard = cx.rpc_token.read().await;
            let token = token_guard.token.clone();

            Ok(HashMap::from([("token".to_string(), token)]))
        }
    }
}

#[derive(Debug, Clone)]
pub struct RpcToken {
    pub token: String,
    pub instance: tokio::time::Instant,
}

impl Default for RpcToken {
    fn default() -> Self {
        Self {
            token: String::new(),
            instance: tokio::time::Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WalletManager {
    pub repo_factory: wallet_database::factory::RepositoryFactory,
}

impl WalletManager {
    pub async fn new(
        sn: &str,
        device_type: &str,
        root_dir: &str,
        sender: Option<UnboundedSender<FrontendNotifyEvent>>,
    ) -> Result<WalletManager, crate::ServiceError> {
        let dir = Dirs::new(root_dir)?;

        let mqtt_url = wallet_transport_backend::consts::MQTT_URL.to_string();
        let context = init_context(sn, device_type, dir, &mqtt_url, sender).await?;

        let pool = context.sqlite_context.get_pool().unwrap();
        let repo_factory = wallet_database::factory::RepositoryFactory::new(pool);

        let manager = WalletManager { repo_factory };

        Ok(manager)
    }

    pub async fn init_data(&self) -> Result<(), crate::ServiceError> {
        // 启动任务检查循环
        let manager = Context::get_global_task_manager()?;
        manager.start_task_check_loop();

        crate::domain::log::periodic_log_report(std::time::Duration::from_secs(60 * 60)).await;

        tokio::spawn(async move {
            do_some_init().await.unwrap();
        });

        Ok(())
    }

    pub async fn init_log(level: Option<&str>) -> Result<(), crate::ServiceError> {
        let context = Context::get_context()?;

        let log_dir = context.dirs.get_log_dir();
        let sn = &context.device.sn;

        wallet_utils::log::set_sn_code(&sn);

        Ok(wallet_utils::log::file::init_log(
            log_dir.to_string_lossy().as_ref(),
            level,
        )?)
    }

    pub async fn set_frontend_notify_sender(
        &self,
        sender: UnboundedSender<FrontendNotifyEvent>,
    ) -> Result<(), crate::ServiceError> {
        crate::manager::Context::set_frontend_notify_sender(Some(sender)).await
    }
}

#[derive(Debug, Clone, Default)]
pub struct Dirs {
    pub root_dir: PathBuf,
    pub wallet_dir: PathBuf,
    pub export_dir: PathBuf,
    pub db_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl Dirs {
    pub fn join_path(root_dir: &str, sub_path: &str) -> PathBuf {
        PathBuf::from(root_dir).join(sub_path)
    }

    pub fn new(root_dir: &str) -> Result<Dirs, crate::ServiceError> {
        let wallet_dir = Self::join_path(root_dir, "wallet_data");

        let db_dir = Self::join_path(root_dir, "db");
        let export_dir = Self::join_path(root_dir, "export");
        let log_dir = Self::join_path(root_dir, "log");

        for dir in [&db_dir, &export_dir, &log_dir] {
            wallet_utils::file_func::create_dir_all(dir)?;
        }

        Ok(Dirs {
            root_dir: PathBuf::from(root_dir),
            wallet_dir,
            export_dir,
            db_dir,
            log_dir,
        })
    }

    pub fn get_wallet_dir(&self, address: Option<&str>) -> std::path::PathBuf {
        address.map_or_else(
            || PathBuf::from(&self.wallet_dir),
            |addr| PathBuf::from(&self.wallet_dir).join(addr),
        )
    }

    pub fn get_export_dir(&self) -> std::path::PathBuf {
        self.export_dir.clone()
    }

    pub fn get_log_dir(&self) -> std::path::PathBuf {
        self.log_dir.clone()
    }

    pub(crate) fn get_root_dir(
        &self,
        wallet_address: &str,
    ) -> Result<std::path::PathBuf, crate::ServiceError> {
        let root_dir = self.wallet_dir.join(wallet_address).join("root");

        wallet_utils::file_func::create_dir_all(&root_dir)?;

        Ok(root_dir)
    }

    pub(crate) fn get_subs_dir(
        &self,
        wallet_address: &str,
    ) -> Result<std::path::PathBuf, crate::ServiceError> {
        let subs_dir = self.wallet_dir.join(wallet_address).join("subs");

        wallet_utils::file_func::create_dir_all(&subs_dir)?;

        Ok(subs_dir)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_traverse_directory_structure() -> Result<(), anyhow::Error> {
        // 创建临时目录结构
        let temp_dir = tempdir()?;
        let root_dir = temp_dir.path();

        // 创建模拟钱包和账户目录结构
        let wallet_a_path = root_dir.join("钱包A");
        let wallet_a_root_path = wallet_a_path.join("root");
        let wallet_a_subs_path = wallet_a_path.join("subs");

        let wallet_b_path = root_dir.join("钱包B");
        let wallet_b_root_path = wallet_b_path.join("root");
        let wallet_b_subs_path = wallet_b_path.join("subs");

        fs::create_dir_all(&wallet_a_root_path)?;
        fs::create_dir_all(&wallet_a_subs_path)?;
        fs::create_dir_all(&wallet_b_root_path)?;
        fs::create_dir_all(&wallet_b_subs_path)?;

        // 创建钱包根密钥文件和种子文件
        let wallet_a_root_pk_file =
            wallet_a_root_path.join("0x296a3C6B001e163409D7df318799bD52B5e3b67d-pk");
        let wallet_a_root_seed_file =
            wallet_a_root_path.join("0x296a3C6B001e163409D7df318799bD52B5e3b67d-seed");
        let wallet_b_root_pk_file =
            wallet_b_root_path.join("0x21A640a53530Aee3feEc2487a01070971d66320f-pk");
        let wallet_b_root_seed_file =
            wallet_b_root_path.join("0x21A640a53530Aee3feEc2487a01070971d66320f-seed");

        File::create(&wallet_a_root_pk_file)?.write_all(b"walletA root pk")?;
        File::create(&wallet_a_root_seed_file)?.write_all(b"walletA root seed")?;
        File::create(&wallet_b_root_pk_file)?.write_all(b"walletB root pk")?;
        File::create(&wallet_b_root_seed_file)?.write_all(b"walletB root seed")?;

        // 创建派生密钥文件
        let wallet_a_sub_key_0 = wallet_a_subs_path.join("address1-m_44'_60'_0'_0_0-pk");
        let wallet_a_sub_key_1 = wallet_a_subs_path.join("address2-m_44'_60'_0'_0_1-pk");
        let wallet_a_sub_key_2 = wallet_a_subs_path.join("address3-m_44'_60'_1'_0_0-pk");

        File::create(&wallet_a_sub_key_0)?.write_all(b"walletA sub key 0")?;
        File::create(&wallet_a_sub_key_1)?.write_all(b"walletA sub key 1")?;
        File::create(&wallet_a_sub_key_2)?.write_all(b"walletA sub key 2")?;

        let dir = &root_dir.to_string_lossy().to_string();

        let _manager = crate::WalletManager::new("sn", "ANDROID", dir, None)
            .await
            .unwrap();
        let dirs = crate::manager::Context::get_global_dirs()?;

        wallet_tree::wallet_tree::WalletTree::traverse_directory_structure(&dirs.wallet_dir)?;

        Ok(())
    }
}
