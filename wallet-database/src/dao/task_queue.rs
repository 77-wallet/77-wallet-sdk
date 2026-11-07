use sqlx::{Executor, Sqlite};

use crate::{
    entities::task_queue::{CreateTaskQueueEntity, TaskName, TaskQueueEntity},
    sql_utils::{
        SqlExecutableNoReturn, SqlExecutableReturn, delete_builder::DynamicDeleteBuilder,
        query_builder::DynamicQueryBuilder, update_builder::DynamicUpdateBuilder,
    },
};

// SQLite is the routing authority for both stored rows and dispatch metadata.
// json_extract/json_type use the first decoded matching key for duplicate owner keys.
const EXTERNAL_OWNER_PREDICATE: &str = "COALESCE(CASE WHEN json_valid(remark) THEN CASE WHEN json_type(remark,'$.owner')='text' THEN json_extract(remark,'$.owner') END END,'') != ''";

pub struct TaskQueueDao {}

impl TaskQueueDao {
    /// Atomic admission enforces the caller-owned active capacity.
    pub async fn insert_owned_with_capacity<'a, E>(
        exec: E,
        row: &TaskQueueEntity,
        owner: &str,
        capacity: i64,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query("INSERT INTO task_queue (id,task_name,request_body,type,status,err_msg,remark,created_at,updated_at) SELECT ?,?,?,?,?,?,?,?,? WHERE (SELECT COUNT(*) FROM task_queue WHERE status IN (0,1) AND CASE WHEN json_valid(remark) THEN json_extract(remark,'$.owner') END = ?) < ? ON CONFLICT(id) DO NOTHING")
            .bind(&row.id).bind(&row.task_name).bind(&row.request_body).bind(row.r#type).bind(row.status).bind(&row.err_msg).bind(&row.remark).bind(row.created_at).bind(row.updated_at).bind(owner).bind(capacity)
            .execute(exec).await.map(|x|x.rows_affected()).map_err(|e|crate::Error::Database(e.into()))
    }

    /// Classify metadata with exactly the same JSON domain as recovery and cleanup.
    pub async fn externally_owned<'a, E>(
        exec: E,
        remark: Option<&str>,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if remark.is_none() {
            return Ok(false);
        }
        let sql = format!("SELECT {EXTERNAL_OWNER_PREDICATE} FROM (SELECT ? AS remark)");
        sqlx::query_scalar::<_, bool>(&sql)
            .bind(remark)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// Default consumer excludes every explicitly owned recovery record.
    pub async fn recoverable_default<'a, E>(exec: E) -> Result<Vec<TaskQueueEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = format!(
            "SELECT id,task_name,request_body,type,status,err_msg,remark,created_at,updated_at FROM task_queue WHERE status IN (0,1,3,4) AND NOT ({EXTERNAL_OWNER_PREDICATE}) ORDER BY created_at,id"
        );
        sqlx::query_as::<_, TaskQueueEntity>(&sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn defer_owned<'a, E>(
        exec: E,
        id: &str,
        owner: &str,
        remark: &str,
        error: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query("UPDATE task_queue SET status=3, remark=?, err_msg=?, retry_times=MIN(retry_times+1,2147483647), updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=? AND status != 2 AND CASE WHEN json_valid(remark) THEN json_extract(remark,'$.owner') END = ?")
            .bind(remark).bind(error).bind(id).bind(owner).execute(exec).await.map(|x|x.rows_affected()).map_err(|e|crate::Error::Database(e.into()))
    }

    pub async fn rearm_owned_done<'a, E>(
        exec: E,
        id: &str,
        owner: &str,
        task_name: TaskName,
        remark: &str,
        status: u8,
        capacity: i64,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query("UPDATE task_queue SET status=?,remark=?,err_msg=NULL,retry_times=0,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=? AND status=2 AND task_name=? AND CASE WHEN json_valid(remark) THEN json_extract(remark,'$.owner') END = ? AND (?=3 OR (SELECT COUNT(*) FROM task_queue WHERE status IN (0,1) AND CASE WHEN json_valid(remark) THEN json_extract(remark,'$.owner') END = ?) < ?)")
            .bind(status).bind(remark).bind(id).bind(task_name).bind(owner).bind(status).bind(owner).bind(capacity).execute(exec).await.map(|x|x.rows_affected()).map_err(|e|crate::Error::Database(e.into()))
    }

    pub async fn claim_owned_running<'a, E>(
        exec: E,
        id: &str,
        owner: &str,
        capacity: i64,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query("UPDATE task_queue SET status=1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=? AND status IN (0,1,3,4) AND CASE WHEN json_valid(remark) THEN json_extract(remark,'$.owner') END = ? AND (status IN (0,1) OR (SELECT COUNT(*) FROM task_queue WHERE status IN (0,1) AND CASE WHEN json_valid(remark) THEN json_extract(remark,'$.owner') END = ?) < ?)")
            .bind(id).bind(owner).bind(owner).bind(capacity).execute(exec).await.map(|x|x.rows_affected()).map_err(|e|crate::Error::Database(e.into()))
    }

    pub async fn active_for_owner<'a, E>(exec: E, owner: &str) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_scalar("SELECT COUNT(*) FROM task_queue WHERE status IN (0,1) AND CASE WHEN json_valid(remark) THEN json_extract(remark,'$.owner') END = ?")
            .bind(owner).fetch_one(exec).await.map_err(|e|crate::Error::Database(e.into()))
    }

    pub async fn list_legacy_core<'a, E>(exec: E) -> Result<Vec<TaskQueueEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        // Old core_db.task_queue rows did not persist err_msg/remark. Fill them with NULLs
        // so the one-time migration into task.db can keep using the current entity shape.
        let sql = "SELECT id, task_name, request_body, type, status, NULL as err_msg, NULL as remark, created_at, updated_at FROM task_queue";
        sqlx::query_as::<sqlx::Sqlite, TaskQueueEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 批量插入task_queue记录
    pub async fn insert_batch<'a, E>(exec: E, tasks: &[TaskQueueEntity]) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if tasks.is_empty() {
            return Ok(());
        }

        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "insert into task_queue (id, task_name, request_body, type, status, err_msg, remark, created_at, updated_at) ",
        );
        query_builder.push_values(tasks, |mut b, task| {
            b.push_bind(task.id.clone())
                .push_bind(task.task_name.clone())
                .push_bind(task.request_body.clone())
                .push_bind(task.r#type)
                .push_bind(task.status)
                .push_bind(task.err_msg.clone())
                .push_bind(task.remark.clone())
                .push_bind(task.created_at)
                .push_bind(task.updated_at);
        });

        let query = query_builder.build();
        query.execute(exec).await.map(|_| ()).map_err(|e| crate::Error::Database(e.into()))
    }

    /// 批量插入task_queue记录，忽略冲突
    pub async fn insert_batch_ignore_conflict<'a, E>(
        exec: E,
        tasks: &[TaskQueueEntity],
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if tasks.is_empty() {
            return Ok(());
        }

        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "insert into task_queue (id, task_name, request_body, type, status, err_msg, remark, created_at, updated_at) ",
        );
        query_builder.push_values(tasks, |mut b, task| {
            b.push_bind(task.id.clone())
                .push_bind(task.task_name.clone())
                .push_bind(task.request_body.clone())
                .push_bind(task.r#type)
                .push_bind(task.status)
                .push_bind(task.err_msg.clone())
                .push_bind(task.remark.clone())
                .push_bind(task.created_at)
                .push_bind(task.updated_at);
        });
        query_builder.push(" ON CONFLICT (id) DO NOTHING");

        let query = query_builder.build();
        query.execute(exec).await.map(|_| ()).map_err(|e| crate::Error::Database(e.into()))
    }

    /// 检查表是否存在
    pub async fn table_exists<'a, E>(exec: E, table_name: &str) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT name FROM sqlite_master WHERE type='table' AND name = ?";
        let result = sqlx::query_scalar::<_, String>(sql)
            .bind(table_name)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result.is_some())
    }

    /// 单个CreateTaskQueueEntity的upsert
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

    /// 多个CreateTaskQueueEntity的upsert
    pub async fn upsert_multi_task<'a, E>(
        exec: E,
        reqs: &[CreateTaskQueueEntity],
    ) -> Result<Vec<TaskQueueEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "insert into task_queue (id, task_name, request_body, type, status, created_at, updated_at) ",
        );
        query_builder.push_values(reqs, |mut b, req| {
            b.push_bind(req.id.clone())
                .push_bind(req.task_name.clone())
                .push_bind(req.request_body.clone().unwrap_or_default())
                .push_bind(req.r#type)
                .push_bind(req.status)
                .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
                .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')");
        });

        query_builder.push(
            " on conflict (id) do update set updated_at = excluded.updated_at
            RETURNING *",
        );

        let query = query_builder.build_query_as::<TaskQueueEntity>();

        query.fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))
    }

    /// 根据状态和类型查询任务
    pub async fn list<'a, E>(
        exec: E,
        status: Option<u8>,
        typ: Option<u8>,
    ) -> Result<Vec<TaskQueueEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let mut builder = DynamicQueryBuilder::new("SELECT * FROM task_queue");
        if let Some(status) = status {
            builder = builder.and_where_eq("status", status);
        }
        if let Some(typ) = typ {
            builder = builder.and_where_eq("type", typ);
        }
        builder.fetch_all(exec).await
    }

    /// 获取所有task_queue记录
    pub async fn get_all<'a, E>(exec: E) -> Result<Vec<TaskQueueEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT id, task_name, request_body, type, status, err_msg, remark, created_at, updated_at FROM task_queue";
        sqlx::query_as::<sqlx::Sqlite, TaskQueueEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 根据ID获取单个task_queue记录
    pub async fn get_task_queue<'a, E>(
        exec: E,
        id: &str,
    ) -> Result<Option<TaskQueueEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = "SELECT * FROM task_queue WHERE id = ?";
        sqlx::query_as::<sqlx::Sqlite, TaskQueueEntity>(sql)
            .bind(id)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 更新task_queue状态
    pub async fn update_status<'a, E>(exec: E, id: &str, status: u8) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE task_queue SET status = ?,
                            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') 
                        WHERE id = ?";
        sqlx::query(sql)
            .bind(status)
            .bind(id)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 更新task_queue为失败状态
    pub async fn task_failed<'a, E>(
        exec: E,
        id: &str,
        status: u8,
        err_msg: &str,
    ) -> Result<Vec<TaskQueueEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicUpdateBuilder::new("task_queue")
            .set("status", status)
            .set("err_msg", err_msg)
            .set_raw("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
            .and_where_eq("id", id)
            .returning("*")
            .fetch_all(exec)
            .await
    }

    /// 根据task_name和status获取单个任务
    pub async fn get_task_with_task_name<'a, E>(
        exec: E,
        task_name: TaskName,
        status: &[u8],
    ) -> Result<Option<TaskQueueEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let builder = DynamicQueryBuilder::new("SELECT * FROM task_queue");
        builder
            .and_where_eq("task_name", task_name)
            .and_where_in("status", status)
            .fetch_optional(exec)
            .await
    }

    /// 根据task_name和status获取任务列表
    pub async fn list_tasks_with_task_name<'a, E>(
        exec: E,
        task_name: TaskName,
        status: &[u8],
    ) -> Result<Vec<TaskQueueEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let builder = DynamicQueryBuilder::new("SELECT * FROM task_queue");
        builder
            .and_where_eq("task_name", task_name)
            .and_where_in("status", status)
            .fetch_all(exec)
            .await
    }

    /// 根据request_body关键词和status获取任务列表
    pub async fn get_tasks_with_request_body<'a, E>(
        exec: E,
        keyword: &str,
        status: &[u8],
    ) -> Result<Vec<TaskQueueEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let builder = DynamicQueryBuilder::new("SELECT * FROM task_queue");
        builder
            .and_where_like("request_body", keyword)
            .and_where_in("status", status)
            .fetch_all(exec)
            .await
    }

    /// 根据task_name、request_body关键词和status获取任务列表
    pub async fn get_tasks_with_request_body_and_task_name<'a, E>(
        exec: E,
        task_name: TaskName,
        keyword: &str,
        status: &[u8],
    ) -> Result<Vec<TaskQueueEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let builder = DynamicQueryBuilder::new("SELECT * FROM task_queue");
        builder
            .and_where_eq("task_name", task_name)
            .and_where_like("request_body", keyword)
            .and_where_in("status", status)
            .fetch_all(exec)
            .await
    }

    /// 更新任务备注
    pub async fn update_task_remark<'a, E>(
        exec: E,
        id: &str,
        remark: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let builder =
            DynamicUpdateBuilder::new("task_queue").set("remark", remark).and_where_eq("id", id);
        SqlExecutableNoReturn::execute(builder, exec).await
    }

    /// 增加重试次数
    pub async fn increase_retry_times<'a, E>(exec: E, id: &str) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE task_queue SET retry_times = retry_times + 1,
                            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                        WHERE id = ?";
        sqlx::query(sql)
            .bind(id)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 删除相关操作
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

    /// Deletes completed rows for one exact task name.
    pub async fn delete_done_by_task_name<'a, E>(
        exec: E,
        task_name: TaskName,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query("DELETE FROM task_queue WHERE task_name = ? AND status = 2")
            .bind(task_name)
            .execute(exec)
            .await
            .map(|result| result.rows_affected())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete_old<'a, E>(exec: E, day: u16) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = format!(
            "DELETE FROM task_queue WHERE julianday('now')-julianday(created_at)>? AND NOT (status IN (0,1,3,4) AND ({EXTERNAL_OWNER_PREDICATE}))"
        );
        sqlx::query(&sql)
            .bind(day)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete_oldest_by_status_when_exceeded<'a, E>(
        exec: &E,
        max_size: u32,
        target_status: u8,
    ) -> Result<(), crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_queue")
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        let max_size = max_size as i64;
        if count <= max_size {
            return Ok(());
        }

        let over_count = count - max_size;
        let sql = "DELETE FROM task_queue WHERE id IN (SELECT id FROM task_queue WHERE status = ? ORDER BY created_at ASC LIMIT ?)";
        sqlx::query(sql)
            .bind(target_status)
            .bind(over_count)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 根据类型删除所有任务
    pub async fn delete_all<'a, E>(exec: E, typ: Option<u8>) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut sql = "DELETE FROM task_queue".to_string();
        let mut conditions = Vec::new();
        if let Some(_typ) = typ {
            conditions.push("type = ?".to_string());
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        let mut query = sqlx::query(&sql);

        if let Some(typ) = typ {
            query = query.bind(typ);
        }

        query.execute(exec).await.map(|_| ()).map_err(|e| crate::Error::Database(e.into()))
    }

    /// 根据request_body关键词删除任务
    pub async fn delete_tasks_with_request_body_like<'a, E>(
        exec: E,
        keyword: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let builder =
            DynamicDeleteBuilder::new("task_queue").and_where_like("request_body", keyword);
        SqlExecutableNoReturn::execute(builder, exec).await
    }

    /// 统计相关操作
    pub async fn count<'a, E>(exec: E) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT COUNT(*) FROM task_queue";
        sqlx::query_scalar(sql).fetch_one(exec).await.map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn has_unfinished_task<'a, E>(exec: E) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT EXISTS(SELECT 1 FROM task_queue WHERE status != 2)";

        let exists: i64 = sqlx::query_scalar(sql)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(exists == 1)
    }

    pub async fn has_unfinished_task_by_type<'a, E>(exec: E, typ: u8) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT EXISTS(SELECT 1 FROM task_queue WHERE status != 2 AND type = ?)";

        let exists: i64 = sqlx::query_scalar(sql)
            .bind(typ)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(exists == 1)
    }

    /// 将task_queue表重命名为task_queue_legacy（冻结旧表）
    pub async fn freeze_table<'a, E>(exec: E) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "ALTER TABLE task_queue RENAME TO task_queue_legacy";
        sqlx::query(sql)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::TaskQueueDao;
    use crate::entities::task_queue::{KnownTaskName, TaskName};

    #[tokio::test]
    async fn delete_done_by_task_name_preserves_other_names_and_statuses() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open in-memory task database");
        sqlx::query(
            "CREATE TABLE task_queue (
                id TEXT PRIMARY KEY,
                task_name TEXT NOT NULL,
                request_body TEXT NOT NULL,
                type INTEGER NOT NULL,
                retry_times INTEGER NOT NULL DEFAULT 0,
                status INTEGER NOT NULL,
                err_msg TEXT,
                remark TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT
            )",
        )
        .execute(&pool)
        .await
        .expect("create task_queue");

        for (id, task_name, status) in [
            ("done-recovery", TaskName::Known(KnownTaskName::CreateApiAccountDeferred), 2),
            ("pending-recovery", TaskName::Known(KnownTaskName::CreateApiAccountDeferred), 0),
            ("failed-recovery", TaskName::Known(KnownTaskName::CreateApiAccountDeferred), 3),
            ("done-mqtt", TaskName::Known(KnownTaskName::ApiWalletAcctChange), 2),
        ] {
            sqlx::query(
                "INSERT INTO task_queue
                 (id, task_name, request_body, type, status, created_at)
                 VALUES (?, ?, 'large-body', 1, ?, '2026-08-25T00:00:00Z')",
            )
            .bind(id)
            .bind(task_name)
            .bind(status)
            .execute(&pool)
            .await
            .expect("insert task fixture");
        }

        let deleted = TaskQueueDao::delete_done_by_task_name(
            &pool,
            TaskName::Known(KnownTaskName::CreateApiAccountDeferred),
        )
        .await
        .expect("delete completed recovery tasks");

        assert_eq!(deleted, 1);
        let remaining: Vec<String> = sqlx::query_scalar("SELECT id FROM task_queue ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("list remaining tasks");
        assert_eq!(remaining, vec!["done-mqtt", "failed-recovery", "pending-recovery"]);
    }
    #[tokio::test]
    async fn age_cleanup_preserves_unfinished_externally_owned_tasks() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE task_queue (id TEXT PRIMARY KEY, task_name TEXT NOT NULL, request_body TEXT NOT NULL, type INTEGER NOT NULL, status INTEGER NOT NULL, err_msg TEXT, remark TEXT, created_at TEXT NOT NULL, updated_at TEXT)")
            .execute(&pool).await.unwrap();
        let owned = r#"{"owner":"independent_consumer","attempts":12,"next_retry_at_ms":0}"#;
        for (id, status, remark) in [
            ("a-pending", 0, owned),
            ("a-running", 1, owned),
            ("a-failed", 3, owned),
            ("a-hanging", 4, owned),
            ("a-done", 2, owned),
            ("normal-failed", 3, "ordinary remark"),
            ("other-owner", 0, r#"{"owner":"other"}"#),
            ("numeric-owner", 0, r#"{"owner":123}"#),
        ] {
            sqlx::query("INSERT INTO task_queue (id, task_name, request_body, type, status, remark, created_at) VALUES (?, 'BackendApi', '{}', 1, ?, ?, '1970-01-01T00:00:00Z')")
                .bind(id).bind(status).bind(remark).execute(&pool).await.unwrap();
        }
        TaskQueueDao::delete_old(&pool, 15).await.unwrap();
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM task_queue ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(ids, vec!["a-failed", "a-hanging", "a-pending", "a-running", "other-owner"]);
    }
}
