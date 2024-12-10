use super::ResourcesRepo;
use crate::{dao::bill::BillDao, entities::bill::BillEntity, pagination::Pagination};

pub struct BillRepo {
    repo: ResourcesRepo,
}

impl BillRepo {
    pub fn new(db_pool: crate::DbPool) -> Self {
        Self {
            repo: ResourcesRepo::new(db_pool),
        }
    }
}

impl BillRepo {
    pub async fn last_bill(
        &self,
        chain_code: &str,
        address: &str,
    ) -> Result<Option<BillEntity>, crate::Error> {
        Ok(BillDao::last_bill(chain_code, address, &*self.repo.db_pool).await?)
    }
}

#[async_trait::async_trait]
pub trait BillRepoTrait: super::TransactionTrait {
    async fn bill_lists(
        &mut self,
        addr: &[String],
        chain_code: Option<&str>,
        symbol: Option<&str>,
        is_multisig: Option<i64>,
        min_value: Option<f64>,
        start: Option<i64>,
        end: Option<i64>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<BillEntity>, crate::Error> {
        let executor = self.get_db_pool();
        let lists = BillDao::bill_lists(
            executor.as_ref(),
            addr,
            chain_code,
            symbol,
            is_multisig,
            min_value,
            start,
            end,
            page,
            page_size,
        )
        .await?;
        Ok(lists)
    }

    async fn get_one_by_hash(
        &mut self,
        hash: &str,
        transfer_type: i64,
    ) -> Result<Option<BillEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, BillDao::get_by_hash_and_type, hash, transfer_type)
    }
}
