use super::CollectIntent;
use crate::{
    error::service::ServiceError,
    infrastructure::api_trans::{
        collect::{
            diagnose::{DiagnoseEvent, DiagnoseSource, DiagnoseStage, maybe_log_stuck},
            shadow::{ChainIntent, SideEffectIntent, stage::CollectStage},
        },
        fast_path::{FastPathFlow, FastPathReason, FastPathTarget, dispatch_fast_path},
        shadow_rpc_policy,
    },
};
use dashmap::DashMap;
use scopeguard::defer;
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Semaphore, mpsc::Sender};
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;
use wallet_database::{
    entities::api_trade_type::ApiTradeType,
    repositories::api_wallet::{
        collect::ApiCollectRepo, resource_delegation::ApiResourceDelegationRepo,
    },
};

/// 运行中记录的TTL（秒）
const RUNNING_TTL: Duration = Duration::from_secs(30);
/// Semaphore获取超时（秒）
const SEM_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(2);
/// DB连接获取超时（秒）
const DB_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(3);
/// DB查询超时（秒）
const DB_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const BROADCAST_GUARD_SKIP_RETRY_DELAY: Duration = Duration::from_secs(1);

/// 从CollectIntent中提取trade_no
fn intent_trade_no(intent: &CollectIntent) -> Option<String> {
    match intent {
        CollectIntent::Chain(chain_intent) => match chain_intent {
            ChainIntent::ExpireUnexecuted(trade_no) => Some(trade_no.clone()),
            ChainIntent::EvalResourceGate(trade_no) => Some(trade_no.clone()),
            ChainIntent::BuildTx(trade_no) => Some(trade_no.clone()),
            ChainIntent::BroadcastTx(trade_no) => Some(trade_no.clone()),
            ChainIntent::RecoverTx(trade_no) => Some(trade_no.clone()),
            ChainIntent::ExecuteLocalResourceDelegation(trade_no) => Some(trade_no.clone()),
            ChainIntent::ExecuteAccountActivation(trade_no) => Some(trade_no.clone()),
        },
        CollectIntent::SideEffect(side_effect_intent) => match side_effect_intent {
            SideEffectIntent::SendOrderAck(trade_no) => Some(trade_no.clone()),
            SideEffectIntent::SendResultAck(trade_no) => Some(trade_no.clone()),
            SideEffectIntent::UploadServiceFee(trade_no) => Some(trade_no.clone()),
            SideEffectIntent::UploadTxExecReceipt(trade_no) => Some(trade_no.clone()),
            SideEffectIntent::SendTxFeeResAck(trade_no) => Some(trade_no.clone()),
            SideEffectIntent::SendResourceResultAck(trade_no) => Some(trade_no.clone()),
            SideEffectIntent::SendResourceTaskAck(trade_no) => Some(trade_no.clone()),
            SideEffectIntent::UploadResourceTxExecReceipt(trade_no) => Some(trade_no.clone()),
        },
    }
}

#[derive(Clone)]
pub struct ShadowAdvancer {
    ctx: &'static crate::context::Context,
    intent_tx: Sender<CollectIntent>,
    diagnose_tx: Option<Sender<DiagnoseEvent>>,
    running: Arc<DashMap<String, (Uuid, Instant)>>,
    last_gc: Arc<AtomicU64>,
    max_concurrency: Arc<AtomicUsize>,
    semaphore: Arc<Semaphore>,
}

impl ShadowAdvancer {
    pub fn new(
        ctx: &'static crate::context::Context,
        intent_tx: Sender<CollectIntent>,
        diagnose_tx: Option<Sender<DiagnoseEvent>>,
    ) -> Self {
        // 从环境变量读取最大并发数（SQLite 场景下默认保守）
        let max_concurrency = shadow_rpc_policy::read_usize_env("SHADOW_MAX_CONCURRENCY", 8, 4, 64);

        Self {
            ctx,
            intent_tx,
            diagnose_tx,
            running: Arc::new(DashMap::new()),
            last_gc: Arc::new(AtomicU64::new(0)),
            max_concurrency: Arc::new(AtomicUsize::new(max_concurrency)),
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
        }
    }

    pub fn configured_max_concurrency(&self) -> usize {
        self.max_concurrency.load(Ordering::Relaxed)
    }

    fn runtime_capacity_snapshot(&self) -> (u32, usize, usize, usize) {
        let sem_available = self.semaphore.available_permits();
        let max_concurrency = self.max_concurrency.load(Ordering::Relaxed);

        match self.ctx.api_transaction_pool() {
            Ok(pool) => {
                let pool = pool.as_ref();
                (pool.size(), pool.num_idle(), sem_available, max_concurrency)
            }
            Err(_) => (0, 0, sem_available, max_concurrency),
        }
    }

    /// 获取当前时间戳（毫秒）
    fn now_ms() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
    }

    /// 执行 running map 的 GC，清理过期记录
    fn gc_running(running: &Arc<DashMap<String, (Uuid, Instant)>>, last_gc: &Arc<AtomicU64>) {
        let now = Self::now_ms();
        let last_gc_time = last_gc.load(Ordering::Relaxed);

        // 每10秒执行一次GC
        if now - last_gc_time > 10000 {
            let before = running.len();
            running.retain(|_, (_, t)| t.elapsed() < RUNNING_TTL);
            let after = running.len();

            if before > after {
                trace!("Running map GC: removed {} expired entries", before - after);
            }

            last_gc.store(now, Ordering::Relaxed);
        }
    }

    /// 尝试基于当前事实推进一个阶段
    ///
    /// 注意：try_advance 每次最多推进一个阶段
    /// 多阶段推进依赖后续 Tick 或定时扫描
    ///
    /// 参数：
    /// - trade_no: 归集交易编号
    ///
    /// 行为：
    /// 1. 检查是否正在运行（带TTL）
    /// 2. 获取semaphore许可（带timeout）
    /// 3. 查询最新的DB状态（带timeout）
    /// 4. 基于事实状态，按照 ADVANCEMENT_ORDER 顺序检查可推进点
    /// 5. 找到第一个满足条件的推进点，生成对应意图
    /// 6. 发送意图并返回
    pub async fn try_advance(&self, trade_no: &str) {
        if let Err(error) = self.try_advance_result(trade_no).await {
            error!(trade_no = %trade_no, %error, "Collect advancer try_advance failed");
        }
    }

    /// Enqueue BuildTx directly after a resource gate release fact is persisted.
    ///
    /// This is a low-latency wakeup for the common platform-resource path. The
    /// BuildTx worker remains fact-driven and idempotent, so an early or
    /// duplicate wakeup is still guarded by the persisted collect state.
    pub fn trigger_build_fast_path_after_resource_gate_release(&self, trade_no: &str) {
        let intent = CollectIntent::Chain(ChainIntent::BuildTx(trade_no.to_string()));
        let failed_trade_no = trade_no.to_string();
        dispatch_fast_path(
            FastPathFlow::Collect,
            FastPathReason::ResourceGateReleased,
            FastPathTarget::BuildTx,
            trade_no,
            || self.intent_tx.try_send(intent),
            || self.emit_intent_dispatch_failed(failed_trade_no),
        );
    }

    pub fn trigger_resource_gate_fast_path_after_order_ack(&self, trade_no: &str) {
        let intent = CollectIntent::Chain(ChainIntent::EvalResourceGate(trade_no.to_string()));
        let failed_trade_no = trade_no.to_string();
        dispatch_fast_path(
            FastPathFlow::Collect,
            FastPathReason::TxAckSent,
            FastPathTarget::EvalResourceGate,
            trade_no,
            || self.intent_tx.try_send(intent),
            || self.emit_intent_dispatch_failed(failed_trade_no),
        );
    }

    pub fn trigger_build_fast_path_after_order_ack(&self, trade_no: &str) {
        let intent = CollectIntent::Chain(ChainIntent::BuildTx(trade_no.to_string()));
        let failed_trade_no = trade_no.to_string();
        dispatch_fast_path(
            FastPathFlow::Collect,
            FastPathReason::TxAckSent,
            FastPathTarget::BuildTx,
            trade_no,
            || self.intent_tx.try_send(intent),
            || self.emit_intent_dispatch_failed(failed_trade_no),
        );
    }

    pub fn trigger_local_delegation_fast_path(&self, resource_trade_no: &str) {
        let intent = CollectIntent::Chain(ChainIntent::ExecuteLocalResourceDelegation(
            resource_trade_no.to_string(),
        ));
        let failed_trade_no = resource_trade_no.to_string();
        dispatch_fast_path(
            FastPathFlow::Collect,
            FastPathReason::LocalDelegationPersisted,
            FastPathTarget::ExecuteLocalResourceDelegation,
            resource_trade_no,
            || self.intent_tx.try_send(intent),
            || self.emit_intent_dispatch_failed(failed_trade_no),
        );
    }

    pub fn schedule_recover_retry(&self, trade_no: &str) {
        let recover_key = shadow_rpc_policy::recover_dispatch_key(trade_no);
        let delay = shadow_rpc_policy::recover_dispatch_cooldown(&recover_key);
        self.schedule_recover_retry_after(trade_no, delay);
    }

    pub fn schedule_broadcast_retry_after_guard_skip(&self, trade_no: &str) {
        let advancer = self.clone();
        let trade_no = trade_no.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(BROADCAST_GUARD_SKIP_RETRY_DELAY).await;
            info!(
                trade_no = %trade_no,
                delay_ms = BROADCAST_GUARD_SKIP_RETRY_DELAY.as_millis(),
                "Waking collect BroadcastTx after pre-execution RPC guard skip"
            );
            advancer.try_advance(&trade_no).await;
        });
    }

    fn schedule_recover_retry_after(&self, trade_no: &str, delay: Duration) {
        let advancer = self.clone();
        let trade_no = trade_no.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            info!(
                trade_no = %trade_no,
                delay_ms = delay.as_millis(),
                "Waking collect Recover point-to-point after cooldown"
            );
            advancer.try_advance(&trade_no).await;
        });
    }

    pub fn trigger_fast_path_after_tx_fee_res_ack(&self, trade_no: &str, raw_tx_present: bool) {
        let target =
            if raw_tx_present { FastPathTarget::BroadcastTx } else { FastPathTarget::BuildTx };
        let intent = if raw_tx_present {
            CollectIntent::Chain(ChainIntent::BroadcastTx(trade_no.to_string()))
        } else {
            CollectIntent::Chain(ChainIntent::BuildTx(trade_no.to_string()))
        };
        let failed_trade_no = trade_no.to_string();
        dispatch_fast_path(
            FastPathFlow::Collect,
            FastPathReason::TxFeeResAckSent,
            target,
            trade_no,
            || self.intent_tx.try_send(intent),
            || self.emit_intent_dispatch_failed(failed_trade_no),
        );
    }

    async fn try_advance_result(&self, trade_no: &str) -> Result<(), ServiceError> {
        // 执行 running map GC
        let running = self.running.clone();
        let last_gc = self.last_gc.clone();
        Self::gc_running(&running, &last_gc);

        // 生成唯一guard id
        let guard_id = Uuid::new_v4();
        let trade_no_str = trade_no.to_string();

        // 检查是否正在运行（带TTL）
        let mut is_running = false;

        match running.entry(trade_no_str.clone()) {
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                // 不存在，插入
                entry.insert((guard_id, Instant::now()));
            }
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                // 存在，检查TTL
                let (_, ts) = entry.get();
                if ts.elapsed() < RUNNING_TTL {
                    // 未过期，跳过
                    debug!(trade_no = %trade_no, "Advance skipped: already running");
                    is_running = true;
                } else {
                    // 已过期，替换
                    entry.insert((guard_id, Instant::now()));
                }
            }
        }

        if is_running {
            return Ok(());
        }

        // 确保清理running记录
        defer! {
            // 只删除自己的guard，使用一次remove操作完成
            if let Some((_, (id, _))) = running.remove(&trade_no_str) {
                if id != guard_id {
                    // 如果不是自己的guard，重新插入
                    running.insert(trade_no_str.clone(), (id, Instant::now()));
                }
            }
        };

        // 获取semaphore许可（带timeout）
        let semaphore = self.semaphore.clone();
        let permit =
            match tokio::time::timeout(SEM_ACQUIRE_TIMEOUT, semaphore.acquire_owned()).await {
                Ok(Ok(permit)) => permit,
                Ok(Err(e)) => {
                    let (db_pool_size, db_pool_idle, sem_available, advancer_max_concurrency) =
                        self.runtime_capacity_snapshot();
                    error!(
                        trade_no = %trade_no,
                        error = %e,
                        db_pool_size,
                        db_pool_idle,
                        sem_available,
                        advancer_max_concurrency,
                        "Failed to acquire semaphore"
                    );
                    return Ok(());
                }
                Err(_) => {
                    let (db_pool_size, db_pool_idle, sem_available, advancer_max_concurrency) =
                        self.runtime_capacity_snapshot();
                    error!(
                        trade_no = %trade_no,
                        timeout = ?SEM_ACQUIRE_TIMEOUT,
                        db_pool_size,
                        db_pool_idle,
                        sem_available,
                        advancer_max_concurrency,
                        "Semaphore acquire timeout"
                    );
                    return Ok(());
                }
            };
        // 确保permit在作用域结束前不会被释放
        let _permit = permit;

        trace!(trade_no = %trade_no, "Try advancing collect transaction");

        // 查询最新的DB状态（带timeout）
        let pool = self.ctx.api_transaction_pool()?;
        let collect = match tokio::time::timeout(
            DB_QUERY_TIMEOUT,
            ApiCollectRepo::get_api_collect_by_trade_no(&pool, trade_no),
        )
        .await
        {
            Ok(Ok(collect)) => collect,
            Ok(Err(e)) => {
                let (db_pool_size, db_pool_idle, sem_available, advancer_max_concurrency) =
                    self.runtime_capacity_snapshot();
                error!(
                    trade_no = %trade_no,
                    error = %e,
                    db_pool_size,
                    db_pool_idle,
                    sem_available,
                    advancer_max_concurrency,
                    "Failed to get api collect by trade_no"
                );
                return Err(e.into());
            }
            Err(_) => {
                let (db_pool_size, db_pool_idle, sem_available, advancer_max_concurrency) =
                    self.runtime_capacity_snapshot();
                error!(
                    trade_no = %trade_no,
                    timeout = ?DB_QUERY_TIMEOUT,
                    db_pool_size,
                    db_pool_idle,
                    sem_available,
                    advancer_max_concurrency,
                    "DB query timeout"
                );
                return Ok(());
            }
        };

        // 架构级保险丝：冻结或已终止的记录不允许推进
        if collect.finished_at.is_some() {
            debug!(trade_no = %trade_no, "Advance skipped: frozen or finished");
            return Ok(());
        }

        match tokio::time::timeout(
            DB_QUERY_TIMEOUT,
            self.pending_resource_result_ack_trade_no(trade_no),
        )
        .await
        {
            Ok(Ok(Some(resource_trade_no))) => {
                info!(
                    trade_no = %trade_no,
                    resource_trade_no = %resource_trade_no,
                    "Resource result ACK is pending; advancing ACK before collect main chain"
                );
                self.dispatch_intent(CollectIntent::SideEffect(
                    SideEffectIntent::SendResourceResultAck(resource_trade_no),
                ));
                return Ok(());
            }
            Ok(Ok(None)) => {}
            Ok(Err(e)) => {
                error!(
                    trade_no = %trade_no,
                    error = %e,
                    "Failed to check pending resource result ACK"
                );
                return Ok(());
            }
            Err(_) => {
                error!(
                    trade_no = %trade_no,
                    timeout = ?DB_QUERY_TIMEOUT,
                    "Pending resource result ACK query timeout"
                );
                return Ok(());
            }
        }

        // err_code 冻结：只允许 UploadTxExecReceipt
        if collect.err_code.is_some() {
            let eval = crate::infrastructure::api_trans::collect::shadow::predicate::evaluate_stage(
                CollectStage::NeedTxExecReceiptUpload,
                &collect,
            );

            if eval.can_advance {
                info!(trade_no = %trade_no, "Need to upload tx exec receipt (err_code frozen state)");
                let intent = CollectIntent::SideEffect(SideEffectIntent::UploadTxExecReceipt(
                    trade_no.to_string(),
                ));
                self.dispatch_intent(intent);
            }
            return Ok(());
        }

        // 按照 COLLECT_ADVANCEMENT_ORDER 顺序检查可推进点
        // 顺序与 scan_round 完全一致，确保行为一致性
        for stage in
            crate::infrastructure::api_trans::collect::shadow::stage::COLLECT_ADVANCEMENT_ORDER
                .iter()
        {
            let eval = crate::infrastructure::api_trans::collect::shadow::predicate::evaluate_stage(
                *stage, &collect,
            );

            if eval.can_advance {
                match stage {
                    CollectStage::NeedOrderAck => {
                        info!(trade_no = %trade_no, "Need to send order ACK");
                        let intent = CollectIntent::SideEffect(SideEffectIntent::SendOrderAck(
                            trade_no.to_string(),
                        ));
                        self.dispatch_intent(intent);
                        return Ok(());
                    }
                    CollectStage::NeedResourceGate => {
                        info!(trade_no = %trade_no, "Need to check resource gate");
                        let intent = CollectIntent::Chain(ChainIntent::EvalResourceGate(
                            trade_no.to_string(),
                        ));
                        self.dispatch_intent(intent);
                        return Ok(());
                    }
                    CollectStage::CanBuild => {
                        info!(trade_no = %trade_no, "Can build transaction");
                        let intent =
                            CollectIntent::Chain(ChainIntent::BuildTx(trade_no.to_string()));
                        self.dispatch_intent(intent);
                        return Ok(());
                    }
                    CollectStage::NeedTxFeeResAck => {
                        info!(trade_no = %trade_no, "Need to send tx fee res ACK");
                        let intent = CollectIntent::SideEffect(SideEffectIntent::SendTxFeeResAck(
                            trade_no.to_string(),
                        ));
                        self.dispatch_intent(intent);
                        return Ok(());
                    }
                    CollectStage::CanBroadcast => {
                        if let Some((host, remaining)) =
                            shadow_rpc_policy::breaker_open_for_chain_code(
                                self.ctx,
                                &collect.chain_code,
                            )
                            .await
                        {
                            debug!(
                                trade_no = %trade_no,
                                chain_code = %collect.chain_code,
                                host = %host,
                                remaining = ?remaining,
                                "try_advance_skip_because_breaker_open: collect advancer broadcast skipped"
                            );
                            if shadow_rpc_policy::should_emit_breaker_warn(&format!(
                                "collect.advancer.try_advance.breaker:{}:{}",
                                collect.chain_code, host
                            )) {
                                warn!(
                                    trade_no = %trade_no,
                                    chain_code = %collect.chain_code,
                                    host = %host,
                                    remaining = ?remaining,
                                    "try_advance_skip_because_breaker_open: collect advancer broadcast skipped"
                                );
                            }
                            return Ok(());
                        }
                        info!(trade_no = %trade_no, "Can broadcast transaction");
                        let intent =
                            CollectIntent::Chain(ChainIntent::BroadcastTx(trade_no.to_string()));
                        self.dispatch_intent(intent);
                        return Ok(());
                    }
                    CollectStage::NeedRecover => {
                        if let Some((host, remaining)) =
                            shadow_rpc_policy::breaker_open_for_chain_code(
                                self.ctx,
                                &collect.chain_code,
                            )
                            .await
                        {
                            debug!(
                                trade_no = %trade_no,
                                chain_code = %collect.chain_code,
                                host = %host,
                                remaining = ?remaining,
                                "try_advance_skip_because_breaker_open: collect advancer recover skipped"
                            );
                            if shadow_rpc_policy::should_emit_breaker_warn(&format!(
                                "collect.advancer.try_advance.breaker:{}:{}",
                                collect.chain_code, host
                            )) {
                                warn!(
                                    trade_no = %trade_no,
                                    chain_code = %collect.chain_code,
                                    host = %host,
                                    remaining = ?remaining,
                                    "try_advance_skip_because_breaker_open: collect advancer recover skipped"
                                );
                            }
                            return Ok(());
                        }
                        let recover_key = shadow_rpc_policy::recover_dispatch_key(trade_no);
                        if !shadow_rpc_policy::allow_broadcast_recover_dispatch(
                            &recover_key,
                            collect.last_broadcast_at,
                        ) {
                            debug!(
                                trade_no = %trade_no,
                                cooldown = ?shadow_rpc_policy::recover_dispatch_cooldown(&recover_key),
                                "recover_skip_because_cooldown: collect advancer recover skipped"
                            );
                            return Ok(());
                        }
                        info!(trade_no = %trade_no, "Need to recover transaction");
                        let intent =
                            CollectIntent::Chain(ChainIntent::RecoverTx(trade_no.to_string()));
                        self.dispatch_intent(intent);
                        return Ok(());
                    }
                    CollectStage::NeedTxExecReceiptUpload => {
                        info!(trade_no = %trade_no, "Need to upload tx exec receipt");
                        let intent = CollectIntent::SideEffect(
                            SideEffectIntent::UploadTxExecReceipt(trade_no.to_string()),
                        );
                        self.dispatch_intent(intent);
                        return Ok(());
                    }
                    CollectStage::NeedResultAck => {
                        info!(trade_no = %trade_no, "Need to send result ACK");
                        let intent = CollectIntent::SideEffect(SideEffectIntent::SendResultAck(
                            trade_no.to_string(),
                        ));
                        self.dispatch_intent(intent);
                        return Ok(());
                    }
                    CollectStage::NeedServiceFeeUpload => {
                        info!(trade_no = %trade_no, "Need to upload service fee");
                        let intent = CollectIntent::SideEffect(SideEffectIntent::UploadServiceFee(
                            trade_no.to_string(),
                        ));
                        self.dispatch_intent(intent);
                        return Ok(());
                    }
                    CollectStage::FullyBlocked => {
                        // 不应该出现在推进顺序中
                        debug_assert!(false, "FullyBlocked must not appear in ADVANCEMENT_ORDER");
                        continue;
                    }
                }
            }
        }

        // 无可用推进点
        trace!(trade_no = %trade_no, "No advancement possible based on current facts");

        // 检查是否可能卡住
        let _ = maybe_log_stuck(
            &collect,
            &self.diagnose_tx,
            DiagnoseSource::Advancer,
            DiagnoseStage::Unknown,
        );
        Ok(())
    }

    /// 分发推进意图
    fn dispatch_intent(&self, intent: CollectIntent) {
        debug!(?intent, "Generated collect intent");

        // 保存trade_no用于后续可能的诊断事件
        let trade_no = intent_trade_no(&intent);

        // 将意图发送给Dispatcher（非阻塞）
        if let Err(e) = self.intent_tx.try_send(intent) {
            error!("Failed to send collect intent: {}", e);

            // 触发scanner revisit
            if let Some(trade_no) = trade_no {
                self.emit_intent_dispatch_failed(trade_no);
            }
        }
    }

    fn emit_intent_dispatch_failed(&self, trade_no: String) {
        if let Some(diagnose_tx) = &self.diagnose_tx {
            let meta = crate::infrastructure::api_trans::collect::diagnose::event::DiagnoseMeta::new(
                trade_no,
                crate::infrastructure::api_trans::collect::diagnose::event::DiagnoseSource::Advancer,
                crate::infrastructure::api_trans::collect::diagnose::event::DiagnoseStage::Unknown,
            );
            if let Err(e) = diagnose_tx.try_send(DiagnoseEvent::IntentDispatchFailed { meta }) {
                error!("Failed to send IntentDispatchFailed event: {}", e);
            }
        }
    }

    async fn pending_resource_result_ack_trade_no(
        &self,
        origin_trade_no: &str,
    ) -> Result<Option<String>, crate::error::service::ServiceError> {
        let pool = self.ctx.api_resource_pool()?;

        Ok(ApiResourceDelegationRepo::find_pending_result_ack_by_origin(
            &pool,
            ApiTradeType::Collect as i64,
            origin_trade_no,
        )
        .await?
        .map(|row| row.resource_trade_no))
    }
}

impl fmt::Debug for ShadowAdvancer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShadowAdvancer").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use wallet_database::{
        entities::{
            api_collect::ApiCollectStatus,
            api_resource_delegation::{
                ApiResourceDelegationResultStatus, NewApiResourceDelegation,
            },
        },
        repositories::api_wallet::{
            collect::ApiCollectRepo, resource_delegation::ApiResourceDelegationRepo,
        },
    };

    #[tokio::test]
    async fn test_try_advance() {
        // 创建测试用的通道
        let (intent_tx, _intent_rx) = mpsc::channel(100);
        let (diagnose_tx, _diagnose_rx) = mpsc::channel(100);

        let ctx = crate::testkit::context::api_trans_test_ctx().await;

        // 创建 ShadowAdvancer 实例
        let advancer = ShadowAdvancer::new(ctx, intent_tx, Some(diagnose_tx));

        // 测试 try_advance 方法
        // 注意：由于我们使用的是内存数据库，这个测试可能会失败
        // 实际测试时需要提供一个真实的数据库连接并确保表结构存在
        let trade_no = "test_trade_no";
        advancer.try_advance(trade_no).await;
    }

    #[tokio::test]
    async fn try_advance_prioritizes_resource_result_ack_before_fee_upload() -> anyhow::Result<()> {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let pool = ctx.api_transaction_pool()?;
        let resource_pool = ctx.api_resource_pool()?;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let (diagnose_tx, _diagnose_rx) = mpsc::channel(100);
        let advancer = ShadowAdvancer::new(ctx, intent_tx, Some(diagnose_tx));

        ApiCollectRepo::upsert_api_collect(
            &pool,
            "uid_1",
            "collect",
            "from_addr",
            "to_addr",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            "C_pending_rsc_ack",
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE api_collect
            SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                resource_gate_released_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                need_service_fee = true,
                service_fee_uploaded_at = NULL
            WHERE trade_no = ?
            "#,
        )
        .bind("C_pending_rsc_ack")
        .execute(pool.as_ref())
        .await?;

        ApiResourceDelegationRepo::upsert_original_order_result_fact(
            &resource_pool,
            NewApiResourceDelegation::platform_delegate(
                "uid_1",
                "C_pending_rsc_ack",
                "C_pending_rsc_ack",
                ApiTradeType::Collect as i64,
                "",
                "",
                "0",
            ),
            ApiResourceDelegationResultStatus::Fail,
            Some(3),
            Some(r#"{"tradeNo":"C_pending_rsc_ack","status":false}"#),
        )
        .await?;

        advancer.try_advance("C_pending_rsc_ack").await;

        let intent = intent_rx.try_recv().expect("resource ACK intent should be dispatched");
        assert!(matches!(
            intent,
            CollectIntent::SideEffect(SideEffectIntent::SendResourceResultAck(ref trade_no))
                if trade_no == "C_pending_rsc_ack"
        ));
        assert!(intent_rx.try_recv().is_err());

        Ok(())
    }

    #[tokio::test]
    async fn collect_resource_gate_fast_path_dispatches_build_intent() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let advancer = ShadowAdvancer::new(ctx, intent_tx, None);

        advancer.trigger_build_fast_path_after_resource_gate_release("C_resource_released");

        let intent = intent_rx.try_recv().expect("BuildTx intent should be dispatched");
        assert!(matches!(
            intent,
            CollectIntent::Chain(ChainIntent::BuildTx(ref trade_no))
                if trade_no == "C_resource_released"
        ));
        assert!(intent_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn collect_order_ack_fast_path_dispatches_resource_gate_intent() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let advancer = ShadowAdvancer::new(ctx, intent_tx, None);

        advancer.trigger_resource_gate_fast_path_after_order_ack("C_order_ack_sent");

        let intent = intent_rx.try_recv().expect("resource gate intent should be dispatched");
        assert!(matches!(
            intent,
            CollectIntent::Chain(ChainIntent::EvalResourceGate(ref trade_no))
                if trade_no == "C_order_ack_sent"
        ));
        assert!(intent_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn collect_order_ack_fast_path_dispatches_build_intent_for_non_tron() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let advancer = ShadowAdvancer::new(ctx, intent_tx, None);

        advancer.trigger_build_fast_path_after_order_ack("C_non_tron_order_ack_sent");

        let intent = intent_rx.try_recv().expect("build intent should be dispatched");
        assert!(matches!(
            intent,
            CollectIntent::Chain(ChainIntent::BuildTx(ref trade_no))
                if trade_no == "C_non_tron_order_ack_sent"
        ));
        assert!(intent_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn collect_local_delegation_fast_path_dispatches_execute_intent() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let advancer = ShadowAdvancer::new(ctx, intent_tx, None);

        advancer.trigger_local_delegation_fast_path("rsc_local_delegate_C_fast");

        let intent = intent_rx.try_recv().expect("local delegation intent should be dispatched");
        assert!(matches!(
            intent,
            CollectIntent::Chain(ChainIntent::ExecuteLocalResourceDelegation(ref trade_no))
                if trade_no == "rsc_local_delegate_C_fast"
        ));
        assert!(intent_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn collect_fresh_broadcast_recover_advancer_dispatches_with_fast_admission()
    -> anyhow::Result<()> {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let pool = ctx.api_transaction_pool()?;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let advancer = ShadowAdvancer::new(ctx, intent_tx, None);
        let trade_no = format!("C_fresh_recover_{}", Uuid::new_v4().simple());

        ApiCollectRepo::upsert_api_collect(
            &pool,
            "uid_fresh_recover",
            "collect",
            "from_addr",
            "to_addr",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            &trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE api_collect
            SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                resource_gate_released_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                raw_tx = '{}',
                tx_hash = '0xfresh',
                last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?
            "#,
        )
        .bind(&trade_no)
        .execute(pool.as_ref())
        .await?;

        advancer.try_advance(&trade_no).await;

        let intent = intent_rx.try_recv().expect("fresh Recover intent should be dispatched");
        assert!(matches!(
            intent,
            CollectIntent::Chain(ChainIntent::RecoverTx(ref dispatched_trade_no))
                if dispatched_trade_no == &trade_no
        ));
        let recover_key = shadow_rpc_policy::recover_dispatch_key(&trade_no);
        assert_eq!(
            shadow_rpc_policy::recover_dispatch_cooldown(&recover_key),
            Duration::from_secs(3)
        );

        Ok(())
    }

    #[tokio::test]
    async fn collect_fresh_broadcast_recover_wakeup_dispatches_without_scanner()
    -> anyhow::Result<()> {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let pool = ctx.api_transaction_pool()?;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let advancer = ShadowAdvancer::new(ctx, intent_tx, None);
        let trade_no = format!("C_fresh_recover_wakeup_{}", Uuid::new_v4().simple());

        ApiCollectRepo::upsert_api_collect(
            &pool,
            "uid_fresh_recover_wakeup",
            "collect",
            "from_addr",
            "to_addr",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            &trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await?;
        sqlx::query(
            r#"
            UPDATE api_collect
            SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                resource_gate_released_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                raw_tx = '{}',
                tx_hash = '0xfresh-wakeup',
                last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?
            "#,
        )
        .bind(&trade_no)
        .execute(pool.as_ref())
        .await?;

        advancer.schedule_recover_retry_after(&trade_no, Duration::from_millis(1));

        let intent = tokio::time::timeout(Duration::from_secs(1), intent_rx.recv())
            .await?
            .expect("delayed wakeup should dispatch Recover");
        assert!(matches!(
            intent,
            CollectIntent::Chain(ChainIntent::RecoverTx(ref dispatched_trade_no))
                if dispatched_trade_no == &trade_no
        ));

        Ok(())
    }

    #[tokio::test]
    async fn collect_tx_fee_res_ack_fast_path_dispatches_build_intent_without_raw_tx() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let advancer = ShadowAdvancer::new(ctx, intent_tx, None);

        advancer.trigger_fast_path_after_tx_fee_res_ack("C_fee_ack_sent", false);

        let intent = intent_rx.try_recv().expect("BuildTx intent should be dispatched");
        assert!(matches!(
            intent,
            CollectIntent::Chain(ChainIntent::BuildTx(ref trade_no))
                if trade_no == "C_fee_ack_sent"
        ));
        assert!(intent_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn collect_tx_fee_res_ack_fast_path_dispatches_broadcast_intent_with_raw_tx() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let (intent_tx, mut intent_rx) = mpsc::channel(100);
        let advancer = ShadowAdvancer::new(ctx, intent_tx, None);

        advancer.trigger_fast_path_after_tx_fee_res_ack("C_fee_ack_sent", true);

        let intent = intent_rx.try_recv().expect("BroadcastTx intent should be dispatched");
        assert!(matches!(
            intent,
            CollectIntent::Chain(ChainIntent::BroadcastTx(ref trade_no))
                if trade_no == "C_fee_ack_sent"
        ));
        assert!(intent_rx.try_recv().is_err());
    }
}
