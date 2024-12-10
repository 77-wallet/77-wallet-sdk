use wallet_database::{
    entities::device::{CreateDeviceEntity, DeviceEntity},
    repositories::device::DeviceRepoTrait,
};
use wallet_transport_backend::{consts::endpoint, request::DeviceInitReq};

// pub const APP_ID: &str = "bc7f694ee0a9488cada7d9308190fe45";
pub const APP_ID: &str = "ada7d9308190fe45";

use crate::{
    domain::{
        self,
        task_queue::{BackendApiTask, Task, Tasks},
    },
    request::devices::InitDeviceReq,
};

pub struct DeviceService<T: DeviceRepoTrait> {
    pub repo: T,
    // keystore: wallet_keystore::Keystore
}

impl<T: DeviceRepoTrait> DeviceService<T> {
    pub fn new(repo: T) -> Self {
        Self { repo }
    }

    pub async fn get_device_info(self) -> Result<Option<DeviceEntity>, crate::ServiceError> {
        let mut tx = self.repo;
        let res = tx.get_device_info().await?;
        Ok(res)
    }

    pub async fn init_device(self, req: InitDeviceReq) -> Result<Option<()>, crate::ServiceError> {
        let mut tx = self.repo;

        let package_id = req.package_id.clone();
        let upsert_req = (&req).into();
        tx.upsert(upsert_req).await?;

        let req: DeviceInitReq = (&req).into();
        // let tasks = vec![Task::BackendApi(BackendApiTask::DeviceInit(req))];
        let task_data = domain::task_queue::BackendApiTaskData {
            endpoint: endpoint::DEVICE_INIT.to_string(),
            body: wallet_utils::serde_func::serde_to_value(&req)?,
        };

        Tasks::new()
            .push(Task::BackendApi(BackendApiTask::BackendApi(task_data)))
            .send()
            .await?;

        let device = tx
            .get_device_info()
            .await?
            .ok_or(crate::BusinessError::Device(
                crate::DeviceError::Uninitialized,
            ))?;

        tokio::spawn(async move {
            let content = domain::app::DeviceDomain::device_content(&device).unwrap();
            let client_id = domain::app::DeviceDomain::client_id_by_device(&device).unwrap();
            crate::mqtt::init_mqtt_processor(
                &device.sn,
                APP_ID,
                crate::mqtt::user_property::UserProperty::new(
                    &package_id.unwrap_or("77wallet".to_string()),
                    &content,
                    &client_id,
                ),
                crate::mqtt::wrap_handle_eventloop,
            )
            .await
            .unwrap();
        });

        Ok(None)
    }

    pub async fn add_device(self, req: CreateDeviceEntity) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        tx.upsert(req).await?;

        Ok(())
    }
}
