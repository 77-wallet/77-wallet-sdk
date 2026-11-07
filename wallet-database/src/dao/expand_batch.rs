use sqlx::{Executor, Sqlite};

use crate::entities::{
    expand_batch::{BatchWithCount, CreateExpandBatchEntity, ExpandBatchEntity, ExpandBatchStatus},
    expand_batch_item::ExpandItemStatus,
};

pub struct ExpandBatchDao {}

impl ExpandBatchDao {
    /// Normal pending work plus an interrupted claim with no frozen item facts.
    pub async fn get_batches_for_planning<'a, E>(
        exec: E,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as("SELECT * FROM expand_batch b WHERE b.status = ? OR (b.status = ? AND NOT EXISTS (SELECT 1 FROM expand_batch_item i WHERE i.batch_id=b.batch_id))")
            .bind(ExpandBatchStatus::Pending).bind(ExpandBatchStatus::Running)
            .fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))
    }

    /// The write claim freezes the world before allocation. Nonempty batches
    /// already have frozen facts and must never be replanned.
    pub async fn claim_item_planning<'a, E>(exec: E, batch_id: &str) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let result = sqlx::query("UPDATE expand_batch SET status=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE batch_id=? AND status IN (?,?) AND NOT EXISTS (SELECT 1 FROM expand_batch_item i WHERE i.batch_id=expand_batch.batch_id) AND NOT EXISTS (SELECT 1 FROM address_query_state q WHERE q.uid=expand_batch.uid AND q.chain_code=expand_batch.chain_code AND q.status<>?)")
            .bind(ExpandBatchStatus::Running).bind(batch_id)
            .bind(ExpandBatchStatus::Pending).bind(ExpandBatchStatus::Running)
            .bind(crate::entities::address_query_state::AddressQueryStatus::Done)
            .execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result.rows_affected() > 0)
    }

    /// 创建新的扩容批次
    pub async fn create<'a, E>(exec: E, req: CreateExpandBatchEntity) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            INSERT INTO expand_batch 
            (uid, batch_id, serial_no, chain_code, total_count, finished_count, retry_count, status, created_at)
            VALUES (?, ?, ?, ?, ?, 0, 0, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT (batch_id) DO UPDATE SET 
                uid = excluded.uid,
                serial_no = excluded.serial_no,
                chain_code = excluded.chain_code,
                total_count = MAX(total_count, excluded.total_count),
                updated_at = excluded.updated_at
        "#;

        sqlx::query(sql)
            .bind(&req.uid)
            .bind(&req.batch_id)
            .bind(&req.serial_no)
            .bind(&req.chain_code)
            .bind(req.total_count)
            .bind(ExpandBatchStatus::Pending)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取批次信息
    pub async fn get_batch<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<Option<ExpandBatchEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM expand_batch WHERE batch_id = ?
        "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchEntity>(sql)
            .bind(batch_id)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 检查批次的expand操作是否已完成（基于事实驱动）
    ///
    /// ⚠️ 已废弃：此方法名称与实际语义不符
    /// 请使用 `is_batch_notified_fact` 方法替代
    ///
    /// 注意：这是事实驱动的完成判断，仅检查 `expand_complete_at` 字段
    /// 该字段是不可逆的事实，一旦设置就永远不会改变
    #[deprecated(note = "Use is_batch_notified_fact instead. This method name is misleading.")]
    pub async fn is_batch_expand_completed<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        Self::is_batch_notified_fact(exec, batch_id).await
    }

    /// 检查批次是否已通知后端完成（基于事实驱动）
    ///
    /// 事实驱动的判断：
    /// - 仅检查 `expand_complete_at` 字段
    /// - 该字段是不可逆的事实，一旦设置就永远不会改变
    /// - 表示批次已成功通知后端完成
    pub async fn is_batch_notified_fact<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT expand_complete_at IS NOT NULL AS is_notified FROM expand_batch WHERE batch_id = ?
        "#;

        let is_notified: Option<bool> = sqlx::query_scalar(sql)
            .bind(batch_id)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(is_notified.unwrap_or(false))
    }

    /// 获取item数量不匹配的running批次（仅用于修复、debug和离线校验）
    ///
    /// ⚠️ 重要警告：
    /// - 此方法仅用于**数据修复**、**debug**和**离线校验**
    /// - **严禁用于状态推进或业务决策**
    /// - 违反此规则将导致事实驱动架构失效
    pub async fn get_running_batches_item_count_mismatch_for_repair<'a, E>(
        exec: E,
        uid: &str,
        chain: &str,
    ) -> Result<Vec<BatchWithCount>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        //   SELECT b.*, COUNT(i.batch_id) as item_count
        // FROM expand_batch b
        // LEFT JOIN expand_batch_item i
        //   ON b.batch_id = i.batch_id
        // WHERE b.uid = ?
        //   AND b.chain_code = ?
        //   AND b.status IN (?, ?)
        // GROUP BY b.batch_id
        // HAVING item_count < b.total_count
        let sql = r#"
            SELECT
            b.batch_id,
            b.uid,
            b.chain_code,
            b.serial_no,
            b.total_count,
            b.finished_count,
            b.status,
            b.retry_count,
            b.created_at,
            b.updated_at,
            COALESCE(i.item_count, 0) AS item_count
        FROM expand_batch b
        LEFT JOIN (
            SELECT
                batch_id,
                COUNT(input_index) AS item_count
            FROM expand_batch_item
            GROUP BY batch_id
        ) i ON b.batch_id = i.batch_id
        WHERE b.uid = ?
        AND b.chain_code = ?
        AND b.status IN (?, ?)
        AND COALESCE(i.item_count, 0) < b.total_count
    "#;

        sqlx::query_as::<_, BatchWithCount>(sql)
            .bind(uid)
            .bind(chain)
            .bind(ExpandBatchStatus::Running)
            .bind(ExpandBatchStatus::Failed)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_status<'a, E>(
        exec: E,
        batch_id: &str,
        from: ExpandBatchStatus,
        to: ExpandBatchStatus,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE expand_batch
        SET status = ?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE batch_id = ?
          AND status = ?
    "#;

        let res = sqlx::query(sql)
            .bind(to)
            .bind(batch_id)
            .bind(from)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() > 0)
    }

    /// 重新计算finished_count缓存值
    ///
    /// ⚠️ 重要注意事项：
    /// - 此方法仅用于修复finished_count缓存值，不得用于业务决策或状态推进
    /// - finished_count是缓存值，不是事实，不参与任何业务判断
    /// - 唯一的完成事实是local_complete_at字段
    pub async fn recompute_finished_count_cache_only<'a, E>(
        exec: E,
        uid: &str,
        chain: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE expand_batch
            SET finished_count = (
                SELECT COUNT(*) FROM expand_batch_item i
                WHERE i.batch_id = expand_batch.batch_id
                  AND i.status = ?
            ),
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE uid = ?
              AND chain_code = ?
              AND status = ?
              AND finished_count < total_count
        "#;

        let res = sqlx::query(sql)
            .bind(ExpandItemStatus::Done)
            .bind(uid)
            .bind(chain)
            .bind(ExpandBatchStatus::Running)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected() > 0)
    }

    /// 检查批次的通知状态与事实是否一致
    ///
    /// 状态一致性检查：
    /// - 检查 status 是否为 Notified 且 expand_complete_at 事实存在
    /// - 用于验证状态与底层事实的一致性
    /// - 此方法**不是**事实判断，仅用于状态验证
    pub async fn is_batch_notified_state_consistent<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT status = ? AND expand_complete_at IS NOT NULL AS is_consistent 
            FROM expand_batch 
            WHERE batch_id = ?
        "#;

        let is_consistent: Option<bool> = sqlx::query_scalar(sql)
            .bind(ExpandBatchStatus::Notified)
            .bind(batch_id)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(is_consistent.unwrap_or(false))
    }

    pub async fn get_by_status<'a, E>(
        exec: E,
        status: ExpandBatchStatus,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT * FROM expand_batch
        WHERE status = ?
    "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchEntity>(sql)
            .bind(status)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取已完成但未通知后端的批次
    pub async fn get_done_but_not_notified<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM expand_batch 
            WHERE status = ? 
                AND uid = ?
                AND chain_code = ?
                AND expand_complete_at IS NULL
        "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchEntity>(sql)
            .bind(ExpandBatchStatus::Done)
            .bind(uid)
            .bind(chain_code)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取已完成但未通知后端的批次
    pub async fn get_all_done<'a, E>(exec: E) -> Result<Vec<ExpandBatchEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM expand_batch 
            WHERE status = ? 
        "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchEntity>(sql)
            .bind(ExpandBatchStatus::Done)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取所有本地完成但仍处于Running状态的批次
    ///
    /// 事实驱动的查询：
    /// - 基于local_complete_at事实字段
    /// - 仅返回local_complete_at已设置但status仍为Running的批次
    pub async fn get_all_finished_but_running<'a, E>(
        exec: E,
        uid: &str,
        chain: &str,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT * FROM expand_batch
        WHERE uid = ?
          AND chain_code = ?
          AND status = ?
          AND local_complete_at IS NOT NULL
    "#;

        sqlx::query_as::<_, ExpandBatchEntity>(sql)
            .bind(uid)
            .bind(chain)
            .bind(ExpandBatchStatus::Running)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 找出所有本地未完成的批次
    ///
    /// 事实驱动的查询：
    /// - 基于local_complete_at事实字段
    /// - 返回local_complete_at未设置的批次
    /// - 包含状态：Pending, Running
    /// - 不包含状态：Failed, Cancelled
    pub async fn get_unfinished_batches<'a, E>(
        exec: E,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT * FROM expand_batch
        WHERE local_complete_at IS NULL
         AND status IN (?, ?)
    "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchEntity>(sql)
            .bind(ExpandBatchStatus::Pending)
            .bind(ExpandBatchStatus::Running)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取所有批次
    pub async fn get_all<'a, E>(exec: E) -> Result<Vec<ExpandBatchEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM expand_batch";

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取需要进行item reconciliation的批次（事实驱动）
    ///
    /// 批次满足：
    /// - 状态为 Running
    ///
    /// 用于 Scanner 的 item reconciliation 逻辑
    ///
    /// IMPORTANT:
    /// - This method MUST NOT filter by `local_complete_at`.
    /// - Scanner is responsible for discovering incomplete facts,
    ///   not waiting for completion facts to appear.
    ///
    /// Any filtering based on completion facts will cause the system
    /// to stall permanently.
    pub async fn get_batches_for_item_reconcile<'a, E>(
        exec: E,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT * FROM expand_batch
        WHERE 
            status = ?
        "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchEntity>(sql)
            .bind(ExpandBatchStatus::Running)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取需要通知的批次（事实驱动）
    ///
    /// 批次满足：
    /// - 状态为 Done 但 expand_complete_at 未设置（需要推进到 Notified）
    ///
    /// 用于 Scanner 的通知逻辑
    pub async fn get_batches_for_notify<'a, E>(
        exec: E,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT * FROM expand_batch
        WHERE 
            status = ? AND expand_complete_at IS NULL
        "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchEntity>(sql)
            .bind(ExpandBatchStatus::Done)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 更新批次的finished_count缓存值
    ///
    /// ⚠️ 重要注意事项：
    /// - 此方法仅用于更新finished_count缓存值，不得用于业务决策或状态推进
    /// - finished_count是缓存值，不是事实，不参与任何业务判断
    /// - 唯一的完成事实是local_complete_at字段
    pub async fn update_finished_count_cache_only<'a, E>(
        exec: E,
        batch_id: &str,
        count: i64,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE expand_batch
        SET finished_count = ?, 
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE batch_id = ?
        "#;

        let res = sqlx::query(sql)
            .bind(count)
            .bind(batch_id)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() > 0)
    }

    /// 将批次状态从Pending转为Running，使用CAS确保只有一个实例能成功
    pub async fn mark_running_if_pending<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE expand_batch
        SET status = ?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE batch_id = ?
          AND status = ?
        "#;

        let res = sqlx::query(sql)
            .bind(ExpandBatchStatus::Running)
            .bind(batch_id)
            .bind(ExpandBatchStatus::Pending)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() > 0)
    }

    /// 更新expand_complete_at字段，仅当它为NULL时
    pub async fn update_expand_complete_at_if_null<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE expand_batch
        SET expand_complete_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE batch_id = ?
          AND expand_complete_at IS NULL
        "#;

        let res = sqlx::query(sql)
            .bind(batch_id)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() > 0)
    }

    /// 将批次状态从Done转为Notified，使用CAS确保只有一个实例能成功
    pub async fn done_to_notified_if_match<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE expand_batch
        SET status = ?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE batch_id = ?
          AND status = ?
          AND expand_complete_at IS NOT NULL
        "#;

        let res = sqlx::query(sql)
            .bind(ExpandBatchStatus::Notified)
            .bind(batch_id)
            .bind(ExpandBatchStatus::Done)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() > 0)
    }

    /// 当所有扩容项都已完成时，标记本地扩容完成
    ///
    /// 事实驱动的本地完成确认：
    /// - 仅当所有 items 都已完成（status = Done）时设置 local_complete_at
    /// - 使用CAS确保只有第一个调用者能成功写入
    /// - local_complete_at 是不可逆事实，一旦设置就永远不会改变
    /// - 返回影响行数，便于上层日志区分状态
    pub async fn mark_local_complete_if_all_items_done<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE expand_batch
        SET local_complete_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE batch_id = ?
          AND local_complete_at IS NULL
          AND (
            SELECT COUNT(*) = SUM(CASE WHEN status = ? THEN 1 ELSE 0 END)
            FROM expand_batch_item
            WHERE batch_id = expand_batch.batch_id
          )
        "#;

        let res = sqlx::query(sql)
            .bind(batch_id)
            .bind(ExpandItemStatus::Done)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    /// 基于本地完成事实推进批次状态：当local_complete_at已设置但状态仍为Running时，推进到Done
    ///
    /// 事实驱动的状态追平：
    /// - 仅当 local_complete_at IS NOT NULL 且 status = Running 时推进
    /// - 使用CAS确保并发安全
    /// - 返回影响行数，便于上层日志区分状态
    /// - 只能用于将状态从 Running → Done
    /// - 显式排除 Notified 状态，防止状态回退
    pub async fn mark_done_if_local_completed<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE expand_batch
        SET status = ?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE batch_id = ?
          AND status = ?
          AND local_complete_at IS NOT NULL
        "#;

        let res = sqlx::query(sql)
            .bind(ExpandBatchStatus::Done)
            .bind(batch_id)
            .bind(ExpandBatchStatus::Running)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }
}
