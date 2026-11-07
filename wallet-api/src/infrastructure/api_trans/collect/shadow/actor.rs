// collect/shadow/actor.rs
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::infrastructure::runtime::time::{
    cancellable_sleep::cancellable_sleep,
    metrics::{LoopLatencyMetrics, SpawnGuardMetrics},
    new_production_interval,
};

// 诊断系统的 spawn 保护，限制并发 worker 数量
lazy_static::lazy_static! {
    static ref SPAWN_GUARD: tokio::sync::Semaphore = tokio::sync::Semaphore::new(50);
    static ref SPAWN_GUARD_METRICS: std::sync::Mutex<SpawnGuardMetrics> = std::sync::Mutex::new(SpawnGuardMetrics::new(50));
}

use dashmap::DashMap;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use wallet_database::ApiTransactionDbPool;

use crate::{
    error::service::ServiceError,
    infrastructure::api_trans::{
        AddressLockManager,
        collect::shadow::{
            dispatcher::ShadowDispatcher,
            worker::{ShadowCollectWorker, SideEffectWorker},
        },
        shadow_rpc_policy,
    },
};

use super::{CollectIntent, DispatcherConfig, ScannerConfig, ShadowAdvancer, ShadowScanner};
use crate::infrastructure::api_trans::collect::diagnose::{
    CachedDiagnoser,
    event::DiagnoseEvent,
    stuck_monitor::{CURRENT_BURST, CollectStuckMonitor, LAST_SUCCESS_SEND_AT},
};
use wallet_database::{
    entities::{
        api_resource_delegation::{
            ApiResourceDelegationOperationType, ApiResourceDelegationSource,
        },
        api_resource_gate::{ApiResourceBlockReason, ApiResourceGateResult},
        api_trade_type::ApiTradeType,
    },
    repositories::api_wallet::collect::ApiCollectRepo,
};

const DIAGNOSE_EVENT_IDLE_THRESHOLD: Duration = Duration::from_secs(30);
const DIAGNOSE_HEALTH_LOG_COOLDOWN: Duration = Duration::from_secs(30);
const STARTUP_FAILED_LOCAL_DELEGATE_DIAG_DEFAULT_LIMIT: usize = 10_000;
const STARTUP_FAILED_LOCAL_DELEGATE_DIAG_MAX_LIMIT: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiagnoseActorHealth {
    Healthy,
    Idle,
    Lagging,
}

fn unix_now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::from_secs(0)).as_millis()
        as u64
}

fn classify_diagnose_actor_health(
    processed_gap: Duration,
    produced_gap: Option<Duration>,
    threshold: Duration,
) -> DiagnoseActorHealth {
    // 优先看“消费端”是否最近处理过事件：处理过就视为健康。
    if processed_gap <= threshold {
        return DiagnoseActorHealth::Healthy;
    }

    // 处理端超时后，再结合“生产端”最近是否还在产出事件：
    // - 生产端仍活跃 => 消费端可能落后（Lagging）
    // - 生产端也不活跃 => 系统只是空闲（Idle）
    match produced_gap {
        Some(gap) if gap <= threshold => DiagnoseActorHealth::Lagging,
        _ => DiagnoseActorHealth::Idle,
    }
}

fn should_emit_health_log(
    now: Instant,
    last_logged_at: &mut Option<Instant>,
    cooldown: Duration,
) -> bool {
    let should_emit =
        last_logged_at.map(|last| now.duration_since(last) >= cooldown).unwrap_or(true);
    if should_emit {
        *last_logged_at = Some(now);
    }
    should_emit
}

/// Dispatcher Actor 消息
#[derive(Debug)]
pub enum DispatcherActorMessage {
    /// 处理推进意图
    HandleIntent(CollectIntent),
}

/// Scanner Actor
pub struct CollectorShadowScannerActor {
    ctx: &'static crate::context::Context,
    scanner: Arc<ShadowScanner>,
    shutdown_rx: tokio::sync::broadcast::Receiver<()>,
}

impl CollectorShadowScannerActor {
    pub fn new(
        ctx: &'static crate::context::Context,
        scanner: Arc<ShadowScanner>,
        shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) -> Self {
        Self { ctx, scanner, shutdown_rx }
    }

    pub async fn run(mut self) {
        crate::infrastructure::system_ready::wait_system_ready().await;
        info!("Collector Shadow Scanner Actor running");

        if let Err(error) = reconcile_startup_failed_local_delegations(self.ctx).await {
            warn!(%error, "Collect shadow startup local delegate reconciliation failed");
        }

        // 自定义扫描循环，支持shutdown信号
        let mut interval = new_production_interval(self.scanner.config.scan_interval);

        loop {
            tokio::select! {
                // 接收关闭信号
                _ = self.shutdown_rx.recv() => {
                    info!("Received shutdown signal for Scanner Actor");
                    break;
                },
                // 定时执行扫描
                _ = interval.tick() => {
                    // scan_round is intentionally sequential; overlapping scans are forbidden
                    self.scanner.scan_round().await;
                },
            }
        }

        info!("Collector Shadow Scanner Actor stopped");
    }
}

async fn reconcile_startup_failed_local_delegations(
    ctx: &'static crate::context::Context,
) -> Result<(), ServiceError> {
    let resource_pool = ctx.api_resource_pool()?;
    let transaction_pool = ctx.api_transaction_pool()?;
    let limit = shadow_rpc_policy::read_usize_env(
        "COLLECT_SHADOW_STARTUP_DIAG_MAX_ITEMS",
        STARTUP_FAILED_LOCAL_DELEGATE_DIAG_DEFAULT_LIMIT,
        1,
        STARTUP_FAILED_LOCAL_DELEGATE_DIAG_MAX_LIMIT,
    );
    let records = wallet_database::repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo::scan_failed_for_origin_type_source_and_operation(
        &resource_pool,
        ApiTradeType::Collect as i64,
        ApiResourceDelegationSource::Local,
        ApiResourceDelegationOperationType::Delegate,
        limit,
    ).await?;

    for record in records {
        let Some(origin_trade_no) = record.origin_trade_no.as_deref() else {
            warn!(
                resource_trade_no = %record.resource_trade_no,
                source = "collect_shadow_startup",
                "Historical collect local resource delegation failure has no origin order"
            );
            continue;
        };
        let origin =
            ApiCollectRepo::get_api_collect_by_trade_no(&transaction_pool, origin_trade_no)
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
        if origin.finished_at.is_some()
            || origin.err_code.is_some()
            || origin.raw_tx.is_some()
            || origin.resource_gate_released_at.is_some()
            || origin.resource_block_reason != Some(ApiResourceBlockReason::NeedLocalDelegate)
            || origin.resource_dependency_trade_no.as_deref()
                != Some(record.resource_trade_no.as_str())
        {
            continue;
        }
        let rows = ApiCollectRepo::mark_resource_released(
            &transaction_pool,
            origin_trade_no,
            ApiResourceGateResult::LocalDelegationFailedBypass,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        warn!(
            resource_trade_no = %record.resource_trade_no,
            origin_trade_no = %origin_trade_no,
            err_code = ?record.err_code,
            err_msg = ?record.err_msg,
            owner_address = %record.owner_address,
            receiver_address = %record.receiver_address,
            native_amount = %record.native_amount,
            amount = %record.amount,
            tx_hash = ?record.tx_hash,
            updated_at = ?record.updated_at,
            rows_affected = rows,
            source = "collect_shadow_startup",
            "Reconciled historical collect local resource delegation failure"
        );
    }

    Ok(())
}

/// Dispatcher Actor
pub struct CollectorShadowDispatcherActor {
    pool: ApiTransactionDbPool,
    config: DispatcherConfig,
    shadow_worker: Arc<ShadowCollectWorker>,
    side_effect_worker: Arc<SideEffectWorker>,
    shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    message_rx: mpsc::Receiver<DispatcherActorMessage>,
    /// 意图发送器，用于 try_advance 生成的意图
    intent_tx: mpsc::Sender<CollectIntent>,
}

impl CollectorShadowDispatcherActor {
    pub fn new(
        pool: ApiTransactionDbPool,
        config: DispatcherConfig,
        shadow_worker: Arc<ShadowCollectWorker>,
        side_effect_worker: Arc<SideEffectWorker>,
        shutdown_rx: tokio::sync::broadcast::Receiver<()>,
        message_rx: mpsc::Receiver<DispatcherActorMessage>,
        intent_tx: mpsc::Sender<CollectIntent>,
    ) -> Self {
        Self { pool, config, shadow_worker, side_effect_worker, shutdown_rx, message_rx, intent_tx }
    }

    pub async fn run(mut self) {
        crate::infrastructure::system_ready::wait_system_ready().await;
        info!("Collector Shadow Dispatcher Actor running");

        // 创建唯一的ShadowDispatcher实例
        let dispatcher = ShadowDispatcher::new(
            self.pool.clone(),
            self.config.clone(),
            self.shadow_worker.clone(),
            self.side_effect_worker.clone(),
            self.intent_tx.clone(),
        );
        // 用Arc包装，方便在spawn的任务中使用
        let dispatcher = Arc::new(dispatcher);

        // 启动watchdog loop
        let watchdog_dispatcher = dispatcher.clone();
        let mut watchdog_shutdown_rx = self.shutdown_rx.resubscribe();
        tokio::spawn(async move {
            let mut interval = new_production_interval(std::time::Duration::from_secs(30));
            loop {
                tokio::select! {
                    // 接收关闭信号
                    _ = watchdog_shutdown_rx.recv() => {
                        debug!("Watchdog loop shutdown");
                        break;
                    },
                    // 定时执行watchdog扫描
                    _ = interval.tick() => {
                        watchdog_dispatcher.watchdog_scan().await;
                    },
                }
            }
        });

        // 创建Semaphore和JoinSet
        let semaphore = Arc::new(tokio::sync::Semaphore::new(
            self.config.chain_semaphore_size + self.config.side_effect_semaphore_size,
        ));
        let mut join_set = tokio::task::JoinSet::new();

        loop {
            tokio::select! {
                // 接收关闭信号
                _ = self.shutdown_rx.recv() => {
                    info!("Received shutdown signal for Dispatcher Actor");
                    break;
                },
                // 接收消息
                msg = self.message_rx.recv() => {
                    match msg {
                        Some(DispatcherActorMessage::HandleIntent(intent)) => {
                            // 处理意图（在新任务中运行，避免阻塞Actor消息处理）
                            let dispatcher_clone = dispatcher.clone();
                            let intent_clone = intent.clone();
                            let semaphore_clone = semaphore.clone();

                            // 所有意图使用正常的 acquire
                            let permit = match semaphore_clone.acquire_owned().await {
                                Ok(permit) => permit,
                                Err(_) => {
                                    error!("Semaphore closed, skipping intent: {:?}", intent_clone);
                                    continue;
                                }
                            };

                            // 使用JoinSet管理任务
                            join_set.spawn(async move {
                                let _permit = permit;
                                if let Err(e) = dispatcher_clone.handle_intent(intent_clone).await {
                                    error!("Failed to handle intent: {}", e);
                                }
                            });
                        },
                        None => {
                            info!("Dispatcher Actor message channel closed");
                            break;
                        },
                    }

                    // 非阻塞清理已完成任务
                    while let Some(res) = join_set.try_join_next() {
                        if let Err(e) = res {
                            error!("Dispatcher task failed: {}", e);
                        }
                    }
                }
            }
        }

        // 在shutdown时等待所有任务完成
        info!("Waiting for all dispatcher tasks to complete");
        while let Some(res) = join_set.join_next().await {
            if let Err(e) = res {
                error!("Dispatcher task failed: {}", e);
            }
        }

        info!("Collector Shadow Dispatcher Actor stopped");
    }
}

/// Shadow系统Actor管理
#[derive(Debug)]
pub struct CollectorShadowActorSystem {
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    dispatcher_message_tx: mpsc::Sender<DispatcherActorMessage>,
    scanner_handle: Option<tokio::task::JoinHandle<()>>,
    dispatcher_handle: Option<tokio::task::JoinHandle<()>>,
    diagnose_handle: Option<tokio::task::JoinHandle<()>>,
    monitor_handle: Option<tokio::task::JoinHandle<()>>,
    intent_tx: mpsc::Sender<CollectIntent>,
    scanner: Arc<ShadowScanner>,
    advancer: Arc<ShadowAdvancer>,
}

impl CollectorShadowActorSystem {
    pub fn new(ctx: &'static crate::context::Context) -> Result<Self, ServiceError> {
        let api_transaction_pool = ctx.api_transaction_pool()?;
        let core_pool = ctx.api_wallet_pool()?;

        let (shutdown_tx, shutdown_rx1) = tokio::sync::broadcast::channel(1);
        let shutdown_rx2 = shutdown_tx.subscribe();
        let shutdown_rx3 = shutdown_tx.subscribe();
        let shutdown_rx4 = shutdown_tx.subscribe();

        let (dispatcher_message_tx, dispatcher_message_rx) = mpsc::channel(100);
        let (intent_tx, mut intent_rx) = mpsc::channel(1000);

        // 创建诊断事件总线
        let (diagnose_tx, diagnose_rx) = mpsc::channel(1000);

        let scanner_config = ScannerConfig::default();

        // 创建共享的 Scanner 实例
        let scanner = Arc::new(ShadowScanner::new(
            ctx,
            scanner_config.clone(),
            intent_tx.clone(),
            Some(diagnose_tx.clone()),
        ));

        // 创建共享的 Advancer 实例
        let advancer =
            Arc::new(ShadowAdvancer::new(ctx, intent_tx.clone(), Some(diagnose_tx.clone())));

        let dispatcher_config = DispatcherConfig::default();
        let funds_pool_ref = api_transaction_pool.as_ref();
        info!(
            scan_interval_secs = scanner_config.scan_interval.as_secs(),
            max_items_per_scan = scanner_config.max_items_per_scan,
            dispatcher_chain_concurrency = dispatcher_config.chain_semaphore_size,
            dispatcher_side_effect_concurrency = dispatcher_config.side_effect_semaphore_size,
            advancer_max_concurrency = advancer.configured_max_concurrency(),
            db_pool_size = funds_pool_ref.size(),
            db_pool_idle = funds_pool_ref.num_idle(),
            "Collect shadow runtime config"
        );

        // 创建Scanner Actor
        let scanner_actor = CollectorShadowScannerActor::new(ctx, scanner.clone(), shutdown_rx1);
        let scanner_handle = Some(tokio::spawn(async move {
            scanner_actor.run().await;
        }));

        // 初始化监控
        let stuck_monitor = CollectStuckMonitor::new(
            api_transaction_pool.clone(),
            std::time::Duration::from_secs(10),  // 最小扫描间隔
            std::time::Duration::from_secs(120), // 最大扫描间隔
            100,                                 // 每次扫描限制
            std::time::Duration::from_secs(30),  // 最大扫描时长
        )
        .with_diagnose_tx(diagnose_tx.clone());

        // 启动诊断处理器
        let advancer_clone = advancer.clone();
        let diagnose_handle = Some(tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx3;
            let mut rx = diagnose_rx;
            // 创建诊断缓存，由diagnose任务持有
            let cached_diagnoser = CachedDiagnoser::default();
            // 用于限制 revisit 频率
            let last_revisit: DashMap<String, Instant> = DashMap::new();
            // 用于节流清理
            let mut last_gc = Instant::now();
            // 最后处理事件时间
            let mut last_processed_event = Instant::now();
            let mut last_idle_log_at = None;
            let mut last_lagging_log_at = None;
            // 用于监控 loop latency
            let mut loop_latency_metrics = LoopLatencyMetrics::new();
            // 用于定期上报 metrics
            let mut last_metrics_report = Instant::now();

            // 创建内部节拍器（用于重置计数器）
            let mut interval = new_production_interval(Duration::from_secs(10));

            loop {
                // 记录 loop 开始
                loop_latency_metrics.record_loop_start();

                tokio::select! {
                    biased;

                    // 内部节拍器，用于重置计数器
                    _ = interval.tick() => {
                        let now = Instant::now();
                        let processed_gap = now.duration_since(last_processed_event);
                        let produced_gap = {
                            // LAST_SUCCESS_SEND_AT 记录“诊断事件成功发送到通道”的时间。
                            // 用它与 last_processed_event 组合，区分“无事件（Idle）”
                            // 和“有事件但没消费（Lagging）”。
                            let last_success_send_at =
                                LAST_SUCCESS_SEND_AT.load(std::sync::atomic::Ordering::Relaxed);
                            if last_success_send_at == 0 {
                                None
                            } else {
                                Some(Duration::from_millis(
                                    unix_now_ms().saturating_sub(last_success_send_at),
                                ))
                            }
                        };

                        match classify_diagnose_actor_health(
                            processed_gap,
                            produced_gap,
                            DIAGNOSE_EVENT_IDLE_THRESHOLD,
                        ) {
                            DiagnoseActorHealth::Healthy => {
                                last_idle_log_at = None;
                                last_lagging_log_at = None;
                            }
                            DiagnoseActorHealth::Idle => {
                                last_lagging_log_at = None;
                                // 空闲是正常状态，打 debug 并做冷却，避免误报/刷屏。
                                if should_emit_health_log(
                                    now,
                                    &mut last_idle_log_at,
                                    DIAGNOSE_HEALTH_LOG_COOLDOWN,
                                ) {
                                    debug!(
                                        processed_gap = ?processed_gap,
                                        produced_gap = ?produced_gap,
                                        "Diagnose Actor idle: no diagnose events produced/processed for 30 seconds"
                                    );
                                }
                            }
                            DiagnoseActorHealth::Lagging => {
                                last_idle_log_at = None;
                                // 生产端还在发送事件，但当前 actor 长时间未消费，才视为风险告警。
                                if should_emit_health_log(
                                    now,
                                    &mut last_lagging_log_at,
                                    DIAGNOSE_HEALTH_LOG_COOLDOWN,
                                ) {
                                    warn!(
                                        processed_gap = ?processed_gap,
                                        produced_gap = ?produced_gap,
                                        "Diagnose Actor may be lagging: diagnose events produced but not processed for 30 seconds"
                                    );
                                }
                            }
                        }

                        // 重置 burst 计数器
                        CURRENT_BURST.store(0, std::sync::atomic::Ordering::Relaxed);

                        // 定期上报 metrics
                        let now = Instant::now();
                        if now.duration_since(last_metrics_report) > Duration::from_secs(5) {
                            let metrics = SPAWN_GUARD_METRICS.lock().unwrap();
                            let available = SPAWN_GUARD.available_permits();

                            debug!(
                                spawn_guard_inflight = %metrics.inflight_workers(available),
                                spawn_guard_rejected_total = %metrics.rejected_count(),
                                spawn_guard_saturation_time_seconds = %metrics.total_saturation_time_seconds(),
                                loop_latency_ms = %loop_latency_metrics.average_loop_latency_ms(),
                                "Diagnose Actor metrics"
                            );

                            last_metrics_report = now;
                        }
                    },

                    // 接收关闭信号
                    _ = shutdown_rx.recv() => {
                        info!("Received shutdown signal for Diagnose Actor");
                        break;
                    },

                    // 接收诊断事件
                    Some(event) = rx.recv() => {
                        // 更新最后处理事件时间
                        last_processed_event = Instant::now();
                        last_idle_log_at = None;
                        last_lagging_log_at = None;

                        match event {
                            DiagnoseEvent::NoAdvancement { meta, entity } => {
                                let diag = cached_diagnoser.diagnose(&entity);
                                if diag.stuck_score >= 2 {
                                    tracing::warn!(
                                        stage = ?meta.stage,
                                        source = ?meta.source,
                                        reasons = ?diag.reasons,
                                        facts = %diag.facts_snapshot,
                                        score = diag.stuck_score,
                                        next_fact = ?diag.next_expected_fact,
                                        "⚠️ Collect order stuck diagnosis - NoAdvancement"
                                    );
                                }
                            },
                            DiagnoseEvent::PeriodicScan { meta, entity } => {
                                let diag = cached_diagnoser.diagnose(&entity);
                                if diag.stuck_score >= 2 {
                                    tracing::warn!(
                                        stage = ?meta.stage,
                                        source = ?meta.source,
                                        reasons = ?diag.reasons,
                                        facts = %diag.facts_snapshot,
                                        score = diag.stuck_score,
                                        next_fact = ?diag.next_expected_fact,
                                        "⚠️ Collect order stuck diagnosis - PeriodicScan"
                                    );
                                }
                            },
                            DiagnoseEvent::ManualDiagnose { meta, entity, extra } => {
                                let diag = cached_diagnoser.diagnose(&entity);
                                tracing::warn!(
                                    stage = ?meta.stage,
                                    source = ?meta.source,
                                    reasons = ?diag.reasons,
                                        facts = %diag.facts_snapshot,
                                    score = diag.stuck_score,
                                    next_fact = ?diag.next_expected_fact,
                                    info = %extra,
                                    "⚠️ Collect order stuck diagnosis - Manual"
                                );
                            },
                            DiagnoseEvent::IntentDispatchFailed { meta } => {
                                let trade_no = &meta.trade_no;
                                info!(trade_no = %trade_no, "Intent dispatch failed, triggering revisit");

                                // 检查是否在 1 秒内已经触发过 revisit
                                let now = Instant::now();
                                let trade_no_str = trade_no.to_string();
                                let should_spawn = last_revisit.get(&trade_no_str).map_or(true, |entry| {
                                    now.duration_since(*entry.value()) > Duration::from_secs(1)
                                });

                                if should_spawn {
                                    // 更新最后 revisit 时间
                                    last_revisit.insert(trade_no_str, now);

                                    // 使用 spawn + 延迟执行，避免无限递归
                                    let advancer = advancer_clone.clone();
                                    let trade_no_clone = trade_no.to_string();

                                    // 尝试获取 spawn permit，限制并发数量
                                    let available = SPAWN_GUARD.available_permits();
                                    if let Ok(permit) = SPAWN_GUARD.try_acquire() {
                                        // 记录 spawn 成功
                                        SPAWN_GUARD_METRICS.lock().unwrap().record_spawn_attempt(available, true);

                                        // 创建 shutdown 信号接收器
                                        let mut shutdown_rx_clone = shutdown_rx.resubscribe();

                                        tokio::spawn(async move {
                                            // 绑定 permit 到 future，确保即使 panic 也会释放
                                            let _permit = permit;

                                            // 添加 200ms 延迟，脱离当前 diagnose loop
                                            let slept = cancellable_sleep(
                                                Duration::from_millis(200),
                                                &mut shutdown_rx_clone,
                                            )
                                            .await;
                                            if !slept {
                                                tracing::debug!(
                                                    trade_no = %trade_no_clone,
                                                    "diagnose advance skipped due to shutdown"
                                                );
                                                return;
                                            }
                                            advancer.try_advance(&trade_no_clone).await;
                                        });
                                    } else {
                                        // 记录 spawn 失败
                                        SPAWN_GUARD_METRICS.lock().unwrap().record_spawn_attempt(available, false);

                                        // spawn 限制命中，记录警告
                                        warn!(trade_no = %trade_no, "Diagnose spawn limit reached, revisit skipped");
                                    }
                                } else {
                                    debug!(trade_no = %trade_no, "Revisit rate limited");
                                }
                            },

                        }

                        // 定期清理 last_revisit 中的过期条目（带节流）
                        let now = Instant::now();
                        if now.duration_since(last_gc) > Duration::from_secs(10) {
                            if last_revisit.len() > 10_000 {
                                let before = last_revisit.len();
                                last_revisit.retain(|_, t| now.duration_since(*t) < Duration::from_secs(60));
                                let after = last_revisit.len();
                                if before > after {
                                    debug!("Cleaned up {} expired revisit entries", before - after);
                                }
                            }
                            last_gc = now;
                        }
                    },
                    // 通道关闭
                    else => {
                        info!("Diagnose event channel closed");
                        break;
                    },
                }

                // 记录 loop 结束
                loop_latency_metrics.record_loop_end();
            }
        }));

        // 启动监控任务
        let monitor_handle = Some(tokio::spawn(async move {
            crate::infrastructure::system_ready::wait_system_ready().await;
            info!("Stuck Monitor starting");
            let shutdown_rx = shutdown_rx4;
            let mut monitor = stuck_monitor;
            monitor.run_loop(shutdown_rx).await;
            info!("Stuck Monitor stopped");
        }));

        // 初始化Shadow Worker
        // 创建AddressLockManager
        let address_locks = Arc::new(AddressLockManager::new());
        let fee_reservations =
            crate::infrastructure::api_trans::resource_reservation::CollectFeeReservationManager::default();
        // 创建ShadowCollectWorker
        let shadow_worker = Arc::new(ShadowCollectWorker::new_with_fee_reservations(
            ctx,
            address_locks,
            advancer.clone(),
            fee_reservations.clone(),
        ));

        // 初始化SideEffect Worker
        let side_effect_worker = Arc::new(SideEffectWorker::new_with_fee_reservations(
            ctx,
            advancer.clone(),
            fee_reservations,
        ));

        // 启动时执行一次 warm single scan
        let scanner_clone = scanner.clone();
        info!("Performing warm single scan on startup");
        // 异步执行，不阻塞启动
        tokio::spawn(async move {
            scanner_clone.scan_round().await;
            info!("Warm single scan completed");
        });

        // 创建Dispatcher Actor
        let dispatcher_actor = CollectorShadowDispatcherActor::new(
            api_transaction_pool,
            dispatcher_config,
            shadow_worker,
            side_effect_worker,
            shutdown_rx2,
            dispatcher_message_rx,
            intent_tx.clone(),
        );
        let dispatcher_handle = Some(tokio::spawn(async move {
            dispatcher_actor.run().await;
        }));

        // 创建意图转发任务（从intent_rx接收意图，发送给Dispatcher Actor）
        let dispatcher_message_tx_clone = dispatcher_message_tx.clone();
        let mut shutdown_rx3 = shutdown_tx.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // 接收关闭信号
                    _ = shutdown_rx3.recv() => {
                        info!("Received shutdown signal for intent forward task");
                        break;
                    },
                    // 接收意图
                    Some(intent) = intent_rx.recv() => {
                        if let Err(e) = dispatcher_message_tx_clone
                            .send(DispatcherActorMessage::HandleIntent(intent))
                            .await
                        {
                            error!("Failed to send intent to Dispatcher Actor: {}", e);
                        }
                    },
                }
            }
        });

        Ok(Self {
            shutdown_tx,
            dispatcher_message_tx,
            scanner_handle,
            dispatcher_handle,
            diagnose_handle,
            monitor_handle,
            intent_tx,
            scanner,
            advancer,
        })
    }

    /// 停止Shadow系统
    pub async fn stop(&mut self) {
        info!("Stopping Collector Shadow System");

        // 发送停止信号
        let _ = self.shutdown_tx.send(());

        // 等待Actor结束
        if let Some(handle) = self.scanner_handle.take() {
            if let Err(err) = handle.await {
                tracing::warn!(error = %err, "collector shadow scanner join failed during stop");
            }
        }

        if let Some(handle) = self.dispatcher_handle.take() {
            if let Err(err) = handle.await {
                tracing::warn!(error = %err, "collector shadow dispatcher join failed during stop");
            }
        }

        if let Some(handle) = self.diagnose_handle.take() {
            if let Err(err) = handle.await {
                tracing::warn!(error = %err, "collector shadow diagnose join failed during stop");
            }
        }

        if let Some(handle) = self.monitor_handle.take() {
            if let Err(err) = handle.await {
                tracing::warn!(error = %err, "collector shadow monitor join failed during stop");
            }
        }

        info!("Collector Shadow System stopped");
    }

    /// 获取意图发送器
    pub fn get_intent_tx(&self) -> mpsc::Sender<CollectIntent> {
        self.intent_tx.clone()
    }

    /// 触发一次针对特定 trade_no 的归集推进
    ///
    /// 语义：
    /// - 有新事实了，立即尝试推进一次
    /// - 不是执行流程，而是提前跑一次 Shadow 的事实驱动推进
    /// - 幂等，多次调用不会导致重复执行
    /// - Tick 是一种低语义、低优先级的推进触发
    /// - 不保证立即执行
    /// - 不保证一定推进
    /// - 只保证"进入 Shadow 的调度视野"
    pub async fn trigger_collect(
        &self,
        trade_no: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        // 直接调用 Advancer 的 try_advance 方法，尝试推进指定交易
        self.advancer.try_advance(trade_no).await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DiagnoseActorHealth, classify_diagnose_actor_health};
    use std::time::Duration;

    #[test]
    fn classify_diagnose_actor_health_is_idle_when_no_events_are_produced() {
        let status =
            classify_diagnose_actor_health(Duration::from_secs(31), None, Duration::from_secs(30));
        assert_eq!(status, DiagnoseActorHealth::Idle);
    }

    #[test]
    fn classify_diagnose_actor_health_is_lagging_when_events_are_recently_produced() {
        let status = classify_diagnose_actor_health(
            Duration::from_secs(31),
            Some(Duration::from_secs(5)),
            Duration::from_secs(30),
        );
        assert_eq!(status, DiagnoseActorHealth::Lagging);
    }

    #[test]
    fn classify_diagnose_actor_health_is_healthy_when_processed_recently() {
        let status = classify_diagnose_actor_health(
            Duration::from_secs(5),
            Some(Duration::from_secs(1)),
            Duration::from_secs(30),
        );
        assert_eq!(status, DiagnoseActorHealth::Healthy);
    }
}
