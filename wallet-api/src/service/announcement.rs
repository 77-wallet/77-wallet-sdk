use wallet_database::{
    entities::announcement::AnnouncementEntity,
    pagination::Pagination,
    repositories::{announcement::AnnouncementRepoTrait, ResourcesRepo, TransactionTrait as _},
};

use crate::domain::announcement::AnnouncementDomain;

pub struct AnnouncementService {
    repo: ResourcesRepo,
}

impl AnnouncementService {
    pub fn new(repo: ResourcesRepo) -> Self {
        Self { repo }
    }

    pub async fn add(
        self,
        input: Vec<wallet_database::entities::announcement::CreateAnnouncementVo>,
    ) -> Result<(), crate::error::ServiceError> {
        let mut tx = self.repo;
        tx.begin_transaction().await?;
        tx.add(input).await?;
        tx.commit_transaction().await?;
        Ok(())
    }

    pub async fn pull_announcement(mut self) -> Result<(), crate::error::ServiceError> {
        let tx = &mut self.repo;
        AnnouncementDomain::pull_announcement(tx).await?;
        Ok(())
    }

    pub async fn get_announcement_list(
        mut self,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<AnnouncementEntity>, crate::error::ServiceError> {
        let res = self.repo.get_announcement_list(page, page_size).await?;

        Ok(res)
    }

    pub async fn read(self, id: Option<&str>) -> Result<(), crate::error::ServiceError> {
        let mut tx = self.repo;
        tx.begin_transaction().await?;
        tx.read(id).await?;
        tx.commit_transaction().await?;
        Ok(())
    }

    pub async fn get_announcement_by_id(
        mut self,
        id: &str,
    ) -> Result<Option<AnnouncementEntity>, crate::error::ServiceError> {
        let res = self.repo.get_announcement_by_id(id).await?;
        Ok(res)
    }

    pub async fn physical_delete(self, id: &str) -> Result<(), crate::error::ServiceError> {
        let mut tx = self.repo;
        tx.begin_transaction().await?;
        tx.physical_delete(id).await?;
        tx.commit_transaction().await?;
        Ok(())
    }
}
