#[cfg(any(test, feature = "integration-tests"))]
use crate::{ApiWalletBackend, context::init_context_with_api_wallet_backend};
use crate::{
    api::ReturnType,
    context::{Context, init_context_with_device_protection},
    data::init_some_data,
    dirs::Dirs,
    domain::{self},
    handles::Handles,
    infrastructure::{
        recovery::{
            address_query_recovery::start_address_recover_worker,
            api_asset_sync_recovery::start_api_asset_sync_worker,
            asset_query_recovery::start_asset_query_worker,
            withdraw_resource_recovery::start_withdraw_resource_recover_worker,
        },
        unlock_session,
    },
    messaging::notify::FrontendNotifyEvent,
    service::{
        api_wallet::wallet::ApiWalletService, console_diagnostics::ConsoleDiagnosticsService,
        device::DeviceService, task_queue::TaskQueueService,
    },
};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use wallet_ecdh::GLOBAL_KEY;

#[derive(Clone)]
pub struct WalletManager {
    pub(crate) ctx: &'static Context,
    pub(crate) handles: Option<Arc<Handles>>,
}

impl WalletManager {
    pub async fn new(
        sn: &str,
        device_type: &str,
        sender: Option<UnboundedSender<FrontendNotifyEvent>>,
        config: crate::config::Config,
        dir: Dirs,
    ) -> Result<WalletManager, crate::error::service::ServiceError> {
        Self::new_with_device_protection(
            sn,
            device_type,
            sender,
            config,
            dir,
            crate::device_protection::DeviceProtection::default(),
        )
        .await
    }

    /// Platform plugins inject hardware callbacks before any wallet workers start.
    /// Keep a clone of the same provider when retrying initialization; replacement
    /// after Context initialization is rejected, including switching to legacy mode.
    pub async fn new_with_device_protection(
        sn: &str,
        device_type: &str,
        sender: Option<UnboundedSender<FrontendNotifyEvent>>,
        config: crate::config::Config,
        dir: Dirs,
        protection: crate::device_protection::DeviceProtection,
    ) -> Result<WalletManager, crate::error::service::ServiceError> {
        #[cfg(any(test, feature = "integration-tests"))]
        crate::infrastructure::task_queue::set_task_execution_mode_for_test(
            crate::infrastructure::task_queue::TaskExecutionMode::Normal,
        );

        tracing::info!(
            "wallet manager startup with feature_profile={}, visible_node_networks={:?}, network_source=backend_node, db_dir={}",
            crate::config::Config::active_feature_profile(),
            crate::config::Config::visible_node_networks(),
            dir.db_dir.display()
        );

        let context =
            init_context_with_device_protection(sn, device_type, dir, sender, config, protection)
                .await?;
        crate::application::wallet::WalletApplication::prepare_device_protection_startup(context)
            .await?;
        GLOBAL_KEY.set_sn(sn);
        unlock_session::start_wallet_unlock_session_rotation_task(context).await?;

        // 执行TaskQueue迁移
        tracing::info!("Running TaskQueue migration");
        crate::domain::task_queue::TaskQueueDomain::migrate_task_queue_to_db_with_ctx(context)
            .await?;
        tracing::info!("TaskQueue migration completed");

        let handles = Arc::new(Handles::new(context.get_client_id(), context).await?);
        context.set_global_handles(Arc::downgrade(&handles)).await;

        context.api_chain_recovery.start(context);

        tracing::info!("start_task_check start");
        handles.get_global_task_manager().start_task_check().await?;

        // // 初始化Actor模型的地址扩容管理器
        // tracing::info!("Initialize address expansion manager (Actor model) start");
        // infrastructure::expand_address::init().await?;
        // tracing::info!("Initialize address expansion manager (Actor model) completed");

        // 启动地址恢复Worker
        tracing::info!("启动地址恢复Worker");
        let background_task_pool = context.get_global_background_task_pool();
        start_address_recover_worker(context, background_task_pool).await?;

        // 启动资产查询恢复Worker
        tracing::info!("启动资产查询恢复Worker");
        let background_task_pool = context.get_global_background_task_pool();
        start_asset_query_worker(context, background_task_pool).await?;

        // 启动 API 钱包资产同步恢复Worker
        tracing::info!("启动 API 钱包资产同步恢复Worker");
        let background_task_pool = context.get_global_background_task_pool();
        start_api_asset_sync_worker(context, background_task_pool).await?;

        // 启动一次性 TRON 出款资源补全 Worker，不阻塞本体启动。
        start_withdraw_resource_recover_worker(context).await?;

        // infrastructure::asset_calc::start_batch_recalculator(1000)?;
        tracing::info!("start_batch_recalculator start");
        let manager = WalletManager { ctx: context, handles: Some(handles) };
        Ok(manager)
    }

    #[cfg(any(test, feature = "integration-tests"))]
    pub async fn new_for_test(
        sn: &str,
        device_type: &str,
        config: crate::config::Config,
        dir: Dirs,
        api_wallet_backend: Arc<dyn ApiWalletBackend>,
    ) -> Result<WalletManager, crate::error::service::ServiceError> {
        crate::infrastructure::task_queue::set_task_execution_mode_for_test(
            crate::infrastructure::task_queue::TaskExecutionMode::Noop,
        );

        let context = init_context_with_api_wallet_backend(
            sn,
            device_type,
            dir,
            None,
            config,
            api_wallet_backend,
        )
        .await?;
        GLOBAL_KEY.set_sn(sn);
        unlock_session::start_wallet_unlock_session_rotation_task(context).await?;

        let handles = Arc::new(Handles::new(context.get_client_id(), context).await?);
        context.set_global_handles(Arc::downgrade(&handles)).await;

        Ok(WalletManager { ctx: context, handles: Some(handles) })
    }

    pub async fn init(&self, req: crate::request::devices::InitDeviceReq) -> ReturnType<()> {
        DeviceService::new(self.ctx).init_device(req).await?;
        // TODO ： 某个版本进行取消,
        domain::app::DeviceDomain::check_wallet_password_is_null(self.ctx).await?;

        // self.init_api_swap().await?;
        let ctx = self.ctx;
        tokio::spawn(async move {
            if let Err(e) = init_some_data(ctx).await {
                tracing::error!("init_data error: {}", e);
            };
        });

        Ok(())
    }

    pub async fn init_api_swap(&self) -> ReturnType<()> {
        tracing::info!(
            "init_api_swap begin -------------------------------------------------------"
        );
        ApiWalletService::new(self.ctx).init_api_swap().await?;
        tracing::info!("init_api_swap end -------------------------------------------------------");
        Ok(())
    }

    pub fn ctx(&self) -> &'static Context {
        self.ctx
    }

    pub async fn process_jpush_message(&self, message: &str) -> ReturnType<()> {
        crate::service::jpush::JPushService::new(self.ctx).jpush(message).await.into()
    }

    pub async fn get_task_queue_status(
        &self,
    ) -> ReturnType<crate::response_vo::standard_wallet::task_queue::TaskQueueStatus> {
        TaskQueueService::new(self.ctx).get_task_queue_status().await
    }

    pub async fn get_console_task_queue_summary(
        &self,
    ) -> ReturnType<crate::response_vo::standard_wallet::diagnostics::ConsoleTaskQueueSummary> {
        ConsoleDiagnosticsService::new(self.ctx).task_queue_summary().await
    }

    pub async fn diagnose_trade(
        &self,
        trade_no: &str,
    ) -> ReturnType<crate::response_vo::standard_wallet::diagnostics::TradeDiagnostics> {
        ConsoleDiagnosticsService::new(self.ctx).diagnose_trade(trade_no).await
    }

    pub async fn set_frontend_notify_sender(
        &self,
        sender: UnboundedSender<FrontendNotifyEvent>,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.ctx.set_frontend_notify_sender(Some(sender)).await
    }

    pub async fn close(&self) -> ReturnType<()> {
        if let Some(handles) = &self.handles { handles.close().await.into() } else { Ok(()) }
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use std::{
        fs::{self, File},
        io::Write,
    };
    use tempfile::tempdir;

    use crate::dirs::Dirs;

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
        let dirs = Dirs::new(dir)?;

        let config = crate::config::Config::new(&crate::testkit::env::get_config()?)?;
        let _manager =
            crate::manager::WalletManager::new("sn", "ANDROID", None, config, dirs).await?;
        let dirs = _manager.ctx.get_global_dirs();

        wallet_tree::wallet_hierarchy::v1::LegacyWalletTree::traverse_directory_structure(
            &dirs.wallet_dir,
        )?;

        Ok(())
    }
}
