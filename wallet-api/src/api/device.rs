use crate::{
    api::ReturnType, manager::WalletManager, request::devices::InitDeviceReq,
    service::device::DeviceService,
};

impl WalletManager {
    pub async fn init_device(&self, req: InitDeviceReq) -> ReturnType<Option<()>> {
        DeviceService::new(self.ctx).init_device(req).await
    }
}
