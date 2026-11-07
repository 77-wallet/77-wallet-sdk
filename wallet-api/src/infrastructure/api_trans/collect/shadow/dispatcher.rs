// collect/shadow/dispatcher.rs
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::{DashMap, DashSet};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{debug, error, info, warn};
use wallet_database::ApiTransactionDbPool;

use crate::infrastructure::api_trans::{
    collect::shadow::{
        ChainIntent, SideEffectIntent,
        worker::{ShadowCollectCommand, ShadowCollectWorker, SideEffectCommand, SideEffectWorker},
    },
    shadow_rpc_policy,
};

use super::CollectIntent;

/// RunningKey 表示当前正在执行的 intent 的唯一标识
/// 用于 trade_no + intent_type 级别的互斥执行
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum RunningKey {
    EvalResourceGate(String),
    TxMutation(String),
    RecoverTx(String),
    ExecuteLocalResourceDelegation(String),
    ExecuteAccountActivation(String),
    SendOrderAck(String),
    SendResultAck(String),
    UploadServiceFee(String),
    UploadTxExecReceipt(String),
    SendTxFeeResAck(String),
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PermitAcquireError {
    Closed,
}

async fn acquire_dispatch_permit(
    semaphore: Arc<Semaphore>,
) -> Result<OwnedSemaphorePermit, PermitAcquireError> {
    semaphore.acquire_owned().await.map_err(|_| PermitAcquireError::Closed)
}

impl DispatcherLane {
    fn for_intent(intent: &CollectIntent) -> Self {
        match intent {
            CollectIntent::SideEffect(_) => Self::SideEffect,
            CollectIntent::Chain(ChainIntent::EvalResourceGate(_)) => Self::ResourceGate,
            CollectIntent::Chain(_) => Self::Chain,
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
    /// 从 CollectIntent 生成对应的 RunningKey
    pub fn from_intent(intent: &CollectIntent) -> Self {
        match intent {
            CollectIntent::Chain(ChainIntent::ExpireUnexecuted(trade_no)) => {
                RunningKey::TxMutation(trade_no.clone())
            }
            CollectIntent::Chain(ChainIntent::EvalResourceGate(trade_no)) => {
                RunningKey::EvalResourceGate(trade_no.clone())
            }
            CollectIntent::Chain(ChainIntent::BuildTx(trade_no)) => {
                RunningKey::TxMutation(trade_no.clone())
            }
            CollectIntent::Chain(ChainIntent::BroadcastTx(trade_no)) => {
                RunningKey::TxMutation(trade_no.clone())
            }
            CollectIntent::Chain(ChainIntent::RecoverTx(trade_no)) => {
                RunningKey::RecoverTx(trade_no.clone())
            }
            CollectIntent::Chain(ChainIntent::ExecuteLocalResourceDelegation(trade_no)) => {
                RunningKey::ExecuteLocalResourceDelegation(trade_no.clone())
            }
            CollectIntent::Chain(ChainIntent::ExecuteAccountActivation(trade_no)) => {
                RunningKey::ExecuteAccountActivation(trade_no.clone())
            }
            CollectIntent::SideEffect(SideEffectIntent::SendOrderAck(trade_no)) => {
                RunningKey::SendOrderAck(trade_no.clone())
            }
            CollectIntent::SideEffect(SideEffectIntent::SendResultAck(trade_no)) => {
                RunningKey::SendResultAck(trade_no.clone())
            }
            CollectIntent::SideEffect(SideEffectIntent::UploadServiceFee(trade_no)) => {
                RunningKey::UploadServiceFee(trade_no.clone())
            }
            CollectIntent::SideEffect(SideEffectIntent::UploadTxExecReceipt(trade_no)) => {
                RunningKey::UploadTxExecReceipt(trade_no.clone())
            }
            CollectIntent::SideEffect(SideEffectIntent::SendTxFeeResAck(trade_no)) => {
                RunningKey::SendTxFeeResAck(trade_no.clone())
            }
            CollectIntent::SideEffect(SideEffectIntent::SendResourceResultAck(trade_no)) => {
                RunningKey::SendResourceResultAck(trade_no.clone())
            }
            CollectIntent::SideEffect(SideEffectIntent::SendResourceTaskAck(trade_no)) => {
                RunningKey::SendResourceTaskAck(trade_no.clone())
            }
            CollectIntent::SideEffect(SideEffectIntent::UploadResourceTxExecReceipt(trade_no)) => {
                RunningKey::UploadResourceTxExecReceipt(trade_no.clone())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DispatcherLane, RunningKey};
    use crate::infrastructure::api_trans::collect::shadow::{ChainIntent, CollectIntent};

    #[test]
    fn expiry_build_and_broadcast_share_mutation_key_but_recover_is_independent() {
        let trade_no = "C_KEY";
        let expiry = RunningKey::from_intent(&CollectIntent::Chain(ChainIntent::ExpireUnexecuted(
            trade_no.to_string(),
        )));
        let build = RunningKey::from_intent(&CollectIntent::Chain(ChainIntent::BuildTx(
            trade_no.to_string(),
        )));
        let broadcast = RunningKey::from_intent(&CollectIntent::Chain(ChainIntent::BroadcastTx(
            trade_no.to_string(),
        )));
        let recover = RunningKey::from_intent(&CollectIntent::Chain(ChainIntent::RecoverTx(
            trade_no.to_string(),
        )));
        assert_eq!(expiry, build);
        assert_eq!(build, broadcast);
        assert_ne!(broadcast, recover);
        assert!(matches!(build, RunningKey::TxMutation(_)));
        assert!(matches!(recover, RunningKey::RecoverTx(_)));
    }

    #[test]
    fn resource_delegation_execution_uses_distinct_running_key() {
        let resource_trade_no = "RSC_EXEC_KEY";
        let resource_exec = RunningKey::from_intent(&CollectIntent::Chain(
            ChainIntent::ExecuteLocalResourceDelegation(resource_trade_no.to_string()),
        ));
        let collect_build = RunningKey::from_intent(&CollectIntent::Chain(ChainIntent::BuildTx(
            resource_trade_no.to_string(),
        )));

        assert!(matches!(resource_exec, RunningKey::ExecuteLocalResourceDelegation(_)));
        assert_ne!(resource_exec, collect_build);
    }

    #[test]
    fn resource_gate_defers_shared_permit_to_chain_first_worker() {
        let resource_gate =
            CollectIntent::Chain(ChainIntent::EvalResourceGate("C_RESOURCE".to_string()));
        let broadcast = CollectIntent::Chain(ChainIntent::BroadcastTx("C_BROADCAST".to_string()));
        let recover = CollectIntent::Chain(ChainIntent::RecoverTx("C_RECOVER".to_string()));

        assert_eq!(DispatcherLane::for_intent(&resource_gate), DispatcherLane::ResourceGate);
        assert!(!DispatcherLane::for_intent(&resource_gate).uses_dispatcher_permit());
        assert_eq!(DispatcherLane::for_intent(&broadcast), DispatcherLane::Chain);
        assert!(DispatcherLane::for_intent(&broadcast).uses_dispatcher_permit());
        assert_eq!(DispatcherLane::for_intent(&recover), DispatcherLane::Chain);
        assert!(DispatcherLane::for_intent(&recover).uses_dispatcher_permit());
    }
}

#[derive(Debug, Clone)]
struct DispatchTrace {
    worker: &'static str,
    command: &'static str,
    phase: &'static str,
    trade_no: String,
    key: RunningKey,
    side_effect: bool,
}

impl DispatchTrace {
    fn from_intent(intent: &CollectIntent, key: RunningKey) -> Self {
        match intent {
            CollectIntent::Chain(ChainIntent::ExpireUnexecuted(trade_no)) => Self {
                worker: "ShadowCollectWorker",
                command: "ExpireUnexecuted",
                phase: "expiry",
                trade_no: trade_no.clone(),
                key,
                side_effect: false,
            },
            CollectIntent::Chain(ChainIntent::EvalResourceGate(trade_no)) => Self {
                worker: "ShadowCollectWorker",
                command: "EvalResourceGate",
                phase: "resource_gate",
                trade_no: trade_no.clone(),
                key,
                side_effect: false,
            },
            CollectIntent::Chain(ChainIntent::BuildTx(trade_no)) => Self {
                worker: "ShadowCollectWorker",
                command: "BuildTx",
                phase: "build",
                trade_no: trade_no.clone(),
                key,
                side_effect: false,
            },
            CollectIntent::Chain(ChainIntent::BroadcastTx(trade_no)) => Self {
                worker: "ShadowCollectWorker",
                command: "BroadcastTx",
                phase: "broadcast",
                trade_no: trade_no.clone(),
                key,
                side_effect: false,
            },
            CollectIntent::Chain(ChainIntent::RecoverTx(trade_no)) => Self {
                worker: "ShadowCollectWorker",
                command: "RecoverTx",
                phase: "recover",
                trade_no: trade_no.clone(),
                key,
                side_effect: false,
            },
            CollectIntent::Chain(ChainIntent::ExecuteLocalResourceDelegation(trade_no)) => Self {
                worker: "ShadowCollectWorker",
                command: "ExecuteLocalResourceDelegation",
                phase: "resource_delegation",
                trade_no: trade_no.clone(),
                key,
                side_effect: false,
            },
            CollectIntent::Chain(ChainIntent::ExecuteAccountActivation(trade_no)) => Self {
                worker: "ShadowCollectWorker",
                command: "ExecuteAccountActivation",
                phase: "account_activation",
                trade_no: trade_no.clone(),
                key,
                side_effect: false,
            },
            CollectIntent::SideEffect(SideEffectIntent::SendOrderAck(trade_no)) => Self {
                worker: "SideEffectWorker",
                command: "SendOrderAck",
                phase: "side_effect",
                trade_no: trade_no.clone(),
                key,
                side_effect: true,
            },
            CollectIntent::SideEffect(SideEffectIntent::SendResultAck(trade_no)) => Self {
                worker: "SideEffectWorker",
                command: "SendResultAck",
                phase: "side_effect",
                trade_no: trade_no.clone(),
                key,
                side_effect: true,
            },
            CollectIntent::SideEffect(SideEffectIntent::UploadServiceFee(trade_no)) => Self {
                worker: "SideEffectWorker",
                command: "UploadServiceFee",
                phase: "side_effect",
                trade_no: trade_no.clone(),
                key,
                side_effect: true,
            },
            CollectIntent::SideEffect(SideEffectIntent::UploadTxExecReceipt(trade_no)) => Self {
                worker: "SideEffectWorker",
                command: "UploadTxExecReceipt",
                phase: "side_effect",
                trade_no: trade_no.clone(),
                key,
                side_effect: true,
            },
            CollectIntent::SideEffect(SideEffectIntent::SendTxFeeResAck(trade_no)) => Self {
                worker: "SideEffectWorker",
                command: "SendTxFeeResAck",
                phase: "side_effect",
                trade_no: trade_no.clone(),
                key,
                side_effect: true,
            },
            CollectIntent::SideEffect(SideEffectIntent::SendResourceResultAck(trade_no)) => Self {
                worker: "SideEffectWorker",
                command: "SendResourceResultAck",
                phase: "side_effect",
                trade_no: trade_no.clone(),
                key,
                side_effect: true,
            },
            CollectIntent::SideEffect(SideEffectIntent::SendResourceTaskAck(trade_no)) => Self {
                worker: "SideEffectWorker",
                command: "SendResourceTaskAck",
                phase: "side_effect",
                trade_no: trade_no.clone(),
                key,
                side_effect: true,
            },
            CollectIntent::SideEffect(SideEffectIntent::UploadResourceTxExecReceipt(trade_no)) => {
                Self {
                    worker: "SideEffectWorker",
                    command: "UploadResourceTxExecReceipt",
                    phase: "side_effect",
                    trade_no: trade_no.clone(),
                    key,
                    side_effect: true,
                }
            }
        }
    }
}

/// RunningGuard 用于 RAII 方式管理 running 标记
/// 确保无论执行路径如何，running 标记都会被正确释放
pub struct RunningGuard {
    key: RunningKey,
    running_set: Arc<DashSet<RunningKey>>,
    queued_times: Arc<DashMap<RunningKey, Instant>>,
    running_times: Arc<DashMap<RunningKey, Instant>>,
}

impl RunningGuard {
    /// 创建一个新的 RunningGuard
    /// 注意：调用者需要确保 key 已经被插入到 running_set 中
    pub fn new(
        key: RunningKey,
        running_set: Arc<DashSet<RunningKey>>,
        queued_times: Arc<DashMap<RunningKey, Instant>>,
        running_times: Arc<DashMap<RunningKey, Instant>>,
    ) -> Self {
        Self { key, running_set, queued_times, running_times }
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        // 无论执行结果如何，都会释放 running 标记
        self.running_set.remove(&self.key);
        self.queued_times.remove(&self.key);
        self.running_times.remove(&self.key);
        debug!(key = ?self.key, "Released running guard");
    }
}

/// Shadow Dispatcher 配置
#[derive(Debug, Clone)]
pub struct DispatcherConfig {
    /// 链路任务并发配额（Build/Broadcast/Recover）
    pub chain_semaphore_size: usize,
    /// 副作用任务并发配额（ACK/回执/服务费上报）
    pub side_effect_semaphore_size: usize,
    /// 二次校验超时时间
    pub db_check_timeout: Duration,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        let chain_semaphore_size =
            shadow_rpc_policy::read_usize_env("COLLECT_SHADOW_DISPATCHER_CONCURRENCY", 12, 2, 64);
        let side_effect_semaphore_size =
            shadow_rpc_policy::read_usize_env("COLLECT_SHADOW_SIDE_EFFECT_CONCURRENCY", 4, 1, 32);
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
/// 3. 路由意图到正确的Worker（Shadow Worker 或 SideEffect Worker）
/// 4. 监控长时间运行的任务（Watchdog Scanner）
pub(crate) struct ShadowDispatcher {
    pool: ApiTransactionDbPool,
    config: DispatcherConfig,
    /// 正在执行的intent的唯一标识集合，防止并发重复执行同一trade_no的同一intent类型
    running: Arc<DashSet<RunningKey>>,
    /// 等待 dispatcher admission 的开始时间，用于排队诊断
    queued_times: Arc<DashMap<RunningKey, Instant>>,
    /// Worker 执行开始时间，用于 Watchdog Scanner
    running_times: Arc<DashMap<RunningKey, Instant>>,
    /// 链路任务并发控制信号量
    chain_semaphore: Arc<Semaphore>,
    /// 副作用任务并发控制信号量
    side_effect_semaphore: Arc<Semaphore>,
    /// Shadow Worker，处理链相关操作
    shadow_worker: Arc<ShadowCollectWorker>,
    /// SideEffect Worker，处理外部依赖的副作用操作
    side_effect_worker: Arc<SideEffectWorker>,
    /// 意图发送器，用于 try_advance 生成的意图
    intent_tx: tokio::sync::mpsc::Sender<CollectIntent>,
}

impl ShadowDispatcher {
    async fn execute_intent(
        intent: CollectIntent,
        shadow_worker: Arc<ShadowCollectWorker>,
        side_effect_worker: Arc<SideEffectWorker>,
    ) -> Result<(), crate::error::service::ServiceError> {
        match intent {
            CollectIntent::Chain(ChainIntent::ExpireUnexecuted(trade_no)) => {
                debug!(trade_no = %trade_no, "Sending ExpireUnexecuted command to Shadow Worker");
                shadow_worker.handle(ShadowCollectCommand::ExpireUnexecuted(trade_no.clone())).await
            }
            CollectIntent::Chain(ChainIntent::EvalResourceGate(trade_no)) => {
                debug!(trade_no = %trade_no, "Sending EvalResourceGate command to Shadow Worker");
                shadow_worker.handle(ShadowCollectCommand::EvalResourceGate(trade_no.clone())).await
            }
            CollectIntent::Chain(ChainIntent::BuildTx(trade_no)) => {
                debug!(trade_no = %trade_no, "Sending BuildTx command to Shadow Worker");
                shadow_worker.handle(ShadowCollectCommand::BuildTx(trade_no.clone())).await
            }
            CollectIntent::Chain(ChainIntent::BroadcastTx(trade_no)) => {
                debug!(trade_no = %trade_no, "Sending Broadcast command to Shadow Worker");
                shadow_worker.handle(ShadowCollectCommand::Broadcast(trade_no.clone())).await
            }
            CollectIntent::Chain(ChainIntent::RecoverTx(trade_no)) => {
                debug!(trade_no = %trade_no, "Sending Recover command to Shadow Worker");
                shadow_worker.handle(ShadowCollectCommand::Recover(trade_no.clone())).await
            }
            CollectIntent::Chain(ChainIntent::ExecuteLocalResourceDelegation(trade_no)) => {
                debug!(trade_no = %trade_no, "Sending ExecuteLocalResourceDelegation command to Shadow Worker");
                shadow_worker
                    .handle(ShadowCollectCommand::ExecuteLocalResourceDelegation(trade_no.clone()))
                    .await
            }
            CollectIntent::Chain(ChainIntent::ExecuteAccountActivation(trade_no)) => {
                debug!(trade_no = %trade_no, "Sending ExecuteAccountActivation command to Shadow Worker");
                shadow_worker
                    .handle(ShadowCollectCommand::ExecuteAccountActivation(trade_no.clone()))
                    .await
            }
            CollectIntent::SideEffect(SideEffectIntent::SendOrderAck(trade_no)) => {
                debug!(trade_no = %trade_no, "Sending SendOrderAck command to SideEffect Worker");
                side_effect_worker.handle(SideEffectCommand::SendOrderAck(trade_no.clone())).await
            }
            CollectIntent::SideEffect(SideEffectIntent::SendResultAck(trade_no)) => {
                debug!(trade_no = %trade_no, "Sending SendResultAck command to SideEffect Worker");
                side_effect_worker.handle(SideEffectCommand::SendResultAck(trade_no.clone())).await
            }
            CollectIntent::SideEffect(SideEffectIntent::UploadServiceFee(trade_no)) => {
                debug!(trade_no = %trade_no, "Sending UploadServiceFee command to SideEffect Worker");
                side_effect_worker
                    .handle(SideEffectCommand::UploadServiceFee(trade_no.clone()))
                    .await
            }
            CollectIntent::SideEffect(SideEffectIntent::UploadTxExecReceipt(trade_no)) => {
                debug!(trade_no = %trade_no, "Sending UploadTxExecReceipt command to SideEffect Worker");
                side_effect_worker
                    .handle(SideEffectCommand::UploadTxExecReceipt(trade_no.clone()))
                    .await
            }
            CollectIntent::SideEffect(SideEffectIntent::SendTxFeeResAck(trade_no)) => {
                info!(trade_no = %trade_no, "Sending SendTxFeeResAck command to SideEffect Worker");
                side_effect_worker
                    .handle(SideEffectCommand::SendTxFeeResAck(trade_no.clone()))
                    .await
            }
            CollectIntent::SideEffect(SideEffectIntent::SendResourceResultAck(trade_no)) => {
                info!(trade_no = %trade_no, "Sending SendResourceResultAck command to SideEffect Worker");
                side_effect_worker
                    .handle(SideEffectCommand::SendResourceResultAck(trade_no.clone()))
                    .await
            }
            CollectIntent::SideEffect(SideEffectIntent::SendResourceTaskAck(trade_no)) => {
                info!(trade_no = %trade_no, "Sending SendResourceTaskAck command to SideEffect Worker");
                side_effect_worker
                    .handle(SideEffectCommand::SendResourceTaskAck(trade_no.clone()))
                    .await
            }
            CollectIntent::SideEffect(SideEffectIntent::UploadResourceTxExecReceipt(trade_no)) => {
                info!(trade_no = %trade_no, "Sending UploadResourceTxExecReceipt command to SideEffect Worker");
                side_effect_worker
                    .handle(SideEffectCommand::UploadResourceTxExecReceipt(trade_no.clone()))
                    .await
            }
        }
    }

    pub(crate) fn new(
        pool: ApiTransactionDbPool,
        config: DispatcherConfig,
        shadow_worker: Arc<ShadowCollectWorker>,
        side_effect_worker: Arc<SideEffectWorker>,
        intent_tx: tokio::sync::mpsc::Sender<CollectIntent>,
    ) -> Self {
        let chain_semaphore_size = config.chain_semaphore_size;
        let side_effect_semaphore_size = config.side_effect_semaphore_size;
        Self {
            pool,
            config,
            running: Arc::new(DashSet::new()),
            queued_times: Arc::new(DashMap::new()),
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
    pub async fn handle_intent(&self, intent: CollectIntent) -> Result<(), anyhow::Error> {
        debug!(?intent, "Received collect intent");
        match &intent {
            CollectIntent::Chain(ChainIntent::BroadcastTx(_)) => {
                shadow_rpc_policy::record_chain_intent_dispatch("broadcast");
            }
            CollectIntent::Chain(ChainIntent::RecoverTx(_)) => {
                shadow_rpc_policy::record_chain_intent_dispatch("recover");
            }
            _ => {}
        }

        // 1. 从intent生成对应的RunningKey
        let running_key = RunningKey::from_intent(&intent);

        // 2. 检查是否正在执行同一类型的intent
        if !self.running.insert(running_key.clone()) {
            debug!(key = ?running_key, "Running key already in running set, skipping");
            return Ok(());
        }

        // 3. 记录排队开始时间。running key 仍提前插入，用于抑制排队期间的重复 intent。
        self.queued_times.insert(running_key.clone(), Instant::now());

        // 4. 克隆需要的字段，用于 spawn 的任务中
        let trace = DispatchTrace::from_intent(&intent, running_key.clone());
        let running = self.running.clone();
        let queued_times = self.queued_times.clone();
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
            // Install the guard before waiting so queue timeout/cancellation
            // always makes the intent eligible for a later scanner retry.
            let _guard = RunningGuard::new(
                running_key.clone(),
                running,
                queued_times,
                running_times.clone(),
            );

            // 获取信号量许可
            let start = std::time::Instant::now();
            let _permit = match semaphore {
                Some(semaphore) => match acquire_dispatch_permit(semaphore).await {
                    Ok(p) => {
                        let acquire_duration = start.elapsed();
                        info!(
                            key = ?running_key,
                            trade_no = %trace.trade_no,
                            command = trace.command,
                            dispatcher_lane = lane.as_str(),
                            queue_wait_ms = acquire_duration.as_millis(),
                            "Acquired dispatcher semaphore permit"
                        );
                        Some(p)
                    }
                    Err(reason) => {
                        warn!(
                            key = ?running_key,
                            trade_no = %trace.trade_no,
                            command = trace.command,
                            dispatcher_lane = lane.as_str(),
                            queue_wait_ms = start.elapsed().as_millis(),
                            reason = ?reason,
                            "Dispatcher semaphore closed; intent will be retried"
                        );
                        return;
                    }
                },
                None => {
                    let acquire_duration = start.elapsed();
                    info!(
                        phase = trace.phase,
                        worker = trace.worker,
                        command = trace.command,
                        trade_no = %trace.trade_no,
                        key = ?trace.key,
                        dispatcher_lane = lane.as_str(),
                        queue_wait_ms = acquire_duration.as_millis(),
                        "Collect resource gate deferred to chain-first worker admission"
                    );
                    None
                }
            };

            // 路由 Intent 到正确的 Worker
            let execute_start = std::time::Instant::now();
            running_times.insert(running_key.clone(), execute_start);
            let result = Self::execute_intent(intent, shadow_worker, side_effect_worker).await;
            if lane == DispatcherLane::ResourceGate {
                info!(
                    phase = trace.phase,
                    worker = trace.worker,
                    command = trace.command,
                    trade_no = %trace.trade_no,
                    key = ?trace.key,
                    dispatcher_lane = lane.as_str(),
                    execution_ms = execute_start.elapsed().as_millis(),
                    "Collect resource gate dispatcher execution finished"
                );
            }
            if let Err(e) = result {
                error!(
                    phase = trace.phase,
                    worker = trace.worker,
                    command = trace.command,
                    trade_no = %trace.trade_no,
                    key = ?trace.key,
                    side_effect = trace.side_effect,
                    error = ?e,
                    "Worker execution failed"
                );
            }
        });

        // 快速返回，Dispatcher 不 await 任务执行
        Ok(())
    }

    /// Watchdog Scanner 方法
    /// 定期检查长时间运行的任务
    pub(crate) async fn watchdog_scan(&self) {
        let now = Instant::now();

        // 排队耗时单独观察，避免把 dispatcher admission 等待误判为 worker 卡住。
        for entry in self.queued_times.iter() {
            let (key, queued_at) = entry.pair();
            if self.running_times.contains_key(key) {
                continue;
            }

            let duration = now.duration_since(*queued_at);
            match duration.as_secs() {
                60..=119 => {
                    warn!(key = ?key, duration = ?duration, "Watchdog: Dispatcher queue waiting for more than 60 seconds");
                }
                120..=179 => {
                    error!(key = ?key, duration = ?duration, "Watchdog: Dispatcher queue waiting for more than 120 seconds");
                }
                180.. => {
                    error!(key = ?key, duration = ?duration, "Watchdog: Dispatcher queue stuck >180s — check dispatcher concurrency");
                }
                _ => {}
            }
        }

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

#[cfg(test)]
#[path = "dispatcher_stack_tests.rs"]
mod stack_tests;
