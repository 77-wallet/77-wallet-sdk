// scanner.rs
// 🔴 核心设计原则（**必须严格遵守，否则将导致不可恢复的数据破坏**）
// 🔴 1. Scanner 只基于 DB 事实推进状态
// 🔴 2. DB 中不再依赖"正在做"的动作状态做决策
// 🔴 3. Worker/Executor 永远不参与状态决策
// 🔴 4. 系统可重启、可重复扫描、可自愈
// 🔴 5. Create/Init操作必须幂等，否则Scanner并发不安全
// 🔴 6. 禁止引入 wait_system_ready 作为全局门闩
// 🔴 7. 禁止用内存标记代替 DB 事实
// 🔴 8. 状态推进只能由 Scanner 基于 DB 强事实触发
// ExpandScanner 是一个事实协调器，不是工作流引擎。
// 它可能会重新派发副作用。
// 正确性完全依赖于数据库事实和幂等性。
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use futures::FutureExt;

use crate::{
    error::{service::ServiceError, system::SystemError},
    infrastructure::expand_address::{
        event::ExpandEvent,
        planner::ExpandPlanner,
        worker::{ExpandJob, WORKER_POOL},
    },
};
use wallet_database::{
    entities::expand_batch::ExpandBatchEntity,
    repositories::api_wallet::{
        expand_batch::ExpandBatchRepo, expand_batch_item::ExpandBatchItemRepo,
    },
};

/// 单轮扫描每个batch最多处理的item数量
/// 确保多batch能在同一轮scan中得到处理，提高并发度
/// 目前已拆分create和init的配额，该常量不再使用
#[allow(dead_code)]
const MAX_ITEMS_PER_BATCH_PER_SCAN: usize = 10;

/// 任务派发键，用于唯一标识一个派发任务
///
/// 🔴 优化建议：
/// - 当前key包含多个String，在大规模batch下HashSet压力较大
/// - 中期建议：使用(batch_id, phase)的紧凑key
/// - 或考虑intern batch_id / uid，减少clone成本
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ExpandDispatchKey {
    pub batch_id: String,
    pub phase: ExpandDispatchPhase,
}

/// 任务派发阶段
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum ExpandDispatchPhase {
    Create,
    Init,
    Notify,
}

/// 任务执行结果事件
pub enum ExpandJobResult {
    Succeeded { key: ExpandDispatchKey, indexes: Vec<i32> },
    Failed { key: ExpandDispatchKey, error: String, indexes: Vec<i32> },
}

/// RuntimeDispatchGuard - 运行期调度事实管理
///
/// 设计语义：
/// - 仅用于抑制"同一时间"的重复派发
/// - 不保存事实，不代表任务是否完成或成功
/// - 不保证跨 scan / 重启 的派发去重
///
/// 重要不变量：
/// - 所有"是否需要派发"的判断，必须基于 DB 事实
/// - runtime 的 in_flight 仅用于防止并发副作用
///
/// 容错语义：
/// - 系统允许在 crash / 重启 后重新派发副作用
/// - 副作用必须具备幂等性，由 DB 事实最终收敛
///
/// 注意：
/// - in_flight 可能因进程崩溃而泄漏，但不会影响系统正确性
/// - 泄漏只影响吞吐，不影响状态推进和收敛性
/// - 只影响并发度，不影响最终状态
#[allow(dead_code)]
struct ExpandDispatchRuntime {
    // 主存储：key映射到时间戳，用于清理
    in_flight: HashMap<ExpandDispatchKey, Instant>,
    // 二级索引：(batch_id, phase) → index集合，用于快速查询
    in_flight_indexes: HashMap<(String, ExpandDispatchPhase), HashSet<i32>>,
    max_in_flight_create: usize, // CREATE操作上限
    max_in_flight_init: usize,   // INIT操作上限
    cleanup_threshold: Duration, // 清理阈值
}

impl ExpandDispatchRuntime {
    /// 创建新的调度运行时
    fn new() -> Self {
        Self {
            in_flight: HashMap::new(),
            in_flight_indexes: HashMap::new(),
            max_in_flight_create: 1000,                  // CREATE操作上限
            max_in_flight_init: 40,                      // INIT操作上限
            cleanup_threshold: Duration::from_secs(300), // 5分钟清理阈值
        }
    }

    /// 判断是否应该派发任务
    /// 仅用于防止同一轮扫描内重复派发
    fn should_dispatch(&self, key: &ExpandDispatchKey) -> bool {
        // 检查该key是否已在飞行中
        !self.in_flight.contains_key(key)
    }

    /// 批量添加in-flight索引
    fn add_in_flight_batch(&mut self, key: &ExpandDispatchKey, indexes: &[i32]) {
        // 1. 添加到主存储（只记录最后派发时间）
        self.in_flight.insert(key.clone(), Instant::now());

        // 2. 添加到二级索引
        let index_key = (key.batch_id.clone(), key.phase.clone());
        let index_set = self.in_flight_indexes.entry(index_key).or_insert(HashSet::new());
        index_set.extend(indexes.iter().copied());
    }

    /// 批量移除in-flight索引
    fn remove_in_flight_batch(&mut self, key: &ExpandDispatchKey, indexes: &[i32]) {
        // 1. 从二级索引移除指定indexes
        let index_key = (key.batch_id.clone(), key.phase.clone());
        if let Some(index_set) = self.in_flight_indexes.get_mut(&index_key) {
            for &index in indexes {
                index_set.remove(&index);
            }
            // 如果该分组已无索引，移除该分组
            if index_set.is_empty() {
                self.in_flight_indexes.remove(&index_key);
                // 同时从主存储移除
                self.in_flight.remove(key);
            }
        }
    }

    /// 处理任务执行结果
    fn on_result(&mut self, result: ExpandJobResult) {
        // 无论结果如何，都从 in_flight 中移除对应的indexes
        match result {
            ExpandJobResult::Succeeded { key, indexes } => {
                // 使用批量移除方法
                self.remove_in_flight_batch(&key, &indexes);
            }
            ExpandJobResult::Failed { key, error, indexes } => {
                tracing::warn!(error = %error, indexes = ?indexes, "ExpandScanner: worker result failed, clearing in-flight state");
                // 使用批量移除方法
                self.remove_in_flight_batch(&key, &indexes);
            }
        }
    }

    /// 清理过期的in-flight keys
    fn cleanup_stale(&mut self) {
        let now = Instant::now();
        let mut stale_keys = Vec::new();

        // 1. 找出过期的keys
        for (key, timestamp) in &self.in_flight {
            if now.duration_since(*timestamp) >= self.cleanup_threshold {
                stale_keys.push(key.clone());
            }
        }

        // 2. 移除过期的keys
        for key in stale_keys {
            self.in_flight.remove(&key);

            // 从二级索引移除
            let index_key = (key.batch_id.clone(), key.phase.clone());
            self.in_flight_indexes.remove(&index_key);
        }
    }

    /// 获取指定批次、阶段的in-flight索引集合
    fn get_in_flight_indexes(&self, batch_id: &str, phase: ExpandDispatchPhase) -> HashSet<i32> {
        let index_key = (batch_id.to_string(), phase);
        self.in_flight_indexes.get(&index_key).cloned().unwrap_or_default()
    }
}

/// ExpandScanner - 定时扫描并推进状态，遵循严格的节流语义
///
/// � 核心语义：
/// - ExpandScanner is NOT a real-time system.
/// - Progress is eventual and throttled by design.
/// - scan_interval 是吞吐控制参数，不是 SLA 参数
///
/// 🔒 不变量：
/// - Scanner不创建Item，Item的创建权只属于Planner
/// - Scanner只基于不可逆事实追平Batch状态，Batch状态由Planner和Item完成状态驱动
/// - Scanner处理Running和Done状态的Batch
/// - address_query_state是扩容系统的唯一时间闸门
/// - 扩容系统永远不尝试与查询系统并发协作，只接受其最终事实
///
/// 🔴 核心驱动：
/// - 每N秒执行一次扫描
/// - 扫描所有非Done/Failed的items
/// - 基于DB事实推进状态
/// - 派生batch状态
/// - recover机制：启动时立即执行一次扫描
///
/// 🔴 核心约束：
/// 1. 状态推进规则：所有状态更新使用compare-and-swap
/// 2. **节流语义**：单轮扫描设置上限，通过多轮扫描完成全量推进
/// 3. finished_count仅为缓存字段：不参与业务判断，只用于展示
/// 4. **事实驱动**：所有状态推进基于现有数据库实体，不依赖外部事件
/// 5. **幂等性要求**：Create/Init操作必须幂等，否则Scanner并发不安全
///
/// 🔴 单轮上限/节流机制：
/// - **max_items_per_scan**：每轮扫描处理的最大item数量（默认100）
/// - **分页处理**：使用LIMIT/OFFSET或cursor-based分页避免单次扫描压力
/// - **分批推进**：多轮扫描完成全量状态推进
/// - **资源保护**：防止DB/节点/RPC被瞬间高并发请求打爆
/// - **自适应调整**：可根据系统负载动态调整单轮上限
///
/// 🔴 设计意图：
/// - 防止重启时一次性扫描大量items导致系统过载
/// - 平滑系统负载，避免资源峰值
/// - 提高系统在大规模数据场景下的可靠性
/// - 支持水平扩展，可通过增加扫描频率而非单次处理量来提升吞吐量
/// - 便于监控和调试，单轮处理量可控
/// - 确保系统可恢复性，不依赖历史状态
///
/// 🔴 关键设计原则：
/// - Scanner scans ALL items that are not Done/Failed
/// - Status does NOT participate in dispatch decision
/// - Status is only a convergence marker
/// - This scanner is a fact reconciler, not a workflow engine
/// - It may re-dispatch side effects multiple times
/// - Correctness relies solely on DB facts and idempotency
pub struct ExpandScanner {
    ctx: &'static crate::context::Context,
    scan_interval: Duration,
    planner: ExpandPlanner,
    max_items_per_scan: u32, // 单轮扫描上限
    event_rx: Option<tokio::sync::mpsc::Receiver<ExpandEvent>>, // 事件接收器，支持事件触发扫描
    need_scan: Arc<AtomicBool>, // 标记是否需要扫描
    // Atomic flag used only for wake-up coalescing.
    // Does NOT guard any shared data, so Relaxed ordering is sufficient.
    notify: Arc<tokio::sync::Notify>, // 通知器，用于唤醒扫描循环
    // 添加原子变量控制并发
    scanning: AtomicBool,
    // 任务执行结果发送器
    result_tx: tokio::sync::mpsc::UnboundedSender<ExpandJobResult>,
    // 任务执行结果接收器
    result_rx: Option<tokio::sync::mpsc::UnboundedReceiver<ExpandJobResult>>,
    // 运行期调度事实管理
    runtime: ExpandDispatchRuntime,
}

impl ExpandScanner {
    pub fn new(
        ctx: &'static crate::context::Context,
        scan_interval: Duration,
        max_items_per_scan: u32,
        event_rx: Option<tokio::sync::mpsc::Receiver<ExpandEvent>>,
    ) -> Self {
        // 先克隆pool，避免移动后借用
        let planner = ExpandPlanner::new(ctx, None);
        let need_scan = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(tokio::sync::Notify::new());

        // 创建结果通道
        let (result_tx, result_rx) = tokio::sync::mpsc::unbounded_channel::<ExpandJobResult>();

        Self {
            ctx,
            scan_interval,
            planner,
            max_items_per_scan,
            event_rx,
            need_scan,
            notify,
            scanning: AtomicBool::new(false),      // 初始化原子变量
            result_tx,                             // 初始化结果发送器
            result_rx: Some(result_rx),            // 初始化结果接收器
            runtime: ExpandDispatchRuntime::new(), // 初始化调度运行时
        }
    }

    /// 处理任务执行结果
    fn drain_results(&mut self) {
        if let Some(ref mut rx) = self.result_rx {
            while let Ok(result) = rx.try_recv() {
                self.runtime.on_result(result);
            }
        }
    }

    /// 启动扫描器，开始定时执行
    ///
    /// start():
    /// - owns event / interval triggers
    /// - transfers scanner ownership to scan loop
    /// - MUST NOT call scan() directly
    pub async fn start(mut self) {
        tracing::trace!(interval = ?self.scan_interval, max_items_per_scan = self.max_items_per_scan, "ExpandScanner: starting");

        // Invariant: scan() is never executed concurrently
        // 🔒 不变量：scan()方法永远不会被并发执行

        // 先获取需要的字段
        let event_rx = self.event_rx.take();
        let need_scan = self.need_scan.clone();
        let notify = self.notify.clone();
        let scan_interval = self.scan_interval;

        // 将scanner完全移交给scan loop
        tokio::spawn(async move {
            self.run_scan_loop().await;
        });

        // 定时扫描
        let mut interval = tokio::time::interval(scan_interval);

        if let Some(mut event_rx) = event_rx {
            // 双驱动模式：事件 + 定时
            // 外层只负责触发信号，不直接使用scanner
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        tracing::debug!("ExpandScanner: triggered by interval");
                        need_scan.store(true, Ordering::Relaxed);
                        notify.notify_one();
                    },
                    Some(event) = event_rx.recv() => {
                        match event{
                            ExpandEvent::HintScan => {
                                tracing::debug!(?event, "ExpandScanner: triggered by event");
                                                        need_scan.store(true, Ordering::Relaxed);
                                                        notify.notify_one();
                            },
                        }
                    },
                }
            }
        } else {
            // 传统单驱动模式：仅定时
            // 外层只负责触发信号，不直接使用scanner
            loop {
                interval.tick().await;
                tracing::debug!("ExpandScanner: triggered by interval (single mode)");
                need_scan.store(true, Ordering::Relaxed);
                notify.notify_one();
            }
        }
    }

    /// 执行一次完整扫描，严格遵循节流语义
    ///
    /// 🔴 核心流程：
    /// 0. 处理任务执行结果（回调即事实输入）
    /// 1. 调用 Planner（唯一允许推进 Pending → Running 的组件，Scanner 不参与任何决策）
    /// 2. 扫描所有非 Done / Failed 的 items
    /// 3. 基于 DB 强事实判断：
    ///    - account 不存在 → 派发 Create 副作用
    ///    - account 存在但未 init → 派发 Init 副作用
    ///    - account 已 init → 推进到 Done
    /// 4. 更新batch状态和finished_count缓存
    /// 5. 处理所有Done状态的批次，派发通知任务
    ///
    /// 🔴 注意：
    /// scan() 是 Scanner 的核心生命体征，只能由 run_scan_loop 调用
    /// 严格禁止外部直接调用
    // #[instrument(skip(self))]
    async fn scan(&mut self) -> Result<(), ServiceError> {
        tracing::trace!(
            max_items_per_scan = self.max_items_per_scan,
            "ExpandScanner: starting scan with throttling"
        );

        // 0. 清理过期的in-flight keys，防止内存泄漏
        self.runtime.cleanup_stale();

        // 1. 处理任务执行结果（回调即事实输入）- 扫描前
        self.drain_results();

        // 1. Planner：推进 Pending Batch → Running + create items
        // 🔒 核心逻辑：Planner是系统的"启动电机"，负责创建Item
        // 🔒 NOTE：Planner在概念上是独立于Scanner的组件
        // 🔒 在这里调用只是为了恢复方便，而非Scanner的核心职责
        if let Err(e) = self.planner.plan_all_batches().await {
            tracing::error!(error = %e, "ExpandScanner: planner failed");
            // Planner失败不影响后续扫描
            // Planner failure does NOT block Scanner progress.
            // Scanner always reconciles existing facts.
        }

        // 🔒 修复3：Scanner的processed_items计数问题
        // 🔒 补充修订B：Scanner的quota应该是「一次scan的全局硬上限」，所有scan_xxx()都消耗它
        // 🔒 全局processed_items计数器，用于限制单轮扫描的总items数量
        // 🔒 设计权衡：所有未完成状态共用quota
        // 🔒 优点：实现简单，全局控制资源使用
        // 🔒 缺点：可能导致饥饿问题，但基于事实驱动，不会永久阻塞
        let mut processed_items = 0;

        // 2. 扫描所有未完成的items，基于DB事实推进状态（不依赖任何中间状态）
        self.scan_unfinished_items_by_db_fact(&mut processed_items).await?;

        // 3. 执行batch状态派生（更新finished_count缓存）
        self.scan_batches().await?;

        // 4. 处理所有Done状态的批次，派发通知任务
        // IMPORTANT:
        // handle_done_batches MUST be called after scan_batches()
        // because Done → Notified is derived strictly from DB facts
        self.handle_done_batches().await?;

        // 0. 处理任务执行结果（回调即事实输入）- 扫描后
        self.drain_results();

        tracing::debug!(
            processed_items = processed_items,
            "ExpandScanner: scan completed with throttling"
        );
        Ok(())
    }

    /// 扫描所有非Done/Failed状态的items，基于DB事实推进状态
    ///
    /// 🔴 核心设计原则：
    /// - Scanner scans ALL items that are not Done/Failed
    /// - Status does NOT participate in dispatch decision
    /// - Status is only a convergence marker
    /// - This scanner is a fact reconciler, not a workflow engine
    /// - It may re-dispatch side effects multiple times
    /// - Correctness relies solely on DB facts and idempotency
    ///
    /// 🔴 Create/Init操作必须幂等，否则Scanner并发不安全
    /// 🔴 原因：Scanner可能并发执行或多次执行，同一个item可能被重复发送create/init任务
    /// 🔴 后果：若Create/Init操作非幂等，将导致不可恢复的数据破坏和状态不一致
    /// 🔴 这是当前设计必须依赖的前提，不是未来优化项
    ///
    /// 🔴 参数说明：
    /// - processed_items: 全局计数器，用于限制单轮扫描的总items数量
    /// - 公平性权衡：
    ///   ✅ 优点：实现简单，全局控制资源使用，避免单batch无限占用
    ///   ❌ 缺点：可能导致饥饿问题（某些batch长期被挤出扫描窗口）
    ///   ⚠️  设计决策：这是明确的trade-off，基于事实驱动的设计不会导致永久阻塞
    ///   ⚠️  未来优化：可根据实际情况调整为更公平的quota分配策略
    ///
    /// NOTE:
    /// This method does NOT guarantee fairness across batches.
    /// Ordering is best-effort and bounded by global quota.
    /// 防止未来有人问："为什么这个batch总是慢？"
    async fn scan_unfinished_items_by_db_fact(
        &mut self,
        processed_items: &mut usize,
    ) -> Result<(), ServiceError> {
        tracing::debug!("ExpandScanner: scanning unfinished items by DB fact");

        let pool = self.ctx.api_wallet_pool()?;
        // 获取需要进行item reconciliation的批次（事实驱动）
        // 只处理 status 为 Running 但 local_complete_at 已设置的批次
        let batches = ExpandBatchRepo::get_batches_for_item_reconcile(&pool).await?;

        for batch in batches {
            tracing::debug!(
                batch_id = %batch.batch_id,
                chain_code = %batch.chain_code,
                status = ?batch.status,
                local_complete_at = ?batch.local_complete_at,
                total_count = batch.total_count,
                "ExpandScanner: processing batch for item reconciliation"
            );

            // 🔒 设计：全局 quota + 批次配额 + 顺序 batch 扫描
            // 🔒 为每个batch分配固定配额，确保多batch能同时推进
            // 🔒 计算全局剩余配额
            let global_remaining =
                self.max_items_per_scan.saturating_sub(*processed_items as u32) as usize;
            tracing::debug!(
                batch_id = %batch.batch_id,
                current_processed = *processed_items,
                max_per_scan = self.max_items_per_scan,
                global_remaining = global_remaining,
                "ExpandScanner: calculated global quota for batch"
            );

            if global_remaining == 0 {
                tracing::debug!(
                    processed_items = *processed_items,
                    max_items_per_scan = self.max_items_per_scan,
                    "ExpandScanner: reached max items per scan, stopping"
                );
                break;
            }

            // 添加常量定义
            const INIT_DISPATCH_COOLDOWN_SEC: i64 = 20;
            const MAX_INIT_PER_ROUND: i64 = 2000;

            // 获取当前批次正在执行的CREATE和INIT任务的indexes
            tracing::debug!(
                batch_id = %batch.batch_id,
                "ExpandScanner: getting in-flight indexes for batch"
            );

            let in_flight_create_indexes =
                self.runtime.get_in_flight_indexes(&batch.batch_id, ExpandDispatchPhase::Create);
            let in_flight_init_indexes =
                self.runtime.get_in_flight_indexes(&batch.batch_id, ExpandDispatchPhase::Init);

            tracing::debug!(
                batch_id = %batch.batch_id,
                create_in_flight_count = in_flight_create_indexes.len(),
                init_in_flight_count = in_flight_init_indexes.len(),
                in_flight_create_indexes = ?in_flight_create_indexes,
                in_flight_init_indexes = ?in_flight_init_indexes,
                "ExpandScanner: retrieved in-flight indexes"
            );

            // 合并所有in-flight indexes
            let mut all_in_flight_indexes = Vec::new();
            all_in_flight_indexes.extend(in_flight_create_indexes);
            all_in_flight_indexes.extend(in_flight_init_indexes);

            tracing::debug!(
                batch_id = %batch.batch_id,
                total_in_flight = all_in_flight_indexes.len(),
                all_in_flight_indexes = ?all_in_flight_indexes,
                "ExpandScanner: merged all in-flight indexes"
            );

            // 使用新的查询方法，直接按 fact_state 分组获取索引列表，排除in-flight indexes
            tracing::debug!(
                batch_id = %batch.batch_id,
                init_cooldown = INIT_DISPATCH_COOLDOWN_SEC,
                max_init_per_round = MAX_INIT_PER_ROUND,
                excluded_indexes_count = all_in_flight_indexes.len(),
                "ExpandScanner: querying items grouped by fact state"
            );

            let items_grouped = ExpandBatchItemRepo::get_items_grouped_by_fact_state(
                &pool,
                &batch.batch_id,
                INIT_DISPATCH_COOLDOWN_SEC,
                MAX_INIT_PER_ROUND,
                &all_in_flight_indexes, // 新增参数：排除正在执行的indexes
            )
            .await?;

            tracing::debug!(
                batch_id = %batch.batch_id,
                total_groups = items_grouped.len(),
                create_items_count = items_grouped.iter().filter(|g| g.fact_state == 0).map(|g| g.indexes.len()).sum::<usize>(),
                init_items_count = items_grouped.iter().filter(|g| g.fact_state == 1).map(|g| g.indexes.len()).sum::<usize>(),
                done_items_count = items_grouped.iter().filter(|g| g.fact_state == 2).map(|g| g.indexes.len()).sum::<usize>(),
                "ExpandScanner: retrieved items grouped by fact state"
            );

            // 记录每个分组的详细信息
            for (group_idx, group) in items_grouped.iter().enumerate() {
                tracing::debug!(
                    batch_id = %batch.batch_id,
                    group_idx = group_idx,
                    fact_state = group.fact_state,
                    indexes_json = %group.indexes,
                    indexes_count = group.indexes.len(),
                    "ExpandScanner: group details"
                );
            }

            // 🔒 为当前batch分配独立配额：create和init独立节流
            let batch_create_quota = 1000usize; // create允许1000个/批次
            tracing::debug!(
                batch_id = %batch.batch_id,
                batch_create_quota = batch_create_quota,
                "ExpandScanner: allocated batch quotas"
            );

            // 预分配Vec容量，减少realloc
            let mut init_indices = Vec::with_capacity(1000); // 初始容量设为1000，后续会根据剩余配额调整
            let mut create_indices = Vec::with_capacity(batch_create_quota);
            let mut done_indices = Vec::new();
            tracing::debug!(
                batch_id = %batch.batch_id,
                "ExpandScanner: initialized processing vectors"
            );

            // 处理分组结果
            tracing::debug!(
                batch_id = %batch.batch_id,
                "ExpandScanner: starting to process grouped items"
            );

            for (group_idx, group) in items_grouped.into_iter().enumerate() {
                tracing::debug!(
                    batch_id = %batch.batch_id,
                    group_idx = group_idx,
                    fact_state = group.fact_state,
                    "ExpandScanner: processing group"
                );

                // 将JSON格式的索引列表转换为Vec<i32>
                let indices: Vec<i32> = wallet_utils::serde_func::serde_from_str(&group.indexes)
                    .map_err(|e| ServiceError::System(SystemError::Internal(e.to_string())))?;

                tracing::trace!(
                    batch_id = %batch.batch_id,
                    group_idx = group_idx,
                    fact_state = group.fact_state,
                    parsed_indices_count = indices.len(),
                    parsed_indices = ?indices,
                    "ExpandScanner: parsed indices from JSON"
                );

                match group.fact_state {
                    0 => {
                        // CREATE：账户不存在，需要发送创建任务
                        tracing::debug!(batch_id = %batch.batch_id, fact_state = 0, indices_count = indices.len(), current_create_buffer = create_indices.len(), batch_create_quota = batch_create_quota, "ExpandScanner: processing CREATE items");
                        // 只取前batch_create_quota个
                        let take_count = batch_create_quota.min(indices.len());
                        let create_indices_chunk = &indices[..take_count];
                        create_indices.extend_from_slice(create_indices_chunk);
                        *processed_items += take_count;

                        tracing::debug!(
                            batch_id = %batch.batch_id,
                            fact_state = 0,
                            took_count = take_count,
                            remaining_create_quota = batch_create_quota.saturating_sub(create_indices.len()),
                            new_create_buffer = create_indices.len(),
                            new_processed_count = *processed_items,
                            "ExpandScanner: processed CREATE items"
                        );
                    }
                    1 => {
                        // INIT：账户存在但未初始化，需要发送初始化任务
                        tracing::debug!(batch_id = %batch.batch_id, fact_state = 1, indices_count = indices.len(), current_init_buffer = init_indices.len(), "ExpandScanner: processing INIT items");
                        // 计算剩余配额
                        let remaining_quota =
                            self.max_items_per_scan.saturating_sub(*processed_items as u32)
                                as usize;

                        tracing::debug!(
                            batch_id = %batch.batch_id,
                            fact_state = 1,
                            global_remaining = remaining_quota,
                            "ExpandScanner: checking remaining quota for INIT items"
                        );

                        if remaining_quota > 0 {
                            // 只取剩余配额内的INIT任务
                            let take_count = remaining_quota.min(indices.len());
                            let init_indices_chunk = &indices[..take_count];
                            init_indices.extend_from_slice(init_indices_chunk);
                            *processed_items += take_count;

                            tracing::debug!(
                                batch_id = %batch.batch_id,
                                fact_state = 1,
                                took_count = take_count,
                                new_init_buffer = init_indices.len(),
                                new_processed_count = *processed_items,
                                "ExpandScanner: processed INIT items"
                            );
                        } else {
                            tracing::debug!(
                                batch_id = %batch.batch_id,
                                fact_state = 1,
                                "ExpandScanner: skipped INIT items due to insufficient quota"
                            );
                        }
                    }
                    2 => {
                        // DONE：账户已初始化，需要推进到Done状态
                        tracing::debug!(batch_id = %batch.batch_id, fact_state = 2, indices_count = indices.len(), current_done_buffer = done_indices.len(), "ExpandScanner: processing DONE items");
                        done_indices.extend(indices);

                        tracing::debug!(
                            batch_id = %batch.batch_id,
                            fact_state = 2,
                            new_done_buffer = done_indices.len(),
                            "ExpandScanner: processed DONE items"
                        );
                    }
                    _ => {
                        tracing::warn!(batch_id = %batch.batch_id, fact_state = %group.fact_state, "ExpandScanner: unknown fact_state");
                    }
                }
            }

            tracing::trace!(
                batch_id = %batch.batch_id,
                final_create_buffer = create_indices.len(),
                final_init_buffer = init_indices.len(),
                final_done_buffer = done_indices.len(),
                total_processed = *processed_items,
                "ExpandScanner: completed processing all groups"
            );

            // 批量处理Done状态的items
            if !done_indices.is_empty() {
                tracing::debug!(
                    batch_id = %batch.batch_id,
                    done_count = done_indices.len(),
                    done_indices = ?done_indices,
                    "ExpandScanner: starting to mark items as Done in batch"
                );
                let updated = ExpandBatchItemRepo::dispatched_to_done_if_fact_match(
                    &pool,
                    &batch.batch_id,
                    &done_indices,
                )
                .await?;
                tracing::debug!(
                    batch_id = %batch.batch_id,
                    done_count = done_indices.len(),
                    updated = updated,
                    "ExpandScanner: completed marking items as Done"
                );
            } else {
                tracing::debug!(batch_id = %batch.batch_id, "ExpandScanner: no Done items to process");
            }

            // 批量发送初始化任务
            if !init_indices.is_empty() {
                tracing::debug!(
                    batch_id = %batch.batch_id,
                    init_count = init_indices.len(),
                    init_indices = ?init_indices,
                    "ExpandScanner: starting to send init jobs batch"
                );
                self.send_init_jobs_batch(&batch, &init_indices).await?;
                tracing::debug!(
                    batch_id = %batch.batch_id,
                    init_count = init_indices.len(),
                    "ExpandScanner: completed sending init jobs batch"
                );
            } else {
                tracing::debug!(batch_id = %batch.batch_id, "ExpandScanner: no init jobs to send");
            }

            // 批量发送创建任务
            if !create_indices.is_empty() {
                tracing::debug!(
                    batch_id = %batch.batch_id,
                    create_count = create_indices.len(),
                    create_indices = ?create_indices,
                    "ExpandScanner: starting to send create jobs batch"
                );
                self.send_create_jobs_batch(&batch, &create_indices).await?;
                tracing::debug!(
                    batch_id = %batch.batch_id,
                    create_count = create_indices.len(),
                    "ExpandScanner: completed sending create jobs batch"
                );
            } else {
                tracing::debug!(batch_id = %batch.batch_id, "ExpandScanner: no create jobs to send");
            }

            // 🔴 关键修改：立即追平当前batch的状态，实现真正的多链并发
            // 不再等待所有batch处理完，而是每个batch处理完items后立即更新状态
            tracing::debug!(batch_id = %batch.batch_id, "ExpandScanner: starting batch state reconciliation");
            let became_done = self.reconcile_single_batch_state(&batch).await?;
            tracing::debug!(
                batch_id = %batch.batch_id,
                became_done = became_done,
                "ExpandScanner: completed batch state reconciliation"
            );

            // 🔴 如果batch刚刚完成，立即触发notify
            // 实现"哪个batch先完成，就先notify"的语义
            if became_done {
                tracing::debug!(
                    batch_id = %batch.batch_id,
                    "ExpandScanner: batch became done, checking if notify needed"
                );
                // 调用notify前，确保batch已经变为Done状态
                // 并且还没有发送过notify
                self.dispatch_notify_job_if_needed(&batch).await?;
                tracing::debug!(
                    batch_id = %batch.batch_id,
                    "ExpandScanner: completed notify job dispatch if needed"
                );
            } else {
                tracing::debug!(
                    batch_id = %batch.batch_id,
                    "ExpandScanner: batch not done, skipping notify"
                );
            }
        }

        tracing::trace!(
            processed_items = *processed_items,
            "ExpandScanner: completed scanning unfinished items by DB fact"
        );
        Ok(())
    }

    /// 批量发送创建账户任务
    async fn send_create_jobs_batch(
        &mut self,
        batch: &wallet_database::entities::expand_batch::ExpandBatchEntity,
        indices: &[i32],
    ) -> Result<(), ServiceError> {
        // IMPORTANT:
        // Scanner does NOT advance item status when dispatching Create/Init.
        // State convergence relies solely on DB facts observed in later scans.
        // 防止未来有人误以为："发了job就等于推进了状态"
        tracing::debug!(batch_id = %batch.batch_id, indices_count = indices.len(), "ExpandScanner: sending create jobs");

        // 使用batch级dispatch key
        let key = ExpandDispatchKey {
            batch_id: batch.batch_id.clone(),
            phase: ExpandDispatchPhase::Create,
        };

        // 创建包含所有indexes的job
        let job = ExpandJob::new_create(
            self.ctx,
            batch.uid.clone(),
            batch.chain_code.clone(),
            batch.batch_id.clone(),
            indices.to_vec(), // 单次发送所有indexes
            key.clone(),
            self.result_tx.clone(), // 使用scanner的sender
        );

        // 使用try_send替代await send，避免阻塞
        match WORKER_POOL.tx.try_send(job) {
            Ok(_) => {
                // 任务发送成功，标记为in-flight
                self.runtime.add_in_flight_batch(&key, indices);
                tracing::debug!(batch_id = %batch.batch_id, indices_count = indices.len(), "ExpandScanner: sent create job batch successfully");
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                // 任务队列已满，记录警告日志
                tracing::warn!(batch_id = %batch.batch_id, "ExpandScanner: worker pool full, skipped create job batch, will retry in next scan");
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                // 任务队列已关闭，记录错误日志
                tracing::error!(batch_id = %batch.batch_id, "ExpandScanner: worker pool closed, skipped create job batch, will retry in next scan");
            }
        }
        Ok(())
    }

    /// 批量发送初始化账户任务
    async fn send_init_jobs_batch(
        &mut self,
        batch: &wallet_database::entities::expand_batch::ExpandBatchEntity,
        indices: &[i32],
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        // IMPORTANT:
        // Scanner does NOT advance item status when dispatching Create/Init.
        // State convergence relies solely on DB facts observed in later scans.
        // 防止未来有人误以为："发了job就等于推进了状态"
        tracing::debug!(batch_id = %batch.batch_id, indices_count = indices.len(), "ExpandScanner: sending init jobs");

        // 使用batch级dispatch key
        let key = ExpandDispatchKey {
            batch_id: batch.batch_id.clone(),
            phase: ExpandDispatchPhase::Init,
        };

        // 创建包含所有indexes的job
        let job = ExpandJob::new_init(
            self.ctx,
            batch.uid.clone(),
            batch.chain_code.clone(),
            batch.batch_id.clone(),
            indices.to_vec(), // 单次发送所有indexes
            key.clone(),
            self.result_tx.clone(), // 使用scanner的sender
        );

        // 使用try_send替代await send，避免阻塞
        match WORKER_POOL.tx.try_send(job) {
            Ok(_) => {
                // 任务发送成功，标记为in-flight
                self.runtime.add_in_flight_batch(&key, indices);
                tracing::debug!(batch_id = %batch.batch_id, indices_count = indices.len(), "ExpandScanner: sent init job batch successfully");

                // 批量更新last_init_dispatched_at字段，记录INIT任务的派发时间
                if let Err(e) = ExpandBatchItemRepo::update_last_init_dispatched_at(
                    &pool,
                    &batch.batch_id,
                    indices,
                )
                .await
                {
                    tracing::warn!(batch_id = %batch.batch_id, error = %e, "ExpandScanner: failed to update last_init_dispatched_at");
                }
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                // 任务队列已满，记录警告日志
                tracing::warn!(batch_id = %batch.batch_id, "ExpandScanner: worker pool full, skipped init job batch, will retry in next scan");
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                // 任务队列已关闭，记录错误日志
                tracing::error!(batch_id = %batch.batch_id, "ExpandScanner: worker pool closed, skipped init job batch, will retry in next scan");
            }
        }

        Ok(())
    }

    /// 处理单个batch的状态追平
    ///
    /// 🔴 核心职责：
    /// - 更新单个batch的finished_count缓存
    /// - 检查并追平local_complete_at事实
    /// - 基于事实推进batch到Done状态
    /// - 返回batch是否刚刚变为Done的标志
    ///
    /// 🔴 注意：
    /// - 仅处理单个batch，不涉及全局batch扫描
    /// - 所有状态更新都基于不可逆事实
    async fn reconcile_single_batch_state(
        &mut self,
        batch: &ExpandBatchEntity,
    ) -> Result<bool, ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        // 重新计算finished_count（仅作为缓存）
        let count = ExpandBatchItemRepo::count_done_items(&pool, &batch.batch_id).await?;

        // 更新finished_count
        // finished_count is a derived cache.
        // Rewriting it multiple times is expected and correct.
        ExpandBatchRepo::update_finished_count_cache_only(&pool, &batch.batch_id, count).await?;

        // 检查本地扩容是否已完成（基于local_complete_at事实）
        let is_local_completed =
            ExpandBatchRepo::is_local_completed(&pool, &batch.batch_id).await?;

        // 记录初始状态
        let was_done = is_local_completed;

        // 如果本地扩容已完成，推进batch状态到Done（事实驱动）
        if is_local_completed {
            let updated =
                ExpandBatchRepo::mark_done_if_local_completed(&pool, &batch.batch_id).await?;
            if updated > 0 {
                tracing::trace!(batch_id = %batch.batch_id, affected_rows = updated, "ExpandScanner: batch marked as Done based on local_complete_at fact");
            }
        } else {
            // 🔴 Scanner 事实修复：如果所有items都已完成但local_complete_at未设置，则补写事实
            // 这是 Scanner 的"最终一致性保证"职责
            let updated =
                ExpandBatchRepo::mark_local_complete_if_all_items_done(&pool, &batch.batch_id)
                    .await?;
            if updated > 0 {
                tracing::warn!(batch_id = %batch.batch_id, "ExpandScanner: repaired missing local_complete_at fact - all items done but fact was missing");
            }
            // 推进到Done状态
            let _ = ExpandBatchRepo::mark_done_if_local_completed(&pool, &batch.batch_id).await?;
        }

        // 检查最终状态是否变为Done
        let became_done =
            !was_done && ExpandBatchRepo::is_local_completed(&pool, &batch.batch_id).await?;
        Ok(became_done)
    }

    /// 扫描并派生batch状态
    /// 扫描所有批次，进行状态追平和缓存更新
    ///
    /// 🔴 核心职责：
    /// - 仅负责批次状态的追平（Running → Done）
    /// - 仅负责更新finished_count缓存
    /// - 不负责任何notify任务的派发
    /// - 所有状态更新都基于不可逆事实
    ///
    /// Does NOT scan Done batches.
    /// Done → Notified is handled separately in handle_done_batches()
    // #[instrument(skip(self))]
    async fn scan_batches(&mut self) -> Result<(), ServiceError> {
        tracing::debug!("ExpandScanner: scanning batches");

        let pool = self.ctx.api_wallet_pool()?;
        // 1. 获取所有状态为Running的批次，用于状态追平
        let running_batches = ExpandBatchRepo::get_by_status(
            &pool,
            wallet_database::entities::expand_batch::ExpandBatchStatus::Running,
        )
        .await?;

        // 2. 更新每个批次的finished_count缓存
        for batch in running_batches {
            // 使用新的辅助方法处理单个batch
            let _ = self.reconcile_single_batch_state(&batch).await?;
        }

        Ok(())
    }

    /// 检查并发送通知任务
    ///
    /// 🔴 前置条件（必须严格遵守）：
    /// - 传入的批次必须满足notify派发条件
    /// - Batch.status ∈ {Done}
    /// - 且 "通知事实尚未形成"（expand_complete_at IS NULL）
    /// - 该条件已经在调用方handle_single_done_batch中检查过
    ///
    /// 🔴 严格禁止：
    /// This method MUST NOT perform any fact checking.
    /// Fact reconciliation must happen before calling this method.
    /// 禁止在此方法中添加任何事实检查逻辑
    ///
    /// 事实驱动的通知分发：
    /// - 优先基于expand_complete_at事实
    /// - 确保幂等性，避免重复通知
    /// - 通知的幂等性只依赖事实字段，不依赖状态
    /// - runtime用于防止并发通知
    ///
    /// 注意：expand_complete_at 表示【已成功上报完成】，不是本地扩容完成
    async fn dispatch_notify_job_if_needed(
        &mut self,
        batch: &ExpandBatchEntity,
    ) -> Result<(), ServiceError> {
        // 生成notify的dispatch key
        let key = ExpandDispatchKey {
            batch_id: batch.batch_id.clone(),
            phase: ExpandDispatchPhase::Notify,
        };

        // 检查是否可以派发
        if !self.runtime.should_dispatch(&key) {
            // 任务已在飞行中，跳过
            tracing::debug!(batch_id = %batch.batch_id, "ExpandScanner: notify job already in flight, skipping");
            return Ok(());
        }

        // 创建通知任务
        let job = ExpandJob::new_notify(
            self.ctx,
            batch.uid.clone(),
            batch.chain_code.clone(),
            batch.batch_id.clone(),
            key.clone(),
            self.result_tx.clone(), // 使用scanner的sender
        );

        // 记录notify job分发
        tracing::debug!(batch_id = %batch.batch_id, "SCANNER: dispatching expand job - Notify");

        // 使用try_send替代await send，避免阻塞
        // Notify使用runtime tracking，防止并发通知
        // 1. 幂等由expand_complete_at IS NULL保证
        // 2. runtime防止并发执行
        // 3. 重复notify是安全的，但并发notify可能导致副作用风暴
        match WORKER_POOL.tx.try_send(job) {
            Ok(_) => {
                // 任务发送成功，标记为in-flight
                self.runtime.add_in_flight_batch(&key, &[]); // 通知任务没有indexes
                tracing::debug!(batch_id = %batch.batch_id, "ExpandScanner: sent notify job successfully");
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(batch_id = %batch.batch_id, "ExpandScanner: worker pool full, skipped notify job, will retry in next scan");
                // 继续执行，不返回错误，下一轮扫描会重试
            }
            Err(e) => {
                tracing::warn!(batch_id = %batch.batch_id, error = %e, "ExpandScanner: failed to send notify job");
                // 继续执行，不返回错误，下一轮扫描会重试
            }
        }

        Ok(())
    }

    /// 处理所有Done状态的批次，派发通知任务
    ///
    /// 🔴 核心职责：
    /// - 扫描所有status = Done的批次
    /// - 派发扩容完成通知任务
    /// - 不负责推进到Notified状态
    /// - Notified状态只能由通知执行者在成功后推进
    ///
    /// NOTE:
    /// This is the ONLY place where notify jobs are dispatched.
    /// Scanner must never dispatch notify jobs elsewhere.
    /// 这是防未来误改的「保险丝」
    // #[instrument(skip(self))]
    async fn handle_done_batches(&mut self) -> Result<(), ServiceError> {
        tracing::debug!("ExpandScanner: handling done batches");

        let pool = self.ctx.api_wallet_pool()?;
        // 获取所有Done状态的批次
        let done_batches = ExpandBatchRepo::get_all_done(&pool).await?;

        for batch in done_batches {
            // 串行处理每个batch，避免并发重复执行expand_complete
            if let Err(e) = self.handle_single_done_batch(&batch).await {
                tracing::error!(batch_id = %batch.batch_id, error = %e, "ExpandScanner: failed to handle done batch");
            }
        }

        Ok(())
    }

    /// 处理单个Done状态的批次
    async fn handle_single_done_batch(
        &mut self,
        batch: &ExpandBatchEntity,
    ) -> Result<(), ServiceError> {
        tracing::debug!(batch_id = %batch.batch_id, "ExpandScanner: handling single done batch");

        // Precondition:
        // - batch.status = Done
        // - expand_complete_at IS NULL
        // Scanner只负责派发通知任务，不负责推进到Notified状态
        // Notified状态只能由通知执行者推进

        // 检查是否已经通知完成（事实已形成）
        let pool = self.ctx.api_wallet_pool()?;
        let is_expand_completed =
            ExpandBatchRepo::is_batch_notified_fact(&pool, &batch.batch_id).await?;
        if is_expand_completed {
            tracing::debug!(batch_id = %batch.batch_id, "ExpandScanner: batch already notified, skipping notification dispatch");
            ExpandBatchRepo::done_to_notified_if_match(&pool, &batch.batch_id).await?;
            return Ok(());
        }

        // 派发通知任务，由通知执行者在成功后推进到Notified状态
        self.dispatch_notify_job_if_needed(batch).await?;

        Ok(())
    }

    /// Recover机制：启动时立即执行一次扫描
    ///
    /// 🔴 核心语义：
    /// recover() ≠ 修复
    /// recover() = 对所有可能推进的状态做一次完整扫描
    /// recover() is semantically equivalent to a normal scan()
    /// The only difference is invocation timing.
    /// 防止未来有人往recover里加"特殊逻辑"
    ///
    /// 包含所有扫描步骤：
    /// 1. 调用Planner，处理Pending Batch（可能推进 Pending → Running 并创建 items）
    /// 2. 扫描所有未完成 items，基于 DB 事实对齐状态
    /// 3. 更新batch状态和finished_count缓存
    pub async fn recover(&mut self) -> Result<(), ServiceError> {
        tracing::trace!("ExpandScanner: starting recover - performing full scan");

        // recover 不管理 scanning，只调用 scan()
        // scanning 的唯一 owner：run_scan_loop
        let result = self.scan().await;

        tracing::trace!("ExpandScanner: recover completed");
        result
    }

    /// 扫描循环 - Scanner的核心生命体征循环
    ///
    /// Invariant:
    /// - scan() panic must NOT kill scanner task
    /// - in_flight must eventually be released via drain_results
    ///
    /// 这是唯一能调用scan()的地方
    async fn run_scan_loop(mut self) {
        tracing::trace!("ExpandScanner: starting scan loop");

        // recover机制：启动时立即执行一次完整扫描
        if let Err(e) = self.recover().await {
            tracing::error!(error = %e, "ExpandScanner: initial recover failed");
        }

        loop {
            // 先检查标志，再睡眠，避免丢唤醒
            loop {
                if self.need_scan.swap(false, Ordering::Relaxed) {
                    break;
                }
                self.notify.notified().await;
            }

            // 抢执行权：使用AtomicBool进行并发控制
            if self.scanning.swap(true, Ordering::Relaxed) {
                continue; // 已有scan在运行，跳过
            }

            // 真正scan：不持锁执行
            // 注意：不使用async move，避免移动self
            let scan_result = std::panic::AssertUnwindSafe(self.scan()).catch_unwind().await;

            // 释放执行权：无论成功失败，都重置scanning标志
            self.scanning.store(false, Ordering::Relaxed);

            // 处理扫描结果
            match scan_result {
                Ok(Ok(_inner_result)) => {
                    // 扫描成功
                }
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "ExpandScanner: scan failed");
                }
                Err(panic) => {
                    tracing::error!(panic = ?panic, "ExpandScanner: scan panicked");
                }
            }
        }
    }
}
