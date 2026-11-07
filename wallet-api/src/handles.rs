use crate::{
    context::Context,
    domain::app::{DeviceDomain, config::ConfigDomain},
    infrastructure::{
        self,
        // asset_calc::actor_model::AssetCalcActorManager,
        api_trans::collect::handle::ProcessCollectTxHandle,
        api_trans::collect_fee::handle::ProcessFeeTxHandle,
        api_trans::resource_operation::shadow::ResourceOperationShadowActorSystem,
        api_trans::resource_reclaim::local_shadow::LocalResourceReclaimShadowActorSystem,
        api_trans::withdraw::handle::ProcessWithdrawTxHandle,
        collector_unconfirm_msg::UnconfirmedMsgCollector,
        inner_event::InnerEventHandle,
        log::upload_log::UploadLogHandle,
        mqtt::{init::ProcessMqttHandle, property::UserProperty},
        private_key_manager::PrivateKeyManager,
        process_unconfirm_msg::UnconfirmedMsgProcessorHandle,
        task_queue::task_manager::TaskManager,
    },
};
use std::sync::Arc;
use tokio::sync::Mutex;
use wallet_database::repositories::device::DeviceRepo;

pub struct Handles {
    context: &'static crate::context::Context,
    task_manager: Arc<TaskManager>,
    inner_event_handle: Arc<InnerEventHandle>,
    unconfirmed_msg_collector: Arc<UnconfirmedMsgCollector>,
    unconfirmed_msg_processor: Arc<UnconfirmedMsgProcessorHandle>,
    process_withdraw_tx_handle: Arc<ProcessWithdrawTxHandle>,
    process_fee_tx_handle: Arc<ProcessFeeTxHandle>,
    process_collect_tx_handle: Arc<ProcessCollectTxHandle>,
    resource_operation_shadow: Arc<Mutex<Option<ResourceOperationShadowActorSystem>>>,
    local_resource_reclaim_shadow: Arc<Mutex<Option<LocalResourceReclaimShadowActorSystem>>>,
    upload_log: Arc<UploadLogHandle>,
    normal_wallet_mqtt: Arc<Mutex<Option<ProcessMqttHandle>>>,
    api_wallet_mqtt: Arc<Mutex<Option<ProcessMqttHandle>>>,
    // asset_calc_actor_manager: Arc<AssetCalcActorManager>,
    private_key_manager: Arc<PrivateKeyManager>,
}

impl Handles {
    pub async fn new(
        client_id: &str,
        context: &'static Context,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let unconfirmed_msg_collector = UnconfirmedMsgCollector::new(context);
        // 创建 TaskManager 实例
        let notify = Arc::new(tokio::sync::Notify::new());
        let task_manager = TaskManager::new(context, notify.clone());

        let unconfirmed_msg_processor =
            UnconfirmedMsgProcessorHandle::new(context, &client_id, notify).await;

        let inner_event_handle = InnerEventHandle::new(context);

        let process_withdraw_tx_handle = ProcessWithdrawTxHandle::new_with_ctx(context).await?;
        let process_fee_tx_handle = ProcessFeeTxHandle::new_with_ctx(context).await?;
        let process_collect_tx_handle = ProcessCollectTxHandle::new_with_ctx(context).await?;
        let resource_operation_shadow =
            infrastructure::api_trans::resource_operation::shadow::init(context).await?;
        let local_resource_reclaim_shadow =
            infrastructure::api_trans::resource_reclaim::local_shadow::init(context).await?;

        // 初始化私钥管理器
        tracing::info!("Initialize private key manager start");
        let private_key_manager =
            Arc::new(crate::infrastructure::private_key_manager::PrivateKeyManager::start(context));
        tracing::info!("Initialize private key manager completed");
        let dirs = context.get_global_dirs();
        let base_path = infrastructure::log::format::LogBasePath(dirs.get_log_dir());
        let upload_log_handle = UploadLogHandle::new_with_diagnostics(
            base_path,
            5 * 60,
            context.get_log_upload_client(),
            context.get_sn().to_owned(),
            crate::messaging::notify::diagnostic::DiagnosticEmitter::from_context(context),
        )
        .await;
        // let asset_calc_actor_manager = AssetCalcActorManager::start(pool.clone());
        Ok(Self {
            context,
            task_manager: Arc::new(task_manager),
            inner_event_handle: Arc::new(inner_event_handle),
            unconfirmed_msg_collector: Arc::new(unconfirmed_msg_collector),
            unconfirmed_msg_processor: Arc::new(unconfirmed_msg_processor),
            process_withdraw_tx_handle: Arc::new(process_withdraw_tx_handle),
            process_fee_tx_handle: Arc::new(process_fee_tx_handle),
            process_collect_tx_handle: Arc::new(process_collect_tx_handle),
            resource_operation_shadow: Arc::new(Mutex::new(Some(resource_operation_shadow))),
            local_resource_reclaim_shadow: Arc::new(Mutex::new(Some(
                local_resource_reclaim_shadow,
            ))),
            upload_log: Arc::new(upload_log_handle),
            normal_wallet_mqtt: Arc::new(Mutex::new(None)),
            api_wallet_mqtt: Arc::new(Mutex::new(None)),
            // asset_calc_actor_manager: Arc::new(asset_calc_actor_manager),
            private_key_manager: private_key_manager.clone(),
        })
    }

    pub(crate) async fn close(&self) -> Result<(), crate::error::service::ServiceError> {
        self.process_withdraw_tx_handle.close().await?;
        self.process_fee_tx_handle.close().await?;
        self.process_collect_tx_handle.close().await?;
        {
            let mut resource_operation_shadow = self.resource_operation_shadow.lock().await;
            if let Some(resource_operation_shadow) = resource_operation_shadow.as_mut() {
                resource_operation_shadow.stop().await;
            }
            resource_operation_shadow.take();
        }
        {
            let mut local_resource_reclaim_shadow = self.local_resource_reclaim_shadow.lock().await;
            if let Some(local_resource_reclaim_shadow) = local_resource_reclaim_shadow.as_mut() {
                local_resource_reclaim_shadow.stop().await;
            }
            local_resource_reclaim_shadow.take();
        }
        self.upload_log.close().await?;
        self.unconfirmed_msg_processor.close().await?;
        {
            let mut normal_wallet_mqtt = self.normal_wallet_mqtt.lock().await;
            if let Some(normal_wallet_mqtt) = normal_wallet_mqtt.take() {
                normal_wallet_mqtt.close().await?;
            }
        }
        {
            let mut api_wallet_mqtt = self.api_wallet_mqtt.lock().await;
            if let Some(api_wallet_mqtt) = api_wallet_mqtt.take() {
                api_wallet_mqtt.close().await?;
            }
        }

        Ok(())
    }

    pub(crate) fn get_global_processed_withdraw_tx_handle(&self) -> Arc<ProcessWithdrawTxHandle> {
        self.process_withdraw_tx_handle.clone()
    }

    pub(crate) fn get_global_processed_fee_tx_handle(&self) -> Arc<ProcessFeeTxHandle> {
        self.process_fee_tx_handle.clone()
    }

    pub(crate) fn get_global_processed_collect_tx_handle(&self) -> Arc<ProcessCollectTxHandle> {
        self.process_collect_tx_handle.clone()
    }

    pub(crate) async fn trigger_resource_operation(
        &self,
        resource_trade_no: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let resource_operation_shadow = self.resource_operation_shadow.lock().await;
        if let Some(resource_operation_shadow) = resource_operation_shadow.as_ref() {
            resource_operation_shadow.trigger_resource_operation(resource_trade_no).await?;
        }
        Ok(())
    }

    pub(crate) fn get_global_task_manager(&self) -> Arc<TaskManager> {
        self.task_manager.clone()
    }

    pub(crate) fn get_global_inner_event_handle(&self) -> Arc<InnerEventHandle> {
        self.inner_event_handle.clone()
    }

    pub(crate) fn get_global_notify(&self) -> Arc<tokio::sync::Notify> {
        self.task_manager.notify.clone()
    }

    pub(crate) fn get_global_unconfirmed_msg_collector(&self) -> Arc<UnconfirmedMsgCollector> {
        self.unconfirmed_msg_collector.clone()
    }

    // pub(crate) fn get_global_asset_calc_actor_manager(&self) -> Arc<AssetCalcActorManager> {
    //     self.asset_calc_actor_manager.clone()
    // }

    pub(crate) fn get_global_private_key_manager(&self) -> Arc<PrivateKeyManager> {
        self.private_key_manager.clone()
    }

    pub(crate) fn get_global_unconfirmed_msg_processor(
        &self,
    ) -> Arc<UnconfirmedMsgProcessorHandle> {
        self.unconfirmed_msg_processor.clone()
    }

    pub(crate) async fn init_normal_wallet_mqtt(
        &self,
    ) -> Result<(), crate::error::service::ServiceError> {
        let ctx = self.context;
        let pool = ctx.core_pool()?;
        let Some(device) = DeviceRepo::get_device_info(pool, ctx.get_sn()).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };
        let content = DeviceDomain::device_content(&device)?;
        let client_id = DeviceDomain::client_id_by_device(&device)?;
        let password = DeviceDomain::md5_sn(&device.sn);

        let app_version = ConfigDomain::get_app_version(ctx).await?;

        let property =
            UserProperty::new(content, client_id, &device.sn, password, &app_version.app_version);

        let url = ConfigDomain::get_mqtt_uri(ctx).await?.ok_or(
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::MqttClientNotInit,
            ),
        )?;
        let h = ProcessMqttHandle::new(property, url, ctx).await?;
        self.normal_wallet_mqtt.lock().await.replace(h);
        Ok(())
    }

    pub(crate) async fn init_api_wallet_mqtt(
        &self,
    ) -> Result<(), crate::error::service::ServiceError> {
        let ctx = self.context;
        let pool = ctx.core_pool()?;
        let Some(device) = DeviceRepo::get_device_info(pool, ctx.get_sn()).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };
        let content = DeviceDomain::device_content(&device)?;
        let client_id = DeviceDomain::client_id_by_device(&device)? + "_aw";
        let password = DeviceDomain::md5_sn(&device.sn);

        let app_version = ConfigDomain::get_app_version(ctx).await?;

        let property =
            UserProperty::new(content, client_id, &device.sn, password, &app_version.app_version);

        let url = ConfigDomain::get_mqtt_uri(ctx).await?.ok_or(
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::MqttClientNotInit,
            ),
        )?;
        let h = ProcessMqttHandle::new(property, url, ctx).await?;
        self.api_wallet_mqtt.lock().await.replace(h);
        Ok(())
    }

    pub(crate) fn get_normal_wallet_mqtt(&self) -> Arc<Mutex<Option<ProcessMqttHandle>>> {
        self.normal_wallet_mqtt.clone()
    }
}
