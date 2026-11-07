/// # Expand 系统架构文档
///
/// ## 核心设计原则
///
/// ### 🔴 1. 事实驱动架构
///
/// Expand 系统是一个**事实驱动系统**，而非传统的状态机系统。
/// 系统的状态推进**完全基于数据库中的不可变事实**，而不是中间状态。
///
/// #### 什么是事实？
/// - **不可回退**：一旦为真，永远为真
/// - **由副作用产生**：不是由 Scanner 推导出来的
/// - **可重复消费**：可以被任何时间、任何进程、任何重启后重复读取
/// - **幂等性**：多次消费不会产生不同结果
///
/// #### 系统中的核心事实字段
/// - `expand_complete_at`：表示 expand 副作用已成功完成的不可逆事实
/// - `notified_at`：（预留）表示通知已成功发送的不可逆事实
///
/// ### 🔴 2. 明确的职责分离
///
/// | 组件 | 职责 | 禁止行为 |
/// |------|------|----------|
/// | **Worker** | 执行副作用（Create/Init/Notify），写入事实 | 修改状态，参与状态决策 |
/// | **Scanner** | 基于 DB 事实推进状态，派发副作用 | 执行副作用，直接修改事实 |
/// | **Planner** | 推进 Pending → Running，创建 Items | 参与状态决策，执行副作用 |
/// | **Executor** | 执行具体的外部操作 | 参与状态管理 |
///
/// ### 🔴 3. 状态是派生值
///
/// - `status` 字段是**派生值**，完全可以从 `expand_complete_at` 等事实推导出来
/// - `finished_count` 字段是**缓存值**，可以从 `expand_batch_item` 表重新计算
/// - 状态字段仅用于**展示层**和**表现层**，不参与业务决策
///
/// ### 🔴 4. 幂等性设计
///
/// - 所有副作用操作（Create/Init/Notify）必须是幂等的
/// - Scanner 可以并发执行，多次扫描不会破坏系统一致性
/// - 系统支持重启、恢复和自愈
///
/// ### 🔴 5. 可恢复性
///
/// - 系统可以从任何状态恢复，不需要特殊的恢复机制
/// - Scanner 的 `recover()` 方法本质上是一次完整的事实扫描
/// - 没有 "正在做" 的状态，只有 "已完成" 的事实
///
/// ## 批次生命周期
///
/// ```text
/// Pending → Running → Done → Notified
/// ```
///
/// ### 阶段说明
///
/// 1. **Pending**：批次已创建，但 Planner 尚未处理
/// 2. **Running**：Planner 已处理，Items 已创建，Scanner 正在处理
/// 3. **Done**：所有 Items 已完成，等待 Notify 副作用
/// 4. **Notified**：Notify 副作用已完成，`expand_complete_at` 事实已设置
///
/// ### 事实驱动的状态转换
///
/// - `Pending → Running`：由 Planner 基于资源可用性推进
/// - `Running → Done`：由 Scanner 基于 `expand_batch_item` 表事实推进
/// - `Done → Notified`：由 Scanner 基于 `expand_complete_at` 事实推进
///
/// ## 最佳实践
///
/// ### ✅ 应该做的
/// - 使用 `is_batch_expand_completed()` 检查批次完成状态
/// - 依赖 `expand_complete_at` 事实做业务决策
/// - 确保副作用操作是幂等的
/// - 遵循职责分离原则
/// - 使用 Scanner 的节流机制控制资源使用
///
/// ### ❌ 不应该做的
/// - 直接修改 `status` 字段
/// - 依赖 `status` 字段做业务决策
/// - 在 Worker 中修改状态
/// - 忽略幂等性要求
/// - 引入 "正在做" 的状态
/// - 使用内存标记代替 DB 事实
///
/// ## 未来演进
///
/// - 添加 `notified_at` 事实字段，实现完整的事实驱动链路
/// - 进一步弱化状态字段，将其完全变为派生值
/// - 增强事实驱动的可观测性
/// - 优化 Scanner 的节流机制
///
/// ## 常见问题
///
/// ### Q: 为什么使用事实驱动？
/// A: 事实驱动系统具有更好的可靠性、可恢复性和可扩展性，避免了状态机的复杂性和死锁风险。
///
/// ### Q: 为什么 Worker 只写事实？
/// A: 职责分离原则确保系统的可维护性和可测试性，避免了状态管理的复杂性。
///
/// ### Q: 为什么 Scanner 不执行副作用？
/// A: Scanner 是系统的 "大脑"，应该专注于状态推进，而不是执行具体的操作，这样可以提高系统的可扩展性。
///
/// ### Q: 为什么 `finished_count` 是缓存？
/// A: 缓存 `finished_count` 可以提高查询性能，但系统不依赖它做决策，确保了系统的可靠性。
///
/// ### Q: 为什么支持重试？
/// A: 重试机制确保系统在面对网络波动、外部服务不可用时能够自动恢复，提高了系统的可用性。
///
/// --- End of Architectural Documentation ---

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ExpandBatchEntity {
    pub batch_id: String,
    pub uid: String,
    pub serial_no: String,
    pub chain_code: String,
    pub total_count: i32,
    pub finished_count: i32,
    pub status: ExpandBatchStatus, // 0=running, 1=done
    pub retry_count: i32,          // 重试次数
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    /// === ARCHITECTURAL FACT ===
    ///
    /// local_complete_at represents the irreversible fact that
    /// all local expand items for this batch have been fully completed.
    ///
    /// - Written by Worker when all items are completed (via CAS operation).
    /// - Scanner MUST NOT write this field.
    /// - Once set, it MUST NEVER be cleared or overwritten.
    /// - This is the foundational fact for the expand system.
    pub local_complete_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 本地扩容完成的时间（所有items已完成）

    /// === ARCHITECTURAL FACT ===
    ///
    /// expand_complete_at represents the irreversible fact that
    /// Notify side-effect has been successfully completed and reported to backend.
    ///
    /// - Written only by Expand Workers upon irreversible completion of Notify side-effect.
    /// - Scanner MUST NOT write this field.
    /// - Once set, it MUST NEVER be cleared or overwritten.
    /// - This fact is used to mark the batch as Notified.
    /// - ❗ It does NOT represent local expand completion
    /// - ❗ Do NOT use it for Running → Done judgment
    pub expand_complete_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 通知后端成功的上报时间
}

/// Pending
///    |
///    | Planner 判断可以创建 Item
///    v
/// Running
///    |
///    | local_complete_at IS NOT NULL (本地扩容完成事实已形成)
///    v
/// Done              // 本地所有items已完成，等待通知执行者
///    |
///    | notify 任务执行成功
///    |
///    | expand_complete_at IS NOT NULL (通知后端成功的上报时间已记录)
///    v
/// Notified          // 通知成功，终态

/// 🔴 核心状态语义
/// - Pending: 批次已创建，Planner 尚未处理
/// - Running: Planner 已处理，Items 已创建，Scanner 正在处理
/// - Done: local_complete_at 事实已形成（所有items已完成），但尚未通知外部系统
/// - Notified: 通知外部系统成功，expand_complete_at 事实已形成，终态
/// - Failed: 批次执行失败

/// 🔴 重要不变量
/// - local_complete_at IS NOT NULL → status IN (Done, Notified)
/// - expand_complete_at IS NOT NULL → status IN (Done, Notified)
/// - status = Notified → expand_complete_at IS NOT NULL
/// - Done 状态表示"本地已完成但未通知"
/// - Notified 状态只能由通知执行者推进
/// - local_complete_at 是不可逆的本地完成事实
/// - expand_complete_at 是不可逆的通知后端成功的上报时间

/// 🔴 状态推进依据
/// - Running → Done: 基于 local_complete_at IS NOT NULL
/// - Done → Notified: 基于 expand_complete_at IS NOT NULL

/// 🔴 禁止的操作
/// - 禁止基于 expand_complete_at 推进状态到 Done
/// - 禁止将 expand_complete_at 作为本地完成的标志
/// - 禁止将 local_complete_at 作为通知完成的标志
#[derive(
    Debug, PartialEq, serde_repr::Serialize_repr, serde_repr::Deserialize_repr, sqlx::Type,
)]
#[repr(i32)]
pub enum ExpandBatchStatus {
    Pending = 0,
    Running = 1,
    Done = 2,
    Notified = 3,
    Failed = 4,
}

#[derive(Debug, Clone)]
pub struct CreateExpandBatchEntity {
    pub uid: String,
    pub batch_id: String,
    pub serial_no: String,
    pub chain_code: String,
    pub total_count: i32,
}

impl CreateExpandBatchEntity {
    pub fn new(
        uid: &str,
        batch_id: &str,
        serial_no: &str,
        chain_code: &str,
        total_count: i32,
    ) -> Self {
        Self {
            uid: uid.to_string(),
            batch_id: batch_id.to_string(),
            serial_no: serial_no.to_string(),
            chain_code: chain_code.to_string(),
            total_count,
        }
    }
}

#[derive(sqlx::FromRow, Debug)]
pub struct BatchWithCount {
    #[sqlx(flatten)]
    pub batch: ExpandBatchEntity,
    pub item_count: i64,
}
