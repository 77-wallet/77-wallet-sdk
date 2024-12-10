use crate::entities::exchange_rate::ExchangeRateEntity;

#[async_trait::async_trait]
pub trait ExchangeRateRepoTrait: super::TransactionTrait {
    async fn upsert(
        &mut self,
        target_currency: &str,
        name: &str,
        rate: f64,
    ) -> Result<Vec<ExchangeRateEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            ExchangeRateEntity::upsert,
            target_currency,
            name,
            rate
        )
    }

    async fn detail(
        &mut self,
        target_currency: Option<String>,
    ) -> Result<Option<ExchangeRateEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        let req = crate::entities::exchange_rate::QueryReq { target_currency };
        crate::execute_with_executor!(executor, ExchangeRateEntity::detail, &req)
    }

    async fn list(&mut self) -> Result<Vec<ExchangeRateEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, ExchangeRateEntity::list,)
    }
}
