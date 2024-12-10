use wallet_chain_interact::QueryTransactionResult;
use wallet_database::{
    dao::bill::BillDao,
    entities::{self},
};

pub struct BillDomain;

impl BillDomain {
    pub async fn create_bill(
        params: entities::bill::NewBillEntity,
    ) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        Ok(BillDao::create(params, &*pool).await?)
    }

    // query tx resource consume
    pub async fn get_bill_resource_consumer(
        tx_hash: &str,
        chain_code: &str,
    ) -> Result<String, crate::ServiceError> {
        let adapter =
            super::chain::adapter::ChainAdapterFactory::get_transaction_adapter(chain_code).await?;
        let res = adapter.query_tx_res(tx_hash).await?;
        match res {
            Some(res) => Ok(res.resource_consume),
            None => Ok("".to_string()),
        }
    }

    pub async fn get_onchain_bill(
        tx_hash: &str,
        chain_code: &str,
    ) -> Result<Option<QueryTransactionResult>, crate::ServiceError> {
        let adapter =
            super::chain::adapter::ChainAdapterFactory::get_transaction_adapter(chain_code).await?;

        Ok(adapter.query_tx_res(tx_hash).await?)
    }
}
