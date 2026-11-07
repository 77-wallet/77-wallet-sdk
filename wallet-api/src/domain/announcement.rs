use crate::{
    context::Context,
    messaging::notify::{FrontendNotifyEvent, event::NotifyEvent},
};
use wallet_database::{
    CoreDbPool,
    repositories::{announcement::AnnouncementRepo, device::DeviceRepo},
};

pub struct AnnouncementDomain;

impl AnnouncementDomain {
    pub async fn pull_announcement(
        pool: &CoreDbPool,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let list = AnnouncementRepo::list(pool).await?;

        let core_pool = ctx.core_pool()?;
        let sn = ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(core_pool.clone(), sn).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };

        let client_id = super::app::DeviceDomain::client_id_by_device(&device)?;
        let req = wallet_transport_backend::request::AnnouncementListReq::new(client_id, 0, 50);
        let backend = ctx.get_global_backend_api();
        let res = backend.announcement_list(req).await?;

        let res_ids: std::collections::HashSet<_> =
            res.list.iter().map(|info| info.id.to_string()).collect();
        let to_delete: Vec<_> = list
            .into_iter()
            .filter(|item| !res_ids.contains(&item.id))
            .map(|item| item.id)
            .collect();

        for id in to_delete {
            AnnouncementRepo::delete(pool, &id).await?;
        }

        let input = res
            .list
            .into_iter()
            .map(|info| wallet_database::entities::announcement::CreateAnnouncementVo {
                id: info.id.to_string(),
                title: info.i18n.title,
                content: info.i18n.content,
                language: info.language,
                status: 0,
                send_time: info.send_time,
            })
            .collect();
        AnnouncementRepo::update_existing(pool, input).await?;

        let data = NotifyEvent::FetchBulletinMsg;
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
        Ok(())
    }
}
