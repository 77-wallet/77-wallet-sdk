use crate::{
    context::Context,
    infrastructure::task_queue::{
        backend::{BackendApiTask, BackendApiTaskData},
        task::Tasks,
    },
};
use wallet_database::entities::task_queue::TaskQueueEntity;
use wallet_transport_backend::request::SendMsgConfirm;

pub(crate) struct TaskQueueDomain;

impl TaskQueueDomain {
    pub async fn migrate_task_queue_to_db_with_ctx(
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = ctx.core_pool()?;
        let task_pool = ctx.task_pool()?;

        let migration_key = "migration.task_queue_db";

        // 1. 检查迁移标记（快速路径）
        if let Some(status) = wallet_database::repositories::config::ConfigRepo::find_by_key(
            migration_key,
            &core_pool,
        )
        .await?
        {
            if status.value == "done" {
                tracing::info!("TaskQueue migration already done, skipping");
                return Ok(());
            }
        }

        tracing::info!("Starting TaskQueue migration from core_db to task_db");

        // 2. 从core_db读取旧数据
        let tasks: Vec<TaskQueueEntity> =
            wallet_database::repositories::task_queue::TaskQueueRepo::all_tasks_queue_core(
                &core_pool,
            )
            .await?;

        if tasks.is_empty() {
            tracing::info!("No task_queue records found in core_db");
        } else {
            tracing::info!("Found {} task_queue records in core_db", tasks.len());

            // 3. 写入task_db（必须是幂等插入）
            wallet_database::repositories::task_queue::TaskQueueRepo::insert_batch_task_ignore_conflict(
                &task_pool,
                &tasks,
            )
            .await?;
        }

        // 4. 数据校验（core_db vs task_db）
        let core_count: i64 =
            wallet_database::repositories::task_queue::TaskQueueRepo::count_tasks_core(&core_pool)
                .await?;

        let task_count: i64 =
            wallet_database::repositories::task_queue::TaskQueueRepo::count_tasks(&task_pool)
                .await?;

        if core_count != task_count {
            panic!(
                "TaskQueue migration failed: count mismatch (core: {}, task: {})\n",
                core_count, task_count
            );
        }

        tracing::info!("TaskQueue migration data verified successfully, count={}", task_count);

        // 5. 冻结旧表（只对core_db，且可重复）
        if wallet_database::repositories::task_queue::TaskQueueRepo::table_exists_core(
            &core_pool,
            "task_queue",
        )
        .await?
        {
            wallet_database::repositories::task_queue::TaskQueueRepo::freeze_table_core(&core_pool)
                .await?;

            tracing::info!("Froze core_db.task_queue as task_queue_legacy");
        } else {
            tracing::info!("core_db.task_queue already frozen, skipping");
        }

        // 6. 写入迁移完成标记（最后一步）
        wallet_database::repositories::config::ConfigRepo::upsert(
            migration_key,
            "done",
            Some(0),
            &core_pool,
        )
        .await?;

        tracing::info!("TaskQueue migration completed successfully");

        Ok(())
    }

    pub async fn send_msg_confirm_with_ctx(
        ctx: &'static Context,
        ids: Vec<SendMsgConfirm>,
    ) -> Result<(), crate::error::service::ServiceError> {
        if !ids.is_empty() {
            const BATCH_SIZE: usize = 500;
            let api = ctx.get_global_backend_api();
            for chunk in ids.chunks(BATCH_SIZE) {
                api.send_msg_confirm(&wallet_transport_backend::request::SendMsgConfirmReq::new(
                    chunk.to_vec(),
                ))
                .await?;
            }
        }
        Ok(())
    }

    pub async fn send_or_wrap_task_with_ctx<T: serde::Serialize + std::fmt::Debug>(
        ctx: &'static Context,
        req: T,
        endpoint: &str,
    ) -> Result<Option<BackendApiTask>, crate::error::service::ServiceError> {
        let backend = ctx.get_global_backend_api();

        let res = backend.post_request::<_, serde_json::Value>(endpoint, &req).await;

        if let Err(e) = res {
            tracing::error!("request backend:{} error:{}", endpoint, e);

            let task = BackendApiTask::BackendApi(BackendApiTaskData::new(endpoint, &req)?);
            return Ok(Some(task));
        }
        Ok(None)
    }

    pub async fn send_or_to_queue_with_ctx<T: serde::Serialize + std::fmt::Debug>(
        ctx: &'static Context,
        req: T,
        endpoint: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let task = Self::send_or_wrap_task_with_ctx(ctx, req, endpoint).await?;

        if let Some(task) = task {
            Tasks::new().push(task).send_with_ctx(ctx).await?;
        }

        Ok(())
    }
}
