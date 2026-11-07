use crate::{
    ApiWalletDbPool,
    dao::expand_batch_item::{
        ExpandBatchItemDao, ExpandBatchItemFactGroup, ExpandBatchItemWithFactState,
    },
    entities::expand_batch_item::{
        CreateExpandBatchItemEntity, ExpandBatchItemEntity, ExpandItemStatus,
    },
};

pub struct ExpandBatchItemRepo;

impl ExpandBatchItemRepo {
    /// 批量创建扩容项
    pub async fn batch_create_items(
        pool: &ApiWalletDbPool,
        uid: &str,
        batch_id: &str,
        chain_code: &str,
        input_indices: &[i32],
    ) -> Result<(), crate::Error> {
        let items: Vec<CreateExpandBatchItemEntity> = input_indices
            .iter()
            .map(|&index| CreateExpandBatchItemEntity::new(batch_id, uid, chain_code, index))
            .collect();
        ExpandBatchItemDao::batch_create(pool.write_ref(), items).await
    }

    /// 获取所有未完成的 items（非 Done/Failed 状态）
    ///
    /// 事实驱动扫描条件：
    /// - 扫描所有 "未完成事实对齐" 的 item
    /// - 即：status != Done AND status != Failed
    /// - 原因：让所有非完成状态都能被扫描，避免因状态过滤导致某些满足事实的 item 永远不被处理
    pub async fn list_unfinished_items(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error> {
        ExpandBatchItemDao::fetch_by_batch_and_not_in_statuses(pool.read_ref(), batch_id).await
    }

    /// 获取带有事实状态的未完成 items
    /// 使用 LEFT JOIN 一次查询获取所有未完成 items 的事实状态
    /// 事实状态: 0=CREATE, 1=INIT, 2=DONE
    pub async fn get_items_with_fact_state(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<Vec<ExpandBatchItemWithFactState>, crate::Error> {
        ExpandBatchItemDao::get_items_with_fact_state(pool.read_ref(), batch_id).await
    }

    /// 获取按 fact_state 分组的未完成 items 索引列表
    /// 使用 LEFT JOIN 和 GROUP BY 一次查询获取所有未完成 items 并按 fact_state 分组
    /// 事实状态: 0=CREATE, 1=INIT, 2=DONE
    pub async fn get_items_grouped_by_fact_state(
        pool: &ApiWalletDbPool,
        batch_id: &str,
        init_dispatch_cooldown_sec: i64,
        max_init_per_round: i64,
        in_flight_indexes: &[i32], // 新增参数：排除正在执行的indexes
    ) -> Result<Vec<ExpandBatchItemFactGroup>, crate::Error> {
        ExpandBatchItemDao::get_items_grouped_by_fact_state(
            pool.read_ref(),
            batch_id,
            init_dispatch_cooldown_sec,
            max_init_per_round,
            in_flight_indexes,
        )
        .await
    }

    /// 批量更新初始化派发时间
    pub async fn update_last_init_dispatched_at(
        pool: &ApiWalletDbPool,
        batch_id: &str,
        input_indices: &[i32],
    ) -> Result<u64, crate::Error> {
        ExpandBatchItemDao::update_last_init_dispatched_at(
            pool.write_ref(),
            batch_id,
            input_indices,
        )
        .await
    }

    /// 确保 Init 已派发（或确认无需派发）
    /// 幂等操作，可重复调用
    pub async fn ensure_init_dispatched(
        pool: &ApiWalletDbPool,
        batch_id: &str,
        input_indices: &[i32],
    ) -> Result<u64, crate::Error> {
        ExpandBatchItemDao::mark_items_status_by_batch_from(
            pool.write_ref(),
            batch_id,
            input_indices,
            ExpandItemStatus::CreateDispatched,
            ExpandItemStatus::InitDispatched,
        )
        .await
    }

    /// 将状态从 InitDispatched 推进到 Done
    pub async fn init_dispatched_to_done_if_match(
        pool: &ApiWalletDbPool,
        batch_id: &str,
        input_indices: &[i32],
    ) -> Result<u64, crate::Error> {
        ExpandBatchItemDao::mark_items_status_by_batch_from(
            pool.write_ref(),
            batch_id,
            input_indices,
            ExpandItemStatus::InitDispatched,
            ExpandItemStatus::Done,
        )
        .await
    }

    /// 将状态从 Dispatched 推进到 Done（基于事实匹配）
    /// 幂等操作，可重复调用
    ///
    /// 事实驱动逻辑：
    /// - 只要 is_init=1，不管当前状态是什么，都应该推进到 Done
    /// - 不检查之前的状态，直接兑现事实
    pub async fn dispatched_to_done_if_fact_match(
        pool: &ApiWalletDbPool,
        batch_id: &str,
        input_indices: &[i32],
    ) -> Result<u64, crate::Error> {
        ExpandBatchItemDao::mark_items_done_by_fact(pool.write_ref(), batch_id, input_indices).await
    }

    /// 根据状态获取批次的所有扩容项
    pub async fn fetch_by_status(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        status: ExpandItemStatus,
        limit: i64,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error> {
        ExpandBatchItemDao::fetch_by_status(pool.read_ref(), uid, chain_code, status, limit).await
    }

    /// 统计 inflight 状态的扩容项数量
    ///
    /// ⚠️ Legacy API: DO NOT use in fact-driven scanner
    /// 这是旧语义 API，与事实驱动模型冲突
    #[deprecated(note = "Legacy API: DO NOT use in fact-driven scanner")]
    pub async fn count_inflight(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
    ) -> Result<i64, crate::Error> {
        ExpandBatchItemDao::count_inflight(pool.read_ref(), uid, chain_code).await
    }

    /// 根据索引列表获取批次的所有扩容项状态
    pub async fn list_status_by_indices(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        input_indices: &[i32],
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error> {
        ExpandBatchItemDao::list_status_by_indices(pool.read_ref(), uid, chain_code, input_indices)
            .await
    }

    /// 获取批次的所有扩容项
    pub async fn get_items_by_batch_id(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error> {
        ExpandBatchItemDao::get_items_by_batch_id(pool.read_ref(), batch_id).await
    }

    /// 检查某个批次的所有扩容项是否都已完成
    pub async fn is_batch_all_done(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<bool, crate::Error> {
        ExpandBatchItemDao::is_batch_all_done(pool.read_ref(), batch_id).await
    }

    /// 获取批次的完成进度
    pub async fn get_batch_progress(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<(i32, i32), crate::Error> {
        ExpandBatchItemDao::get_batch_progress(pool.read_ref(), batch_id).await
    }

    pub async fn find_batches_by_indices(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        indices: &[i32],
    ) -> Result<Vec<(String, i64)>, crate::Error> {
        ExpandBatchItemDao::find_batches_by_indices(pool.read_ref(), uid, chain_code, indices).await
    }

    /// 根据批次 ID 和状态获取扩容项
    pub async fn fetch_by_batch_and_status(
        pool: &ApiWalletDbPool,
        batch_id: &str,
        status: ExpandItemStatus,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error> {
        ExpandBatchItemDao::fetch_by_batch_and_status(pool.read_ref(), batch_id, status).await
    }

    /// 获取重试中的扩容项
    ///
    /// ⚠️ Legacy API: DO NOT use in fact-driven scanner
    /// 这是旧语义 API，与事实驱动模型冲突
    #[deprecated(note = "Legacy API: DO NOT use in fact-driven scanner")]
    pub async fn fetch_retryable(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        limit: i64,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error> {
        ExpandBatchItemDao::fetch_retryable(pool.read_ref(), uid, chain_code, limit).await
    }

    /// 批量更新扩容项状态
    ///
    /// ⚠️ Legacy API: DO NOT use in fact-driven scanner
    /// 这是旧语义 API，与事实驱动模型冲突
    #[deprecated(note = "Legacy API: DO NOT use in fact-driven scanner")]
    pub async fn mark_failed_and_inc_retry(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        input_indices: &[i32],
        phase: ExpandItemStatus,
    ) -> Result<u64, crate::Error> {
        ExpandBatchItemDao::mark_failed_and_inc_retry(
            pool.write_ref(),
            uid,
            chain_code,
            input_indices,
            phase,
        )
        .await
    }

    /// 将所有未完成的 item 重置为 CreateDispatched（用于 recover）
    ///
    /// 注意：不再重置为 Pending 状态，因为 Item 现在直接被创建为 CreateDispatched 状态
    ///
    /// ⚠️ Legacy API: DO NOT use in fact-driven scanner
    /// 这是旧语义 API，与事实驱动模型冲突
    #[deprecated(note = "Legacy API: DO NOT use in fact-driven scanner")]
    pub async fn reset_unfinished_to_create_dispatched(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
    ) -> Result<u64, crate::Error> {
        ExpandBatchItemDao::reset_unfinished_to_create_dispatched(pool.write_ref(), uid, chain_code)
            .await
    }

    /// 获取当前 uid + chain 下，所有已占用的 input_index
    ///
    /// 包括所有 batch / 所有状态：
    /// Pending / Creating / Initing / Failed / Done
    ///
    /// 用于 index 分配 & recover 逻辑
    pub async fn get_all_used_indices(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
    ) -> Result<Vec<i32>, crate::Error> {
        ExpandBatchItemDao::get_all_occupied_indices(pool.read_ref(), uid, chain_code).await
    }

    /// 统计批次下的扩容项数量
    pub async fn count_by_batch_id(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<i64, crate::Error> {
        ExpandBatchItemDao::count_by_batch_id(pool.read_ref(), batch_id).await
    }

    /// 统计所有扩容项数量
    pub async fn count_all(pool: &ApiWalletDbPool) -> Result<i64, crate::Error> {
        ExpandBatchItemDao::count_all(pool.read_ref()).await
    }

    /// 统计特定状态的扩容项数量
    pub async fn count_by_status(
        pool: &ApiWalletDbPool,
        status: ExpandItemStatus,
    ) -> Result<i64, crate::Error> {
        ExpandBatchItemDao::count_by_status(pool.read_ref(), status).await
    }

    /// 获取所有扩容项
    pub async fn get_all(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error> {
        ExpandBatchItemDao::get_all(pool.read_ref()).await
    }

    /// 获取需要扫描的items
    pub async fn get_items_for_scan(
        pool: &ApiWalletDbPool,
        batch_id: &str,
        limit: i64,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error> {
        ExpandBatchItemDao::get_items_for_scan(pool.read_ref(), batch_id, limit).await
    }

    /// 统计批次下的done状态item数量
    pub async fn count_done_items(
        pool: &ApiWalletDbPool,
        batch_id: &str,
    ) -> Result<i64, crate::Error> {
        ExpandBatchItemDao::count_done_items(pool.read_ref(), batch_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::ExpandBatchItemRepo;
    use crate::{
        dao::expand_batch_item::ExpandBatchItemDao,
        entities::expand_batch_item::ExpandItemStatus,
        repositories::{
            api_wallet::expand_batch::ExpandBatchRepo, test_helper::setup_api_wallet_pool,
        },
    };

    async fn seed_batch(pool: &crate::ApiWalletDbPool, batch_id: &str) {
        ExpandBatchRepo::create_batch(
            pool,
            "uid_item_test",
            batch_id,
            "serial_item_test",
            wallet_types::constant::chain_code::ETHEREUM,
            3,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn expand_batch_item_repo_batch_create_and_query_success() {
        let pool = setup_api_wallet_pool("wallet_db_expand_item_success").await;
        let batch_id = "batch_item_success_1";
        let uid = "uid_item_test";
        let chain = wallet_types::constant::chain_code::ETHEREUM;
        seed_batch(&pool, batch_id).await;

        ExpandBatchItemRepo::batch_create_items(&pool, uid, batch_id, chain, &[10, 11])
            .await
            .unwrap();

        let items = ExpandBatchItemRepo::get_items_by_batch_id(&pool, batch_id).await.unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|x| x.batch_id == batch_id && x.chain_code == chain));
        assert!(items.iter().any(|x| x.input_index == 10
            && x.uid == uid
            && x.status == ExpandItemStatus::CreateDispatched));
        assert!(items.iter().any(|x| x.input_index == 11
            && x.uid == uid
            && x.status == ExpandItemStatus::CreateDispatched));

        let statuses = ExpandBatchItemRepo::list_status_by_indices(&pool, uid, chain, &[10, 11])
            .await
            .unwrap();
        assert_eq!(statuses.len(), 2);
        assert!(statuses.iter().all(|x| x.status == ExpandItemStatus::CreateDispatched));

        let done_count = ExpandBatchItemRepo::count_done_items(&pool, batch_id).await.unwrap();
        assert_eq!(done_count, 0);
    }

    #[tokio::test]
    async fn expand_batch_item_repo_missing_batch_returns_empty() {
        let pool = setup_api_wallet_pool("wallet_db_expand_item_edge").await;
        let items =
            ExpandBatchItemRepo::get_items_by_batch_id(&pool, "batch_item_missing").await.unwrap();
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn expand_batch_item_repo_tx_rollback_keeps_status_unchanged() {
        let pool = setup_api_wallet_pool("wallet_db_expand_item_rollback").await;
        let batch_id = "batch_item_rollback_1";
        seed_batch(&pool, batch_id).await;
        let chain = wallet_types::constant::chain_code::ETHEREUM;

        ExpandBatchItemRepo::batch_create_items(&pool, "uid_item_test", batch_id, chain, &[20])
            .await
            .unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        ExpandBatchItemDao::mark_item_status_by_batch(
            tx.as_mut(),
            batch_id,
            20,
            ExpandItemStatus::Done,
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();

        let statuses =
            ExpandBatchItemRepo::list_status_by_indices(&pool, "uid_item_test", chain, &[20])
                .await
                .unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].input_index, 20);
        assert_eq!(statuses[0].status, ExpandItemStatus::CreateDispatched);

        let done_count = ExpandBatchItemRepo::count_done_items(&pool, batch_id).await.unwrap();
        assert_eq!(done_count, 0);
    }
}
