use crate::{
    CoreDbPool, TaskDbPool,
    dao::task_queue::TaskQueueDao,
    entities::task_queue::{CreateTaskQueueEntity, TaskName, TaskQueueEntity},
};

pub struct TaskQueueRepo {}

impl TaskQueueRepo {
    pub async fn replace_owned_with_children(
        pool: &TaskDbPool,
        id: &str,
        owner: &str,
        remark: &str,
        children: &[TaskQueueEntity],
        capacity: i64,
    ) -> Result<bool, crate::Error> {
        let mut tx =
            pool.write_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        // Conservative reservation for every child; an existing child needs no extra slot.
        let changed=sqlx::query("UPDATE task_queue SET status=2,remark=?,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=? AND status != 2 AND CASE WHEN json_valid(remark) THEN json_extract(remark,'$.owner') END = ? AND (SELECT COUNT(*) FROM task_queue WHERE status IN (0,1) AND CASE WHEN json_valid(remark) THEN json_extract(remark,'$.owner') END = ?) + ? - CASE WHEN status IN (0,1) THEN 1 ELSE 0 END <= ?")
            .bind(remark).bind(id).bind(owner).bind(owner).bind(children.len() as i64).bind(capacity).execute(&mut *tx).await.map_err(|e|crate::Error::Database(e.into()))?.rows_affected();
        if changed == 1 {
            for child in children {
                TaskQueueDao::insert_batch_ignore_conflict(&mut *tx, std::slice::from_ref(child))
                    .await?;
                TaskQueueDao::rearm_owned_done(
                    &mut *tx,
                    &child.id,
                    owner,
                    child.task_name.clone(),
                    child.remark.as_deref().unwrap_or(""),
                    child.status,
                    capacity,
                )
                .await?;
            }
        }
        tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(changed == 1)
    }

    pub async fn claim_owned_running(
        pool: &TaskDbPool,
        id: &str,
        owner: &str,
        capacity: i64,
    ) -> Result<bool, crate::Error> {
        Ok(TaskQueueDao::claim_owned_running(pool.write_ref(), id, owner, capacity).await? == 1)
    }

    pub async fn insert_owned_with_capacity(
        pool: &TaskDbPool,
        row: &TaskQueueEntity,
        owner: &str,
        capacity: i64,
    ) -> Result<u64, crate::Error> {
        TaskQueueDao::insert_owned_with_capacity(pool.write_ref(), row, owner, capacity).await
    }

    pub async fn externally_owned(
        pool: &TaskDbPool,
        remark: Option<&str>,
    ) -> Result<bool, crate::Error> {
        TaskQueueDao::externally_owned(pool.read_ref(), remark).await
    }

    pub async fn recoverable_default(
        pool: &TaskDbPool,
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        TaskQueueDao::recoverable_default(pool.read_ref()).await
    }
    pub async fn defer_owned(
        pool: &TaskDbPool,
        id: &str,
        owner: &str,
        remark: &str,
        error: &str,
    ) -> Result<u64, crate::Error> {
        TaskQueueDao::defer_owned(pool.write_ref(), id, owner, remark, error).await
    }
    pub async fn rearm_owned_done(
        pool: &TaskDbPool,
        id: &str,
        owner: &str,
        task_name: TaskName,
        remark: &str,
        status: u8,
        capacity: i64,
    ) -> Result<u64, crate::Error> {
        TaskQueueDao::rearm_owned_done(
            pool.write_ref(),
            id,
            owner,
            task_name,
            remark,
            status,
            capacity,
        )
        .await
    }
    pub async fn active_for_owner(pool: &TaskDbPool, owner: &str) -> Result<i64, crate::Error> {
        TaskQueueDao::active_for_owner(pool.read_ref(), owner).await
    }

    pub fn build_backend_task(
        task_name: TaskName,
        request_body: Option<String>,
        remark: Option<String>,
    ) -> Result<CreateTaskQueueEntity, crate::Error> {
        CreateTaskQueueEntity::with_backend_request_string(task_name, request_body, remark)
    }

    pub fn build_mqtt_task(
        id: &str,
        task_name: TaskName,
        request_body: Option<String>,
        remark: Option<String>,
    ) -> Result<CreateTaskQueueEntity, crate::Error> {
        CreateTaskQueueEntity::with_mqtt_request_string(id, task_name, request_body, remark)
    }

    pub async fn create_multi_task(
        pool: &TaskDbPool,
        req: &[CreateTaskQueueEntity],
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::upsert_multi_task(pool.write_ref(), req).await?)
    }

    pub async fn create_task(
        pool: &TaskDbPool,
        req: CreateTaskQueueEntity,
    ) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::upsert(pool.write_ref(), req).await?)
    }

    pub async fn all_tasks_queue(pool: &TaskDbPool) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::list(pool.read_ref(), None, None).await?)
    }

    pub async fn all_tasks_queue_core(
        pool: &CoreDbPool,
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::list_legacy_core(pool.read_ref()).await?)
    }

    pub async fn task_failed(
        pool: &TaskDbPool,
        id: &str,
        err_msg: &str,
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::task_failed(pool.write_ref(), id, 3, err_msg).await?)
    }

    pub async fn task_detail(
        pool: &TaskDbPool,
        id: &str,
    ) -> Result<Option<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::get_task_queue(pool.read_ref(), id).await?)
    }

    pub async fn get_task_with_task_name(
        pool: &TaskDbPool,
        task_name: TaskName,
        status: &[u8],
    ) -> Result<Option<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::get_task_with_task_name(pool.read_ref(), task_name, status).await?)
    }

    pub async fn list_tasks_with_task_name(
        pool: &TaskDbPool,
        task_name: TaskName,
        status: &[u8],
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::list_tasks_with_task_name(pool.read_ref(), task_name, status).await?)
    }

    pub async fn get_tasks_with_request_body(
        pool: &TaskDbPool,
        keyword: &str,
        status: &[u8],
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::get_tasks_with_request_body(pool.read_ref(), keyword, status).await?)
    }

    pub async fn get_tasks_with_request_body_and_task_name(
        pool: &TaskDbPool,
        task_name: TaskName,
        keyword: &str,
        status: &[u8],
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::get_tasks_with_request_body_and_task_name(
            pool.read_ref(),
            task_name,
            keyword,
            status,
        )
        .await?)
    }

    pub async fn update_task_remark(
        pool: &TaskDbPool,
        id: &str,
        remark: &str,
    ) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::update_task_remark(pool.write_ref(), id, remark).await?)
    }

    pub async fn delete_task(pool: &TaskDbPool, id: &str) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::delete(pool.write_ref(), id).await?)
    }

    pub async fn delete_done_by_task_name(
        pool: &TaskDbPool,
        task_name: TaskName,
    ) -> Result<u64, crate::Error> {
        TaskQueueDao::delete_done_by_task_name(pool.write_ref(), task_name).await
    }

    pub async fn done_task_queue(pool: &TaskDbPool) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::list(pool.read_ref(), Some(2), None).await?)
    }

    pub async fn failed_task_queue(
        pool: &TaskDbPool,
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::list(pool.read_ref(), Some(3), None).await?)
    }

    pub async fn failed_mqtt_task_queue(
        pool: &TaskDbPool,
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::list(pool.read_ref(), Some(3), Some(2)).await?)
    }

    pub async fn running_task_queue(
        pool: &TaskDbPool,
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::list(pool.read_ref(), Some(1), None).await?)
    }

    pub async fn hanging_task_queue(
        pool: &TaskDbPool,
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::list(pool.read_ref(), Some(4), None).await?)
    }

    pub async fn pending_task_queue(
        pool: &TaskDbPool,
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        Ok(TaskQueueDao::list(pool.read_ref(), Some(0), None).await?)
    }

    pub async fn task_running(pool: &TaskDbPool, id: &str) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::update_status(pool.write_ref(), id, 1).await?)
    }

    pub async fn task_done(pool: &TaskDbPool, id: &str) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::update_status(pool.write_ref(), id, 2).await?)
    }

    pub async fn task_hang_up(pool: &TaskDbPool, id: &str) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::update_status(pool.write_ref(), id, 4).await?)
    }

    pub async fn increase_retry_times(pool: &TaskDbPool, id: &str) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::increase_retry_times(pool.write_ref(), id).await?)
    }

    pub async fn delete_old(pool: &TaskDbPool, day: u16) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::delete_old(pool.write_ref(), day).await?)
    }

    pub async fn delete_oldest_by_status_when_exceeded(
        pool: &TaskDbPool,
        max_size: u32,
        target_status: u8,
    ) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::delete_oldest_by_status_when_exceeded(
            pool.write_ref(),
            max_size,
            target_status,
        )
        .await?)
    }

    pub async fn delete_all(pool: &TaskDbPool, typ: Option<u8>) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::delete_all(pool.write_ref(), typ).await?)
    }

    pub async fn has_unfinished_task(pool: &TaskDbPool) -> Result<bool, crate::Error> {
        Ok(TaskQueueDao::has_unfinished_task(pool.read_ref()).await?)
    }

    pub async fn delete_tasks_with_request_body_like(
        pool: &TaskDbPool,
        keyword: &str,
    ) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::delete_tasks_with_request_body_like(pool.write_ref(), keyword).await?)
    }

    /// 批量插入TaskQueueEntity
    pub async fn insert_batch_task(
        pool: &TaskDbPool,
        tasks: &[TaskQueueEntity],
    ) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::insert_batch(pool.write_ref(), tasks).await?)
    }

    /// 批量插入TaskQueueEntity，忽略冲突
    pub async fn insert_batch_task_ignore_conflict(
        pool: &TaskDbPool,
        tasks: &[TaskQueueEntity],
    ) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::insert_batch_ignore_conflict(pool.write_ref(), tasks).await?)
    }

    /// 获取task_queue表记录数
    pub async fn count_tasks(pool: &TaskDbPool) -> Result<i64, crate::Error> {
        Ok(TaskQueueDao::count(pool.read_ref()).await?)
    }

    /// 获取core_db中task_queue表记录数
    pub async fn count_tasks_core(pool: &CoreDbPool) -> Result<i64, crate::Error> {
        Ok(TaskQueueDao::count(pool.read_ref()).await?)
    }

    /// 冻结旧表
    pub async fn freeze_table(pool: &TaskDbPool) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::freeze_table(pool.write_ref()).await?)
    }

    /// 冻结core_db中的旧表
    pub async fn freeze_table_core(pool: &CoreDbPool) -> Result<(), crate::Error> {
        Ok(TaskQueueDao::freeze_table(pool.write_ref()).await?)
    }

    /// 检查表是否存在
    pub async fn table_exists(pool: &TaskDbPool, table_name: &str) -> Result<bool, crate::Error> {
        Ok(TaskQueueDao::table_exists(pool.read_ref(), table_name).await?)
    }

    /// 检查core_db中表是否存在
    pub async fn table_exists_core(
        pool: &CoreDbPool,
        table_name: &str,
    ) -> Result<bool, crate::Error> {
        Ok(TaskQueueDao::table_exists(pool.read_ref(), table_name).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::TaskQueueRepo;
    use crate::{
        dao::task_queue::TaskQueueDao,
        entities::task_queue::{KnownTaskName, TaskName, TaskQueueEntity},
        repositories::test_helper::setup_core_pool,
    };

    fn make_temp_dir(prefix: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "{}_{}_{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().to_string()
    }

    async fn setup_task_pool(prefix: &str) -> crate::TaskDbPool {
        let dir = make_temp_dir(prefix);
        let ctx = crate::SqliteContext::new(&dir, Some("task.db")).await.unwrap();
        ctx.into_task_db_pool().unwrap()
    }

    #[test]
    fn task_queue_repo_build_backend_task_sets_backend_type() {
        let entity = TaskQueueRepo::build_backend_task(
            TaskName::Known(KnownTaskName::PullHotCoins),
            Some("{\"k\":\"v\"}".to_string()),
            Some("remark".to_string()),
        )
        .unwrap();

        assert_eq!(entity.r#type, 1);
        assert_eq!(entity.status, 0);
        assert_eq!(entity.remark.as_deref(), Some("remark"));
        assert_eq!(entity.request_body.as_deref(), Some("{\"k\":\"v\"}"));
    }

    #[test]
    fn task_queue_repo_build_mqtt_task_sets_mqtt_type_and_fixed_id() {
        let entity = TaskQueueRepo::build_mqtt_task(
            "fixed-id",
            TaskName::Known(KnownTaskName::PullApiWalletCoins),
            Some("{\"a\":1}".to_string()),
            None,
        )
        .unwrap();

        assert_eq!(entity.id, "fixed-id");
        assert_eq!(entity.r#type, 2);
        assert_eq!(entity.status, 0);
    }

    #[tokio::test]
    async fn task_queue_repo_create_and_detail_success() {
        let pool = setup_task_pool("wallet_db_task_queue_repo_success").await;
        let task = TaskQueueRepo::build_backend_task(
            TaskName::Known(KnownTaskName::PullHotCoins),
            Some("{\"k\":\"v\"}".to_string()),
            Some("remark".to_string()),
        )
        .unwrap();
        let id = task.id.clone();

        TaskQueueRepo::create_task(&pool, task).await.unwrap();
        let found = TaskQueueRepo::task_detail(&pool, &id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, id);
    }

    #[tokio::test]
    async fn task_queue_repo_detail_missing_returns_none() {
        let pool = setup_task_pool("wallet_db_task_queue_repo_edge").await;
        let found = TaskQueueRepo::task_detail(&pool, "task_missing").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn task_queue_repo_tx_rollback_keeps_task_absent() {
        let pool = setup_task_pool("wallet_db_task_queue_repo_rollback").await;
        let task = TaskQueueRepo::build_backend_task(
            TaskName::Known(KnownTaskName::PullApiWalletCoins),
            Some("{\"a\":1}".to_string()),
            None,
        )
        .unwrap();
        let id = task.id.clone();

        let mut tx = pool.write_ref().begin().await.unwrap();
        TaskQueueDao::upsert(tx.as_mut(), task).await.unwrap();
        tx.rollback().await.unwrap();

        let found = TaskQueueRepo::task_detail(&pool, &id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn task_queue_repo_all_tasks_queue_core_reads_legacy_shape_success() {
        let pool = setup_core_pool("wallet_db_task_queue_core_legacy_success").await;

        sqlx::query(
            "INSERT INTO task_queue (id, task_name, request_body, type, retry_times, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), NULL)",
        )
        .bind("legacy-task-1")
        .bind("PullHotCoins")
        .bind("{\"k\":\"v\"}")
        .bind(1_i64)
        .bind(0_i64)
        .bind(0_i64)
        .execute(pool.write_ref())
        .await
        .unwrap();

        let tasks = TaskQueueRepo::all_tasks_queue_core(&pool).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "legacy-task-1");
        assert_eq!(tasks[0].request_body, "{\"k\":\"v\"}");
        assert_eq!(tasks[0].err_msg, None);
        assert_eq!(tasks[0].remark, None);
    }

    #[tokio::test]
    async fn task_queue_repo_all_tasks_queue_core_empty_legacy_table_keeps_empty() {
        let pool = setup_core_pool("wallet_db_task_queue_core_legacy_empty").await;

        assert_eq!(TaskQueueRepo::count_tasks_core(&pool).await.unwrap(), 0);

        let tasks = TaskQueueRepo::all_tasks_queue_core(&pool).await.unwrap();
        assert!(tasks.is_empty());

        assert_eq!(TaskQueueRepo::count_tasks_core(&pool).await.unwrap(), 0);
    }
    #[tokio::test]
    async fn generic_waiting_rearm_is_controlled_by_explicit_status_and_capacity() {
        let pool = setup_task_pool("generic_waiting_rearm").await;
        let owner = "other_recovery";
        let now = chrono::Utc::now();
        let mut row = TaskQueueEntity {
            id: "generic-parent".into(),
            task_name: TaskName::Unknown("OtherRecovery".into()),
            request_body: "{}".into(),
            r#type: 1,
            status: 2,
            err_msg: None,
            remark: None,
            created_at: now,
            updated_at: Some(now),
        };
        row.task_name = TaskName::Unknown("OtherRecovery".into());
        row.status = 2;
        row.remark = Some(r#"{"owner":"other_recovery","consumer":"other_recovery"}"#.into());
        TaskQueueRepo::insert_batch_task_ignore_conflict(&pool, std::slice::from_ref(&row))
            .await
            .unwrap();
        TaskQueueRepo::rearm_owned_done(
            &pool,
            &row.id,
            owner,
            row.task_name.clone(),
            row.remark.as_deref().unwrap(),
            3,
            1,
        )
        .await
        .unwrap();
        assert_eq!(TaskQueueRepo::task_detail(&pool, &row.id).await.unwrap().unwrap().status, 3);
    }

    #[tokio::test]
    async fn default_recovery_excludes_every_external_owner_before_parsing_names() {
        let pool = setup_task_pool("generic_consumer_isolation").await;
        let now = chrono::Utc::now();
        let mut rows = Vec::new();
        for (id, remark) in [
            ("default", None),
            ("owned", Some(r#"{"owner":"unrelated_consumer"}"#.into())),
            ("invalid", Some("ordinary remark".into())),
            ("numeric", Some(r#"{"owner":123}"#.into())),
            ("empty", Some(r#"{"owner":""}"#.into())),
        ] {
            rows.push(TaskQueueEntity {
                id: id.into(),
                task_name: TaskName::Unknown("UnknownRecovery".into()),
                request_body: "{}".into(),
                r#type: 1,
                status: 3,
                err_msg: None,
                remark,
                created_at: now,
                updated_at: Some(now),
            });
        }
        TaskQueueRepo::insert_batch_task_ignore_conflict(&pool, &rows).await.unwrap();
        let selected = TaskQueueRepo::recoverable_default(&pool).await.unwrap();
        assert_eq!(
            selected.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["default", "empty", "invalid", "numeric"]
        );
        assert!(TaskQueueRepo::externally_owned(&pool, rows[1].remark.as_deref()).await.unwrap());
        assert!(!TaskQueueRepo::externally_owned(&pool, rows[2].remark.as_deref()).await.unwrap());
    }
    #[tokio::test]
    async fn remark_routing_and_retention_are_compatible_for_all_json_owner_types() {
        let pool = setup_task_pool("remark_routing_compatibility").await;
        let mut expected_default = Vec::new();
        let mut expected_retained = Vec::new();
        let deep =
            format!(r#"{{"owner":"external","unused":{}0{}}}"#, "[".repeat(140), "]".repeat(140));
        let cases: [(Option<&str>, bool); 21] = [
            (None, false),
            (Some(""), false),
            (Some("plain legacy remark"), false),
            (Some(r#"{"owner":broken}"#), false),
            (Some(r#"{"owner":""}"#), false),
            (Some(r#"{"owner":null}"#), false),
            (Some(r#"{"owner":42}"#), false),
            (Some(r#"{"owner":true}"#), false),
            (Some(r#"{"owner":{}}"#), false),
            (Some(r#"{"owner":[]}"#), false),
            (Some(r#"{"body":{"owner":"payload-only"}}"#), false),
            (Some(r#"["owner"]"#), false),
            (Some(r#"{"owner":"unknown-consumer"}"#), true),
            (Some(r#"{"owner":"api_missing_chain"}"#), true),
            (Some(r#"{"owner":"","owner":"external"}"#), false),
            (Some(r#"{"owner":"external","owner":""}"#), true),
            (Some(r#"{"owner":"","\u006fwner":"external"}"#), false),
            (Some(r#"{"\u006fwner":"external","owner":""}"#), true),
            (Some(r#"{"owner":42,"owner":"external"}"#), false),
            (Some(deep.as_str()), true),
            (Some(r#"{"owner":"external","unused":1e999}"#), true),
        ];
        let old = chrono::DateTime::parse_from_rfc3339("1970-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        for (case, (remark, external)) in cases.into_iter().enumerate() {
            for status in 0..=4 {
                let id = format!("case-{case:02}-{status}");
                let row = TaskQueueEntity {
                    id: id.clone(),
                    task_name: TaskName::Unknown("FutureName".into()),
                    request_body: "{}".into(),
                    r#type: 1,
                    status,
                    err_msg: None,
                    remark: remark.map(str::to_owned),
                    created_at: old,
                    updated_at: Some(old),
                };
                assert_eq!(
                    TaskQueueRepo::externally_owned(&pool, row.remark.as_deref()).await.unwrap(),
                    external,
                    "case {case}"
                );
                if status != 2 {
                    if external {
                        expected_retained.push(id.clone());
                    } else {
                        expected_default.push(id.clone());
                    }
                }
                TaskQueueRepo::insert_batch_task_ignore_conflict(&pool, &[row]).await.unwrap();
            }
        }
        let mut actual_default = TaskQueueRepo::recoverable_default(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>();
        actual_default.sort();
        expected_default.sort();
        assert_eq!(actual_default, expected_default);
        TaskQueueRepo::delete_old(&pool, 15).await.unwrap();
        let mut actual = TaskQueueRepo::all_tasks_queue(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>();
        actual.sort();
        expected_retained.sort();
        assert_eq!(actual, expected_retained);
    }
}
