use crate::entities::api_collect_strategy_chain_config::ApiCollectStrategyChainConfigEntity;
use sqlx::{Executor, Sqlite};

pub(crate) struct ApiCollectStrategyChainConfigDao;

impl ApiCollectStrategyChainConfigDao {
    pub(crate) async fn upsert<'c, E>(
        executor: E,
        input: ApiCollectStrategyChainConfigEntity,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        let sql = r#"
            INSERT INTO api_collect_strategy_chain_config
                (strategy_id, chain_code, chain_address_type, normal_idx, normal_address, risk_idx, risk_address, created_at, updated_at)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT (strategy_id, chain_code)
            DO UPDATE SET
                chain_address_type = excluded.chain_address_type,
                normal_idx = excluded.normal_idx,
                normal_address = excluded.normal_address,
                risk_idx = excluded.risk_idx,
                risk_address = excluded.risk_address,
                updated_at = excluded.updated_at
            RETURNING *
        "#;

        sqlx::query_as::<_, ApiCollectStrategyChainConfigEntity>(sql)
            .bind(&input.strategy_id)
            .bind(&input.chain_code)
            .bind(&input.chain_address_type)
            .bind(&input.normal_idx)
            .bind(&input.normal_address)
            .bind(&input.risk_idx)
            .bind(&input.risk_address)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

    pub(crate) async fn get_chain_configs_by_strategy_id<'a, E>(
        exec: E,
        strategy_id: i64,
    ) -> Result<Vec<ApiCollectStrategyChainConfigEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"SELECT * FROM api_collect_strategy_chain_config WHERE strategy_id = ?"#;
        let result = sqlx::query_as::<_, ApiCollectStrategyChainConfigEntity>(sql)
            .bind(strategy_id)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    pub(crate) async fn delete_chain_configs_by_strategy_id<'a, E>(
        exec: E,
        strategy_id: i64,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"DELETE FROM api_collect_strategy_chain_config WHERE strategy_id = ?"#;
        sqlx::query(sql)
            .bind(strategy_id)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

    pub(crate) async fn delete_chain_config<'a, E>(
        exec: E,
        strategy_id: i64,
        chain_code: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"DELETE FROM api_collect_strategy_chain_config WHERE strategy_id = ? AND chain_code = ?"#;
        sqlx::query(sql)
            .bind(strategy_id)
            .bind(chain_code)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }
}
