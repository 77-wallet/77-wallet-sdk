/// ============================================================================
///                            架构规则（必须遵守）
/// ============================================================================
///
/// 1. Scanner 只读取“不可逆事实字段”，不读取、不推断、不解释 status
/// 2. Scanner 不使用时间字段做任何决策（building_at / last_broadcast_at 仅用于观测）
/// 3. Scanner 不判断“该不该做”，只判断“是否满足事实条件”
/// 4. Scanner 的唯一职责：
///    事实快照 -> 生成 WithdrawIntent
/// 5. Scanner 中的方法命名必须是事实条件的直接翻译，禁止使用状态语义词（done / finished / completed）
/// 6. Worker 不更新 status
/// 7. 所有副作用只能从 intent 进入 side_effect_worker
/// 8. 禁止在 process_* 中直接调用 backend_api
///
/// ============================================================================
///                          Code Review Checklist
/// ============================================================================
///
/// - [ ] 是否调用legacy_* API？如果是，必须说明原因
/// - [ ] 是否写finished_at？如果不是mark_chain_finished，禁止
/// - [ ] 是否在Worker中引入外部副作用？如果是，必须是SideEffectWorker
/// - [ ] Scanner谓词是否只基于事实字段？
/// - [ ] 是否使用了基于行为的推断，而不是基于事实的判断？
/// - [ ] 所有操作是否有并发安全保障？
mod actor;
mod dispatcher;
mod predicate;
mod scanner;
mod stage;
mod worker;

use std::sync::atomic::{AtomicBool, Ordering};

// Shadow系统开关，默认关闭
pub(crate) static WITHDRAW_SHADOW_ENABLED: AtomicBool = AtomicBool::new(false);

/// 注意：Confirm 不由 Shadow Worker 处理
///
/// 链上结果由 MQTT 注入，由 Domain 层落库，Shadow Worker 只负责：
/// - BuildTx：构建交易
/// - BroadcastTx：广播交易
///
/// Confirm 是 Domain 层对"外部事实注入"的处理，不是 Worker 的工作

/// 链事实轴意图
///
/// 🔒 规则：Confirm 是链事实轴的唯一终态
/// 🔒 规则：Build / Broadcast 是可回滚、可重试的中间态
/// 🔒 规则：只有 Confirm（transaction_time != NULL）才是"世界已发生"
#[derive(Debug, Clone)]
pub enum WithdrawChainIntent {
    /// 预估 TRON 提币手续费快照。
    ///
    /// 这是审计页展示用的旁路快照：
    /// - 只在 fee_estimated_at 缺失时尝试写入
    /// - 不参与 ADVANCEMENT_ORDER
    /// - 失败后留给下一轮扫描重试，不写 err_code/status
    EstimateFee(String),
    /// 评估 TRON 资源闸门。
    ///
    /// 这是真实操作步骤：
    /// - 读取链上资源与本地提币事实
    /// - 落下评估结果事实（resource_ready / need_platform_delegate）
    ///
    /// `resource_ready` 会释放闸门；`need_platform_delegate` 会先持久化为
    /// 独立事实，再由 ApplyPlatformDelegate intent 推进。
    EvalResourceGate(String),
    /// 重试已经由资源决策事实要求的幂等平台代理申请。
    ApplyPlatformDelegate(String),
    /// 构建交易
    BuildTx(String),
    /// 广播交易
    BroadcastTx(String),
    /// 恢复交易
    RecoverTx(String),
    /// 执行资源代理任务
    ExecuteResourceDelegation(String),
}

/// 副作用轴意图
///
/// 🔒 规则：所有副作用必须基于已确认的链事实（transaction_time != NULL）
/// 🔒 规则：所有副作用必须有并发安全保障（DB约束或WHERE CAS）
/// 🔒 规则：finished_at 表示链事实完成，不表示系统处理完成
///
/// SideEffectIntent MUST:
/// - depend only on persisted facts
/// - be safe to execute zero or many times
/// - never modify chain facts
#[derive(Debug, Clone)]
pub enum WithdrawSideEffectIntent {
    /// 发送交易 ACK
    SendTxAck(String),
    /// 发送交易结果 ACK
    SendTxResAck(String),
    /// 上传交易执行回执
    UploadTxExecReceipt(String),
    /// 发送资源任务结果 ACK
    SendResourceResultAck(String),
    /// 发送资源任务 ACK
    SendResourceTaskAck(String),
    /// 上传资源任务交易执行回执
    UploadResourceTxExecReceipt(String),
}

/// 推进意图枚举
///
/// 表示Shadow Scanner生成的状态推进建议
#[derive(Debug, Clone)]
pub enum WithdrawIntent {
    /// 链事实轴意图
    Chain(WithdrawChainIntent),
    /// 副作用轴意图
    SideEffect(WithdrawSideEffectIntent),
}

// 重新导出内部模块的类型，方便外部使用
pub use actor::WithdrawShadowActorSystem;
pub use dispatcher::DispatcherConfig;
pub(crate) use predicate::evaluate_point;
pub use scanner::{ScannerConfig, ShadowScanner};
pub(crate) use stage::{ADVANCEMENT_ORDER, AdvancementPoint};
pub(crate) use worker::{
    ShadowWithdrawCommand, ShadowWithdrawWorker, SideEffectCommand as ShadowSideEffectCommand,
    SideEffectWorker as ShadowSideEffectWorker,
};

/// Shadow系统初始化
pub(crate) async fn init(
    ctx: &'static crate::context::Context,
) -> Option<actor::WithdrawShadowActorSystem> {
    // 检查开关是否开启
    if !WITHDRAW_SHADOW_ENABLED.load(Ordering::Relaxed) {
        tracing::info!("Withdraw Shadow System is disabled");
        return None;
    }

    // 初始化Shadow Actor系统
    let actor_system = match actor::WithdrawShadowActorSystem::new(ctx) {
        Ok(actor_system) => actor_system,
        Err(error) => {
            tracing::error!(?error, "Withdraw Shadow System failed to initialize");
            return None;
        }
    };

    tracing::info!("Withdraw Shadow System initialized and started");
    Some(actor_system)
}

/// 启用Shadow系统
pub fn enable() {
    WITHDRAW_SHADOW_ENABLED.store(true, Ordering::Relaxed);
    tracing::info!("Withdraw Shadow System enabled");
}

/// 禁用Shadow系统
pub fn disable() {
    WITHDRAW_SHADOW_ENABLED.store(false, Ordering::Relaxed);
    tracing::info!("Withdraw Shadow System disabled");
}

/// 检查Shadow系统是否启用
pub fn is_enabled() -> bool {
    WITHDRAW_SHADOW_ENABLED.load(Ordering::Relaxed)
}
