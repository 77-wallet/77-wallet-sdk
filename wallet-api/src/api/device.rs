use crate::{api::ReturnType, service::device::DeviceService};

impl crate::WalletManager {
    pub async fn init_device(&self, req: crate::InitDeviceReq) -> ReturnType<Option<()>> {
        DeviceService::new(self.repo_factory.resuource_repo())
            .init_device(req)
            .await?
            .into()
    }
}
