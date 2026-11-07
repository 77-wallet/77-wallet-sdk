// collect_fee/shadow/dispatcher.rs
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::{DashMap, DashSet};
use tokio::sync::Semaphore;
use tracing::{error, info, warn};
use wallet_database::ApiTransactionDbPool;

use crate::infrastructure::api_trans::{
    collect_fee::shadow::{
        FeeChainIntent, FeeSideEffectIntent,
        worker::{ShadowFeeCommand, ShadowFeeWorker, SideEffectCommand, SideEffectWorker},
    },
    shadow_rpc_policy,
};

use super::FeeIntent;

/// RunningKey 表示当前正在执行的 intent 的唯一标识
/// 用于 trade_no + intent_type 级别的互斥执行
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum RunningKey {
    TxMutation(String),
    RecoverTx(String),
    SendTxAck(String),
    SendTxResAck(String),
    UploadTxExecReceipt(String),
}

impl RunningKey {
    /// 从 FeeIntent 生成对应的 RunningKey
    pub fn from_intent(intent: &FeeIntent) -> Self {
        match intent {
            FeeIntent::Chain(FeeChainIntent::BuildTx(trade_no)) => {
                RunningKey::TxMutation(trade_no.clone())
            }
            FeeIntent::Chain(FeeChainIntent::BroadcastTx(trade_no)) => {
                RunningKey::TxMutation(trade_no.clone())
            }
            FeeIntent::Chain(FeeChainIntent::RecoverTx(trade_no)) => {
                RunningKey::RecoverTx(trade_no.clone())
            }
            FeeIntent::SideEffect(FeeSideEffectIntent::SendTxAck(trade_no)) => {
                RunningKey::SendTxAck(trade_no.clone())
            }
            FeeIntent::SideEffect(FeeSideEffectIntent::SendTxResAck(trade_no)) => {
                RunningKey::SendTxResAck(trade_no.clone())
            }
            FeeIntent::SideEffect(FeeSideEffectIntent::UploadTxExecReceipt(trade_no)) => {
                RunningKey::UploadTxExecReceipt(trade_no.clone())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RunningKey;
    use crate::infrastructure::api_trans::collect_fee::shadow::{FeeChainIntent, FeeIntent};

    #[test]
    fn build_and_broadcast_share_mutation_key_but_recover_is_independent() {
        let trade_no = "F_KEY";
        let build = RunningKey::from_intent(&FeeIntent::Chain(FeeChainIntent::BuildTx(
            trade_no.to_string(),
        )));
        let broadcast = RunningKey::from_intent(&FeeIntent::Chain(FeeChainIntent::BroadcastTx(
            trade_no.to_string(),
        )));
        let recover = RunningKey::from_intent(&FeeIntent::Chain(FeeChainIntent::RecoverTx(
            trade_no.to_string(),
        )));
        assert_eq!(build, broadcast);
        assert_ne!(broadcast, recover);
        assert!(matches!(build, RunningKey::TxMutation(_)));
        assert!(matches!(recover, RunningKey::RecoverTx(_)));
    }
}

/// RunningGuard 用于 RAII 方式管理 running 标记
/// 确保无论执行路径如何，running 标记都会被正确释放
pub struct RunningGuard {
    key: RunningKey,
    running_set: Arc<DashSet<RunningKey>>,
    running_times: Arc<DashMap<RunningKey, Instant>>,
}

impl RunningGuard {
    /// 创建一个新的 RunningGuard
    /// 注意：调用者需要确保 key 已经被插入到 running_set 中
    pub fn new(
        key: RunningKey,
        running_set: Arc<DashSet<RunningKey>>,
        running_times: Arc<DashMap<RunningKey, Instant>>,
    ) -> Self {
        Self { key, running_set, running_times }
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        // 无论执行结果如何，都会释放 running 标记
        self.running_set.remove(&self.key);
        self.running_times.remove(&self.key);
        info!(key = ?self.key, "Released running guard");
    }
}

/// Shadow Dispatcher 配置
#[derive(Debug, Clone)]
pub struct DispatcherConfig {
    /// 链路任务并发配额（Build/Broadcast/Recover）
    pub chain_semaphore_size: usize,
    /// 副作用任务并发配额（ACK/回执）
    pub side_effect_semaphore_size: usize,
    /// 二次校验超时时间
    pub db_check_timeout: Duration,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        let chain_semaphore_size =
            shadow_rpc_policy::read_usize_env("FEE_SHADOW_DISPATCHER_CONCURRENCY", 16, 4, 100);
        let side_effect_semaphore_size =
            shadow_rpc_policy::read_usize_env("FEE_SHADOW_SIDE_EFFECT_CONCURRENCY", 8, 2, 100);
        Self {
            chain_semaphore_size,
            side_effect_semaphore_size,
            db_check_timeout: Duration::from_secs(5), // 5秒
        }
    }
}

/// Shadow Dispatcher
///
/// 负责：
/// 1. 防止并发重复执行同一trade_no的同一intent类型
/// 2. 控制全局吞吐
/// 3. DB状态二次校验
/// 4. 决策是否推进状态
/// 5. 路由意图到正确的Worker（Shadow Worker 或 SideEffect Worker）
/// 6. 监控长时间运行的任务（Watchdog Scanner）
pub(crate) struct ShadowDispatcher {
    pool: ApiTransactionDbPool,
    config: DispatcherConfig,
    /// 正在执行的intent的唯一标识集合，防止并发重复执行同一trade_no的同一intent类型
    running: Arc<DashSet<RunningKey>>,
    /// 运行中任务的开始时间，用于 Watchdog Scanner
    running_times: Arc<DashMap<RunningKey, Instant>>,
    /// 链路任务并发控制信号量
    chain_semaphore: Arc<Semaphore>,
    /// 副作用任务并发控制信号量
    side_effect_semaphore: Arc<Semaphore>,
    /// Shadow Worker，处理链相关操作
    shadow_worker: Arc<ShadowFeeWorker>,
    /// SideEffect Worker，处理外部依赖的副作用操作
    side_effect_worker: Arc<SideEffectWorker>,
    /// 意图发送器，用于 try_advance 生成的意图
    intent_tx: tokio::sync::mpsc::Sender<FeeIntent>,
}

impl ShadowDispatcher {
    pub(crate) fn new(
        pool: ApiTransactionDbPool,
        config: DispatcherConfig,
        shadow_worker: Arc<ShadowFeeWorker>,
        side_effect_worker: Arc<SideEffectWorker>,
        intent_tx: tokio::sync::mpsc::Sender<FeeIntent>,
    ) -> Self {
        let chain_semaphore_size = config.chain_semaphore_size;
        let side_effect_semaphore_size = config.side_effect_semaphore_size;
        Self {
            pool,
            config,
            running: Arc::new(DashSet::new()),
            running_times: Arc::new(DashMap::new()),
            chain_semaphore: Arc::new(Semaphore::new(chain_semaphore_size)),
            side_effect_semaphore: Arc::new(Semaphore::new(side_effect_semaphore_size)),
            shadow_worker,
            side_effect_worker,
            intent_tx,
        }
    }

    /// 处理推进意图
    pub async fn handle_intent(&self, intent: FeeIntent) -> Result<(), anyhow::Error> {
        info!(?intent, "Received fee intent");
        match &intent {
            FeeIntent::Chain(FeeChainIntent::BroadcastTx(_)) => {
                shadow_rpc_policy::record_chain_intent_dispatch("broadcast");
            }
            FeeIntent::Chain(FeeChainIntent::RecoverTx(_)) => {
                shadow_rpc_policy::record_chain_intent_dispatch("recover");
            }
            _ => {}
        }

        // 1. 从intent生成对应的RunningKey
        let running_key = RunningKey::from_intent(&intent);

        // 2. 检查是否正在执行同一类型的intent
        if !self.running.insert(running_key.clone()) {
            info!(key = ?running_key, "Running key already in running set, skipping");
            return Ok(());
        }

        // 3. 记录任务开始时间
        self.running_times.insert(running_key.clone(), Instant::now());

        // 4. 克隆需要的字段，用于 spawn 的任务中
        let running = self.running.clone();
        let running_times = self.running_times.clone();
        let is_side_effect_intent = matches!(&intent, FeeIntent::SideEffect(_));
        let semaphore = if is_side_effect_intent {
            self.side_effect_semaphore.clone()
        } else {
            self.chain_semaphore.clone()
        };
        let shadow_worker = self.shadow_worker.clone();
        let side_effect_worker = self.side_effect_worker.clone();

        // 4. Spawn 任务执行，实现并发
        tokio::spawn(async move {
            // 获取信号量许可
            let start = std::time::Instant::now();
            let _permit = match semaphore.acquire_owned().await {
                Ok(p) => {
                    let acquire_duration = start.elapsed();
                    info!(
                        key = ?running_key,
                        duration = ?acquire_duration,
                        side_effect = is_side_effect_intent,
                        "Acquired dispatcher semaphore permit"
                    );
                    p
                }
                Err(_) => {
                    // 信号量已关闭，释放 running 标记并返回
                    running.remove(&running_key);
                    running_times.remove(&running_key);
                    return;
                }
            };

            // 创建 RunningGuard，确保无论如何都会释放 running 标记
            let _guard = RunningGuard::new(running_key, running, running_times);

            // 路由 Intent 到正确的 Worker
            match intent {
                FeeIntent::Chain(FeeChainIntent::BuildTx(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending BuildTx command to Shadow Worker");
                    if let Err(e) =
                        shadow_worker.handle(ShadowFeeCommand::BuildTx(trade_no.clone())).await
                    {
                        error!(error = ?e, "Worker execution failed");
                    }
                }
                FeeIntent::Chain(FeeChainIntent::BroadcastTx(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending Broadcast command to Shadow Worker");
                    if let Err(e) =
                        shadow_worker.handle(ShadowFeeCommand::Broadcast(trade_no.clone())).await
                    {
                        error!(error = ?e, "Worker execution failed");
                    }
                }
                FeeIntent::Chain(FeeChainIntent::RecoverTx(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending Recover command to Shadow Worker");
                    if let Err(e) =
                        shadow_worker.handle(ShadowFeeCommand::Recover(trade_no.clone())).await
                    {
                        error!(error = ?e, "Worker execution failed");
                    }
                }
                FeeIntent::SideEffect(FeeSideEffectIntent::SendTxAck(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending SendTxAck command to SideEffect Worker");
                    side_effect_worker
                        .handle(SideEffectCommand::SendOrderAck(trade_no.clone()))
                        .await;
                }
                FeeIntent::SideEffect(FeeSideEffectIntent::SendTxResAck(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending SendTxResAck command to SideEffect Worker");
                    side_effect_worker
                        .handle(SideEffectCommand::SendResultAck(trade_no.clone()))
                        .await;
                }
                FeeIntent::SideEffect(FeeSideEffectIntent::UploadTxExecReceipt(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending UploadTxExecReceipt command to SideEffect Worker");
                    side_effect_worker
                        .handle(SideEffectCommand::UploadTxExecReceipt(trade_no.clone()))
                        .await;
                }
            }
        });

        // 快速返回，Dispatcher 不 await 任务执行
        Ok(())
    }

    /// Watchdog Scanner 方法
    /// 定期检查长时间运行的任务
    pub(crate) async fn watchdog_scan(&self) {
        let now = Instant::now();

        // 遍历所有运行中的任务
        for entry in self.running_times.iter() {
            let (key, start_time) = entry.pair();
            let duration = now.duration_since(*start_time);

            match duration.as_secs() {
                60..=119 => {
                    // 60秒：warn
                    warn!(key = ?key, duration = ?duration, "Watchdog: Task running for more than 60 seconds");
                }
                120..=179 => {
                    // 120秒：error
                    error!(key = ?key, duration = ?duration, "Watchdog: Task running for more than 120 seconds");
                }
                180.. => {
                    // 180秒：只打日志，不强制释放
                    error!(key = ?key, duration = ?duration, "Watchdog: Task stuck >180s — manual investigation required");
                }
                _ => {
                    // 正常运行时间，忽略
                }
            }
        }
    }
}
