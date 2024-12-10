use crate::repositories::{address_book, stake};
use sqlx::{Pool, Sqlite};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct RepositoryFactory {
    db_pool: crate::DbPool,
}
impl RepositoryFactory {
    pub fn new(db_pool: crate::DbPool) -> Self {
        Self { db_pool }
    }
}

impl RepositoryFactory {
    pub fn stake_repo(&self) -> stake::StakeRepo {
        stake::StakeRepo::new(self.db_pool.clone())
    }

    pub fn address_book_repo(&self) -> address_book::AddressBookRepo {
        address_book::AddressBookRepo::new(self.db_pool.clone())
    }

    pub fn repo(pool: Arc<Pool<Sqlite>>) -> crate::repositories::ResourcesRepo {
        crate::repositories::ResourcesRepo::new(pool)
    }

    pub fn resuource_repo(&self) -> crate::repositories::ResourcesRepo {
        crate::repositories::ResourcesRepo::new(self.db_pool.clone())
    }

    pub fn multisig_account_repo(
        &self,
    ) -> crate::repositories::multisig_account::MultisigAccountRepo {
        crate::repositories::multisig_account::MultisigAccountRepo::new(self.db_pool.clone())
    }
}
