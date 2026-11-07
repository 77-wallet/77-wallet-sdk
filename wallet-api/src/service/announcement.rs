use wallet_database::{
    entities::announcement::AnnouncementEntity, pagination::Pagination,
    repositories::announcement::AnnouncementRepo,
};

use crate::{context::Context, domain::announcement::AnnouncementDomain};

pub struct AnnouncementService {
    ctx: &'static Context,
}

impl AnnouncementService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn add(
        self,
        input: Vec<wallet_database::entities::announcement::CreateAnnouncementVo>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        AnnouncementRepo::add(&core_pool, input).await?;
        Ok(())
    }

    pub async fn pull_announcement(self) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        AnnouncementDomain::pull_announcement(&core_pool, self.ctx).await?;
        Ok(())
    }

    pub async fn get_announcement_list(
        self,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<AnnouncementEntity>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let res = AnnouncementRepo::get_announcement_list(&core_pool, page, page_size).await?;

        Ok(res)
    }

    pub async fn read(self, id: Option<&str>) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        AnnouncementRepo::read(&core_pool, id).await?;
        Ok(())
    }

    pub async fn get_announcement_by_id(
        self,
        id: &str,
    ) -> Result<Option<AnnouncementEntity>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let res = AnnouncementRepo::get_announcement_by_id(&core_pool, id).await?;
        Ok(res)
    }

    pub async fn physical_delete(
        self,
        id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        AnnouncementRepo::delete(&core_pool, id).await?;
        Ok(())
    }
}
