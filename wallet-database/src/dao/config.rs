use crate::{
    entities::config::ConfigEntity,
    sql_utils::{SqlExecutableReturn as _, query_builder::DynamicQueryBuilder},
};
use sqlx::{Executor, Sqlite};

pub struct ConfigDao;

impl ConfigDao {
    pub async fn upsert<'a, E>(
        key: &str,
        value: &str,
        types: Option<i8>,
        exec: E,
    ) -> Result<ConfigEntity, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            INSERT INTO config (key,value,types,created_at,updated_at)
            VALUES (?,?,?,strftime('%Y-%m-%dT%H:%M:%SZ','now'),strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            on conflict (key) do update set value = EXCLUDED.value
            RETURNING *
        "#;

        let mut res = sqlx::query_as::<_, ConfigEntity>(sql)
            .bind(key)
            .bind(value)
            .bind(types.unwrap_or_default())
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.pop().ok_or(crate::DatabaseError::ReturningNone)?)
    }

    pub async fn list_v2<'a, E>(executor: E) -> Result<Vec<ConfigEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let builder = DynamicQueryBuilder::new("SELECT * FROM config where types = 0");
        builder.fetch_all(executor).await
    }

    pub async fn lists<'a, E>(exec: E) -> Result<Vec<ConfigEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from config where types = 0";

        let res = sqlx::query_as::<_, ConfigEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res)
    }

    pub async fn find_by_key<'a, E>(
        key: &str,
        exec: E,
    ) -> Result<Option<ConfigEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from config where key = ?";

        let res = sqlx::query_as::<_, ConfigEntity>(sql)
            .bind(key)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res)
    }
}
