// withdraw/shadow/dispatcher.rs
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::{DashMap, DashSet};
use tokio::sync::Semaphore;
use tracing::{error, info, warn};
use wallet_database::ApiTransactionDbPool;

use crate::infrastructure::api_trans::{
    shadow_rpc_policy,
    withdraw::shadow::worker::{
        ShadowWithdrawCommand, ShadowWithdrawWorker, SideEffectCommand, SideEffectWorker,
    },
};

use super::{WithdrawChainIntent, WithdrawIntent, WithdrawSideEffectIntent};

/// RunningKey 表示当前正在执行的 intent 的唯一标识
/// 用于 trade_no + intent_type 级别的互斥执行
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum RunningKey {
    EstimateFee(String),
    EvalResourceGate(String),
    ApplyPlatformDelegate(String),
    TxMutation(String),
    RecoverTx(String),
    ExecuteResourceDelegation(String),
    SendTxAck(String),
    SendTxResAck(String),
    UploadTxExecReceipt(String),
    SendResourceResultAck(String),
    SendResourceTaskAck(String),
    UploadResourceTxExecReceipt(String),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum DispatcherLane {
    Chain,
    ResourceGate,
    SideEffect,
}

impl DispatcherLane {
    fn for_intent(intent: &WithdrawIntent) -> Self {
        match intent {
            WithdrawIntent::SideEffect(_) => Self::SideEffect,
            WithdrawIntent::Chain(WithdrawChainIntent::EvalResourceGate(_)) => Self::ResourceGate,
            WithdrawIntent::Chain(_) => Self::Chain,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Chain => "chain",
            Self::ResourceGate => "resource_gate",
            Self::SideEffect => "side_effect",
        }
    }

    fn uses_dispatcher_permit(self) -> bool {
        self != Self::ResourceGate
    }
}

impl RunningKey {
    /// 从 WithdrawIntent 生成对应的 RunningKey
    pub fn from_intent(intent: &WithdrawIntent) -> Self {
        match intent {
            WithdrawIntent::Chain(WithdrawChainIntent::EstimateFee(trade_no)) => {
                RunningKey::EstimateFee(trade_no.clone())
            }
            WithdrawIntent::Chain(WithdrawChainIntent::EvalResourceGate(trade_no)) => {
                RunningKey::EvalResourceGate(trade_no.clone())
            }
            WithdrawIntent::Chain(WithdrawChainIntent::ApplyPlatformDelegate(trade_no)) => {
                RunningKey::ApplyPlatformDelegate(trade_no.clone())
            }
            WithdrawIntent::Chain(WithdrawChainIntent::BuildTx(trade_no)) => {
                RunningKey::TxMutation(trade_no.clone())
            }
            WithdrawIntent::Chain(WithdrawChainIntent::BroadcastTx(trade_no)) => {
                RunningKey::TxMutation(trade_no.clone())
            }
            WithdrawIntent::Chain(WithdrawChainIntent::RecoverTx(trade_no)) => {
                RunningKey::RecoverTx(trade_no.clone())
            }
            WithdrawIntent::Chain(WithdrawChainIntent::ExecuteResourceDelegation(trade_no)) => {
                RunningKey::ExecuteResourceDelegation(trade_no.clone())
            }
            WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxAck(trade_no)) => {
                RunningKey::SendTxAck(trade_no.clone())
            }
            WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxResAck(trade_no)) => {
                RunningKey::SendTxResAck(trade_no.clone())
            }
            WithdrawIntent::SideEffect(WithdrawSideEffectIntent::UploadTxExecReceipt(trade_no)) => {
                RunningKey::UploadTxExecReceipt(trade_no.clone())
            }
            WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendResourceResultAck(
                trade_no,
            )) => RunningKey::SendResourceResultAck(trade_no.clone()),
            WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendResourceTaskAck(trade_no)) => {
                RunningKey::SendResourceTaskAck(trade_no.clone())
            }
            WithdrawIntent::SideEffect(WithdrawSideEffectIntent::UploadResourceTxExecReceipt(
                trade_no,
            )) => RunningKey::UploadResourceTxExecReceipt(trade_no.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dashmap::{DashMap, DashSet};

    use super::{DispatcherConfig, DispatcherLane, RunningGuard, RunningKey};
    use crate::infrastructure::api_trans::withdraw::shadow::{WithdrawChainIntent, WithdrawIntent};

    #[test]
    fn build_and_broadcast_share_mutation_key_but_recover_is_independent() {
        let trade_no = "W_KEY";
        let build = RunningKey::from_intent(&WithdrawIntent::Chain(WithdrawChainIntent::BuildTx(
            trade_no.to_string(),
        )));
        let broadcast = RunningKey::from_intent(&WithdrawIntent::Chain(
            WithdrawChainIntent::BroadcastTx(trade_no.to_string()),
        ));
        let recover = RunningKey::from_intent(&WithdrawIntent::Chain(
            WithdrawChainIntent::RecoverTx(trade_no.to_string()),
        ));
        assert_eq!(build, broadcast);
        assert_ne!(broadcast, recover);
        assert!(matches!(build, RunningKey::TxMutation(_)));
        assert!(matches!(recover, RunningKey::RecoverTx(_)));
    }

    #[test]
    fn resource_gate_defers_shared_permit_to_chain_first_worker() {
        let resource_gate =
            WithdrawIntent::Chain(WithdrawChainIntent::EvalResourceGate("W_RESOURCE".to_string()));
        let broadcast =
            WithdrawIntent::Chain(WithdrawChainIntent::BroadcastTx("W_BROADCAST".to_string()));
        let recover =
            WithdrawIntent::Chain(WithdrawChainIntent::RecoverTx("W_RECOVER".to_string()));

        assert_eq!(DispatcherLane::for_intent(&resource_gate), DispatcherLane::ResourceGate);
        assert!(!DispatcherLane::for_intent(&resource_gate).uses_dispatcher_permit());
        assert_eq!(DispatcherLane::for_intent(&broadcast), DispatcherLane::Chain);
        assert!(DispatcherLane::for_intent(&broadcast).uses_dispatcher_permit());
        assert_eq!(DispatcherLane::for_intent(&recover), DispatcherLane::Chain);
        assert!(DispatcherLane::for_intent(&recover).uses_dispatcher_permit());
    }

    #[test]
    fn withdraw_dispatcher_outer_admission_includes_resource_gate_lane_capacity() {
        let config = DispatcherConfig {
            chain_semaphore_size: 24,
            side_effect_semaphore_size: 12,
            db_check_timeout: std::time::Duration::from_secs(5),
        };

        assert_eq!(config.outer_admission_size(), 40);
    }

    #[test]
    fn queued_intent_is_not_watchdog_tracked_until_execution_starts() {
        let key = RunningKey::EvalResourceGate("W_QUEUED".to_string());
        let running = Arc::new(DashSet::new());
        let running_times = Arc::new(DashMap::new());

        running.insert(key.clone());
        assert!(running.contains(&key));
        assert!(!running_times.contains_key(&key));

        let guard = RunningGuard::start(key.clone(), running.clone(), running_times.clone());
        assert!(running_times.contains_key(&key));

        drop(guard);
        assert!(!running.contains(&key));
        assert!(!running_times.contains_key(&key));
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
    /// 开始执行并创建 RunningGuard。
    /// 调用者需要先将 key 插入 running_set；watchdog 从这里开始计时。
    pub fn start(
        key: RunningKey,
        running_set: Arc<DashSet<RunningKey>>,
        running_times: Arc<DashMap<RunningKey, Instant>>,
    ) -> Self {
        running_times.insert(key.clone(), Instant::now());
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
            shadow_rpc_policy::read_usize_env("WITHDRAW_SHADOW_DISPATCHER_CONCURRENCY", 24, 4, 100);
        let side_effect_semaphore_size = shadow_rpc_policy::read_usize_env(
            "WITHDRAW_SHADOW_SIDE_EFFECT_CONCURRENCY",
            12,
            2,
            100,
        );
        Self {
            chain_semaphore_size,
            side_effect_semaphore_size,
            db_check_timeout: Duration::from_secs(5), // 5秒
        }
    }
}

impl DispatcherConfig {
    pub(crate) fn outer_admission_size(&self) -> usize {
        self.chain_semaphore_size
            .saturating_add(shadow_rpc_policy::shared_resource_gate_concurrency())
            .saturating_add(self.side_effect_semaphore_size)
    }
}

/// Shadow Dispatcher
///
/// 负责：
/// 1. 防止并发重复执行同一trade_no的同一intent类型
/// 2. 控制全局吞吐
/// 3. 路由意图到正确的Worker（Shadow Worker 或 SideEffect Worker）
/// 4. 监控长时间运行的任务（Watchdog Scanner）
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
    shadow_worker: Arc<ShadowWithdrawWorker>,
    /// SideEffect Worker，处理外部依赖的副作用操作
    side_effect_worker: Arc<SideEffectWorker>,
    /// 意图发送器，用于 try_advance 生成的意图
    intent_tx: tokio::sync::mpsc::Sender<WithdrawIntent>,
}

impl ShadowDispatcher {
    pub(crate) fn new(
        pool: ApiTransactionDbPool,
        config: DispatcherConfig,
        shadow_worker: Arc<ShadowWithdrawWorker>,
        side_effect_worker: Arc<SideEffectWorker>,
        intent_tx: tokio::sync::mpsc::Sender<WithdrawIntent>,
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
    ///
    /// 注意：
    /// - Scanner 是纯事实扫描器，不依赖事件、时间或触发源
    /// - Scanner 只需要两种入口：
    ///   1. 周期性全量/分段扫描 (scan_round)
    ///   2. 点对点唤醒 (try_advance)
    /// - 扫描是只读的，不应该参与并发控制
    /// - 并发互斥只存在于执行阶段
    pub async fn handle_intent(&self, intent: WithdrawIntent) -> Result<(), anyhow::Error> {
        info!(?intent, "Received withdraw intent");
        match &intent {
            WithdrawIntent::Chain(WithdrawChainIntent::BroadcastTx(_)) => {
                shadow_rpc_policy::record_chain_intent_dispatch("broadcast");
            }
            WithdrawIntent::Chain(WithdrawChainIntent::RecoverTx(_)) => {
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

        // 3. 克隆需要的字段，用于 spawn 的任务中
        let running = self.running.clone();
        let running_times = self.running_times.clone();
        let lane = DispatcherLane::for_intent(&intent);
        debug_assert_eq!(lane.uses_dispatcher_permit(), lane != DispatcherLane::ResourceGate);
        let semaphore = match lane {
            DispatcherLane::Chain => Some(self.chain_semaphore.clone()),
            DispatcherLane::ResourceGate => None,
            DispatcherLane::SideEffect => Some(self.side_effect_semaphore.clone()),
        };
        let shadow_worker = self.shadow_worker.clone();
        let side_effect_worker = self.side_effect_worker.clone();

        // 4. Spawn 任务执行，实现并发
        tokio::spawn(async move {
            // 获取信号量许可
            let start = std::time::Instant::now();
            let _permit = match semaphore {
                Some(semaphore) => match semaphore.acquire_owned().await {
                    Ok(p) => {
                        let acquire_duration = start.elapsed();
                        info!(
                            key = ?running_key,
                            duration = ?acquire_duration,
                            dispatcher_lane = lane.as_str(),
                            "Acquired dispatcher semaphore permit"
                        );
                        Some(p)
                    }
                    Err(_) => {
                        running.remove(&running_key);
                        running_times.remove(&running_key);
                        return;
                    }
                },
                None => {
                    let acquire_duration = start.elapsed();
                    info!(
                        key = ?running_key,
                        duration = ?acquire_duration,
                        dispatcher_lane = lane.as_str(),
                        "Withdraw resource gate deferred to chain-first worker admission"
                    );
                    None
                }
            };

            // 先克隆一个 key 用于错误日志
            let running_key_for_log = running_key.clone();

            // 创建 RunningGuard，确保无论如何都会释放 running 标记
            let _guard = RunningGuard::start(running_key, running, running_times);

            // 路由 Intent 到正确的 Worker
            if let Err(e) = match intent {
                WithdrawIntent::Chain(WithdrawChainIntent::EstimateFee(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending EstimateFee command to Shadow Worker");
                    shadow_worker.handle(ShadowWithdrawCommand::EstimateFee(trade_no.clone())).await
                }
                WithdrawIntent::Chain(WithdrawChainIntent::EvalResourceGate(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending EvalResourceGate command to Shadow Worker");
                    shadow_worker
                        .handle(ShadowWithdrawCommand::EvalResourceGate(trade_no.clone()))
                        .await
                }
                WithdrawIntent::Chain(WithdrawChainIntent::ApplyPlatformDelegate(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending ApplyPlatformDelegate command to Shadow Worker");
                    shadow_worker
                        .handle(ShadowWithdrawCommand::ApplyPlatformDelegate(trade_no.clone()))
                        .await
                }
                WithdrawIntent::Chain(WithdrawChainIntent::BuildTx(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending BuildTx command to Shadow Worker");
                    shadow_worker.handle(ShadowWithdrawCommand::BuildTx(trade_no.clone())).await
                }
                WithdrawIntent::Chain(WithdrawChainIntent::BroadcastTx(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending Broadcast command to Shadow Worker");
                    shadow_worker.handle(ShadowWithdrawCommand::Broadcast(trade_no.clone())).await
                }
                WithdrawIntent::Chain(WithdrawChainIntent::RecoverTx(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending Recover command to Shadow Worker");
                    shadow_worker.handle(ShadowWithdrawCommand::Recover(trade_no.clone())).await
                }
                WithdrawIntent::Chain(WithdrawChainIntent::ExecuteResourceDelegation(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending ExecuteResourceDelegation command to Shadow Worker");
                    shadow_worker
                        .handle(ShadowWithdrawCommand::ExecuteResourceDelegation(trade_no.clone()))
                        .await
                }
                WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxAck(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending SendTxAck command to SideEffect Worker");
                    side_effect_worker.handle(SideEffectCommand::SendTxAck(trade_no.clone())).await
                }
                WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxResAck(trade_no)) => {
                    info!(trade_no = %trade_no, "Sending SendTxResAck command to SideEffect Worker");
                    side_effect_worker
                        .handle(SideEffectCommand::SendTxResAck(trade_no.clone()))
                        .await
                }
                WithdrawIntent::SideEffect(WithdrawSideEffectIntent::UploadTxExecReceipt(
                    trade_no,
                )) => {
                    info!(trade_no = %trade_no, "Sending UploadTxExecReceipt command to SideEffect Worker");
                    side_effect_worker
                        .handle(SideEffectCommand::UploadTxExecReceipt(trade_no.clone()))
                        .await
                }
                WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendResourceResultAck(
                    trade_no,
                )) => {
                    info!(trade_no = %trade_no, "Sending SendResourceResultAck command to SideEffect Worker");
                    side_effect_worker
                        .handle(SideEffectCommand::SendResourceResultAck(trade_no.clone()))
                        .await
                }
                WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendResourceTaskAck(
                    trade_no,
                )) => {
                    info!(trade_no = %trade_no, "Sending SendResourceTaskAck command to SideEffect Worker");
                    side_effect_worker
                        .handle(SideEffectCommand::SendResourceTaskAck(trade_no.clone()))
                        .await
                }
                WithdrawIntent::SideEffect(
                    WithdrawSideEffectIntent::UploadResourceTxExecReceipt(trade_no),
                ) => {
                    info!(trade_no = %trade_no, "Sending UploadResourceTxExecReceipt command to SideEffect Worker");
                    side_effect_worker
                        .handle(SideEffectCommand::UploadResourceTxExecReceipt(trade_no.clone()))
                        .await
                }
            } {
                error!(error = ?e, key = ?running_key_for_log, "Worker execution failed");
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
