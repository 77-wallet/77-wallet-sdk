pub(crate) mod dispatcher;
pub(crate) mod scheduler;
use crate::{
    domain::app::config::ConfigDomain,
    error::service::ServiceError,
    infrastructure::task_queue::task::{TaskTrait, task_type::TaskType},
};

use dashmap::DashSet;
use dispatcher::{Dispatcher, PriorityTask, TaskSender};
use rand::Rng as _;
use std::{collections::BTreeMap, sync::Arc};
use wallet_database::{
    entities::task_queue::{KnownTaskName, TaskName, TaskQueueEntity},
    repositories::{device::DeviceRepo, task_queue::TaskQueueRepo},
};
use wallet_transport_backend::{
    consts::endpoint::SEND_MSG_CONFIRM, request::ClientTaskLogUploadReq,
};
use wallet_utils::{RetryableError as _, error::RetryPolicy};

/// 定义共享的 running_tasks 类型
type RunningTasks = Arc<DashSet<String>>;

fn delete_after_success(task_name: &TaskName) -> bool {
    matches!(task_name, TaskName::Known(KnownTaskName::CreateApiAccountDeferred))
}

fn is_rpc_service_unavailable(error: &ServiceError) -> bool {
    crate::infrastructure::chain_rpc_guard::ChainRpcErrorContext::from_display(error)
        .is_service_unavailable()
}

fn next_retry_delay_ms(
    current_delay_ms: u64,
    is_rpc_503: bool,
    is_rate_limit: bool,
    is_retryable: bool,
) -> u64 {
    if is_rpc_503 || is_rate_limit {
        current_delay_ms.saturating_mul(2).min(60_000)
    } else if is_retryable {
        current_delay_ms.saturating_add(1_000).min(30_000)
    } else {
        current_delay_ms.saturating_mul(2).min(120_000)
    }
}

#[derive(Clone)]
pub struct TaskManager {
    ctx: &'static crate::context::Context,
    running_tasks: RunningTasks,
    // task_sender: crate::manager::TaskSender,
    pub(crate) notify: Arc<tokio::sync::Notify>,
    dispatcher: Dispatcher,
}

impl TaskManager {
    /// 创建一个新的 TaskManager 实例
    pub fn new(ctx: &'static crate::context::Context, notify: Arc<tokio::sync::Notify>) -> Self {
        let running_tasks: RunningTasks = Arc::new(DashSet::new());
        let dispatcher = Dispatcher::new(ctx, Arc::clone(&running_tasks));
        // let task_sender = dispatcher.task_dispatcher(Arc::clone(&running_tasks));
        Self {
            ctx,
            running_tasks,
            // task_sender,
            notify,
            dispatcher,
        }
    }

    /// 启动任务检查循环
    pub async fn start_task_check(&self) -> Result<(), ServiceError> {
        let running_tasks = Arc::clone(&self.running_tasks);

        let pool = self.ctx.task_pool()?;
        TaskQueueRepo::delete_tasks_with_request_body_like(&pool, SEND_MSG_CONFIRM).await?;
        let deleted_recovery_tasks = TaskQueueRepo::delete_done_by_task_name(
            &pool,
            TaskName::Known(KnownTaskName::CreateApiAccountDeferred),
        )
        .await?;
        if deleted_recovery_tasks > 0 {
            tracing::info!(
                deleted_count = deleted_recovery_tasks,
                task_name = KnownTaskName::CreateApiAccountDeferred.as_ref(),
                "deleted completed ephemeral recovery tasks"
            );
        }
        let ctx = self.ctx;

        tokio::spawn(async move {
            Self::task_check(ctx, running_tasks).await;
        });
        Ok(())
    }

    /// 获取任务发送器
    pub fn get_task_sender(&self) -> TaskSender {
        self.dispatcher.external_tx.clone()
    }

    /// 任务检查函数
    async fn task_check(ctx: &'static crate::context::Context, running_tasks: RunningTasks) {
        // 在 TaskManager 的方法中启动
        tracing::info!("task check start");
        if let Err(e) = Self::check_handle(ctx, &running_tasks).await {
            tracing::error!("task check error: {}", e);
        }
        tracing::info!("task check end");
    }

    /// 检查并发送任务的处理函数
    async fn check_handle(
        ctx: &'static crate::context::Context,
        running_tasks: &RunningTasks,
    ) -> Result<(), ServiceError> {
        let handles = ctx.get_handles_arc().await?;
        let pool = ctx.task_pool()?;
        let manager = handles.get_global_task_manager();

        TaskQueueRepo::delete_old(&pool, 15).await?;

        let failed_queue = TaskQueueRepo::recoverable_default(&pool).await?;

        let mut grouped_tasks: BTreeMap<u8, Vec<TaskQueueEntity>> = BTreeMap::new();
        // tracing::info!("failed_queue: {:#?}", failed_queue);
        for task_entity in failed_queue.into_iter() {
            if !running_tasks.contains(&task_entity.id) {
                let Ok(task) = TryInto::<Box<dyn TaskTrait>>::try_into(&task_entity) else {
                    tracing::error!("task queue entity convert to task error: {}", task_entity.id);
                    TaskQueueRepo::delete_task(&pool, &task_entity.id).await?;
                    continue;
                };

                let priority = scheduler::assign_priority(&*task, true)?;
                grouped_tasks.entry(priority).or_default().push(task_entity);
            }
        }

        for (priority, tasks) in grouped_tasks {
            if let Err(e) = manager.get_task_sender().send(PriorityTask { priority, tasks }) {
                tracing::error!("send task queue error: {}", e);
            }
        }

        Ok(())
    }

    async fn process_single_task(
        ctx: &'static crate::context::Context,
        task: TaskQueueEntity,
        running_tasks: RunningTasks,
    ) {
        let task_id = task.id.clone();

        let mut retry_count = 0;
        let mut delay = 200; // 初始延迟设为 200 毫秒
        // const MAX_RETRY_COUNT: i32 = 5;

        loop {
            // if retry_count >= MAX_RETRY_COUNT {
            //     tracing::warn!(
            //         "[process_single_task] task {} exceeded max retries ({})",
            //         task_id,
            //         MAX_RETRY_COUNT
            //     );
            //     if let Ok(pool) = crate::manager::Context::get_global_sqlite_pool() {
            //         let mut repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());
            //         let _ = repo.task_failed(&task_id).await;
            //     };
            //     break;
            // }

            match Self::handle_task(ctx, &task).await {
                Ok(()) => break, // 成功
                Err(e) => {
                    tracing::error!(?task, "[task_process] error: {}", e);
                    let is_retryable = matches!(e.retry_policy(), RetryPolicy::Delay);

                    // 检查是否为429限流错误（用于选择不同的 backoff 曲线）
                    let is_rate_limit = matches!(&e,
                        crate::error::service::ServiceError::TransportBackend(
                            wallet_transport_backend::error::Error::ApiBackend(code, _)
                        ) if *code == 429
                    );
                    let is_rpc_503 = is_rpc_service_unavailable(&e);

                    if is_retryable {
                        // 如果是可重试错误，则重试
                        tracing::warn!(
                            "[process_single_task] task {} retry {} due to retryable error",
                            task_id,
                            retry_count
                        );
                    } else {
                        // 否则，记录错误并增加重试次数
                        if let Err(e) = Self::increase_retry_times(ctx, &task.id, retry_count).await
                        {
                            tracing::error!("[process_single_task] error: {}", e);
                        }
                    }

                    if let Err(err) = Self::mark_task_failed(ctx, &task_id, &e.to_string()).await {
                        tracing::warn!(
                            task_id = %task_id,
                            error = %err,
                            "failed to mark task as failed"
                        );
                    }

                    if retry_count >= 10 {
                        tracing::warn!(
                            "[process_single_task] task {} exceeded max retries ({}), breaking",
                            task_id,
                            retry_count
                        );
                        if let Err(err) = Self::hang_up_task(ctx, &task_id).await {
                            tracing::warn!(
                                task_id = %task_id,
                                error = %err,
                                "failed to mark task as hang up"
                            );
                        } else {
                            tracing::warn!("[process_single_task] task {} hang up", task_id);
                        }

                        if let Err(e) =
                            Self::upload_task_error_info(ctx, &task, &e.to_string()).await
                        {
                            tracing::error!(
                                "[process_single_task] upload_task_error_info error: {}",
                                e
                            );
                        };

                        break;
                    }

                    // 根据错误类型调整延迟策略
                    if is_rpc_503 || is_rate_limit {
                        // RPC 503 和限流都使用指数退避，避免恢复时多个任务同时重试。
                        delay = next_retry_delay_ms(delay, is_rpc_503, is_rate_limit, is_retryable);
                    } else if is_retryable {
                        // 其他可重试错误使用线性退避，每次增加1秒
                        delay = next_retry_delay_ms(delay, false, false, true);
                    } else {
                        // 不可重试错误继续使用原来的指数退避策略
                        delay = next_retry_delay_ms(delay, false, false, false);
                    }

                    let jitter = std::time::Duration::from_millis(
                        rand::thread_rng().gen_range(0..(delay / 2)),
                    );
                    delay += jitter.as_millis() as u64; // 将延迟加上抖动
                    retry_count += 1;

                    tracing::debug!(
                        "[process_single_task] delay: {delay} ms, retry_count: {retry_count}, jitter: {jitter:?}, is_rpc_503: {is_rpc_503}, is_rate_limit: {is_rate_limit}, is_retryable: {is_retryable}"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
            }
        }

        running_tasks.remove(&task_id);
        // if running_tasks.is_empty() {
        //     let notify = crate::manager::Context::get_global_notify().unwrap();
        //     notify.notify_one();
        //     tracing::info!("notify_one");
        // }
    }

    async fn upload_task_error_info(
        ctx: &'static crate::context::Context,
        task_entity: &TaskQueueEntity,
        error_info: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let sn = ctx.get_sn().to_string();
        let Some(device) = DeviceRepo::get_device_info(pool, &sn).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };

        let client_id = crate::domain::app::DeviceDomain::client_id_by_device(&device)?;
        let app_version = ConfigDomain::get_app_version(ctx).await?;

        let req = ClientTaskLogUploadReq::new(
            &device.sn,
            &client_id,
            &app_version.app_version,
            &task_entity.id,
            &wallet_utils::serde_func::serde_to_string(&task_entity.task_name)?,
            &task_entity.r#type.to_string(),
            &task_entity.request_body,
            error_info,
        );

        let backend_api = ctx.get_global_backend_api();
        backend_api.client_task_log_upload(req).await?;

        let task: Box<dyn TaskTrait> = task_entity.try_into()?;
        if task.get_type() == TaskType::Mqtt {
            let handles = ctx.get_handles_arc().await?;
            let unconfirmed_msg_collector = handles.get_global_unconfirmed_msg_collector();
            tracing::info!(
                "upload task error info mqtt submit unconfirmed msg collector: {}",
                task_entity.id
            );
            unconfirmed_msg_collector.submit(vec![task_entity.id.to_string()])?;
        }
        Ok(())
    }

    async fn increase_retry_times(
        ctx: &'static crate::context::Context,
        task_id: &str,
        retry_count: i32,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.task_pool()?;

        if retry_count > 0 {
            TaskQueueRepo::increase_retry_times(&pool, task_id).await?;
        }

        Ok(())
    }

    async fn mark_task_failed(
        ctx: &'static crate::context::Context,
        task_id: &str,
        error: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.task_pool()?;
        TaskQueueRepo::task_failed(&pool, task_id, error).await?;
        Ok(())
    }

    async fn hang_up_task(
        ctx: &'static crate::context::Context,
        task_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.task_pool()?;
        TaskQueueRepo::task_hang_up(&pool, task_id).await?;
        Ok(())
    }

    async fn handle_task(
        ctx: &'static crate::context::Context,
        task_entity: &TaskQueueEntity,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.task_pool()?;
        if TaskQueueRepo::externally_owned(&pool, task_entity.remark.as_deref()).await? {
            return Ok(());
        }

        let id = task_entity.id.clone();
        let task: Box<dyn TaskTrait> = task_entity.try_into()?;
        let task_type = task.get_type(); // update task running status

        TaskQueueRepo::task_running(&pool, &id).await?;

        task.execute(&id, ctx).await?;

        TaskQueueRepo::task_done(&pool, &id).await?;

        // This recovery task persists a large batch payload but has no remote
        // redelivery/deduplication semantics. Mark Done first so a cleanup
        // failure can never cause already-executed work to run again.
        if delete_after_success(&task_entity.task_name)
            && let Err(err) = TaskQueueRepo::delete_task(&pool, &id).await
        {
            tracing::warn!(
                task_id = %id,
                error = %err,
                "failed to delete completed ephemeral recovery task"
            );
        }

        if task_type == TaskType::Mqtt {
            let handles = ctx.get_handles_arc().await?;
            let unconfirmed_msg_collector = handles.get_global_unconfirmed_msg_collector();
            tracing::info!("handle task mqtt submit unconfirmed msg collector: {}", id);
            unconfirmed_msg_collector.submit(vec![id])?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{delete_after_success, is_rpc_service_unavailable, next_retry_delay_ms};
    use crate::error::{service::ServiceError, system::SystemError};
    use wallet_database::entities::task_queue::{KnownTaskName, TaskName};

    #[test]
    fn only_deferred_api_account_recovery_is_deleted_after_success() {
        assert!(delete_after_success(&TaskName::Known(KnownTaskName::CreateApiAccountDeferred,)));
        assert!(!delete_after_success(&TaskName::Known(KnownTaskName::QueryCoinPrice)));
        assert!(!delete_after_success(&TaskName::Known(KnownTaskName::ApiWalletAcctChange)));
        assert!(!delete_after_success(&TaskName::Unknown("future-task".to_string())));
    }

    #[test]
    fn rpc_503_uses_bounded_exponential_backoff() {
        assert!(is_rpc_service_unavailable(&ServiceError::System(SystemError::Internal(
            "Node response error: code=503, rpc=https://api.nileex.io/wallet/getaccount"
                .to_string(),
        ))));
        assert_eq!(next_retry_delay_ms(200, true, false, true), 400);
        assert_eq!(next_retry_delay_ms(30_000, true, false, true), 60_000);
        assert_eq!(next_retry_delay_ms(60_000, true, false, true), 60_000);
    }

    #[test]
    fn non_503_retryable_backoff_keeps_linear_curve() {
        assert_eq!(next_retry_delay_ms(200, false, false, true), 1_200);
        assert_eq!(next_retry_delay_ms(29_500, false, false, true), 30_000);
    }
}
