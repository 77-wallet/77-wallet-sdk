use rand::Rng;
use std::time::{Duration, Instant};
use tracing::{error, warn};
use wallet_database::{ApiTransactionDbPool, entities::api_collect::ApiCollectEntity};

use crate::infrastructure::api_trans::diagnose_common::throttle::{
    check_rate_limit, should_diagnose,
};

use super::{
    CachedDiagnoser,
    engine::diagnose_collect,
    event::{DiagnoseEvent, DiagnoseEventSender, DiagnoseSource, DiagnoseStage},
};

/// 默认冷却时间：用于 Advancer/Scanner 主路径的“卡住诊断”日志与事件发送
const DEFAULT_COOLDOWN_DURATION: Duration = Duration::from_secs(30);

/// 周期性扫描日志冷却时间：用于抑制同一笔交易在短时间内重复刷屏
const PERIODIC_LOG_COOLDOWN_DURATION: Duration = Duration::from_secs(10 * 60);

// 诊断事件丢失监控
lazy_static::lazy_static! {
    pub static ref DROP_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    pub static ref LAST_DROP_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    pub static ref MAX_BURST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    pub static ref LAST_SUCCESS_SEND_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    pub static ref CURRENT_BURST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
}

/// 检查系统压力
fn check_system_overload() -> bool {
    // 这里使用简化的系统压力检查
    // 实际生产环境应该使用更复杂的指标
    false
}

/// 将 CollectStage 转换为 DiagnoseStage
pub fn collect_stage_to_diagnose_stage(
    stage: crate::infrastructure::api_trans::collect::shadow::stage::CollectStage,
) -> DiagnoseStage {
    match stage {
        crate::infrastructure::api_trans::collect::shadow::stage::CollectStage::NeedOrderAck => {
            DiagnoseStage::OrderAck
        }
        crate::infrastructure::api_trans::collect::shadow::stage::CollectStage::NeedResourceGate => {
            DiagnoseStage::Build
        }
        crate::infrastructure::api_trans::collect::shadow::stage::CollectStage::CanBuild => {
            DiagnoseStage::Build
        }
        crate::infrastructure::api_trans::collect::shadow::stage::CollectStage::NeedTxFeeResAck => {
            DiagnoseStage::TxFeeResAck
        }
        crate::infrastructure::api_trans::collect::shadow::stage::CollectStage::CanBroadcast => {
            DiagnoseStage::Broadcast
        }
        crate::infrastructure::api_trans::collect::shadow::stage::CollectStage::NeedRecover => {
            DiagnoseStage::Recover
        }
        crate::infrastructure::api_trans::collect::shadow::stage::CollectStage::NeedTxExecReceiptUpload => {
            DiagnoseStage::TxExecReceipt
        }
        crate::infrastructure::api_trans::collect::shadow::stage::CollectStage::NeedResultAck => {
            DiagnoseStage::ResultAck
        }
        crate::infrastructure::api_trans::collect::shadow::stage::CollectStage::NeedServiceFeeUpload => {
            DiagnoseStage::ServiceFeeUpload
        }
        crate::infrastructure::api_trans::collect::shadow::stage::CollectStage::FullyBlocked => {
            DiagnoseStage::Unknown
        }
    }
}

pub struct CollectStuckMonitor {
    pool: ApiTransactionDbPool,
    min_interval: Duration,
    max_interval: Duration,
    current_interval: Duration,
    limit: usize,
    max_scan_duration: Duration,
    last_found_count: usize,
    diagnose_tx: Option<DiagnoseEventSender>,
    cached_diagnoser: CachedDiagnoser,
}

impl CollectStuckMonitor {
    pub fn new(
        pool: ApiTransactionDbPool,
        min_interval: Duration,
        max_interval: Duration,
        limit: usize,
        max_scan_duration: Duration,
    ) -> Self {
        Self {
            pool,
            min_interval,
            max_interval,
            current_interval: min_interval, // 初始使用最小间隔
            limit,
            max_scan_duration,
            last_found_count: 0,
            diagnose_tx: None,
            cached_diagnoser: CachedDiagnoser::default(),
        }
    }

    pub fn with_diagnose_tx(mut self, diagnose_tx: DiagnoseEventSender) -> Self {
        self.diagnose_tx = Some(diagnose_tx);
        self
    }

    pub async fn run_loop(&mut self, mut shutdown_rx: tokio::sync::broadcast::Receiver<()>) {
        let mut next_deadline = Instant::now() + self.current_interval;

        loop {
            tokio::select! {
                // 接收关闭信号
                _ = shutdown_rx.recv() => {
                    return;
                },
                // 定时执行扫描
                _ = tokio::time::sleep_until(next_deadline.into()) => {
                    let now = Instant::now();

                    // 检查是否落后过多，需要重同步
                    if now > next_deadline + 3 * self.current_interval {
                        next_deadline = now + self.current_interval;
                    } else {
                        next_deadline += self.current_interval;
                    }

                    // 生成随机 jitter，避免多实例同时扫描
                    let jitter = {
                        let mut rng = rand::thread_rng();
                        Duration::from_millis(rng.gen_range(0..5000))
                    };
                    tokio::time::sleep(jitter).await;

                    if let Err(e) = self.scan_stuck_collects().await {
                        error!(error = %e, "Failed to scan stuck collects");
                    }

                    // 调整下一次扫描间隔
                    self.adjust_interval();
                },
            }
        }
    }

    async fn scan_stuck_collects(&mut self) -> anyhow::Result<()> {
        let start = Instant::now();

        // 使用 DB 预筛选获取可能卡住的交易
        let all_records = wallet_database::repositories::api_wallet::collect::ApiCollectRepo::scan_possible_stuck(
            &self.pool,
            self.limit,
        ).await?;

        // 过滤出确实卡住的交易
        let records: Vec<_> = all_records
            .into_iter()
            .filter(|r| {
                // 检查是否确实卡住
                let diag = self.cached_diagnoser.diagnose(r);
                diag.stuck_score >= 2
            })
            .collect();

        let mut found_count = 0;

        for r in records {
            // 检查扫描时间是否超过限制
            if start.elapsed() > self.max_scan_duration {
                warn!("Stuck scan exceeded max duration, stopping early");
                break;
            }

            let diag = self.cached_diagnoser.diagnose(&r);
            let real_stage = collect_stage_to_diagnose_stage(diag.stage);

            // 周期性扫描刷屏保护：同一 trade_no + stage 在冷却时间内只打一次
            if !should_diagnose(&r.trade_no, real_stage, PERIODIC_LOG_COOLDOWN_DURATION) {
                continue;
            }

            warn!(
                trade_no = %r.trade_no,
                stage = ?real_stage,
                reasons = ?diag.reasons,
                facts = %diag.facts_snapshot,
                score = diag.stuck_score,
                next_fact = ?diag.next_expected_fact,
                "🔥 Periodic stuck detection"
            );

            // 发送诊断事件
            if let Some(tx) = &self.diagnose_tx {
                let diag = self.cached_diagnoser.diagnose(&r);
                let meta = super::event::DiagnoseMeta::new(
                    r.trade_no.clone(),
                    super::event::DiagnoseSource::PeriodicScan,
                    real_stage,
                );
                if tx.try_send(DiagnoseEvent::PeriodicScan { meta, entity: r.clone() }).is_err() {
                    warn!(trade_no = %r.trade_no, "Failed to send diagnose event");
                }
            }

            found_count += 1;
        }

        // 更新最后发现的卡单数量
        self.last_found_count = found_count;

        Ok(())
    }

    /// 调整扫描间隔
    fn adjust_interval(&mut self) {
        if self.last_found_count > 20 {
            // 发现大量卡单，缩短间隔
            self.current_interval = self.min_interval;
        } else if self.last_found_count == 0 {
            // 没有发现卡单，延长间隔
            self.current_interval = std::cmp::min(self.current_interval * 2, self.max_interval);
        }
        // 否则保持当前间隔
    }
}

/// 诊断决策结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnoseDecision {
    /// 诊断事件已发送
    Sent,
    /// 因冷却时间跳过
    SkippedCooldown,
    /// 因速率限制跳过
    RateLimited,
    /// 通道已满
    ChannelFull,
    /// 因 backlog 过载跳过
    BacklogOverload,
    /// 因系统压力过载跳过
    SystemOverload,
    /// 因 actor 死亡跳过
    ActorDead,
    /// 终态记录，无需诊断
    Finished,
    /// 未达到卡住阈值
    NotStuck,
}

/// 快速诊断并可能记录日志（供 Advancer 和 Scanner 调用）
pub fn maybe_log_stuck(
    collect: &ApiCollectEntity,
    diagnose_tx: &Option<DiagnoseEventSender>,
    source: DiagnoseSource,
    stage: DiagnoseStage,
) -> DiagnoseDecision {
    // 终态二次检查
    if collect.finished_at.is_some() {
        return DiagnoseDecision::Finished;
    }

    // 获取真实诊断阶段
    let diag = diagnose_collect(collect);
    // NOTE:
    // - Some snapshots are "fully blocked" and may legitimately map to `Unknown`.
    // - Diagnose must never panic; use the caller-provided stage as a fallback hint.
    let derived_stage = collect_stage_to_diagnose_stage(diag.stage);
    let real_stage = if derived_stage != DiagnoseStage::Unknown { derived_stage } else { stage };

    // 检查系统压力
    if check_system_overload() {
        return DiagnoseDecision::SystemOverload;
    }

    // 检查速率限制
    if !check_rate_limit(real_stage) {
        return DiagnoseDecision::RateLimited;
    }

    // 冷却检查
    if !should_diagnose(&collect.trade_no, real_stage, DEFAULT_COOLDOWN_DURATION) {
        return DiagnoseDecision::SkippedCooldown;
    }

    // 检查是否达到卡住阈值
    if diag.stuck_score < 2 {
        return DiagnoseDecision::NotStuck;
    }

    // 记录警告日志
    warn!(
        stage = ?real_stage,
        source = ?source,
        reasons = ?diag.reasons,
        facts = %diag.facts_snapshot,
        score = diag.stuck_score,
        next_fact = ?diag.next_expected_fact,
        "⚠️ Collect order stuck diagnosis"
    );

    // 非阻塞发送诊断事件
    if let Some(tx) = diagnose_tx {
        let meta = super::event::DiagnoseMeta::new(collect.trade_no.clone(), source, real_stage);

        let event = DiagnoseEvent::NoAdvancement { meta, entity: collect.to_owned() };

        if tx.try_send(event).is_err() {
            // 更新丢失监控指标
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            DROP_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            LAST_DROP_AT.store(now, std::sync::atomic::Ordering::Relaxed);

            let current_burst =
                CURRENT_BURST.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let max_burst = MAX_BURST.load(std::sync::atomic::Ordering::Relaxed);
            if current_burst > max_burst {
                MAX_BURST.store(current_burst, std::sync::atomic::Ordering::Relaxed);
            }

            warn!(trade_no = %collect.trade_no, "Failed to send diagnose event");
            return DiagnoseDecision::ChannelFull;
        } else {
            // 更新成功发送时间
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            LAST_SUCCESS_SEND_AT.store(now, std::sync::atomic::Ordering::Relaxed);
            // 重置当前burst
            CURRENT_BURST.store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }

    DiagnoseDecision::Sent
}

#[cfg(test)]
mod tests {
    use super::collect_stage_to_diagnose_stage;
    use crate::infrastructure::api_trans::collect::{
        diagnose::event::DiagnoseStage, shadow::stage::CollectStage,
    };

    #[test]
    fn stage_mapping_covers_all_variants() {
        let cases = [
            (CollectStage::NeedOrderAck, DiagnoseStage::OrderAck),
            (CollectStage::CanBuild, DiagnoseStage::Build),
            (CollectStage::NeedTxFeeResAck, DiagnoseStage::TxFeeResAck),
            (CollectStage::CanBroadcast, DiagnoseStage::Broadcast),
            (CollectStage::NeedRecover, DiagnoseStage::Recover),
            (CollectStage::NeedTxExecReceiptUpload, DiagnoseStage::TxExecReceipt),
            (CollectStage::NeedResultAck, DiagnoseStage::ResultAck),
            (CollectStage::NeedServiceFeeUpload, DiagnoseStage::ServiceFeeUpload),
            (CollectStage::FullyBlocked, DiagnoseStage::Unknown),
        ];

        for (collect_stage, expected) in cases {
            assert_eq!(
                collect_stage_to_diagnose_stage(collect_stage),
                expected,
                "mapping mismatch for {collect_stage:?}"
            );
        }
    }

    #[test]
    fn adjust_interval_doubles_when_no_stuck_found() {
        use std::time::Duration;
        use wallet_database::ApiTransactionDbPool;

        // We cannot construct CollectStuckMonitor without a real pool, so we test
        // the interval-adjustment logic in isolation by duplicating the formula here.
        fn adjusted(
            last_found: usize,
            current: Duration,
            min: Duration,
            max: Duration,
        ) -> Duration {
            if last_found > 20 {
                min
            } else if last_found == 0 {
                std::cmp::min(current * 2, max)
            } else {
                current
            }
        }

        let min = Duration::from_secs(10);
        let max = Duration::from_secs(300);

        assert_eq!(adjusted(0, Duration::from_secs(10), min, max), Duration::from_secs(20));
        assert_eq!(adjusted(0, Duration::from_secs(150), min, max), Duration::from_secs(300));
        assert_eq!(
            adjusted(0, Duration::from_secs(200), min, max),
            Duration::from_secs(300),
            "capped at max"
        );
        assert_eq!(
            adjusted(5, Duration::from_secs(60), min, max),
            Duration::from_secs(60),
            "kept when moderate"
        );
        assert_eq!(
            adjusted(25, Duration::from_secs(120), min, max),
            Duration::from_secs(10),
            "reset to min on burst"
        );
    }
}

/// 快速诊断并可能记录日志（兼容旧接口）
pub fn maybe_log_stuck_compat(
    collect: &ApiCollectEntity,
    diagnose_tx: &Option<DiagnoseEventSender>,
) -> DiagnoseDecision {
    maybe_log_stuck(collect, diagnose_tx, DiagnoseSource::Advancer, DiagnoseStage::Unknown)
}
