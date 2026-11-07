use crate::{
    context::Context,
    domain::app::config::ConfigDomain,
    infrastructure::task_queue::backend::{BackendApiTask, BackendApiTaskData},
};
use wallet_database::{
    entities::{
        config::config_key::APP_VERSION,
        device::{CreateDeviceEntity, DeviceEntity},
    },
    repositories::device::DeviceRepo,
};
use wallet_transport_backend::{consts::endpoint, request::DeviceInitReq};
// pub const APP_ID: &str = "bc7f694ee0a9488cada7d9308190fe45";
pub const APP_ID: &str = "ada7d9308190fe45";

use crate::{infrastructure::task_queue::task::Tasks, request::devices::InitDeviceReq};

pub struct DeviceService {
    ctx: &'static Context,
}

impl DeviceService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn get_device_info(
        self,
        sn: &str,
    ) -> Result<Option<DeviceEntity>, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;
        Ok(DeviceRepo::get_device_info(pool, sn).await?)
    }

    pub async fn init_device(
        self,
        req: InitDeviceReq,
    ) -> Result<Option<()>, crate::error::service::ServiceError> {
        // let package_id = req.package_id.clone();
        let upsert_req = (&req).into();
        let pool = self.ctx.core_pool()?;
        DeviceRepo::upsert(pool.clone(), upsert_req).await?;

        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(pool.clone(), sn).await? else {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Device(
                    crate::error::business::device::DeviceError::Uninitialized,
                )
                .into(),
            ));
        };

        if device.is_init == 0 {
            let task_req: DeviceInitReq = (&req).into();
            let task_data = BackendApiTaskData {
                endpoint: endpoint::DEVICE_INIT.to_string(),
                body: wallet_utils::serde_func::serde_to_value(&task_req)?,
            };

            Tasks::new()
                .push(BackendApiTask::BackendApi(task_data))
                .send_with_ctx(self.ctx)
                .await?;
        }

        let app_version =
            wallet_database::entities::config::AppVersion { app_version: req.app_version };
        ConfigDomain::set_config(self.ctx, APP_VERSION, &app_version.to_json_str()?).await?;

        // 第一次初始化设备时，主动bump epoch，将首次安装视为一次reset
        // 确保系统进入有效世代(epoch >= 1)，允许后续Init请求执行
        let current_epoch = ConfigDomain::get_keys_reset_epoch(self.ctx).await?;
        if current_epoch == 0 {
            ConfigDomain::bump_keys_reset_epoch(self.ctx).await?;
            let new_epoch = ConfigDomain::get_keys_reset_epoch(self.ctx).await?;
            tracing::info!(
                sn = sn,
                old_epoch = current_epoch,
                new_epoch = new_epoch,
                "init_device: First time init, bumped epoch to valid generation"
            );
        }

        Ok(None)
    }

    pub async fn add_device(
        self,
        req: CreateDeviceEntity,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;
        DeviceRepo::upsert(pool, req).await?;

        Ok(())
    }

    pub async fn unbind_device(self, sn: &str) -> Result<(), crate::error::service::ServiceError> {
        // 1. 首先递增Epoch，切换世代，这是reset的核心事实
        // 确保reset开始后，所有后续操作都使用新世代的Epoch
        ConfigDomain::bump_keys_reset_epoch(self.ctx).await?;
        // 获取新的epoch值用于日志
        let new_epoch = ConfigDomain::get_keys_reset_epoch(self.ctx).await?;
        tracing::info!(
            epoch = new_epoch,
            sn = sn,
            "unbind_device: Epoch bumped, generation switched"
        );

        let task_data = BackendApiTaskData {
            endpoint: endpoint::KEYS_RESET.to_string(),
            body: serde_json::json!({
                "sn": sn,
            }),
        };

        Tasks::new().push(BackendApiTask::BackendApi(task_data)).send_with_ctx(self.ctx).await?;
        Ok(())
    }
}
