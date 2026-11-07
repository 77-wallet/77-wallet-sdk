// withdraw/shadow/actor.rs
use std::{sync::Arc, time::Duration};

use crate::infrastructure::runtime::time::new_production_interval;

use tokio::sync::mpsc;
use tracing::{error, info, warn};
use wallet_database::ApiTransactionDbPool;

use crate::{
    error::service::ServiceError,
    infrastructure::api_trans::withdraw::diagnose::{
        CachedDiagnoser, DiagnoseEvent, WithdrawStuckMonitor,
    },
};
use dashmap::DashMap;
use std::time::Instant;

use crate::infrastructure::api_trans::withdraw::shadow::worker::{
    ShadowWithdrawWorker, SideEffectWorker,
};

use super::{DispatcherConfig, ScannerConfig, ShadowScanner, WithdrawIntent};

/// Dispatcher Actor 消息
#[derive(Debug)]
pub enum DispatcherActorMessage {
    /// 处理推进意图
    HandleIntent(WithdrawIntent),
}

/// Scanner Actor
pub struct WithdrawShadowScannerActor {
    scanner: Arc<ShadowScanner>,
    shutdown_rx: tokio::sync::broadcast::Receiver<()>,
}

impl WithdrawShadowScannerActor {
    pub fn new(
        scanner: Arc<ShadowScanner>,
        shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) -> Self {
        Self { scanner, shutdown_rx }
    }

    pub async fn run(mut self) {
        crate::infrastructure::system_ready::wait_system_ready().await;
        info!("Withdraw Shadow Scanner Actor running");

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

        info!("Withdraw Shadow Scanner Actor stopped");
    }
}

/// Dispatcher Actor
pub struct WithdrawShadowDispatcherActor {
    pool: ApiTransactionDbPool,
    config: DispatcherConfig,
    shadow_worker: Arc<ShadowWithdrawWorker>,
    side_effect_worker: Arc<SideEffectWorker>,
    shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    message_rx: mpsc::Receiver<DispatcherActorMessage>,
    /// 意图发送器，用于 try_advance 生成的意图
    intent_tx: mpsc::Sender<WithdrawIntent>,
}

impl WithdrawShadowDispatcherActor {
    pub fn new(
        pool: ApiTransactionDbPool,
        config: DispatcherConfig,
        shadow_worker: Arc<ShadowWithdrawWorker>,
        side_effect_worker: Arc<SideEffectWorker>,
        shutdown_rx: tokio::sync::broadcast::Receiver<()>,
        message_rx: mpsc::Receiver<DispatcherActorMessage>,
        intent_tx: mpsc::Sender<WithdrawIntent>,
    ) -> Self {
        Self { pool, config, shadow_worker, side_effect_worker, shutdown_rx, message_rx, intent_tx }
    }

    pub async fn run(mut self) {
        crate::infrastructure::system_ready::wait_system_ready().await;
        info!("Withdraw Shadow Dispatcher Actor running");

        // 创建唯一的ShadowDispatcher实例
        let dispatcher = super::dispatcher::ShadowDispatcher::new(
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
                        info!("Watchdog loop shutdown");
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
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.outer_admission_size()));
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

        info!("Withdraw Shadow Dispatcher Actor stopped");
    }
}

/// Shadow系统Actor管理
#[derive(Debug)]
pub struct WithdrawShadowActorSystem {
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    dispatcher_message_tx: mpsc::Sender<DispatcherActorMessage>,
    scanner_handle: Option<tokio::task::JoinHandle<()>>,
    dispatcher_handle: Option<tokio::task::JoinHandle<()>>,
    diagnose_handle: Option<tokio::task::JoinHandle<()>>,
    monitor_handle: Option<tokio::task::JoinHandle<()>>,
    self_transfer_observer_handle: Option<tokio::task::JoinHandle<()>>,
    intent_tx: mpsc::Sender<WithdrawIntent>,
    scanner: Arc<ShadowScanner>,
}

impl WithdrawShadowActorSystem {
    pub fn new(ctx: &'static crate::context::Context) -> Result<Self, ServiceError> {
        let api_withdraw_pool = ctx.api_transaction_pool()?;
        let core_pool = ctx.api_wallet_pool()?;

        let (shutdown_tx, shutdown_rx1) = tokio::sync::broadcast::channel(1);
        let shutdown_rx2 = shutdown_tx.subscribe();
        let shutdown_rx3 = shutdown_tx.subscribe();
        let shutdown_rx4 = shutdown_tx.subscribe();

        let (dispatcher_message_tx, dispatcher_message_rx) = mpsc::channel(100);
        let (intent_tx, mut intent_rx) = mpsc::channel(1000);

        // 创建诊断事件总线
        let (diagnose_tx, diagnose_rx) =
            crate::infrastructure::api_trans::withdraw::diagnose::channel(1000);

        // 创建共享的 Scanner 实例
        let scanner = Arc::new(ShadowScanner::new(
            ctx,
            ScannerConfig::default(),
            intent_tx.clone(),
            Some(diagnose_tx.clone()),
        ));

        // 创建Scanner Actor
        let scanner_actor = WithdrawShadowScannerActor::new(scanner.clone(), shutdown_rx1);
        let scanner_handle = Some(tokio::spawn(async move {
            scanner_actor.run().await;
        }));

        // 初始化Shadow Worker
        let shadow_worker = Arc::new(ShadowWithdrawWorker::new(ctx, scanner.clone()));

        // 初始化SideEffect Worker
        let side_effect_worker = Arc::new(SideEffectWorker::new(ctx, scanner.clone()));

        // 启动时执行一次 warm single scan
        let scanner_clone = scanner.clone();
        info!("Performing warm single scan on startup");
        // 异步执行，不阻塞启动
        tokio::spawn(async move {
            scanner_clone.scan_round().await;
            info!("Warm single scan completed");
        });

        // 初始化监控
        let stuck_monitor = WithdrawStuckMonitor::new(
            api_withdraw_pool.clone(),
            Duration::from_secs(10),
            Duration::from_secs(120),
            100,
            Duration::from_secs(30),
        )
        .with_diagnose_tx(diagnose_tx.clone());

        let monitor_handle = Some(tokio::spawn(async move {
            let mut monitor = stuck_monitor;
            monitor.run_loop(shutdown_rx4).await;
        }));

        // 启动诊断处理器
        let scanner_clone = scanner.clone();
        let diagnose_handle = Some(tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx3;
            let mut rx = diagnose_rx;
            let cached_diagnoser = CachedDiagnoser::default();
            let last_revisit: DashMap<String, Instant> = DashMap::new();
            let revisit_sem = Arc::new(tokio::sync::Semaphore::new(50));

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        return;
                    }
                    event = rx.recv() => {
                        let Some(event) = event else { return; };

                        match event {
                            DiagnoseEvent::NoAdvancement { meta, entity } => {
                                let diag = cached_diagnoser.diagnose(&entity);
                                if diag.stuck_score >= 2 {
                                    warn!(
                                        stage = ?meta.stage,
                                        source = ?meta.source,
                                        reasons = ?diag.reasons,
                                        facts = %diag.facts_snapshot,
                                        score = diag.stuck_score,
                                        next_fact = ?diag.next_expected_fact,
                                        "⚠️ Withdraw order stuck diagnosis - NoAdvancement"
                                    );
                                }
                            }
                            DiagnoseEvent::PeriodicScan { meta, entity } => {
                                let diag = cached_diagnoser.diagnose(&entity);
                                if diag.stuck_score >= 2 {
                                    warn!(
                                        stage = ?meta.stage,
                                        source = ?meta.source,
                                        reasons = ?diag.reasons,
                                        facts = %diag.facts_snapshot,
                                        score = diag.stuck_score,
                                        next_fact = ?diag.next_expected_fact,
                                        "⚠️ Withdraw order stuck diagnosis - PeriodicScan"
                                    );
                                }
                            }
                            DiagnoseEvent::ManualDiagnose { meta, entity, extra } => {
                                let diag = cached_diagnoser.diagnose(&entity);
                                warn!(
                                    stage = ?meta.stage,
                                    source = ?meta.source,
                                    reasons = ?diag.reasons,
                                    facts = %diag.facts_snapshot,
                                    score = diag.stuck_score,
                                    next_fact = ?diag.next_expected_fact,
                                    info = %extra,
                                    "⚠️ Withdraw order stuck diagnosis - Manual"
                                );
                            }
                            DiagnoseEvent::IntentDispatchFailed { meta } => {
                                let trade_no = meta.trade_no.to_string();
                                let now = Instant::now();
                                let should_spawn = last_revisit.get(&trade_no).map_or(true, |entry| {
                                    now.duration_since(*entry.value()) > Duration::from_secs(1)
                                });

                                if !should_spawn {
                                    continue;
                                }
                                last_revisit.insert(trade_no.clone(), now);

                                let scanner = scanner_clone.clone();
                                let sem = revisit_sem.clone();
                                if let Ok(permit) = sem.try_acquire_owned() {
                                    tokio::spawn(async move {
                                        let _permit = permit;
                                        tokio::time::sleep(Duration::from_millis(200)).await;
                                        scanner.try_advance(&trade_no).await;
                                    });
                                } else {
                                    warn!(trade_no = %trade_no, "Withdraw diagnose revisit skipped: too many inflight");
                                }
                            }
                        }
                    }
                }
            }
        }));

        // 创建Dispatcher Actor
        let dispatcher_actor = WithdrawShadowDispatcherActor::new(
            api_withdraw_pool,
            DispatcherConfig::default(),
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

        let observer_shutdown = shutdown_tx.subscribe();
        let ordinary_observer_shutdown = shutdown_tx.subscribe();
        let self_transfer_observer_handle = Some(tokio::spawn(async move {
            let api = crate::application::api_wallet_transfer::ApiTransferApplication::new(ctx);
            let ordinary = crate::service::transaction::TransactionService::new(ctx);
            // Independent loops: a stalled API receipt must not stall Core bills.
            tokio::join!(
                api.run_self_transfer_observer(
                    observer_shutdown,
                    std::time::Duration::from_secs(30)
                ),
                ordinary.run_ordinary_receipt_observer(
                    ordinary_observer_shutdown,
                    std::time::Duration::from_secs(30)
                ),
            );
        }));

        Ok(Self {
            shutdown_tx,
            dispatcher_message_tx,
            scanner_handle,
            dispatcher_handle,
            diagnose_handle,
            monitor_handle,
            self_transfer_observer_handle,
            intent_tx,
            scanner,
        })
    }

    /// 停止Shadow系统
    pub async fn stop(&mut self) {
        info!("Stopping Withdraw Shadow System");

        // 发送停止信号
        let _ = self.shutdown_tx.send(());

        if let Some(handle) = self.self_transfer_observer_handle.take() {
            if let Err(error) = handle.await {
                tracing::warn!(%error, "ordinary transfer observer join failed during stop");
            }
        }

        // 等待Actor结束
        if let Some(handle) = self.scanner_handle.take() {
            if let Err(err) = handle.await {
                tracing::warn!(error = %err, "withdraw shadow scanner join failed during stop");
            }
        }

        if let Some(handle) = self.dispatcher_handle.take() {
            if let Err(err) = handle.await {
                tracing::warn!(error = %err, "withdraw shadow dispatcher join failed during stop");
            }
        }

        if let Some(handle) = self.diagnose_handle.take() {
            if let Err(err) = handle.await {
                tracing::warn!(error = %err, "withdraw shadow diagnose join failed during stop");
            }
        }

        if let Some(handle) = self.monitor_handle.take() {
            if let Err(err) = handle.await {
                tracing::warn!(error = %err, "withdraw shadow monitor join failed during stop");
            }
        }

        info!("Withdraw Shadow System stopped");
    }

    /// 获取意图发送器
    pub fn get_intent_tx(&self) -> mpsc::Sender<WithdrawIntent> {
        self.intent_tx.clone()
    }

    /// 触发一次针对特定 trade_no 的提币推进
    ///
    /// 语义：
    /// - 有新事实了，立即尝试推进一次
    /// - 不是执行流程，而是提前跑一次 Shadow 的事实驱动推进
    /// - 幂等，多次调用不会导致重复执行
    /// - Tick 是一种低语义、低优先级的推进触发
    /// - 不保证立即执行
    /// - 不保证一定推进
    /// - 只保证"进入 Shadow 的调度视野"
    pub async fn trigger_withdraw(
        &self,
        trade_no: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        // 直接调用 Scanner 的 try_advance 方法，尝试推进指定交易
        self.scanner.try_advance(trade_no).await;

        Ok(())
    }
}
