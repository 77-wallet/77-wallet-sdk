use crate::{
    ApiWalletDbPool,
    dao::expand_batch::ExpandBatchDao,
    entities::expand_batch::{
        BatchWithCount, CreateExpandBatchEntity, ExpandBatchEntity, ExpandBatchStatus,
    },
};

pub struct ExpandBatchRepo;

impl ExpandBatchRepo {
    pub async fn get_batches_for_planning(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error> {
        ExpandBatchDao::get_batches_for_planning(pool.read_ref()).await
    }

    /// Own the claim and every insert chunk in one transaction. The calculator
    /// may only read the separate reader pool: no writes, backend or derivation.
    /// Holding the writer freezes reservations across concurrent planners.
    pub async fn plan_items<F, Fut, E>(
        pool: &ApiWalletDbPool,
        batch_id: &str,
        calculate: F,
    ) -> Result<usize, E>
    where
        E: From<crate::Error>,
        F: FnOnce(ExpandBatchEntity) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<i32>, E>>,
    {
        let _guard = pool.lock_write_with_metric("plan_expand_items").await;
        let mut tx =
            pool.write_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        if !ExpandBatchDao::claim_item_planning(tx.as_mut(), batch_id).await? {
            return Ok(0);
        }
        let batch = ExpandBatchDao::get_batch(tx.as_mut(), batch_id)
            .await?
            .ok_or_else(|| crate::Error::NotFound(batch_id.to_owned()))?;
        let uid = batch.uid.clone();
        let chain = batch.chain_code.clone();
        let count = batch.total_count;
        let indices = calculate(batch).await?;
        if count < 0 || indices.len() != count as usize {
            return Err(
                crate::Error::Other("expansion item count does not match batch".into()).into()
            );
        }
        let created = indices.len();
        let items = indices
            .into_iter()
            .map(|idx| {
                crate::entities::expand_batch_item::CreateExpandBatchItemEntity::new(
                    batch_id, &uid, &chain, idx,
                )
            })
            .collect();
        crate::dao::expand_batch_item::ExpandBatchItemDao::batch_create_in_transaction(
            tx.as_mut(),
            items,
        )
        .await?;
        tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(created)
    }

    /// 创建新的扩容批次
    pub async fn create_batch(
        pool: &ApiWalletDbPool,
        uid: &str,
        batch_id: &str,
        serial_no: &str,
        chain_code: &str,
        total_count: i32,
    ) -> Result<(), crate::Error> {
        let create_entity =
            CreateExpandBatchEntity::new(uid, batch_id, serial_no, chain_code, total_count);

        ExpandBatchDao::create(pool.write_ref(), create_entity).await
    }

    /// 获取批次信息
    pub async fn get_batch(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<Option<ExpandBatchEntity>, crate::Error> {
        ExpandBatchDao::get_batch(pool.read_ref(), batch_id).await
    }

    /// 获取运行中的批次中，item 数量不匹配的批次（仅用于修复、debug和离线校验）
    ///
    /// ⚠️ 重要警告：
    /// - 此方法仅用于**数据修复**、**debug**和**离线校验**
    /// - **严禁用于状态推进或业务决策**
    /// - 违反此规则将导致事实驱动架构失效
    pub async fn get_running_batches_item_count_mismatch_for_repair(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
    ) -> Result<Vec<BatchWithCount>, crate::Error> {
        ExpandBatchDao::get_running_batches_item_count_mismatch_for_repair(
            pool.read_ref(),
            uid,
            chain_code,
        )
        .await
    }

    /// 检查批次是否已通知后端完成（基于事实驱动）
    ///
    /// 事实驱动的判断：
    /// - 仅检查 `expand_complete_at` 字段
    /// - 该字段是不可逆的事实，一旦设置就永远不会改变
    /// - 表示批次已成功通知后端完成
    pub async fn is_batch_notified_fact(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<bool, crate::Error> {
        ExpandBatchDao::is_batch_notified_fact(pool.read_ref(), batch_id).await
    }

    /// 获取所有已完成但未通知后端的批次
    pub async fn get_all_finished_but_running(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error> {
        ExpandBatchDao::get_all_finished_but_running(pool.read_ref(), uid, chain_code).await
    }

    /// 获取批次的完成进度
    pub async fn get_batch_progress(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<Option<(i32, i32)>, crate::Error> {
        if let Some(batch) = Self::get_batch(pool, batch_id).await? {
            Ok(Some((batch.total_count, batch.finished_count)))
        } else {
            Ok(None)
        }
    }

    /// 将状态从 Done 推进到 Notified
    pub async fn done_to_notified_if_match(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<bool, crate::Error> {
        ExpandBatchDao::done_to_notified_if_match(pool.write_ref(), batch_id).await
    }

    /// 更新expand_complete_at字段，仅当它为NULL时
    pub async fn update_expand_complete_at_if_null(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<bool, crate::Error> {
        ExpandBatchDao::update_expand_complete_at_if_null(pool.write_ref(), batch_id).await
    }

    /// 更新批次的finished_count缓存值
    ///
    /// ⚠️ 重要注意事项：
    /// - 此方法仅用于更新finished_count缓存值，不得用于业务决策或状态推进
    /// - finished_count是缓存值，不是事实，不参与任何业务判断
    /// - 唯一的完成事实是local_complete_at字段
    pub async fn update_finished_count_cache_only(
        pool: &ApiWalletDbPool,
        batch_id: &str,
        count: i64,
    ) -> Result<bool, crate::Error> {
        ExpandBatchDao::update_finished_count_cache_only(pool.write_ref(), batch_id, count).await
    }

    /// 获取所有运行中的批次
    pub async fn get_all_running_batches(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error> {
        ExpandBatchDao::get_by_status(pool.read_ref(), ExpandBatchStatus::Running).await
    }

    /// 获取需要进行item reconciliation的批次（事实驱动）
    ///
    /// 批次满足：
    /// - 状态为 Running 但 local_complete_at 已设置（需要推进到 Done）
    ///
    /// 用于 Scanner 的 item reconciliation 逻辑
    pub async fn get_batches_for_item_reconcile(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error> {
        ExpandBatchDao::get_batches_for_item_reconcile(pool.read_ref()).await
    }

    /// 获取需要通知的批次（事实驱动）
    ///
    /// 批次满足：
    /// - 状态为 Done 但 expand_complete_at 未设置（需要推进到 Notified）
    ///
    /// 用于 Scanner 的通知逻辑
    pub async fn get_batches_for_notify(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error> {
        ExpandBatchDao::get_batches_for_notify(pool.read_ref()).await
    }

    /// 检查批次的通知状态与事实是否一致
    ///
    /// 状态一致性检查：
    /// - 检查 status 是否为 Notified 且 expand_complete_at 事实存在
    /// - 用于验证状态与底层事实的一致性
    /// - 此方法**不是**事实判断，仅用于状态验证
    pub async fn is_batch_notified_state_consistent(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<bool, crate::Error> {
        ExpandBatchDao::is_batch_notified_state_consistent(pool.read_ref(), batch_id).await
    }

    /// 获取已完成但未通知后端的批次
    pub async fn get_done_but_not_notified(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error> {
        ExpandBatchDao::get_done_but_not_notified(pool.read_ref(), uid, chain_code).await
    }

    /// 获取所有已完成但未通知后端的批次
    pub async fn get_all_done_but_not_notified(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error> {
        let sql = r#"
            SELECT * FROM expand_batch 
            WHERE status = ? 
                AND expand_complete_at IS NULL
        "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchEntity>(sql)
            .bind(ExpandBatchStatus::Done)
            .fetch_all(pool.read_ref())
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取所有已完成的批次
    pub async fn get_all_done(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error> {
        ExpandBatchDao::get_all_done(pool.read_ref()).await
    }

    /// 找出所有未完成的 batch（finished < total）
    pub async fn get_unfinished_batches(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error> {
        ExpandBatchDao::get_unfinished_batches(pool.read_ref()).await
    }

    /// 获取所有批次
    pub async fn get_all(pool: &ApiWalletDbPool) -> Result<Vec<ExpandBatchEntity>, crate::Error> {
        ExpandBatchDao::get_all(pool.read_ref()).await
    }

    /// 获取指定状态的批次
    pub async fn get_by_status(
        pool: &ApiWalletDbPool,
        status: ExpandBatchStatus,
    ) -> Result<Vec<ExpandBatchEntity>, crate::Error> {
        ExpandBatchDao::get_by_status(pool.read_ref(), status).await
    }

    /// 将批次状态从Pending转为Running，使用CAS确保只有一个实例能成功
    pub async fn mark_running_if_pending(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<bool, crate::Error> {
        ExpandBatchDao::mark_running_if_pending(pool.write_ref(), batch_id).await
    }

    /// 当所有扩容项都已完成时，标记本地扩容完成
    ///
    /// 事实驱动的本地完成确认：
    /// - 仅当所有 items 都已完成（status = Done）时设置 local_complete_at
    /// - 使用CAS确保只有第一个调用者能成功写入
    /// - local_complete_at 是不可逆事实，一旦设置就永远不会改变
    pub async fn mark_local_complete_if_all_items_done(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<u64, crate::Error> {
        ExpandBatchDao::mark_local_complete_if_all_items_done(pool.write_ref(), batch_id).await
    }

    /// 基于本地完成事实推进批次状态：当local_complete_at已设置但状态仍为Running时，推进到Done
    ///
    /// 事实驱动的状态追平：
    /// - 仅当 local_complete_at IS NOT NULL 且 status = Running 时推进
    /// - 使用CAS确保并发安全
    /// - 返回影响行数，便于上层日志区分状态
    pub async fn mark_done_if_local_completed(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<u64, crate::Error> {
        ExpandBatchDao::mark_done_if_local_completed(pool.write_ref(), batch_id).await
    }

    /// 检查批次的本地扩容是否已完成（基于事实驱动）
    ///
    /// 注意：这是事实驱动的完成判断，仅检查 `local_complete_at` 字段
    /// 该字段是不可逆的事实，一旦设置就永远不会改变
    pub async fn is_local_completed(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<bool, crate::Error> {
        let batch = match Self::get_batch(pool, batch_id).await? {
            Some(batch) => batch,
            None => return Ok(false),
        };
        Ok(batch.local_complete_at.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::ExpandBatchRepo;
    use crate::{
        dao::expand_batch::ExpandBatchDao, repositories::test_helper::setup_api_wallet_pool,
    };

    #[tokio::test]
    async fn expand_batch_repo_create_and_get_success() {
        let pool = setup_api_wallet_pool("wallet_db_expand_batch_success").await;
        let batch_id = "batch_success_1";
        let uid = "uid_expand_s";
        let serial_no = "serial_s";
        let chain_code = wallet_types::constant::chain_code::ETHEREUM;

        ExpandBatchRepo::create_batch(&pool, uid, batch_id, serial_no, chain_code, 5)
            .await
            .unwrap();

        let got = ExpandBatchRepo::get_batch(&pool, batch_id).await.unwrap();
        assert!(got.is_some());
        let got = got.unwrap();
        assert_eq!(got.uid, uid);
        assert_eq!(got.batch_id, batch_id);
        assert_eq!(got.serial_no, serial_no);
        assert_eq!(got.chain_code, chain_code);
        assert_eq!(got.total_count, 5);
        assert_eq!(got.finished_count, 0);
        assert!(got.local_complete_at.is_none());

        let completed = ExpandBatchRepo::is_local_completed(&pool, batch_id).await.unwrap();
        assert!(!completed);
    }

    #[tokio::test]
    async fn expand_batch_repo_missing_batch_returns_none() {
        let pool = setup_api_wallet_pool("wallet_db_expand_batch_edge").await;
        let got = ExpandBatchRepo::get_batch(&pool, "batch_missing").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn expand_batch_repo_tx_rollback_keeps_finished_count_unchanged() {
        let pool = setup_api_wallet_pool("wallet_db_expand_batch_rollback").await;
        let batch_id = "batch_rollback_1";
        ExpandBatchRepo::create_batch(
            &pool,
            "uid_expand_rb",
            batch_id,
            "serial_rb",
            wallet_types::constant::chain_code::ETHEREUM,
            3,
        )
        .await
        .unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        ExpandBatchDao::update_finished_count_cache_only(tx.as_mut(), batch_id, 2).await.unwrap();
        tx.rollback().await.unwrap();

        let got = ExpandBatchRepo::get_batch(&pool, batch_id).await.unwrap().unwrap();
        assert_eq!(got.total_count, 3);
        assert_eq!(got.finished_count, 0);
        assert!(got.local_complete_at.is_none());

        let completed = ExpandBatchRepo::is_local_completed(&pool, batch_id).await.unwrap();
        assert!(!completed);
    }
}
