use wallet_database::repositories::{announcement::AnnouncementRepoTrait, device::DeviceRepoTrait};

pub struct AnnouncementDomain;

impl AnnouncementDomain {
    pub async fn pull_announcement(
        repo: &mut wallet_database::repositories::ResourcesRepo,
    ) -> Result<(), crate::error::ServiceError> {
        let backend = crate::Context::get_global_backend_api()?;

        let list = AnnouncementRepoTrait::list(repo).await?;

        if let Some(device) = DeviceRepoTrait::get_device_info(repo).await? {
            let client_id = super::app::DeviceDomain::client_id_by_device(&device)?;
            let req = wallet_transport_backend::request::AnnouncementListReq::new(client_id, 0, 50);
            let res = backend.announcement_list(req).await?;

            let res_ids: std::collections::HashSet<_> =
                res.list.iter().map(|info| info.id.to_string()).collect();
            let to_delete: Vec<_> = list
                .into_iter()
                .filter(|item| !res_ids.contains(&item.id))
                .map(|item| item.id)
                .collect();

            for id in to_delete {
                AnnouncementRepoTrait::physical_delete(repo, &id).await?;
            }

            let input = res
                .list
                .into_iter()
                .map(
                    |info| wallet_database::entities::announcement::CreateAnnouncementVo {
                        id: info.id.to_string(),
                        title: info.i18n.title,
                        content: info.i18n.content,
                        language: info.language,
                        status: 0,
                        send_time: info.send_time,
                    },
                )
                .collect();
            AnnouncementRepoTrait::update_existing(repo, input).await?;
        } else {
            return Err(crate::BusinessError::Device(crate::DeviceError::Uninitialized).into());
        }
        let data = crate::notify::NotifyEvent::FetchBulletinMsg;
        crate::notify::FrontendNotifyEvent::new(data).send().await?;
        Ok(())
    }
}
