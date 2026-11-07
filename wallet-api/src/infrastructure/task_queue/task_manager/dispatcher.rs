use crate::infrastructure::task_queue::{
    task::{TaskTrait, task_type::TaskType},
    task_manager::{TaskManager, scheduler::task_category_limit},
};

use super::RunningTasks;

use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, Semaphore, mpsc};
use wallet_database::{
    entities::task_queue::TaskQueueEntity, repositories::task_queue::TaskQueueRepo,
};

type Priority = u8;
pub type TaskSender = tokio::sync::mpsc::UnboundedSender<PriorityTask>;

pub(crate) struct PriorityTask {
    pub(crate) priority: u8,
    pub(crate) tasks: Vec<TaskQueueEntity>,
}

// static RATE_LIMITERS: Lazy<
//     std::collections::HashMap<TaskType, Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>,
// > = Lazy::new(|| {
//     use TaskType::*;
//     let mut m = std::collections::HashMap::new();
//     m.insert(
//         BackendApi,
//         Arc::new(RateLimiter::direct(Quota::per_second(
//             NonZeroU32::new(50).unwrap(),
//         ))),
//     );
//     m.insert(
//         Mqtt,
//         Arc::new(RateLimiter::direct(Quota::per_second(
//             NonZeroU32::new(200).unwrap(),
//         ))),
//     );
//     m.insert(
//         Initialization,
//         Arc::new(RateLimiter::direct(Quota::per_second(
//             NonZeroU32::new(10).unwrap(),
//         ))),
//     );
//     m.insert(
//         Common,
//         Arc::new(RateLimiter::direct(Quota::per_second(
//             NonZeroU32::new(10).unwrap(),
//         ))),
//     );
//     m
// });

// fn get_rate_limiter(
//     category: &TaskType,
// ) -> Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>> {
//     RATE_LIMITERS.get(category).cloned().unwrap()
// }

/// 多优先级任务调度器 `Dispatcher`
///
/// 该调度器用于管理异步任务执行，支持任务优先级调度、类型限速、并发控制，并通过无锁架构提升吞吐能力。
///
/// ### 🚀 设计思想
/// 类似于“分层 Reactor + 任务分级执行器”，每个优先级即一个独立的执行单元，
/// 任务间通过 channel 解耦，具备天然的并行性与隔离性。
///
///
/// ## ✨ 核心特性
///
/// - **每个优先级独立通道**：按优先级自动创建 `UnboundedSender`，每个优先级都有自己的消费任务，无需共享任务队列或锁。
/// - **任务类型限速**：支持对不同 `TaskType` 的任务设置并发上限（如 `sync` 每轮最多处理 N 个）。
/// - **运行中去重**：使用 `RunningTasks` 去重，避免同一个任务重复并发执行。
/// - **全局并发控制**：使用 `Semaphore` 限制系统总并发任务数，防止过载。
/// - **低资源占用**：无中心锁、无全局轮询，按需动态创建执行器，空闲时不会占用 CPU。
///
/// ## ⚙ 架构说明
///
/// ```text
///             上层调用
///                 │
///     ┌────────────┴────────────┐
///     │ external_tx.send(...)   │
///     └────────────┬────────────┘
///                  ▼
///        Dispatcher::start_internal_task
///                  │
///     ┌────────────┴────────────┐
///     │ external_rx 接收任务     │
///     │ 按 priority 分配到对应通道 │
///     └────────────┬────────────┘
///                  ▼
///      HashMap<Priority, UnboundedSender<TaskQueueEntity>>
///          （如无则动态创建）
///
///    每个优先级通道绑定一个 Tokio 任务:
///
///         UnboundedReceiver<TaskQueueEntity>
///                     │
///          每轮从通道中取任务执行：
///           - 类型限速（task_category_limit）
///           - 并发控制（Semaphore）
///           - 运行中任务去重
///           - 每任务延时，避免突发冲击
/// ```
///
/// ## 📌 示例流程（新任务进来时）
///
/// ```text
/// 1. Dispatcher.external_tx.send((priority, Vec<TaskQueueEntity>))
/// 2. Dispatcher 内部 task 接收并遍历 Vec<TaskQueueEntity>
/// 3. 若该优先级通道不存在，则动态创建 UnboundedSender + Receiver + Tokio 任务
/// 4. 每个任务推入通道，由其独立 Tokio task 处理
/// 5. 任务消费时：
///    - 限速检查（同类型任务是否已达上限）
///    - Semaphore 控制并发许可
///    - 使用 RunningTasks 去重
///    - TaskManager::process_single_task 执行
/// ```
///
///
/// ## 🛠 可配置项
///
/// - `task_category_limit()`：限制每类任务并发数
/// - `runtime_defaults::task_queue().dispatch_max_concurrent`：全局最大并发任务数
///
///
/// ## 🚧 注意事项
///
/// - 当前方案假设任务处理过程较长，动态 spawn 开销可以忽略。
/// - 不再存在中心调度器，所有优先级由其自身任务“自治消费”。
/// - 空闲时不会有忙等待，节省 CPU 资源。
///
#[derive(Debug, Clone)]
pub(crate) struct Dispatcher {
    pub(crate) external_tx: TaskSender,
}

impl Dispatcher {
    pub fn new(ctx: &'static crate::context::Context, running_tasks: RunningTasks) -> Self {
        let (external_tx, external_rx) = tokio::sync::mpsc::unbounded_channel();
        // Dispatcher 负责执行并发上限，默认值来源统一配置模块，避免与 scheduler 分叉。
        let defaults = crate::config::runtime_defaults::task_queue();
        let max_concurrent = defaults.dispatch_max_concurrent;

        Self::start_internal_task(
            ctx,
            external_rx,
            running_tasks,
            Arc::new(Semaphore::new(max_concurrent)),
        );
        tracing::info!(
            max_concurrent = max_concurrent,
            "Dispatcher started with global semaphore limit"
        );
        tracing::debug!("Dispatcher 启动完成，开始监听外部任务输入...");
        Self {
            external_tx, // task_queues: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
                         // semaphore: Arc::new(Semaphore::new(max_concurrent)), // 最大并发数可调整
        }
    }

    /// 启动内部异步调度器：
    ///
    /// - 动态接收外部任务（按优先级）
    /// - 若该优先级未注册消费通道，则自动创建通道 + 启动 Tokio task 处理器
    /// - 每个优先级通道独立消费任务，避免全局锁或抢占资源
    fn start_internal_task(
        ctx: &'static crate::context::Context,
        mut external_rx: tokio::sync::mpsc::UnboundedReceiver<PriorityTask>,
        running_tasks: RunningTasks,
        semaphore: Arc<Semaphore>,
    ) {
        tokio::spawn(async move {
            let mut priority_senders: HashMap<Priority, mpsc::UnboundedSender<TaskQueueEntity>> =
                HashMap::new();
            // 将静态默认值展开为 HashMap，便于任务执行阶段按类型快速查限额。
            let category_limit = task_category_limit().into_iter().collect::<HashMap<_, _>>();

            while let Some(PriorityTask { priority, tasks }) = external_rx.recv().await {
                // tracing::info!("收到 {} 个任务，优先级 = {}", tasks.len(), priority,);
                let sender = priority_senders.entry(priority).or_insert_with(|| {
                    // tracing::info!("创建新的优先级 {} 通道并启动任务消费器", priority);
                    Self::create_priority_channel_task(
                        ctx,
                        priority,
                        running_tasks.clone(),
                        semaphore.clone(),
                        category_limit.clone(),
                    )
                });

                for task_entity in tasks {
                    if let Err(err) = sender.send(task_entity) {
                        tracing::warn!(error = %err, priority = priority, "failed to enqueue task");
                    }
                }
            }
            tracing::warn!("Dispatcher 的 external_rx 关闭，任务监听结束");
        });
    }

    /// 为某个优先级动态创建通道及绑定异步消费任务
    ///
    /// 返回该优先级的 `UnboundedSender<TaskQueueEntity>`，用于后续投递任务
    fn create_priority_channel_task(
        ctx: &'static crate::context::Context,
        priority: u8,
        running_tasks: RunningTasks,
        semaphore: Arc<Semaphore>,
        category_limit: HashMap<TaskType, usize>,
    ) -> mpsc::UnboundedSender<TaskQueueEntity> {
        let (tx, mut rx) = mpsc::unbounded_channel::<TaskQueueEntity>();
        // let tx_c = tx.clone(); // 不需要重排但保留 clone 用于其他情况
        tokio::spawn(async move {
            let category_counter = Arc::new(Mutex::new(HashMap::<TaskType, usize>::new()));

            while let Some(task_entity) = rx.recv().await {
                let category_counter = category_counter.clone();
                let running_tasks = running_tasks.clone();
                let semaphore = semaphore.clone();
                let category_limit = category_limit.clone();
                tokio::spawn(async move {
                    Self::handle_task_entity(
                        ctx,
                        priority,
                        task_entity,
                        category_counter,
                        &category_limit,
                        running_tasks,
                        semaphore,
                    )
                    .await;
                });
            }
            tracing::warn!("优先级通道消费者退出，该通道已关闭");
        });
        tx
    }

    /// 执行任务处理逻辑，包含：
    /// - 任务类型限速
    /// - 并发控制（Semaphore）
    /// - 去重（RunningTasks）
    /// - 任务调度（spawn）
    async fn handle_task_entity(
        ctx: &'static crate::context::Context,
        priority: u8,
        task_entity: TaskQueueEntity,
        category_counter: Arc<Mutex<HashMap<TaskType, usize>>>,
        category_limit: &HashMap<TaskType, usize>,
        running_tasks: RunningTasks,
        semaphore: Arc<Semaphore>,
    ) {
        if task_entity.remark.is_some() {
            let ownership = match ctx.task_pool() {
                Ok(pool) => {
                    TaskQueueRepo::externally_owned(&pool, task_entity.remark.as_deref()).await
                }
                Err(error) => {
                    tracing::warn!(%error,"routing unavailable; durable record retained");
                    return;
                }
            };
            match ownership {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(%error,"routing unavailable; durable record retained");
                    return;
                }
            }
        }
        let task_id = &task_entity.id;

        let task: Box<dyn TaskTrait> = match (&task_entity).try_into() {
            Ok(t) => t,
            Err(_) => {
                tracing::warn!("任务解析失败，跳过：id = {:?}", task_id);
                return;
            } // 转换失败直接跳过
        };

        // let task: Task = match (&task_entity).try_into() {
        //     Ok(t) => t,
        //     Err(_) => {
        //         tracing::warn!("任务解析失败，跳过：id = {:?}", task_id);
        //         return;
        //     } // 转换失败直接跳过
        // };

        // === 限速逻辑 ===
        let category = task.get_type();

        // let limiter = get_rate_limiter(&category);

        // limiter.until_ready().await;

        let limit = *category_limit.get(&category).unwrap_or(&1);

        // === 等待类型限速窗口 ===
        loop {
            let mut counter = category_counter.lock().await;
            let count = counter.entry(category.clone()).or_insert(0);
            if *count < limit {
                *count += 1;
                // tracing::info!(
                //     "任务类型 {:?} 优先级 {} 未达到限速上限 ({}/{}), 待执行：{}",
                //     category,
                //     priority,
                //     *count,
                //     limit,
                //     task_id
                // );
                break;
            } else {
                tracing::debug!(
                    "任务类型 {:?} 优先级 {} 达到限速上限 ({}/{}), 等待中：{}",
                    category,
                    priority,
                    *count,
                    limit,
                    task_id
                );
            }
            drop(counter); // 释放锁，避免死锁
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }

        // === 去重逻辑 + 并发控制 ===
        let task_id = task_entity.id.clone();
        if running_tasks.insert(task_id.clone()) {
            // tracing::info!(
            //     "准备执行任务 {}，类型 = {:?}，优先级 = {}, 当前并发 = {}",
            //     task_id,
            //     category,
            //     priority,
            //     running_tasks.len()
            // );

            // 如果成功插入，说明之前没有该任务，开始处理
            let permit = semaphore.acquire_owned().await.unwrap();
            let running_tasks_inner = running_tasks.clone();
            let category_counter = category_counter.clone();

            tokio::spawn(async move {
                tracing::debug!("开始执行任务 {}", task_id);
                TaskManager::process_single_task(ctx, task_entity, running_tasks_inner).await;
                let mut counter = category_counter.lock().await;
                if let Some(count) = counter.get_mut(&category) {
                    *count = count.saturating_sub(1);
                    // tracing::info!(?category, current = *count, "任务计数 -1");
                }
                drop(permit); // 释放信号量
                tracing::debug!("任务 {} 执行完成", task_id);
            });

            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        } else {
            tracing::debug!("任务 {} 已在运行中，跳过重复执行", task_id);
        }
    }
}

// struct CoreThread {}

// impl CoreThread {
//     fn new() -> Self {
//         Self {}
//     }
// }
