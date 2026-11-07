use crate::{
    CoreDbPool, dao::exchange_rate::ExchangeRateDao, entities::exchange_rate::ExchangeRateEntity,
};
use sqlx::{Sqlite, Transaction};

pub struct ExchangeRateRepo;

impl ExchangeRateRepo {
    pub async fn upsert(
        pool: CoreDbPool,
        target_currency: &str,
        name: &str,
        rate: f64,
    ) -> Result<Vec<ExchangeRateEntity>, crate::Error> {
        ExchangeRateDao::upsert(pool.write_ref(), target_currency, name, rate).await
    }

    pub async fn upsert_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        target_currency: &str,
        name: &str,
        rate: f64,
    ) -> Result<Vec<ExchangeRateEntity>, crate::Error> {
        ExchangeRateDao::upsert(tx.as_mut(), target_currency, name, rate).await
    }

    pub async fn list(pool: CoreDbPool) -> Result<Vec<ExchangeRateEntity>, crate::Error> {
        ExchangeRateDao::list(pool.read_ref()).await
    }

    pub async fn list_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<Vec<ExchangeRateEntity>, crate::Error> {
        ExchangeRateDao::list(tx.as_mut()).await
    }

    // get exchange rate by target currency
    pub async fn exchange_rate(
        target: &str,
        pool: CoreDbPool,
    ) -> Result<ExchangeRateEntity, crate::Error> {
        ExchangeRateDao::detail(pool.read_ref(), target)
            .await?
            .ok_or(crate::Error::NotFound(format!("exchange rate not found currency: {}", target)))
    }

    pub async fn exchange_rate_with_executor(
        target: &str,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<ExchangeRateEntity, crate::Error> {
        ExchangeRateDao::detail(tx.as_mut(), target)
            .await?
            .ok_or(crate::Error::NotFound(format!("exchange rate not found currency: {}", target)))
    }

    pub async fn get_by_target_currency(
        pool: CoreDbPool,
        target_currency: &str,
    ) -> Result<Option<ExchangeRateEntity>, crate::Error> {
        ExchangeRateDao::get_by_target_currency(pool.read_ref(), target_currency).await
    }

    pub async fn get_by_target_currency_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        target_currency: &str,
    ) -> Result<Option<ExchangeRateEntity>, crate::Error> {
        ExchangeRateDao::get_by_target_currency(tx.as_mut(), target_currency).await
    }

    pub async fn get_by_target_currency_or_default(
        pool: CoreDbPool,
        target_currency: &str,
    ) -> Result<ExchangeRateEntity, crate::Error> {
        Ok(ExchangeRateRepo::get_by_target_currency(pool, target_currency).await?.unwrap_or(
            ExchangeRateEntity {
                name: "USD".to_string(),
                rate: 1.0,
                target_currency: "USD".to_string(),
                created_at: Default::default(),
                updated_at: Default::default(),
            },
        ))
    }

    pub async fn get_by_target_currency_or_default_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        target_currency: &str,
    ) -> Result<ExchangeRateEntity, crate::Error> {
        Ok(ExchangeRateRepo::get_by_target_currency_with_executor(tx, target_currency)
            .await?
            .unwrap_or(ExchangeRateEntity {
                name: "USD".to_string(),
                rate: 1.0,
                target_currency: "USD".to_string(),
                created_at: Default::default(),
                updated_at: Default::default(),
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::ExchangeRateRepo;
    use crate::repositories::test_helper::setup_core_pool;

    #[tokio::test]
    async fn exchange_rate_repo_upsert_and_query_success() {
        let pool = setup_core_pool("wallet_db_exchange_rate_success").await;

        let upserted = ExchangeRateRepo::upsert(pool.clone(), "CNY", "CNY", 7.12).await.unwrap();
        assert_eq!(upserted.len(), 1);
        assert_eq!(upserted[0].target_currency, "CNY");

        let listed = ExchangeRateRepo::list(pool.clone()).await.unwrap();
        assert!(listed.iter().any(|x| x.target_currency == "CNY"));

        let detail = ExchangeRateRepo::exchange_rate("CNY", pool).await.unwrap();
        assert_eq!(detail.name, "CNY");
        assert_eq!(detail.rate, 7.12);
    }

    #[tokio::test]
    async fn exchange_rate_repo_missing_currency_returns_not_found() {
        let pool = setup_core_pool("wallet_db_exchange_rate_edge").await;
        let err = ExchangeRateRepo::exchange_rate("ZZZ", pool).await.unwrap_err();
        assert!(matches!(err, crate::Error::NotFound(_)));
    }

    #[tokio::test]
    async fn exchange_rate_target_currency_treats_sql_shaped_input_as_data() {
        let pool = setup_core_pool("wallet_db_exchange_rate_bind").await;
        ExchangeRateRepo::upsert(pool.clone(), "USD", "USD", 1.0).await.unwrap();

        let found =
            ExchangeRateRepo::get_by_target_currency(pool, "missing' OR 1=1 --").await.unwrap();

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn exchange_rate_repo_tx_rollback_keeps_rate_unchanged() {
        let pool = setup_core_pool("wallet_db_exchange_rate_rollback").await;
        ExchangeRateRepo::upsert(pool.clone(), "USD", "USD", 1.0).await.unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        ExchangeRateRepo::upsert_with_executor(&mut tx, "USD", "USD", 2.5).await.unwrap();
        tx.rollback().await.unwrap();

        let after = ExchangeRateRepo::exchange_rate("USD", pool).await.unwrap();
        assert_eq!(after.rate, 1.0);
    }
}
