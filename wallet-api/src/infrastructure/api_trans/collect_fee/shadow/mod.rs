/// ============================================================================
///                            架构规则（必须遵守）
/// ============================================================================
///
/// 1. Scanner 只读取“不可逆事实字段”，不读取、不推断、不解释 status
/// 2. Scanner 不使用时间字段做任何决策（building_at / last_broadcast_at 仅用于观测）
/// 3. Scanner 不判断“该不该做”，只判断“是否满足事实条件”
/// 4. Scanner 的唯一职责：
///    事实快照 -> 生成 FeeIntent
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
pub(crate) static FEE_SHADOW_ENABLED: AtomicBool = AtomicBool::new(false);

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
pub enum FeeChainIntent {
    /// 构建交易
    BuildTx(String),
    /// 广播交易
    BroadcastTx(String),
    /// 恢复交易
    RecoverTx(String),
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
pub enum FeeSideEffectIntent {
    /// 发送交易 ACK
    SendTxAck(String),
    /// 发送交易结果 ACK
    SendTxResAck(String),
    /// 上传交易执行回执
    UploadTxExecReceipt(String),
}

/// 推进意图枚举
///
/// 表示Shadow Scanner生成的状态推进建议
#[derive(Debug, Clone)]
pub enum FeeIntent {
    /// 链事实轴意图
    Chain(FeeChainIntent),
    /// 副作用轴意图
    SideEffect(FeeSideEffectIntent),
}

// 重新导出内部模块的类型，方便外部使用
pub use actor::FeeShadowActorSystem;
pub use dispatcher::DispatcherConfig;
pub(crate) use predicate::evaluate_point;
pub use scanner::{ScannerConfig, ShadowScanner};
pub(crate) use stage::{ADVANCEMENT_ORDER, AdvancementPoint};
pub use worker::ShadowFeeWorker;
pub(crate) use worker::{ShadowFeeCommand, SideEffectCommand, SideEffectWorker};

/// Shadow系统初始化
pub(crate) async fn init(
    ctx: &'static crate::context::Context,
) -> Option<actor::FeeShadowActorSystem> {
    // 检查开关是否开启
    if !FEE_SHADOW_ENABLED.load(Ordering::Relaxed) {
        tracing::info!("Fee Shadow System is disabled");
        return None;
    }

    // 初始化Shadow Actor系统
    let actor_system = match actor::FeeShadowActorSystem::new(ctx) {
        Ok(actor_system) => actor_system,
        Err(error) => {
            tracing::error!(?error, "Fee Shadow System failed to initialize");
            return None;
        }
    };

    tracing::info!("Fee Shadow System initialized and started");
    Some(actor_system)
}

/// 启用Shadow系统
pub fn enable() {
    FEE_SHADOW_ENABLED.store(true, Ordering::Relaxed);
    tracing::info!("Fee Shadow System enabled");
}

/// 禁用Shadow系统
pub fn disable() {
    FEE_SHADOW_ENABLED.store(false, Ordering::Relaxed);
    tracing::info!("Fee Shadow System disabled");
}

/// 检查Shadow系统是否启用
pub fn is_enabled() -> bool {
    FEE_SHADOW_ENABLED.load(Ordering::Relaxed)
}
