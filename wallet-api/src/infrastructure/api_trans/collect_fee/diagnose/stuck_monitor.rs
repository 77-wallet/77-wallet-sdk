use rand::Rng;
use std::time::{Duration, Instant};
use tracing::{error, warn};
use wallet_database::{ApiTransactionDbPool, entities::api_fee::ApiFeeEntity};

use crate::infrastructure::api_trans::diagnose_common::throttle::{
    check_rate_limit, should_diagnose,
};

use super::{
    CachedDiagnoser, DiagnoseEvent, DiagnoseEventSender, DiagnoseMeta, DiagnoseSource,
    DiagnoseStage, engine::diagnose_fee,
};

/// 默认冷却时间：用于 Advancer/Scanner 主路径的“卡住诊断”日志与事件发送
const DEFAULT_COOLDOWN_DURATION: Duration = Duration::from_secs(30);

/// 周期性扫描日志冷却时间：用于抑制同一笔交易在短时间内重复刷屏
const PERIODIC_LOG_COOLDOWN_DURATION: Duration = Duration::from_secs(10 * 60);

/// 诊断决策结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnoseDecision {
    Sent,
    SkippedCooldown,
    RateLimited,
    ChannelFull,
    Finished,
    NotStuck,
}

fn fee_stage_to_diagnose_stage(
    stage: crate::infrastructure::api_trans::collect_fee::shadow::AdvancementPoint,
) -> DiagnoseStage {
    use crate::infrastructure::api_trans::collect_fee::shadow::AdvancementPoint::*;
    match stage {
        NeedTxAck => DiagnoseStage::OrderAck,
        CanBuild => DiagnoseStage::Build,
        CanBroadcast => DiagnoseStage::Broadcast,
        NeedRecover => DiagnoseStage::Recover,
        NeedTxExecReceiptUpload => DiagnoseStage::TxExecReceipt,
        NeedTxResAck => DiagnoseStage::ResultAck,
        FullyBlocked => DiagnoseStage::Unknown,
    }
}

/// 快速诊断并可能记录日志（供 try_advance 调用）
pub fn maybe_log_stuck(
    fee: &ApiFeeEntity,
    diagnose_tx: &Option<DiagnoseEventSender>,
    source: DiagnoseSource,
    stage: DiagnoseStage,
) -> DiagnoseDecision {
    if fee.finished_at.is_some() {
        return DiagnoseDecision::Finished;
    }

    let diag = diagnose_fee(fee);
    // NOTE:
    // - Some snapshots are "fully blocked" and may legitimately map to `Unknown`.
    // - Diagnose must never panic; use the caller-provided stage as a fallback hint.
    let derived_stage = fee_stage_to_diagnose_stage(diag.stage);
    let real_stage = if derived_stage != DiagnoseStage::Unknown { derived_stage } else { stage };

    if !check_rate_limit(real_stage) {
        return DiagnoseDecision::RateLimited;
    }

    if !should_diagnose(&fee.trade_no, real_stage, DEFAULT_COOLDOWN_DURATION) {
        return DiagnoseDecision::SkippedCooldown;
    }

    if diag.stuck_score < 2 {
        return DiagnoseDecision::NotStuck;
    }

    warn!(
        stage = ?real_stage,
        source = ?source,
        reasons = ?diag.reasons,
        facts = %diag.facts_snapshot,
        score = diag.stuck_score,
        next_fact = ?diag.next_expected_fact,
        "⚠️ Fee order stuck diagnosis"
    );

    if let Some(tx) = diagnose_tx {
        let meta = DiagnoseMeta::new(fee.trade_no.clone(), source, real_stage);
        if tx.try_send(DiagnoseEvent::NoAdvancement { meta, entity: fee.clone() }).is_err() {
            return DiagnoseDecision::ChannelFull;
        }
    }

    DiagnoseDecision::Sent
}

pub struct FeeStuckMonitor {
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

impl FeeStuckMonitor {
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
            current_interval: min_interval,
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
                _ = shutdown_rx.recv() => {
                    return;
                },
                _ = tokio::time::sleep_until(next_deadline.into()) => {
                    let now = Instant::now();
                    if now > next_deadline + 3 * self.current_interval {
                        next_deadline = now + self.current_interval;
                    } else {
                        next_deadline += self.current_interval;
                    }

                    let jitter = {
                        let mut rng = rand::thread_rng();
                        Duration::from_millis(rng.gen_range(0..5000))
                    };
                    tokio::time::sleep(jitter).await;

                    if let Err(e) = self.scan_stuck_fees().await {
                        error!(error = %e, "Failed to scan stuck fees");
                    }

                    self.adjust_interval();
                }
            }
        }
    }

    async fn scan_stuck_fees(&mut self) -> anyhow::Result<()> {
        let start = Instant::now();

        let all_records =
            wallet_database::repositories::api_wallet::fee::ApiFeeRepo::scan_possible_stuck(
                &self.pool, self.limit,
            )
            .await?;

        let records: Vec<_> = all_records
            .into_iter()
            .filter(|r| self.cached_diagnoser.diagnose(r).stuck_score >= 2)
            .collect();

        let mut found_count = 0usize;

        for r in records {
            if start.elapsed() > self.max_scan_duration {
                warn!("Fee stuck scan exceeded max duration, stopping early");
                break;
            }

            let diag = self.cached_diagnoser.diagnose(&r);
            let real_stage = fee_stage_to_diagnose_stage(diag.stage);

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

            if let Some(tx) = &self.diagnose_tx {
                let meta =
                    DiagnoseMeta::new(r.trade_no.clone(), DiagnoseSource::PeriodicScan, real_stage);
                let _ = tx.try_send(DiagnoseEvent::PeriodicScan { meta, entity: r.clone() });
            }

            found_count += 1;
        }

        self.last_found_count = found_count;
        Ok(())
    }

    fn adjust_interval(&mut self) {
        if self.last_found_count > 20 {
            self.current_interval = self.min_interval;
        } else if self.last_found_count == 0 {
            self.current_interval = std::cmp::min(self.current_interval * 2, self.max_interval);
        }
    }
}
