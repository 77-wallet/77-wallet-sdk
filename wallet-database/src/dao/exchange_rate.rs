use crate::entities::exchange_rate::ExchangeRateEntity;
use sqlx::{Executor, Sqlite};

impl ExchangeRateEntity {
    pub async fn upsert<'a, E>(
        exec: E,
        target_currency: &str,
        name: &str,
        rate: f64,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            insert into exchange_rate (target_currency, name, rate, created_at, updated_at)
            values ($1, $2, $3, $4, $5)
            on conflict (target_currency)
            do update set
                rate = $3,
                updated_at = $5
            RETURNING *
            "#;
        let time = sqlx::types::chrono::Utc::now().timestamp();
        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(target_currency)
            .bind(name)
            .bind(rate)
            .bind(time)
            .bind(time)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_rate<'a, E>(
        exec: E,
        target_currency: &str,
        rate: f64,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update exchange_rate set
                rate = $2,
                updated_at = $3
            where target_currency = $1
            RETURNING *
            "#;
        let time = sqlx::types::chrono::Utc::now().timestamp();
        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(target_currency)
            .bind(rate)
            .bind(time)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list<'a, E>(exec: E) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM exchange_rate;";

        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_by_multi_exchange_rate_id<'a, E>(
        exec: E,
        target_currency: Vec<String>,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if target_currency.is_empty() {
            return Ok(vec![]);
        }

        // 动态生成 SQL 查询语句
        let mut query = String::from("SELECT * FROM exchange_rate WHERE ");
        let mut binds = Vec::new();

        for (i, id) in target_currency.into_iter().enumerate() {
            if i > 0 {
                query.push_str(" OR ");
            }
            query.push_str("(currency = ? )");
            binds.push(id);
        }

        // 构建 SQL 查询并绑定参数
        let mut query = sqlx::query_as::<sqlx::Sqlite, Self>(&query);
        for bind in binds {
            query = query.bind(bind);
        }

        // 执行查询
        query
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
