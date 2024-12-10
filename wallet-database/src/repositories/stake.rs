use super::ResourcesRepo;
use crate::{
    dao::stake,
    entities::stake::{DelegateEntity, NewDelegateEntity, NewUnFreezeEntity, UnFreezeEntity},
    pagination::Pagination,
};

pub struct StakeRepo {
    repo: ResourcesRepo,
}

impl StakeRepo {
    pub fn new(db_pool: crate::DbPool) -> Self {
        Self {
            repo: ResourcesRepo::new(db_pool),
        }
    }
}

impl StakeRepo {
    pub async fn add_unfreeze(&self, stake: NewUnFreezeEntity) -> Result<(), crate::Error> {
        let pool = self.repo.pool();
        Ok(stake::add_unfreeze(stake, &*pool).await?)
    }

    pub async fn unfreeze_list(
        &self,
        owner: &str,
        resource_type: &str,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<UnFreezeEntity>, crate::Error> {
        let pool = self.repo.pool();
        Ok(stake::unfreeze_list(owner, resource_type, page, page_size, &pool).await?)
    }

    pub async fn add_delegate(&self, delegate: NewDelegateEntity) -> Result<(), crate::Error> {
        let pool = self.repo.pool();
        Ok(stake::add_delegate(delegate, &*pool).await?)
    }

    pub async fn update_delegate(&self, id: &str) -> Result<(), crate::Error> {
        let pool = self.repo.pool();
        Ok(stake::update_delegate(id, &*pool).await?)
    }

    pub async fn delegate_list(
        &self,
        owner_address: &str,
        resource_type: &str,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<DelegateEntity>, crate::Error> {
        let pool = self.repo.pool();
        Ok(stake::delegate_list(owner_address, resource_type, page, page_size, pool).await?)
    }

    pub async fn find_delegate_by_id(&self, id: &str) -> Result<DelegateEntity, crate::Error> {
        let pool = self.repo.pool();
        Ok(stake::find_delegate_by_id(id, &*pool).await?)
    }
}
