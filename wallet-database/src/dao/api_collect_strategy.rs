use crate::entities::api_collect_strategy::ApiCollectStrategyEntity;
use sqlx::{Executor, Sqlite};

pub(crate) struct ApiCollectStrategyDao;

impl ApiCollectStrategyDao {
    pub async fn all_api_collect_strategy<'a, E>(
        exec: E,
    ) -> Result<Vec<ApiCollectStrategyEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"SELECT * FROM api_collect_strategy"#;
        let result = sqlx::query_as::<_, ApiCollectStrategyEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    pub(crate) async fn page_api_collect_strategy<'a, E>(
        exec: E,
        page: i64,
        page_size: i64,
    ) -> Result<(i64, Vec<ApiCollectStrategyEntity>), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + Clone,
    {
        let count_sql = "SELECT count(*) FROM api_collect_strategy";
        let count = sqlx::query_scalar::<_, i64>(count_sql)
            .fetch_one(exec.clone())
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        let sql = "SELECT * FROM api_collect_strategy ORDER BY created_at DESC LIMIT ? OFFSET ?";
        let offset = page * page_size;
        let res = sqlx::query_as::<_, ApiCollectStrategyEntity>(sql)
            .bind(page_size)
            .bind(offset)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok((count, res))
    }

    pub(crate) async fn upsert<'c, E>(
        executor: E,
        input: ApiCollectStrategyEntity,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        let sql = r#"
            Insert into api_collect_strategy
                (uid, threshold, created_at, updated_at)
            values
                ($1, $2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            on conflict (uid)
            do update set
                threshold = excluded.threshold,
                updated_at = excluded.updated_at
            returning *
        "#;

        sqlx::query_as::<_, ApiCollectStrategyEntity>(sql)
            .bind(&input.uid)
            .bind(&input.threshold)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

    pub(crate) async fn get_by_uid<'a, E>(
        exec: E,
        uid: &str,
    ) -> Result<Option<ApiCollectStrategyEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"SELECT * FROM api_collect_strategy WHERE uid = ?"#;
        let result = sqlx::query_as::<_, ApiCollectStrategyEntity>(sql)
            .bind(uid)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    pub(crate) async fn delete<'a, E>(exec: E, uid: &str) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"DELETE FROM api_collect_strategy WHERE uid = ?"#;
        sqlx::query(sql)
            .bind(uid)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }
}
