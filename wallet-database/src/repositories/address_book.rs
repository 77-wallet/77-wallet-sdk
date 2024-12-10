use crate::{
    dao::address_book::AddressBookDao, entities::address_book::AddressBookEntity,
    pagination::Pagination,
};

use super::ResourcesRepo;

pub struct AddressBookRepo {
    repo: ResourcesRepo,
}

impl AddressBookRepo {
    pub fn new(db_pool: crate::DbPool) -> Self {
        Self {
            repo: ResourcesRepo::new(db_pool),
        }
    }
}

impl AddressBookRepo {
    pub async fn insert(
        &self,
        name: &str,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::Error> {
        Ok(AddressBookDao::insert(self.repo.pool().as_ref(), name, address, chain_code).await?)
    }

    pub async fn update(
        &self,
        id: u32,
        name: &str,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::Error> {
        let pool = self.repo.pool().clone();
        Ok(AddressBookDao::update(pool.as_ref(), id, name, address, chain_code).await?)
    }

    pub async fn find_by_conditions(
        &self,
        conditions: Vec<(&str, &str)>,
    ) -> Result<Option<AddressBookEntity>, crate::Error> {
        Ok(AddressBookDao::find_condition(self.repo.pool().as_ref(), conditions).await?)
    }

    pub async fn check_not_self(
        &self,
        id: u32,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::Error> {
        Ok(
            AddressBookDao::check_not_self(self.repo.pool().as_ref(), id, address, chain_code)
                .await?,
        )
    }

    pub async fn delete(&self, id: i32) -> Result<(), crate::Error> {
        Ok(AddressBookDao::delete(self.repo.pool().as_ref(), id).await?)
    }

    pub async fn list(
        &self,
        chain_code: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<AddressBookEntity>, crate::Error> {
        Ok(AddressBookDao::list(self.repo.pool(), chain_code, page, page_size).await?)
    }

    pub async fn find_by_address(
        &self,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::Error> {
        Ok(AddressBookDao::find_by_address(self.repo.pool().as_ref(), address, chain_code).await?)
    }
}
