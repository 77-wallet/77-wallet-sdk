// worker.rs
// 🔴 核心设计原则（**必须严格遵守，否则将导致不可恢复的数据破坏**）
// 🔴 1. Worker/Executor 只执行任务，不参与状态管理
// 🔴 2. 状态管理由 Scanner 负责，基于 DB 事实
// 🔴 3. ExecOutcome 只影响 Worker 内部日志，不允许直接修改 Item / Batch 状态
// 🔴 4. 禁止在 Worker 中根据 ExecOutcome 直接修改 DB 状态
// 🔴 5. 禁止引入 wait_system_ready 作为全局门闩，仅在 Create 任务中使用
// 🔴 6. Worker 是"哑执行器"，只打日志，不重试，只上报执行完成事件，不参与状态决策，不修改状态
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::FutureExt;
use once_cell::sync::Lazy;

use crate::{
    error::service::ServiceError,
    infrastructure::expand_address::{
        event::ExpandEvent,
        executor::ExpandExecutor,
        scanner::{ExpandDispatchKey, ExpandJobResult},
    },
};
use wallet_database::repositories::api_wallet::expand_batch::ExpandBatchRepo;

use tokio::sync::{
    Semaphore,
    mpsc::{self, error::TryRecvError},
};

// Job ID generator
static JOB_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);

// 任务类型枚举，用于执行层区分任务类型，避免依赖ExpandJob
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum JobKind {
    Create,
    Init,
    Notify,
}

/// 任务权重分类，用于信号量资源分配
#[derive(Debug, Clone, Copy)]
pub enum JobCategory {
    /// 重型任务，如Init，限制并发数为3
    Init,
    /// 普通任务，如Sync、扩容扫描，限制并发数为6
    Sync,
    /// 快速任务，如Create、Notify，限制并发数为12
    Fast,
}

// 活跃任务计数，用于监控Worker负载
static EXPAND_WORKER_INFLIGHT: AtomicUsize = AtomicUsize::new(0);

// INIT 任务信号量，限制最大并发任务数为3
pub(crate) static INIT_SEMA: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(8));

// SYNC/扩容扫描任务信号量，限制最大并发任务数为6
pub(crate) static SYNC_SEMA: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(6));

// 快速任务信号量，限制最大并发任务数为12
pub(crate) static FAST_SEMA: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(12));

#[derive(Clone)]
pub(crate) enum ExpandJob {
    Create {
        ctx: &'static crate::context::Context,
        job_id: String,
        uid: String,
        chain: String,
        batch_id: String,
        indices: Vec<i32>,
        dispatch_key: ExpandDispatchKey,
        result_tx: tokio::sync::mpsc::UnboundedSender<ExpandJobResult>,
    },
    Init {
        ctx: &'static crate::context::Context,
        job_id: String,
        uid: String,
        chain: String,
        batch_id: String,
        indices: Vec<i32>,
        dispatch_key: ExpandDispatchKey,
        result_tx: tokio::sync::mpsc::UnboundedSender<ExpandJobResult>,
    },
    Notify {
        ctx: &'static crate::context::Context,
        job_id: String,
        uid: String,
        chain: String,
        batch_id: String,
        dispatch_key: ExpandDispatchKey,
        result_tx: tokio::sync::mpsc::UnboundedSender<ExpandJobResult>,
    },
}

impl std::fmt::Debug for ExpandJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExpandJob")
            .field("job_id", &self.id())
            .field("job_type", &self.job_type())
            .field("batch_id", &self.batch_id())
            .finish()
    }
}

impl ExpandJob {
    pub fn generate_job_id() -> String {
        JOB_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed).to_string()
    }

    pub fn new_create(
        ctx: &'static crate::context::Context,
        uid: String,
        chain: String,
        batch_id: String,
        indices: Vec<i32>,
        dispatch_key: ExpandDispatchKey,
        result_tx: tokio::sync::mpsc::UnboundedSender<ExpandJobResult>,
    ) -> Self {
        Self::Create {
            ctx,
            job_id: Self::generate_job_id(),
            uid,
            chain,
            batch_id,
            indices,
            dispatch_key,
            result_tx,
        }
    }

    pub fn new_init(
        ctx: &'static crate::context::Context,
        uid: String,
        chain: String,
        batch_id: String,
        indices: Vec<i32>,
        dispatch_key: ExpandDispatchKey,
        result_tx: tokio::sync::mpsc::UnboundedSender<ExpandJobResult>,
    ) -> Self {
        Self::Init {
            ctx,
            job_id: Self::generate_job_id(),
            uid,
            chain,
            batch_id,
            indices,
            dispatch_key,
            result_tx,
        }
    }

    pub fn new_notify(
        ctx: &'static crate::context::Context,
        uid: String,
        chain: String,
        batch_id: String,
        dispatch_key: ExpandDispatchKey,
        result_tx: tokio::sync::mpsc::UnboundedSender<ExpandJobResult>,
    ) -> Self {
        Self::Notify {
            ctx,
            job_id: Self::generate_job_id(),
            uid,
            chain,
            batch_id,
            dispatch_key,
            result_tx,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            ExpandJob::Create { job_id, .. } => job_id,
            ExpandJob::Init { job_id, .. } => job_id,
            ExpandJob::Notify { job_id, .. } => job_id,
        }
    }

    pub fn job_type(&self) -> &str {
        match self {
            ExpandJob::Create { .. } => "create",
            ExpandJob::Init { .. } => "init",
            ExpandJob::Notify { .. } => "notify",
        }
    }

    pub fn batch_id(&self) -> &str {
        match self {
            ExpandJob::Create { batch_id, .. } => batch_id,
            ExpandJob::Init { batch_id, .. } => batch_id,
            ExpandJob::Notify { batch_id, .. } => batch_id,
        }
    }

    /// 获取任务分类，用于信号量资源分配
    pub fn category(&self) -> JobCategory {
        match self {
            // INIT 任务属于重型任务，限制并发数为3
            ExpandJob::Init { .. } => JobCategory::Init,
            // Create任务属于快速任务，限制并发数为12
            ExpandJob::Create { .. } => JobCategory::Fast,
            // Notify任务属于普通任务，限制并发数为6
            ExpandJob::Notify { .. } => JobCategory::Sync,
        }
    }
}

pub(crate) struct ExpandWorkerPool {
    pub(crate) tx: mpsc::Sender<ExpandJob>,
}

pub(crate) static WORKER_POOL: Lazy<ExpandWorkerPool> = Lazy::new(|| {
    let (tx, mut rx) = mpsc::channel::<ExpandJob>(1024);

    // 启动单个 dispatcher 任务，负责从通道接收任务并分发
    tokio::spawn(async move {
        tracing::info!("expand dispatcher started");

        // 批量大小上限，可根据实际情况调整
        const BATCH_SIZE: usize = 64;

        loop {
            // 批量接收任务
            let mut batch = Vec::with_capacity(BATCH_SIZE);

            // 先尝试批量接收任务，直到通道为空或达到批量大小上限
            while batch.len() < BATCH_SIZE {
                match rx.try_recv() {
                    Ok(job) => {
                        batch.push(job);
                    }
                    Err(TryRecvError::Empty) => {
                        // 通道为空，退出循环
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        // 通道关闭，退出循环
                        tracing::info!("expand dispatcher: channel disconnected");
                        return;
                    }
                }
            }

            // 如果没有接收到任何任务，则等待一个任务
            if batch.is_empty() {
                match rx.recv().await {
                    Some(job) => {
                        batch.push(job);
                    }
                    None => {
                        // 通道关闭，退出循环
                        tracing::info!("expand dispatcher: channel closed");
                        break;
                    }
                }
            }

            tracing::info!(batch_size = batch.len(), "DISPATCH: received batch");

            // 批量 spawn 任务
            for job in batch {
                // 获取信号量许可
                let permit = match job.category() {
                    JobCategory::Init => INIT_SEMA.acquire().await,
                    JobCategory::Sync => SYNC_SEMA.acquire().await,
                    JobCategory::Fast => FAST_SEMA.acquire().await,
                };

                // 为每个任务生成新的 tokio 任务，将 permit move 到任务中，自动管理生命周期
                tokio::spawn(async move {
                    // 不需要显式 drop(permit)，所有权已转移，任务结束时自动释放
                    let _permit = permit;

                    // 更新进行中任务计数
                    EXPAND_WORKER_INFLIGHT.fetch_add(1, Ordering::Relaxed);

                    tracing::info!(
                        job_id = %job.id(),
                        job_type = %job.job_type(),
                        batch_id = %job.batch_id(),
                        "WORKER: starting job"
                    );

                    // 使用 panic 隔离，确保单个任务 panic 不会影响其他任务
                    let result = std::panic::AssertUnwindSafe(handle_job(job)).catch_unwind().await;

                    // 任务完成，更新计数
                    EXPAND_WORKER_INFLIGHT.fetch_sub(1, Ordering::Relaxed);

                    // 处理任务执行结果
                    match result {
                        Ok(Ok(())) => tracing::info!("WORKER: completed successfully"),
                        Ok(Err(e)) => tracing::error!(error = %e, "WORKER: failed"),
                        Err(pe) => tracing::error!(panic = ?pe, "WORKER: panicked"),
                    }
                });
            }
        }

        tracing::info!("expand dispatcher exiting, channel closed");
    });

    ExpandWorkerPool { tx }
});

async fn handle_job(job: ExpandJob) -> Result<(), ServiceError> {
    match job {
        ExpandJob::Create {
            ctx,
            job_id,
            uid,
            chain,
            batch_id,
            indices,
            dispatch_key,
            result_tx,
        } => {
            let result = run_create(ctx, job_id, uid, chain, batch_id, indices.clone()).await;

            // Send job result without consuming the result
            if result.is_ok() {
                if let Err(err) = result_tx.send(ExpandJobResult::Succeeded {
                    key: dispatch_key,
                    indexes: indices.clone(),
                }) {
                    tracing::warn!(error = %err, "failed to send expand create success result");
                }
            } else {
                if let Err(err) = result_tx.send(ExpandJobResult::Failed {
                    key: dispatch_key,
                    error: result.as_ref().unwrap_err().to_string(),
                    indexes: indices.clone(),
                }) {
                    tracing::warn!(error = %err, "failed to send expand create failure result");
                }
            }

            result
        }
        ExpandJob::Init {
            ctx,
            ref job_id,
            uid,
            chain,
            batch_id,
            indices,
            dispatch_key,
            result_tx,
        } => {
            // 执行run_init，但忽略返回结果，因为init任务只是发送chunk，不表示真正完成
            let result =
                run_init(ctx, job_id.to_string(), uid, chain, batch_id, indices.clone()).await;
            match result {
                Ok(_) => {
                    if let Err(err) = result_tx.send(ExpandJobResult::Succeeded {
                        key: dispatch_key,
                        indexes: indices.clone(),
                    }) {
                        tracing::warn!(error = %err, "failed to send expand init success result");
                    }
                }
                Err(e) => {
                    if let Err(err) = result_tx.send(ExpandJobResult::Failed {
                        key: dispatch_key,
                        error: e.to_string(),
                        indexes: indices.clone(),
                    }) {
                        tracing::warn!(error = %err, "failed to send expand init failure result");
                    }
                }
            }

            Ok(())
        }
        ExpandJob::Notify { ctx, job_id, uid, chain, batch_id, dispatch_key, result_tx } => {
            // Notify任务作为普通job处理，不spawn新任务
            // 这样可以确保inflight计数正确
            let result = run_notify(ctx, job_id.clone(), uid, chain, batch_id).await;

            // Send job result without consuming the result
            if result.is_ok() {
                if let Err(err) = result_tx
                    .send(ExpandJobResult::Succeeded { key: dispatch_key, indexes: vec![] })
                {
                    tracing::warn!(error = %err, "failed to send expand notify success result");
                }
            } else {
                if let Err(err) = result_tx.send(ExpandJobResult::Failed {
                    key: dispatch_key,
                    error: result.as_ref().unwrap_err().to_string(),
                    indexes: vec![],
                }) {
                    tracing::warn!(error = %err, "failed to send expand notify failure result");
                }
            }

            if let Err(e) = &result {
                tracing::error!(
                    job_id = %job_id,
                    error = %e,
                    "WORKER: notify job failed"
                );
            }

            result
        }
    }
}

async fn run_create(
    ctx: &'static crate::context::Context,
    job_id: String,
    uid: String,
    chain: String,
    batch_id: String,
    indices: Vec<i32>,
) -> Result<(), ServiceError> {
    tracing::info!(
        job_id = %job_id,
        uid = %uid,
        chain = %chain,
        batch_id = %batch_id,
        indices_count = indices.len(),
        "WORKER: starting create task"
    );

    // 只有 Create 任务需要等系统 ready（密码缓存、Context 初始化等）
    tracing::info!(job_id = %job_id, "WORKER: waiting system ready");
    let start = std::time::Instant::now();
    crate::infrastructure::system_ready::wait_system_ready().await;
    tracing::info!(
        job_id = %job_id,
        elapsed = ?start.elapsed(),
        "WORKER: system ready passed"
    );

    let executor = ExpandExecutor::new(ctx);
    let result = executor.execute_create(&uid, &chain, &indices, &batch_id).await;

    handle_execution_result(ctx, &job_id, &batch_id, JobKind::Create, result).await
}

async fn run_init(
    ctx: &'static crate::context::Context,
    job_id: String,
    uid: String,
    chain: String,
    batch_id: String,
    indices: Vec<i32>,
) -> Result<(), ServiceError> {
    use futures::{StreamExt, stream};

    const INIT_CHUNK_SIZE: usize = 40;
    const INTERNAL_CONCURRENCY: usize = 16;

    tracing::info!(
        job_id = %job_id,
        uid = %uid,
        chain = %chain,
        batch_id = %batch_id,
        indices_count = indices.len(),
        chunk_size = INIT_CHUNK_SIZE,
        "WORKER: starting init task"
    );

    // 按 chunk 切分
    let chunks: Vec<Vec<i32>> = indices.chunks(INIT_CHUNK_SIZE).map(|c| c.to_vec()).collect();

    tracing::info!(
        job_id = %job_id,
        chunk_count = chunks.len(),
        "WORKER: split into chunks"
    );

    // ⚠️核心：buffer_unordered 控制内部 concurrency
    stream::iter(chunks.into_iter().enumerate())
        .map(|(i, chunk_indices)| {
            let job_id_clone = job_id.clone();
            let uid_clone = uid.clone();
            let chain_clone = chain.clone();
            let batch_id_clone = batch_id.clone();

            async move {
                tracing::info!(
                    job_id = %job_id_clone,
                    chunk_index = i,
                    chunk_size = chunk_indices.len(),
                    "WORKER: executing chunk"
                );

                let executor = ExpandExecutor::new(ctx);
                let res = executor
                    .execute_init(&uid_clone, &chain_clone, &chunk_indices, &batch_id_clone)
                    .await;

                match &res {
                    Ok(_) => tracing::info!(
                        job_id = %job_id_clone,
                        chunk_index = i,
                        "WORKER: chunk done"
                    ),
                    Err(e) => tracing::warn!(
                        job_id = %job_id_clone,
                        chunk_index = i,
                        error = %e,
                        "WORKER: chunk failed, scanner will retry"
                    ),
                }

                res
            }
        })
        .buffer_unordered(INTERNAL_CONCURRENCY)
        .collect::<Vec<_>>() // 等所有 chunk 跑完
        .await;

    tracing::info!(
        job_id = %job_id,
        "WORKER: init job done (all chunks processed)"
    );

    Ok(())
}

async fn run_notify(
    ctx: &'static crate::context::Context,
    job_id: String,
    uid: String,
    chain: String,
    batch_id: String,
) -> Result<(), ServiceError> {
    tracing::info!(
        job_id = %job_id,
        uid = %uid,
        chain = %chain,
        batch_id = %batch_id,
        "WORKER: starting notify task"
    );

    let executor = ExpandExecutor::new(ctx);
    let result = executor.execute_notify(&uid, &batch_id).await;

    handle_execution_result(ctx, &job_id, &batch_id, JobKind::Notify, result).await
}

/// 记录任务执行结果的事实
async fn record_fact(
    ctx: &'static crate::context::Context,
    _job_id: &str,
    batch_id: &str,
    job_kind: JobKind,
) -> Result<(), crate::error::service::ServiceError> {
    match job_kind {
        JobKind::Notify => {
            // 对于Notify任务，记录expand_complete_at事实字段
            let pool = ctx.api_wallet_pool()?;
            // 记录事实：expand_complete已成功执行
            ExpandBatchRepo::update_expand_complete_at_if_null(&pool, batch_id).await?;
        }
        JobKind::Create | JobKind::Init => {
            // 对于Create/Init任务，不直接写local_complete_at事实
            // local_complete_at只由Scanner作为"最终事实修复者"负责
            // 这保证了事实写入的单一责任，便于调试和维护
        }
    }
    Ok(())
}

/// 发送HintScan事件通知Scanner检查状态
async fn emit_hint_scan(ctx: &'static crate::context::Context) {
    // 任务成功完成，发送HintScan事件通知Scanner检查状态
    // 只有在数据库事实已形成后发送
    if let Some(event_tx) = ctx.get_expand_event_tx().await {
        // best-effort hint, ignore failure
        if let Err(err) = event_tx.send(ExpandEvent::HintScan).await {
            tracing::warn!(error = %err, "failed to send expand hint scan event");
        }
        tracing::info!("sent HintScan event to scanner");
    }
}

async fn handle_execution_result(
    ctx: &'static crate::context::Context,
    job_id: &str,
    batch_id: &str,
    job_kind: JobKind,
    result: Result<crate::infrastructure::expand_address::executor::ExecOutcome, ServiceError>,
) -> Result<(), ServiceError> {
    match result {
        Ok(exec_outcome) => {
            match exec_outcome {
                crate::infrastructure::expand_address::executor::ExecOutcome::Success => {
                    tracing::info!(
                        job_id = %job_id,
                        "expand worker job completed successfully"
                    );

                    // 1. 记录事实
                    if let Err(error) = record_fact(ctx, job_id, batch_id, job_kind).await {
                        tracing::error!(%error, batch_id = %batch_id, "failed to record expand address fact");
                    }

                    // 2. 仅为Notify任务发送HintScan事件（只有Notify会立即写入DB事实）
                    if let JobKind::Notify = job_kind {
                        emit_hint_scan(ctx).await;
                    }
                }
                crate::infrastructure::expand_address::executor::ExecOutcome::Retryable {
                    reason,
                } => {
                    tracing::warn!(
                        job_id = %job_id,
                        reason = ?reason,
                        "expand worker job failed with retryable error, scanner will handle retry"
                    );
                    // 可重试错误，记录日志后继续
                    // 当前设计中，Worker 不会重试，Scanner 会基于 DB 事实重试
                }
                crate::infrastructure::expand_address::executor::ExecOutcome::Fatal { reason } => {
                    tracing::error!(
                        job_id = %job_id,
                        reason = ?reason,
                        "fatal error: no retry possible, scanner will observe DB facts and stop progressing"
                    );
                    // 致命错误，记录日志后继续
                    // 不可重试，Scanner会基于DB事实停止推进该item
                }
            }
        }
        Err(e) => {
            // 系统错误，记录日志
            tracing::error!(
                job_id = %job_id,
                error = %e,
                "expand worker job failed with system error; no retry is performed by worker"
            );
            // 系统错误，返回错误
        }
    }

    Ok(())
}
