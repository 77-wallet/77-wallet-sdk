use sqlx::{Executor, Sqlite};

use crate::{
    entities::system_notification::{CreateSystemNotificationEntity, SystemNotificationEntity},
    error::database::DatabaseError,
    pagination::Pagination,
};

impl SystemNotificationEntity {
    pub async fn upsert<'a, E>(
        exec: E,
        id: &str,
        r#type: &str,
        content: String,
        status: i8,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "INSERT INTO system_notification (id, type, content, status, created_at, updated_at)
            VALUES (?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT(id) DO UPDATE SET type = excluded.type, 
            content = excluded.content, status = excluded.status, 
            updated_at = excluded.updated_at
            ";
        // let id = wallet_utils::snowflake::get_uid()?.to_string();
        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(id)
            .bind(r#type)
            .bind(content)
            .bind(status)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn upsert_with_key_value<'a, E>(
        exec: E,
        id: &str,
        r#type: &str,
        content: String,
        status: i8,
        key: Option<String>,
        value: Option<String>,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "INSERT INTO system_notification (id, type, content, status, key, value, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT(id) DO UPDATE SET type = excluded.type,
            content = excluded.content, status = excluded.status,
            updated_at = excluded.updated_at
            ";
        // let id = wallet_utils::snowflake::get_uid()?.to_string();
        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(id)
            .bind(r#type)
            .bind(content)
            .bind(status)
            .bind(key)
            .bind(value)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn upsert_multi_with_key_value<'a, E>(
        exec: E,
        reqs: &[CreateSystemNotificationEntity],
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if reqs.is_empty() {
            return Ok(());
        }

        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "INSERT INTO system_notification (id, type, content, status, key, value, created_at, updated_at) ",
        );

        query_builder.push_values(reqs, |mut b, req| {
            b.push_bind(&req.id)
                .push_bind(&req.r#type)
                .push_bind(&req.content)
                .push_bind(req.status)
                .push_bind(&req.key)
                .push_bind(&req.value)
                .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
                .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')");
        });

        query_builder.push(
            " ON CONFLICT(id) DO UPDATE SET type = excluded.type,
            content = excluded.content, status = excluded.status,
            updated_at = excluded.updated_at
            ",
        );

        let query = query_builder.build();

        query
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn system_notification_list_page<'a, E>(
        tx: &E,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<Self>, crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        let sql = "SELECT *
            FROM system_notification
            ORDER BY created_at DESC"
            .to_string();

        // 执行查询并返回结果
        let paginate = Pagination::<SystemNotificationEntity>::init(page, page_size);
        Ok(paginate.page(tx, &sql).await?)
    }

    pub async fn update_system_notification_status<'a, E>(
        tx: E,
        id: Option<String>,
        status: i8,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = if id.is_some() {
            r#"UPDATE system_notification SET status = $1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = $2;"#
        } else {
            r#"UPDATE system_notification SET status = $1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now');"#
        };
        let mut query = sqlx::query(sql).bind(status);

        if let Some(id_value) = id {
            query = query.bind(id_value);
        }

        query
            .execute(tx)
            .await
            .map(|_| ())
            .map_err(|_| crate::Error::Database(DatabaseError::UpdateFailed))
    }

    pub async fn count_status_zero<'a, E>(exec: E) -> Result<i64, crate::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
    {
        let sql = "SELECT COUNT(*) FROM system_notification WHERE status = 0;";
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete_system_notification<'a, E>(exec: E, id: String) -> Result<(), crate::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
    {
        let sql = "DELETE FROM system_notification WHERE id = $1";
        sqlx::query(sql)
            .bind(id)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete_system_notification_before_date<'a, E>(
        exec: E,
        date: String,
    ) -> Result<(), crate::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
    {
        let sql = "DELETE FROM system_notification WHERE created_at < $1";
        sqlx::query(sql)
            .bind(date)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
