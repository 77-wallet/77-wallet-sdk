use crate::entities::device::DeviceEntity;

#[async_trait::async_trait]
pub trait DeviceRepoTrait: super::TransactionTrait {
    async fn get_device_info(&mut self) -> Result<Option<DeviceEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, DeviceEntity::get_device_info,)
    }

    async fn upsert(
        &mut self,
        req: crate::entities::device::CreateDeviceEntity,
    ) -> Result<DeviceEntity, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, DeviceEntity::upsert, req)
    }

    async fn update_currency(&mut self, currency: &str) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, DeviceEntity::update_currency, currency)
    }

    async fn update_password(&mut self, password: Option<&str>) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, DeviceEntity::update_password, password)
    }

    async fn update_uid(&mut self, uid: Option<&str>) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, DeviceEntity::update_uid, uid)
    }

    async fn update_app_id(&mut self, app_id: &str) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, DeviceEntity::update_app_id, app_id)
    }

    async fn device_init(&mut self) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, DeviceEntity::init,)
    }

    async fn language_init(&mut self) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, DeviceEntity::language_init,)
    }
}
