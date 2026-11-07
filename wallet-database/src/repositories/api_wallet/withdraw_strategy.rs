use crate::{
    ApiWalletDbPool, dao::api_withdraw_strategy::ApiWithdrawStrategyDao,
    entities::api_withdraw_strategy::ApiWithdrawStrategyEntity,
};
pub struct ApiWithdrawStrategyRepo;

impl ApiWithdrawStrategyRepo {
    /// Replace the complete local configuration in one transaction. A failed
    /// chain insert must not expose a new header or a partially deleted list.
    pub async fn replace_with_chain_configs(
        pool: &ApiWalletDbPool,
        input: ApiWithdrawStrategyEntity,
        configs: Vec<crate::entities::api_withdraw_strategy_chain_config::ApiWithdrawStrategyChainConfigEntity>,
    ) -> Result<(), crate::Error> {
        use crate::dao::api_withdraw_strategy_chain_config::ApiWithdrawStrategyChainConfigDao;
        let _guard = pool.lock_write_with_metric("replace_withdraw_strategy").await;
        let mut tx =
            pool.write_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        let uid = input.uid.clone();
        ApiWithdrawStrategyDao::upsert(tx.as_mut(), input).await?;
        let strategy = ApiWithdrawStrategyDao::get_by_uid(tx.as_mut(), &uid)
            .await?
            .ok_or_else(|| crate::Error::NotFound(uid))?;
        ApiWithdrawStrategyChainConfigDao::delete_chain_configs_by_strategy_id(
            tx.as_mut(),
            strategy.id,
        )
        .await?;
        for mut config in configs {
            config.strategy_id = strategy.id;
            ApiWithdrawStrategyChainConfigDao::upsert(tx.as_mut(), config).await?;
        }
        tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

    pub async fn list_api_withdraw_strategy(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ApiWithdrawStrategyEntity>, crate::Error> {
        ApiWithdrawStrategyDao::all_api_withdraw_strategy(pool.read_ref()).await
    }

    pub async fn page_api_withdraw_strategy(
        pool: &ApiWalletDbPool,
        page: i64,
        page_size: i64,
    ) -> Result<(i64, Vec<ApiWithdrawStrategyEntity>), crate::Error> {
        ApiWithdrawStrategyDao::page_api_withdraw_strategy(pool.read_ref(), page, page_size).await
    }

    pub async fn upsert(
        pool: &ApiWalletDbPool,
        input: ApiWithdrawStrategyEntity,
    ) -> Result<(), crate::Error> {
        ApiWithdrawStrategyDao::upsert(pool.write_ref(), input).await
    }

    pub async fn get_by_uid(
        pool: &ApiWalletDbPool,
        uid: &str,
    ) -> Result<Option<ApiWithdrawStrategyEntity>, crate::Error> {
        ApiWithdrawStrategyDao::get_by_uid(pool.read_ref(), uid).await
    }

    pub async fn delete(pool: &ApiWalletDbPool, uid: &str) -> Result<(), crate::Error> {
        ApiWithdrawStrategyDao::delete(pool.write_ref(), uid).await
    }
}

#[cfg(test)]
mod tests {
    use super::ApiWithdrawStrategyRepo;
    use crate::{
        dao::api_withdraw_strategy::ApiWithdrawStrategyDao,
        entities::api_withdraw_strategy::ApiWithdrawStrategyEntity,
        repositories::test_helper::setup_api_wallet_pool,
    };

    fn make_strategy(uid: &str, threshold: i32) -> ApiWithdrawStrategyEntity {
        ApiWithdrawStrategyEntity {
            id: 0,
            uid: uid.to_string(),
            threshold,
            created_at: Default::default(),
            updated_at: None,
        }
    }

    #[tokio::test]
    async fn withdraw_strategy_repo_upsert_and_get_success() {
        let pool = setup_api_wallet_pool("wallet_db_withdraw_strategy_success").await;
        let uid = "withdraw_strategy_uid_s";
        ApiWithdrawStrategyRepo::upsert(&pool, make_strategy(uid, 30)).await.unwrap();

        let got = ApiWithdrawStrategyRepo::get_by_uid(&pool, uid).await.unwrap();
        assert!(got.is_some());
        let got = got.unwrap();
        assert_eq!(got.uid, uid);
        assert_eq!(got.threshold, 30);

        let list = ApiWithdrawStrategyRepo::list_api_withdraw_strategy(&pool).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].uid, uid);

        let (count, rows) =
            ApiWithdrawStrategyRepo::page_api_withdraw_strategy(&pool, 0, 10).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].uid, uid);
    }

    #[tokio::test]
    async fn withdraw_strategy_repo_missing_uid_returns_none() {
        let pool = setup_api_wallet_pool("wallet_db_withdraw_strategy_edge").await;
        let got = ApiWithdrawStrategyRepo::get_by_uid(&pool, "withdraw_strategy_uid_missing")
            .await
            .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn withdraw_strategy_repo_tx_rollback_keeps_threshold_unchanged() {
        let pool = setup_api_wallet_pool("wallet_db_withdraw_strategy_rollback").await;
        let uid = "withdraw_strategy_uid_rb";
        ApiWithdrawStrategyRepo::upsert(&pool, make_strategy(uid, 15)).await.unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        ApiWithdrawStrategyDao::upsert(tx.as_mut(), make_strategy(uid, 77)).await.unwrap();
        tx.rollback().await.unwrap();

        let got = ApiWithdrawStrategyRepo::get_by_uid(&pool, uid).await.unwrap().unwrap();
        assert_eq!(got.threshold, 15);

        let (count, rows) =
            ApiWithdrawStrategyRepo::page_api_withdraw_strategy(&pool, 0, 10).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].threshold, 15);
    }

    #[tokio::test]
    async fn withdraw_strategy_repo_page_zero_is_first_page() {
        let pool = setup_api_wallet_pool("wallet_db_withdraw_strategy_page_zero").await;

        for idx in 0..12i32 {
            let uid = format!("withdraw_strategy_uid_{idx:02}");
            ApiWithdrawStrategyRepo::upsert(&pool, make_strategy(&uid, idx)).await.unwrap();
        }

        let (count, first_page) =
            ApiWithdrawStrategyRepo::page_api_withdraw_strategy(&pool, 0, 10).await.unwrap();
        assert_eq!(count, 12);
        assert_eq!(first_page.len(), 10);

        let (count, second_page) =
            ApiWithdrawStrategyRepo::page_api_withdraw_strategy(&pool, 1, 10).await.unwrap();
        assert_eq!(count, 12);
        assert_eq!(second_page.len(), 2);
    }
}
