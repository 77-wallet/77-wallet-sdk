// planner.rs
use wallet_database::entities::{
    address_query_state::AddressQueryStatus, expand_batch::ExpandBatchStatus,
};

use crate::{
    domain::api_wallet::account::ApiAccountDomain, error::service::ServiceError,
    infrastructure::expand_address::event::ExpandEventSender,
};
use wallet_database::repositories::api_wallet::{
    address_query_state::AddressQueryStateRepo, expand_batch::ExpandBatchRepo,
};

/// ExpandPlanner - 负责将Pending状态的Batch转换为Running状态，并一次性创建所有Item
///
/// 🔴 核心职责：
/// - 将Pending状态的Batch转换为Running状态
/// - 一次性创建所有Item，确保扩容边界不被后续地址查询污染
/// - 确保在并发/crash/多实例环境下的正确性
/// - 发送HintScan事件通知Scanner检查新创建的Item
///
/// 🔴 核心幂等边界声明：
/// - **可重复执行**：多次调用不会产生副作用
/// - **可并发执行**：多实例同时运行不会导致数据不一致
/// - **可在任意时刻执行**：无时间依赖，可随时触发
/// - **纯创建逻辑**：仅创建Item，不修改已存在item的状态
///
/// 🔴 核心操作约束：
/// 1. **只允许CREATE**：禁止UPDATE操作，已存在的items状态不会被改变
/// 2. **唯一性保证**：基于(uid, chain, input_index)唯一索引，冲突视为已存在
/// 3. **一次性创建**：Batch从Pending→Running时一次性创建所有Item
/// 4. **触发时机**：仅由Scanner或定时任务触发，不主动运行
/// 5. **幂等性实现**：通过唯一索引和INSERT IGNORE(或ON DUPLICATE KEY)机制保证
/// 6. **CAS保护**：使用数据库级CAS确保Batch状态转换的原子性
/// 7. **事件驱动**：创建Item后发送HintScan事件，提高系统响应性
///
/// 🔴 设计意图：
/// - Planner = 延迟决策工具，确保扩容边界在安全时机冻结
/// - 解决"扩容请求可能早于地址事实收敛"的并发问题
/// - 确保系统在各种故障场景下的可恢复性
/// - 支持水平扩展，可部署多个实例
/// - 简化错误处理，失败可直接重试
/// - 事件驱动提高响应性，定时兜底保证可靠性
#[derive(Clone)]
pub struct ExpandPlanner {
    ctx: &'static crate::context::Context,
    event_tx: Option<ExpandEventSender>,
}

impl ExpandPlanner {
    pub fn new(ctx: &'static crate::context::Context, event_tx: Option<ExpandEventSender>) -> Self {
        Self { ctx, event_tx }
    }

    /// 🔒 显式门控函数：can_plan - 明确真值表，不可被绕过
    /// 🔒 设计真值表：
    /// | address_query_state | can_plan | 原因          |
    /// | ------------------- | -------- | ----------- |
    /// | Some(Running)       | ❌ false  | 边界未收敛       |
    /// | Some(Done)          | ✅ true   | 边界已稳定       |
    /// | Some(Failed)        | ❌ false  | 数据不可信       |
    /// | **None**            | ✅ true   | 不需要 / 不会有查询 |
    /// 🔒 核心语义：
    /// - `None` 表示该 uid + chain 从设计上就不需要地址查询
    /// - 绝不表示"查询尚未开始"或"查询被遗漏"
    /// - `None` is a design-time guarantee, not a runtime missing state
    fn can_plan(
        &self,
        query_state: Option<
            wallet_database::entities::address_query_state::AddressQueryStateEntity,
        >,
        batch_id: &str,
    ) -> bool {
        match query_state {
            Some(state) => {
                match state.status {
                    AddressQueryStatus::Running => {
                        // 查询正在进行中，不允许创建Item
                        tracing::info!(batch_id = %batch_id, "ExpandPlanner: address query is still running, skipping - wait for query completion");
                        false
                    }
                    AddressQueryStatus::Done => {
                        // 查询完成，可以创建Item
                        tracing::info!(batch_id = %batch_id, "ExpandPlanner: address query done, proceeding to create items");
                        true
                    }
                    AddressQueryStatus::Failed => {
                        // 查询失败，不允许创建Item
                        tracing::error!(batch_id = %batch_id, "ExpandPlanner: address query failed, skipping permanently - manual intervention required");
                        false
                    }
                }
            }
            None => {
                // 🔒 系统级语义约束：None 是设计时保证，不是运行时缺失状态
                // 🔒 唯一合法含义：该 uid + chain 从设计上就不需要地址查询
                // 🔒 反例注释：
                // 🔒 ❌ 错误：None = "还没写入 address_query_state"
                // 🔒 ❌ 错误：None = "查询尚未开始"
                // 🔒 ❌ 错误：None = "查询被遗漏"
                // 🔒 ✅ 正确：None = "该链从设计上就不需要地址查询"
                // 🔒 若未来该链新增查询逻辑，AddressQueryState 必须显式写入 Running / Done
                // 🔒 明确禁止将 None 解释为任何运行时状态
                tracing::info!(batch_id = %batch_id, "ExpandPlanner: address query state not found, treating as DONE - this is a design-time guarantee");
                true
            }
        }
    }

    /// 规划 Pending 批次，以及中断后 Running 但没有任何明细的批次
    ///
    /// 🔒 工程级最终注释模板
    /// plan_all_batches 只是调度入口，不包含任何业务判断
    /// 所有决策必须发生在 plan_batch 内
    /// 🔒 明确禁止在该函数中添加任何条件判断
    /// 🔒 所有业务判断必须且只能存在于 plan_batch() 函数中
    /// 🔒 该函数的唯一作用是遍历可规划批次并调用plan_batch()
    /// 🔒 禁止修改该函数的核心逻辑，禁止添加任何业务假设
    // #[instrument(skip(self))]
    pub async fn plan_all_batches(&self) -> Result<(), ServiceError> {
        tracing::debug!("ExpandPlanner: planning all batches");

        let pool = self.ctx.api_wallet_pool()?;
        // 查询普通待规划批次及没有冻结明细的中断批次
        let pending_batches = ExpandBatchRepo::get_batches_for_planning(&pool).await?;

        for batch in pending_batches {
            tracing::debug!(batch_id = %batch.batch_id, status = ?batch.status, "ExpandPlanner: processing pending batch");

            // 处理单个批次，所有业务判断由plan_batch()完成
            self.plan_batch(&batch.batch_id).await?;
        }

        Ok(())
    }

    /// 原子规划待执行批次；已有任何明细的批次由仓储拒绝重新分配。
    // #[instrument(skip(self))]
    pub async fn plan_batch(&self, batch_id: &str) -> Result<(), ServiceError> {
        tracing::info!(batch_id = %batch_id, "ExpandPlanner: planning batch items");

        let pool = self.ctx.api_wallet_pool()?;
        // 读取候选批次；写事务会再次检查状态及是否已有冻结明细。
        let batch = ExpandBatchRepo::get_batch(&pool, batch_id).await?;

        if let Some(batch) = batch {
            // 终态不再规划；Running 仅在没有任何明细时可由仓储恢复。
            if !matches!(batch.status, ExpandBatchStatus::Pending | ExpandBatchStatus::Running) {
                tracing::info!(batch_id = %batch_id, status = ?batch.status, "ExpandPlanner: batch is not eligible for planning, skipping");
                return Ok(());
            }

            // 检查地址查询是否完成
            // 🔒 核心约束：扩容边界冻结必须晚于地址查询完成
            // 🔒 含义：Batch可以提前创建，但不能提前Running
            let query_state =
                AddressQueryStateRepo::get_by_uid_and_chain(&pool, &batch.uid, &batch.chain_code)
                    .await?;

            // 🔒 显式门控函数：can_plan - 明确真值表，不可被绕过
            // 🔒 这是把"设计真值表"变成"代码护城河"
            // 🔒 防止未来某人绕过address_query_state
            let can_plan = self.can_plan(query_state, batch_id);
            if !can_plan {
                return Ok(());
            }

            // The repository owns the write transaction. Allocation still runs
            // after the winning claim, using the existing read-only algorithm.
            let created = ExpandBatchRepo::plan_items(&pool, batch_id, |batch| async move {
                ApiAccountDomain::calculate_indices_for_expansion_with_ctx(
                    self.ctx,
                    &batch.uid,
                    &batch.chain_code,
                    &batch.batch_id,
                    batch.total_count as u32,
                )
                .await
            })
            .await?;
            if created > 0 {
                if let Some(tx) = &self.event_tx {
                    let _ = tx
                        .send(crate::infrastructure::expand_address::event::ExpandEvent::HintScan)
                        .await;
                }
            }
        }

        Ok(())
    }
}
