use sqlx::{Executor, Sqlite};

use crate::entities::task_queue::{CreateTaskQueueEntity, TaskQueueEntity};

impl TaskQueueEntity {
    pub async fn upsert_multi_task<'a, E>(
        exec: E,
        reqs: &[CreateTaskQueueEntity],
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let time = sqlx::types::chrono::Utc::now().timestamp();

        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "insert into task_queue (id, task_name, request_body, type, status, created_at, updated_at) ",
        );
        query_builder.push_values(reqs, |mut b, req| {
            b.push_bind(req.id.clone())
                .push_bind(req.task_name)
                .push_bind(req.request_body.clone().unwrap_or_default())
                .push_bind(req.r#type)
                .push_bind(req.status)
                .push_bind(time)
                .push_bind(time);
        });

        query_builder.push(
            " on conflict (id) do update set updated_at = excluded.updated_at
            RETURNING *",
        );

        let query = query_builder.build_query_as::<TaskQueueEntity>();

        query
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn upsert<'a, E>(exec: E, req: CreateTaskQueueEntity) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = "INSERT INTO task_queue (id, task_name, request_body, type, status, created_at, updated_at)
            VALUES
            (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT (id) DO UPDATE SET updated_at = excluded.updated_at";
        sqlx::query(sql)
            .bind(req.id)
            .bind(req.task_name)
            .bind(req.request_body)
            .bind(req.r#type)
            .bind(req.status)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_status<'a, E>(exec: E, id: &str, status: u8) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE task_queue SET status = ? WHERE id = ?";
        sqlx::query(sql)
            .bind(status)
            .bind(id)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete<'a, E>(exec: E, id: &str) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "DELETE FROM task_queue WHERE id = ?";

        sqlx::query(sql)
            .bind(id)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_task_queue<'a, E>(exec: E, id: &str) -> Result<Option<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = "SELECT * FROM task_queue WHERE id = ?";

        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(id)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list<'a, E>(exec: E, status: u8) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = "SELECT * FROM task_queue WHERE status = ?";
        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(status)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
