use super::{BackendApiTask, CommonTask, InitializationTask, MqttTask, Task};
use crate::{
    domain::{
        self,
        {
            multisig::MultisigQueueDomain,
            task_queue::task_handle::backend_handle::BackendTaskHandle,
        },
    },
    service::{announcement::AnnouncementService, coin::CoinService, device::DeviceService},
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_stream::StreamExt as _;
use wallet_database::repositories::task_queue::TaskQueueRepoTrait;
use wallet_database::{entities::task_queue::TaskQueueEntity, factory::RepositoryFactory};

/// 定义共享的 running_tasks 类型
type RunningTasks = Arc<Mutex<std::collections::HashSet<String>>>;

#[derive(Debug, Clone)]
pub struct TaskManager {
    running_tasks: RunningTasks,
    task_sender: crate::manager::TaskSender,
}

impl TaskManager {
    /// 创建一个新的 TaskManager 实例
    pub fn new() -> Self {
        let running_tasks: RunningTasks = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let task_sender = Self::task_process(Arc::clone(&running_tasks));
        Self {
            running_tasks,
            task_sender,
        }
    }

    /// 启动任务检查循环
    pub fn start_task_check_loop(&self) {
        let running_tasks = Arc::clone(&self.running_tasks);
        tokio::spawn(async move {
            Self::task_check_loop(running_tasks).await;
        });
    }

    /// 获取任务发送器
    pub fn get_task_sender(&self) -> tokio::sync::mpsc::UnboundedSender<Vec<TaskQueueEntity>> {
        self.task_sender.clone()
    }

    /// 任务检查循环函数
    async fn task_check_loop(running_tasks: RunningTasks) {
        // 在 TaskManager 的方法中启动
        tokio::spawn(async move {
            let pool = crate::manager::Context::get_global_sqlite_pool().unwrap();
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            let mut first = true;
            loop {
                interval.tick().await;
                let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

                if let Err(e) = Self::check_handle(repo, &mut first, &running_tasks).await {
                    tracing::error!("task check error: {}", e);
                    continue;
                }
            }
        });
    }

    /// 检查并发送任务的处理函数
    async fn check_handle(
        mut repo: wallet_database::repositories::ResourcesRepo,
        first: &mut bool,
        running_tasks: &RunningTasks,
    ) -> Result<(), crate::ServiceError> {
        let manager = crate::manager::Context::get_global_task_manager()?;
        let mut failed_queue = repo.failed_task_queue().await?;
        let pending_queue = repo.pending_task_queue().await?;
        if *first {
            let running_queue = repo.running_task_queue().await?;
            failed_queue.extend(running_queue);
            *first = false;
        }
        failed_queue.extend(pending_queue);
        let mut tasks = Vec::new();

        // 获取当前正在运行的任务
        let running = running_tasks.lock().await;

        for task in failed_queue {
            if !running.contains(&task.id) {
                tasks.push(task);
            }
        }
        drop(running);

        if let Err(e) = manager.get_task_sender().send(tasks) {
            tracing::error!("send task queue error: {}", e);
        }
        Ok(())
    }

    /// 定义 task_process 方法，接受共享的 running_tasks
    fn task_process(
        running_tasks: RunningTasks,
    ) -> tokio::sync::mpsc::UnboundedSender<Vec<TaskQueueEntity>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<TaskQueueEntity>>();
        let mut rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);

        tokio::spawn(async move {
            while let Some(tasks) = rx.next().await {
                tracing::debug!("[task_process] tasks: {tasks:?}");
                for task in tasks {
                    let task_id = task.id.clone();

                    // 检查并更新 running_tasks
                    {
                        let mut running = running_tasks.lock().await;
                        if running.contains(&task_id) {
                            continue;
                        }
                        running.insert(task_id.clone());
                    } // 释放锁

                    let running_tasks_clone = Arc::clone(&running_tasks);

                    tokio::spawn(async move {
                        if let Err(e) = Self::once_handle(&task).await {
                            if let Ok(pool) = crate::manager::Context::get_global_sqlite_pool() {
                                let mut repo =
                                    wallet_database::factory::RepositoryFactory::repo(pool.clone());
                                let _ = repo.task_failed(&task_id).await;
                            };
                            tracing::error!(?task, "[task_process] error: {}", e)
                        }
                        let mut running = running_tasks_clone.lock().await;
                        running.remove(&task_id);
                    });
                }
            }
        });
        tx
    }

    async fn once_handle(task_entity: &TaskQueueEntity) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let mut repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        let id = task_entity.id.clone();
        let task: Task = task_entity.try_into()?;
        let backend_api = crate::manager::Context::get_global_backend_api()?;

        // update task running status
        repo.task_running(&id).await?;

        match task {
            Task::Initialization(initialization_task) => match initialization_task {
                InitializationTask::PullAnnouncement => {
                    let repo = RepositoryFactory::repo(pool.clone());
                    let announcement_service = AnnouncementService::new(repo);
                    let res = announcement_service.pull_announcement().await;

                    res?;
                }
                InitializationTask::PullHotCoins => {
                    let repo = RepositoryFactory::repo(pool.clone());
                    let mut coin_service = CoinService::new(repo);
                    coin_service.pull_hot_coins().await?;
                    let repo = RepositoryFactory::repo(pool.clone());
                    let coin_service = CoinService::new(repo);
                    coin_service.init_token_price().await?;
                }
                InitializationTask::InitTokenPrice => {
                    let repo = RepositoryFactory::repo(pool.clone());
                    let coin_service = CoinService::new(repo);

                    coin_service.init_token_price().await?;
                }
                InitializationTask::ProcessUnconfirmMsg => {
                    let repo = RepositoryFactory::repo(pool.clone());
                    let device_service = DeviceService::new(repo);
                    let Some(device) = device_service.get_device_info().await? else {
                        tracing::error!("get device info failed");
                        return Ok(());
                    };
                    let client_id = crate::domain::app::DeviceDomain::client_id_by_device(&device)?;
                    let data = backend_api
                        .send_msg_query_unconfirm_msg(
                            &wallet_transport_backend::request::SendMsgQueryUnconfirmMsgReq {
                                client_id,
                            },
                        )
                        .await?
                        .list;
                    crate::service::jpush::JPushService::jpush_multi(data, "API").await?;
                }
                InitializationTask::SetBlockBrowserUrl => {
                    let repo = RepositoryFactory::repo(pool.clone());
                    let mut app_service = crate::service::app::AppService::new(repo);
                    app_service.set_block_browser_url().await?;
                }
                InitializationTask::SetFiat => {
                    let repo = RepositoryFactory::repo(pool.clone());
                    let device_service = DeviceService::new(repo);
                    let res = device_service.get_device_info().await;

                    let repo = RepositoryFactory::repo(pool.clone());
                    let mut app_service = crate::service::app::AppService::new(repo);
                    if let Ok(Some(device_info)) = res {
                        let _ = app_service.set_fiat(&device_info.currency).await;
                    }
                }

                InitializationTask::RecoverQueueData => {
                    MultisigQueueDomain::recover_all_uid_queue_data().await?;
                }
            },
            Task::BackendApi(backend_api_task) => match backend_api_task {
                BackendApiTask::BackendApi(data) => {
                    BackendTaskHandle::do_handle(&data.endpoint, data.body, backend_api).await?;
                }
            },
            Task::Mqtt(mqtt_task) => match *mqtt_task {
                MqttTask::OrderMultiSignAccept(data) => data.exec(&id).await?,
                MqttTask::OrderMultiSignAcceptCompleteMsg(data) => data.exec(&id).await?,
                MqttTask::OrderMultiSignServiceComplete(data) => data.exec(&id).await?,
                MqttTask::OrderMultiSignCreated(data) => data.exec(&id).await?,
                MqttTask::OrderMultiSignCancel(data) => data.exec(&id).await?,
                MqttTask::MultiSignTransAccept(data) => data.exec(&id).await?,
                MqttTask::MultiSignTransCancel(data) => data.exec(&id).await?,
                MqttTask::MultiSignTransAcceptCompleteMsg(data) => data.exec(&id).await?,
                MqttTask::AcctChange(data) => data.exec(&id).await?,
                MqttTask::Init(data) => data.exec(&id).await?,
                MqttTask::BulletinMsg(data) => data.exec(&id).await?,
                MqttTask::RpcChange(data) => data.exec(&id).await?,
            },
            Task::Common(common_task) => match common_task {
                CommonTask::QueryCoinPrice(data) => {
                    let repo = RepositoryFactory::repo(pool.clone());
                    let coin_service = CoinService::new(repo);
                    coin_service.query_token_price(data).await?;
                }
                CommonTask::QueryQueueResult(data) => {
                    domain::multisig::MultisigQueueDomain::sync_queue_status(&data.id).await?
                }
                CommonTask::RecoverMultisigAccountData(uid) => {
                    domain::multisig::MultisigDomain::recover_uid_multisig_data(&uid).await?;
                    MultisigQueueDomain::recover_all_queue_data(&uid).await?;
                }
            },
        }
        repo.task_done(&id).await?;

        Ok(())
    }
}
