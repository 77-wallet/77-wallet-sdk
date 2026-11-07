pub(crate) mod api_wallet_backend;

use crate::context::api_wallet_backend::{ApiWalletBackend, RealApiWalletBackend};
/// The unlock/session data model lives with the wallet crypto helpers so the key hierarchy
/// stays defined in one place. Context only stores the current active session snapshot.
pub(crate) use crate::domain::api_wallet::unlock::{WalletUnlockMaterial, WalletUnlockSession};
use crate::{
    config::ChainNetwork,
    data::{DeviceInfo, RpcToken},
    dirs::Dirs,
    error::system::SystemError,
    handles::Handles,
    infrastructure::{
        // asset_calc::actor_model::AssetCalcActorManager,
        cache::SharedCache,
        expand_address::event::ExpandEventSender,
        recovery::{pool::BackgroundTaskPool, recovery_coordinator::RecoveryCoordinator},
    },
    messaging::{mqtt::subscribed::Topics, notify::FrontendNotifyEvent},
};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};
use tokio::sync::{Mutex, RwLock};
use wallet_database::{SqliteContext, entities::api_wallet::ApiWalletType};

pub type FrontendNotifySender = Option<tokio::sync::mpsc::UnboundedSender<FrontendNotifyEvent>>;

pub(crate) static CONTEXT: once_cell::sync::Lazy<tokio::sync::OnceCell<Context>> =
    once_cell::sync::Lazy::new(tokio::sync::OnceCell::new);

/// 安全获取上下文，如果上下文未初始化则返回错误
pub fn get_context() -> Result<&'static Context, crate::error::service::ServiceError> {
    CONTEXT.get().ok_or_else(|| crate::error::system::SystemError::ContextNotInit.into())
}

pub(crate) async fn init_context<'a>(
    sn: &str,
    device_type: &str,
    dirs: Dirs,
    frontend_notify: Option<tokio::sync::mpsc::UnboundedSender<FrontendNotifyEvent>>,
    config: crate::config::Config,
) -> Result<&'a Context, crate::error::service::ServiceError> {
    init_context_with_device_protection(
        sn,
        device_type,
        dirs,
        frontend_notify,
        config,
        crate::device_protection::DeviceProtection::default(),
    )
    .await
}

pub(crate) async fn init_context_with_device_protection<'a>(
    sn: &str,
    device_type: &str,
    dirs: Dirs,
    frontend_notify: FrontendNotifySender,
    config: crate::config::Config,
    protection: crate::device_protection::DeviceProtection,
) -> Result<&'a Context, crate::error::service::ServiceError> {
    let context = CONTEXT
        .get_or_try_init::<crate::error::service::ServiceError, _, _>(|| async {
            let mut context = Context::new(sn, device_type, dirs, frontend_notify, config).await?;
            // Assign before publishing Context or starting WalletManager workers.
            context.device_protection = protection.clone();
            Ok(context)
        })
        .await?;
    if !context.device_protection.same_provider(&protection) {
        return Err(crate::error::service::ServiceError::Context(
            "device protection configuration differs from initialized context".to_owned(),
        ));
    }
    Ok(context)
}

#[cfg(any(test, feature = "integration-tests"))]
pub(crate) async fn init_context_with_api_wallet_backend<'a>(
    sn: &str,
    device_type: &str,
    dirs: Dirs,
    frontend_notify: Option<tokio::sync::mpsc::UnboundedSender<FrontendNotifyEvent>>,
    config: crate::config::Config,
    api_wallet_backend: Arc<dyn ApiWalletBackend>,
) -> Result<&'a Context, crate::error::service::ServiceError> {
    let context = CONTEXT
        .get_or_try_init::<crate::error::service::ServiceError, _, _>(|| async {
            let context = Context::new_with_api_wallet_backend(
                sn,
                device_type,
                dirs,
                frontend_notify,
                config,
                api_wallet_backend,
            )
            .await?;
            Ok(context)
        })
        .await?;

    Ok(context)
}

pub struct Context {
    device_protection: crate::device_protection::DeviceProtection,
    sn: String,
    client_id: String,
    dirs: Arc<Dirs>,
    config: crate::config::Config,
    aggregate_api: String,
    chain_network: ChainNetwork,
    backend_api: Arc<wallet_transport_backend::api::BackendApi>,
    api_wallet_backend: Arc<dyn ApiWalletBackend>,
    core_db: Arc<wallet_database::SqliteContext>, // data.db
    api_wallet_db: Arc<wallet_database::SqliteContext>, // api_wallet.db
    api_transaction_db: Arc<wallet_database::SqliteContext>, // api_transaction.db
    api_resource_db: Arc<wallet_database::SqliteContext>, // api_resource.db
    task_db: Arc<wallet_database::SqliteContext>, // task.db
    api_wallet_pool: wallet_database::ApiWalletDbPool,
    log_upload_client: Option<Arc<wallet_transport_backend::log_upload::LogUploadClient>>,
    frontend_notify: Arc<RwLock<FrontendNotifySender>>,
    mqtt_topics: Arc<RwLock<Topics>>,
    rpc_token: Arc<RwLock<RpcToken>>,
    rpc_token_refresh_lock: Mutex<()>,
    device: Arc<DeviceInfo>,
    cache: Arc<SharedCache>,
    wallet_unlock_session: Arc<RwLock<Option<WalletUnlockSession>>>,
    // API creation and global password rotation only. Not a process-wide reset/import gate.
    pub(crate) api_wallet_credential_gate: Mutex<()>,
    pub(crate) api_chain_sync_lock: Mutex<()>,
    pub(crate) ordinary_catalog_sync_lock: Mutex<()>,
    pub(crate) transfer_gates: crate::infrastructure::nonce::transfer_gate::TransferGates,
    pub(crate) ordinary_balance_order: crate::domain::assets::refresh_order::RefreshOrder,
    pub(crate) api_chain_recovery:
        Arc<crate::domain::api_wallet::chain::recovery::RecoveryCoordinator>,
    current_wallet_type: Arc<RwLock<Option<ApiWalletType>>>,
    handles: RwLock<Weak<Handles>>,
    init_api_swap: Mutex<bool>,
    pub(crate) app_warning_config_sync: Mutex<()>,
    pub(crate) app_warning_config_refresh: tokio::sync::Notify,
    pub(crate) address_balance_check:
        Mutex<crate::service::api_wallet::business_notification::AddressBalanceCheckCache>,
    expand_event_tx: Arc<RwLock<Option<ExpandEventSender>>>,
    background_task_pool: Arc<BackgroundTaskPool>,
    recovery_coordinator: Arc<RecoveryCoordinator>,
}

fn active_backend_urls(config: &crate::config::Config) -> (String, String) {
    if cfg!(feature = "prod") {
        (config.backend_api.prod_url.clone(), config.aggregate_api.prod_url.clone())
    } else if cfg!(feature = "dev") {
        (config.backend_api.dev_url.clone(), config.aggregate_api.dev_url.clone())
    } else if cfg!(feature = "test") {
        (config.backend_api.test_url.clone(), config.aggregate_api.test_url.clone())
    } else {
        let offline_url = "http://127.0.0.1:9".to_string();
        (offline_url.clone(), offline_url)
    }
}

impl Context {
    /// Isolated integration context; callers explicitly drive workflow steps.
    #[cfg(any(test, feature = "integration-tests"))]
    pub async fn new_without_handles_for_test(
        dirs: Dirs,
        config: crate::config::Config,
    ) -> Result<Self, crate::error::service::ServiceError> {
        Self::new("collect_receipt_test", "unittest", dirs, None, config).await
    }

    #[cfg(test)]
    pub(crate) fn with_device_protection_for_test(
        mut self,
        protection: crate::device_protection::DeviceProtection,
    ) -> Self {
        self.device_protection = protection;
        self
    }

    async fn new(
        sn: &str,
        device_type: &str,
        dirs: Dirs,
        frontend_notify: FrontendNotifySender,
        config: crate::config::Config,
    ) -> Result<Context, crate::error::service::ServiceError> {
        let db_path = &dirs.db_dir.to_string_lossy();
        tracing::info!("new db_path {:?}", db_path);
        let core_db = SqliteContext::new(db_path, Some("data.db")).await?;
        let api_wallet_db = SqliteContext::new(db_path, Some("api_wallet.db")).await?;
        let api_transaction_db = SqliteContext::new(db_path, Some("api_transaction.db")).await?;
        let api_resource_db = SqliteContext::new(db_path, Some("api_resource.db")).await?;
        let task_db = SqliteContext::new(db_path, Some("task.db")).await?;

        let client_id = crate::domain::app::DeviceDomain::client_device_by_sn(sn, device_type);
        tracing::info!(" ======================================  client id: {}", client_id);

        let chain_network = crate::config::Config::feature_chain_network();
        let (api_url, aggregate_api) = active_backend_urls(&config);

        tracing::info!("api_url: {}, client_id: {}", api_url, client_id);
        tracing::info!(
            "feature_profile: {}, network_source=backend_node, visible_node_networks={:?}, db_dir: {}",
            crate::config::Config::active_feature_profile(),
            crate::config::Config::visible_node_networks(),
            dirs.db_dir.display()
        );
        let mut headers_opt = HashMap::new();
        headers_opt.insert("clientId".to_string(), client_id.clone());
        headers_opt.insert("AW-SEC-ID".to_string(), sn.to_string());
        let aes_cbc_cryptor =
            wallet_utils::cbc::AesCbcCryptor::new(&config.crypto.aes_key, &config.crypto.aes_iv);
        let backend_api = Arc::new(wallet_transport_backend::api::BackendApi::new(
            Some(api_url.clone()),
            Some(headers_opt),
            aes_cbc_cryptor,
        )?);
        let api_wallet_backend = Arc::new(RealApiWalletBackend::new(backend_api.clone()));

        Self::build_context(
            sn,
            dirs,
            frontend_notify,
            config,
            backend_api,
            api_wallet_backend,
            aggregate_api,
            chain_network,
            client_id.clone(),
            core_db,
            api_wallet_db,
            api_transaction_db,
            api_resource_db,
            task_db,
        )
        .await
    }

    async fn new_with_api_wallet_backend(
        sn: &str,
        device_type: &str,
        dirs: Dirs,
        frontend_notify: FrontendNotifySender,
        config: crate::config::Config,
        api_wallet_backend: Arc<dyn ApiWalletBackend>,
    ) -> Result<Context, crate::error::service::ServiceError> {
        let db_path = &dirs.db_dir.to_string_lossy();
        let core_db = SqliteContext::new(db_path, Some("data.db")).await?;
        let api_wallet_db = SqliteContext::new(db_path, Some("api_wallet.db")).await?;
        let api_transaction_db = SqliteContext::new(db_path, Some("api_transaction.db")).await?;
        let api_resource_db = SqliteContext::new(db_path, Some("api_resource.db")).await?;
        let task_db = SqliteContext::new(db_path, Some("task.db")).await?;

        let client_id = crate::domain::app::DeviceDomain::client_device_by_sn(sn, device_type);
        tracing::info!(" ======================================  client id: {}", client_id);

        let chain_network = crate::config::Config::feature_chain_network();
        let (api_url, aggregate_api) = active_backend_urls(&config);

        tracing::info!("api_url: {}, client_id: {}", api_url, client_id);
        tracing::info!(
            "feature_profile: {}, network_source=backend_node, visible_node_networks={:?}, db_dir: {}",
            crate::config::Config::active_feature_profile(),
            crate::config::Config::visible_node_networks(),
            dirs.db_dir.display()
        );
        let mut headers_opt = HashMap::new();
        headers_opt.insert("clientId".to_string(), client_id.clone());
        headers_opt.insert("AW-SEC-ID".to_string(), sn.to_string());
        let aes_cbc_cryptor =
            wallet_utils::cbc::AesCbcCryptor::new(&config.crypto.aes_key, &config.crypto.aes_iv);
        let backend_api = Arc::new(wallet_transport_backend::api::BackendApi::new(
            Some(api_url.clone()),
            Some(headers_opt),
            aes_cbc_cryptor,
        )?);

        Self::build_context(
            sn,
            dirs,
            frontend_notify,
            config,
            backend_api,
            api_wallet_backend,
            aggregate_api,
            chain_network,
            client_id.clone(),
            core_db,
            api_wallet_db,
            api_transaction_db,
            api_resource_db,
            task_db,
        )
        .await
    }

    async fn build_context(
        sn: &str,
        dirs: Dirs,
        frontend_notify: FrontendNotifySender,
        config: crate::config::Config,
        backend_api: Arc<wallet_transport_backend::api::BackendApi>,
        api_wallet_backend: Arc<dyn ApiWalletBackend>,
        aggregate_api: String,
        chain_network: ChainNetwork,
        client_id: String,
        core_db: SqliteContext,
        api_wallet_db: SqliteContext,
        api_transaction_db: SqliteContext,
        api_resource_db: SqliteContext,
        task_db: SqliteContext,
    ) -> Result<Context, crate::error::service::ServiceError> {
        let log_upload_client = config
            .log_upload
            .clone()
            .map(wallet_transport_backend::log_upload::LogUploadClient::new)
            .transpose()
            .map_err(|e| crate::error::service::ServiceError::Parameter(e.to_string()))?
            .map(Arc::new);
        let frontend_notify = Arc::new(RwLock::new(frontend_notify));

        {
            let mut app_state = crate::app_state::APP_STATE.write().await;
            app_state.set_backend_url(Some(backend_api.base_url.clone()));
        }

        let defaults = crate::config::runtime_defaults::recovery();
        let background_task_pool =
            Arc::new(BackgroundTaskPool::new(defaults.background_task_pool_max_concurrent));

        // Keep the split connections and writer gate stable across accessor calls.
        let api_wallet_pool = api_wallet_db.clone().into_api_wallet_db_pool()?;
        let context = Context {
            device_protection: crate::device_protection::DeviceProtection::default(),
            sn: sn.to_string(),
            client_id: client_id.clone(),
            dirs: Arc::new(dirs),
            config,
            backend_api,
            api_wallet_backend,
            aggregate_api,
            chain_network,
            core_db: Arc::new(core_db),
            api_wallet_db: Arc::new(api_wallet_db),
            api_wallet_pool,
            api_transaction_db: Arc::new(api_transaction_db),
            api_resource_db: Arc::new(api_resource_db),
            task_db: Arc::new(task_db),
            frontend_notify,
            log_upload_client,
            mqtt_topics: Arc::new(RwLock::new(Topics::new())),
            rpc_token: Arc::new(RwLock::new(RpcToken::default())),
            rpc_token_refresh_lock: Mutex::new(()),
            device: Arc::new(DeviceInfo::new(sn, &client_id)),
            cache: Arc::new(SharedCache::new()),
            wallet_unlock_session: Arc::new(RwLock::new(None)),
            api_wallet_credential_gate: Mutex::new(()),
            api_chain_sync_lock: Mutex::new(()),
            ordinary_catalog_sync_lock: Mutex::new(()),
            transfer_gates: Default::default(),
            ordinary_balance_order: Default::default(),
            api_chain_recovery: Arc::new(Default::default()),
            current_wallet_type: Arc::new(RwLock::new(None)),
            handles: RwLock::new(Weak::new()),
            init_api_swap: Mutex::new(false),
            app_warning_config_sync: Mutex::new(()),
            app_warning_config_refresh: tokio::sync::Notify::new(),
            address_balance_check: Mutex::new(Default::default()),
            expand_event_tx: Arc::new(RwLock::new(None)),
            background_task_pool,
            recovery_coordinator: Arc::new(RecoveryCoordinator::default()),
        };
        crate::messaging::notify::writer_diagnostics::start(
            &context.api_wallet_pool,
            crate::messaging::notify::diagnostic::DiagnosticEmitter::from_context(&context),
        );
        Ok(context)
    }

    /// Immutable provider selected during SDK initialization.
    pub fn device_protection(&self) -> &crate::device_protection::DeviceProtection {
        &self.device_protection
    }

    pub fn get_sn(&self) -> &str {
        &self.sn
    }

    pub fn set_recovery_paused(&self, paused: bool) {
        self.recovery_coordinator.set_globally_paused(paused);
    }

    pub fn is_recovery_paused(&self) -> bool {
        self.recovery_coordinator.is_globally_paused()
    }

    pub(crate) fn recovery_coordinator(&self) -> &RecoveryCoordinator {
        &self.recovery_coordinator
    }

    pub fn get_client_id(&self) -> &str {
        &self.client_id
    }

    pub async fn set_frontend_notify_sender(
        &self,
        frontend_notify: FrontendNotifySender,
    ) -> Result<(), crate::error::service::ServiceError> {
        let mut lock = self.frontend_notify.write().await;
        *lock = frontend_notify;
        Ok(())
    }

    pub async fn set_current_wallet_type(
        &self,
        wallet_type: ApiWalletType,
    ) -> Result<(), crate::error::service::ServiceError> {
        let mut lock = self.current_wallet_type.write().await;
        *lock = Some(wallet_type);
        Ok(())
    }

    pub async fn get_current_wallet_type(
        &self,
    ) -> Result<ApiWalletType, crate::error::service::ServiceError> {
        let lock = self.current_wallet_type.read().await;
        (*lock).ok_or_else(|| {
            crate::error::business::BusinessError::Wallet(
                crate::error::business::wallet::WalletError::WalletTypeNotSet,
            )
            .into()
        })
    }

    pub(crate) async fn set_wallet_unlock_session(
        &self,
        unlock_session: WalletUnlockSession,
    ) -> Result<(), crate::error::service::ServiceError> {
        let mut lock = self.wallet_unlock_session.write().await;
        *lock = Some(unlock_session);
        Ok(())
    }

    pub(crate) async fn clear_wallet_unlock_session(
        &self,
    ) -> Result<(), crate::error::service::ServiceError> {
        let mut lock = self.wallet_unlock_session.write().await;
        *lock = None;
        Ok(())
    }

    pub(crate) async fn wallet_unlock_session_snapshot(&self) -> Option<WalletUnlockSession> {
        let lock = self.wallet_unlock_session.read().await;
        lock.clone()
    }

    pub(crate) fn get_global_device(&self) -> Arc<DeviceInfo> {
        self.device.clone()
    }

    pub(crate) fn config(&self) -> &crate::config::Config {
        &self.config
    }

    pub(crate) fn get_global_sqlite_pool(
        &self,
    ) -> Result<std::sync::Arc<sqlx::SqlitePool>, crate::error::service::ServiceError> {
        let pool = self.core_db.get_pool()?;
        Ok(pool)
    }

    pub(crate) fn core_db(&self) -> &SqliteContext {
        &self.core_db
    }

    pub(crate) fn api_transaction_db(&self) -> &SqliteContext {
        &self.api_transaction_db
    }

    pub(crate) fn api_resource_db(&self) -> &SqliteContext {
        &self.api_resource_db
    }

    pub(crate) fn api_funds_db(&self) -> &SqliteContext {
        self.api_transaction_db()
    }

    pub(crate) fn api_wallet_db(&self) -> &SqliteContext {
        &self.api_wallet_db
    }

    pub(crate) fn core_pool(
        &self,
    ) -> Result<wallet_database::CoreDbPool, crate::error::service::ServiceError> {
        let pool = self.core_db.get_pool()?;
        Ok(wallet_database::CoreDbPool::new(pool))
    }

    pub(crate) fn api_wallet_pool(
        &self,
    ) -> Result<wallet_database::ApiWalletDbPool, crate::error::service::ServiceError> {
        Ok(self.api_wallet_pool.clone())
    }

    pub(crate) fn api_transaction_pool(
        &self,
    ) -> Result<wallet_database::ApiTransactionDbPool, crate::error::service::ServiceError> {
        let pool = self.api_transaction_db.get_pool()?;
        Ok(wallet_database::ApiTransactionDbPool::new(pool))
    }

    pub(crate) fn api_resource_pool(
        &self,
    ) -> Result<wallet_database::ApiResourceDbPool, crate::error::service::ServiceError> {
        let pool = self.api_resource_db.get_pool()?;
        Ok(wallet_database::ApiResourceDbPool::new(pool))
    }

    pub(crate) fn task_db(&self) -> &SqliteContext {
        &self.task_db
    }

    pub(crate) fn task_pool(
        &self,
    ) -> Result<wallet_database::TaskDbPool, crate::error::service::ServiceError> {
        let pool = self.task_db.get_pool()?;
        Ok(wallet_database::TaskDbPool::new(pool))
    }

    pub(crate) fn get_global_backend_api(&self) -> Arc<wallet_transport_backend::api::BackendApi> {
        self.backend_api.clone()
    }

    pub(crate) fn get_api_wallet_backend(&self) -> Arc<dyn ApiWalletBackend> {
        self.api_wallet_backend.clone()
    }

    pub(crate) fn chain_network(&self) -> ChainNetwork {
        self.chain_network
    }

    pub(crate) fn chain_network_kind(&self) -> wallet_types::chain::network::NetworkKind {
        self.chain_network.to_network_kind()
    }

    pub fn get_global_dirs(&self) -> Arc<crate::dirs::Dirs> {
        self.dirs.clone()
    }

    pub(crate) fn get_aggregate_api(&self) -> String {
        self.aggregate_api.clone()
    }

    pub(crate) fn get_log_upload_client(
        &self,
    ) -> Option<Arc<wallet_transport_backend::log_upload::LogUploadClient>> {
        self.log_upload_client.clone()
    }

    pub(crate) fn get_global_cache(&self) -> Arc<SharedCache> {
        self.cache.clone()
    }

    pub(crate) fn get_global_mqtt_topics(&self) -> std::sync::Arc<RwLock<Topics>> {
        self.mqtt_topics.clone()
    }

    pub(crate) fn get_global_frontend_notify_sender(
        &self,
    ) -> std::sync::Arc<RwLock<FrontendNotifySender>> {
        // tracing::info!("context: {:#?}", self);
        // tracing::info!("frontend_notify: {:#?}", self.frontend_notify);
        self.frontend_notify.clone()
    }

    pub(crate) async fn get_rpc_header(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, crate::error::service::ServiceError>
    {
        self.get_rpc_header_with_mode(false).await
    }

    pub(crate) async fn get_rpc_header_force_refresh(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, crate::error::service::ServiceError>
    {
        self.get_rpc_header_with_mode(true).await
    }

    pub(crate) async fn invalidate_rpc_token_cache(&self) {
        let mut token_guard = self.rpc_token.write().await;
        token_guard.token.clear();
        token_guard.instance = tokio::time::Instant::now() - tokio::time::Duration::from_secs(1);
        tracing::warn!(client_id = %self.device.client_id, "rpc token cache invalidated");
    }

    fn rpc_header_with_token(token: String) -> HashMap<String, String> {
        HashMap::from([("token".to_string(), token)])
    }

    async fn get_rpc_header_with_mode(
        &self,
        force_refresh: bool,
    ) -> Result<std::collections::HashMap<String, String>, crate::error::service::ServiceError>
    {
        let token_expired = {
            let token_guard = self.rpc_token.read().await;
            token_guard.token.is_empty() || token_guard.instance < tokio::time::Instant::now()
        };

        if force_refresh || token_expired {
            let _refresh_guard = self.rpc_token_refresh_lock.lock().await;

            if !force_refresh {
                let token_guard = self.rpc_token.read().await;
                let token_still_valid = !token_guard.token.is_empty()
                    && token_guard.instance >= tokio::time::Instant::now();
                if token_still_valid {
                    return Ok(Self::rpc_header_with_token(token_guard.token.clone()));
                }
            }

            let backend_api = self.backend_api.clone();
            tracing::info!(
                client_id = %self.device.client_id,
                force_refresh = force_refresh,
                "rpc token refresh start"
            );
            let new_token_response = backend_api.rpc_token(&self.device.client_id).await;
            match new_token_response {
                Ok(token) => {
                    let new_token = RpcToken {
                        token: token.clone(),
                        instance: tokio::time::Instant::now()
                            + tokio::time::Duration::from_secs(30 * 60),
                    };
                    {
                        let mut token_guard = self.rpc_token.write().await;
                        *token_guard = new_token.clone();
                    }
                    tracing::info!(
                        client_id = %self.device.client_id,
                        force_refresh = force_refresh,
                        "rpc token refresh success"
                    );
                    Ok(Self::rpc_header_with_token(new_token.token))
                }
                Err(e) => {
                    tracing::warn!(
                        client_id = %self.device.client_id,
                        force_refresh = force_refresh,
                        error = %e,
                        "rpc token refresh failed"
                    );
                    if force_refresh {
                        return Err(e.into());
                    }

                    // 服务端报错,如果token有值那么使用原来的值，服务端token的过期时间会大于我本地的。
                    let token_guard = self.rpc_token.read().await;
                    let token = token_guard.token.clone();
                    if !token.is_empty() {
                        Ok(Self::rpc_header_with_token(token))
                    } else {
                        tracing::error!("get_rpc_header failed to get token, error: {:?}", e);
                        Ok(HashMap::new())
                        // Err(crate::error::business::BusinessError::Chain(
                        //     crate::error::business::chain::ChainError::NodeToken(e.to_string()),
                        // ))?
                    }
                }
            }
        } else {
            // 未过期使用缓存里面的token
            let token_guard = self.rpc_token.read().await;
            let token = token_guard.token.clone();
            Ok(Self::rpc_header_with_token(token))
        }
    }

    // 保持原有方法签名以兼容现有代码
    pub(crate) async fn get_global_handles(&self) -> Weak<Handles> {
        self.handles.read().await.clone()
    }

    // 新增方法，返回Result<Arc<Handles>>
    pub(crate) async fn get_handles_arc(
        &self,
    ) -> Result<Arc<Handles>, crate::error::service::ServiceError> {
        self.handles.read().await.upgrade().ok_or_else(|| {
            crate::error::service::ServiceError::System(SystemError::Internal(
                "Handles not initialized or already dropped".to_string(),
            ))
        })
    }

    // pub(crate) async fn get_global_asset_calc_actor_manager(
    //     &self,
    // ) -> Result<Arc<AssetCalcActorManager>, crate::error::service::ServiceError> {
    //     let handles = self.get_handles_arc().await?;
    //     Ok(handles.get_global_asset_calc_actor_manager())
    // }

    pub(crate) async fn set_global_handles(&self, handles: Weak<Handles>) {
        let mut lock = self.handles.write().await;
        *lock = handles;
    }

    pub(crate) async fn is_init_api_swap(&self) -> bool {
        let r = self.init_api_swap.lock().await;
        *r
    }

    pub(crate) async fn set_init_api_swap(&self, swap: bool) {
        let mut r = self.init_api_swap.lock().await;
        *r = swap;
    }

    pub(crate) async fn set_expand_event_tx(&self, tx: Option<ExpandEventSender>) {
        let mut lock = self.expand_event_tx.write().await;
        *lock = tx;
    }

    pub(crate) async fn get_expand_event_tx(&self) -> Option<ExpandEventSender> {
        let lock = self.expand_event_tx.read().await;
        lock.clone()
    }

    /// 获取全局后台任务池
    pub(crate) fn get_global_background_task_pool(&self) -> Arc<BackgroundTaskPool> {
        self.background_task_pool.clone()
    }
}
