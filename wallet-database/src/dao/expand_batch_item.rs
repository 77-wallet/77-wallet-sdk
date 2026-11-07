use sqlx::{Executor, Sqlite};

use crate::{
    entities::expand_batch_item::{
        CreateExpandBatchItemEntity, ExpandBatchItemEntity, ExpandItemStatus,
    },
    sql_utils::{SqlExecutableReturn as _, query_builder::DynamicQueryBuilder},
};

const INSERT_CHUNK: usize = 150; // 每批 insert 行数
const IN_CHUNK: usize = 900; // 每批 IN 参数数量（< 999）

/// 带有事实状态的扩容项
#[derive(Debug, sqlx::FromRow)]
pub struct ExpandBatchItemWithFactState {
    #[sqlx(flatten)]
    pub item: ExpandBatchItemEntity,
    /// 事实状态: 0=CREATE, 1=INIT, 2=DONE
    pub fact_state: i32,
}

/// 按 fact_state 分组的扩容项索引列表
#[derive(Debug, sqlx::FromRow)]
pub struct ExpandBatchItemFactGroup {
    /// 事实状态: 0=CREATE, 1=INIT, 2=DONE
    pub fact_state: i32,
    /// JSON 格式的 input_index 列表
    pub indexes: String,
}

pub struct ExpandBatchItemDao {}

impl ExpandBatchItemDao {
    /// 批量创建扩容项
    pub async fn batch_create<'a, E>(
        exec: E,
        items: Vec<CreateExpandBatchItemEntity>,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + Copy,
    {
        if items.is_empty() {
            return Ok(());
        }

        for chunk in items.chunks(INSERT_CHUNK) {
            let mut qb = Self::insert_chunk(chunk);
            qb.build().execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;
        }

        Ok(())
    }

    pub(crate) async fn batch_create_in_transaction(
        conn: &mut sqlx::SqliteConnection,
        items: Vec<CreateExpandBatchItemEntity>,
    ) -> Result<(), crate::Error> {
        for chunk in items.chunks(INSERT_CHUNK) {
            Self::insert_chunk(chunk)
                .build()
                .execute(&mut *conn)
                .await
                .map_err(|e| crate::Error::Database(e.into()))?;
        }
        Ok(())
    }

    fn insert_chunk(chunk: &[CreateExpandBatchItemEntity]) -> sqlx::QueryBuilder<'_, Sqlite> {
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            "INSERT INTO expand_batch_item
         (batch_id, uid, chain_code, input_index, status, created_at, updated_at)",
        );

        qb.push_values(chunk, |mut b, item| {
            b.push_bind(&item.batch_id)
                .push_bind(&item.uid)
                .push_bind(&item.chain_code)
                .push_bind(item.input_index)
                .push_bind(ExpandItemStatus::CreateDispatched)
                .push("strftime('%Y-%m-%dT%H:%M:%SZ','now')")
                .push("strftime('%Y-%m-%dT%H:%M:%SZ','now')");
        });

        qb.push(" ON CONFLICT (batch_id, input_index) DO NOTHING");

        qb
    }

    /// 新增 list_status_by_indices
    pub async fn list_status_by_indices<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
        input_indices: &[i32],
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT * FROM expand_batch_item")
            .and_where_eq("uid", uid)
            .and_where_eq("chain_code", chain_code)
            .and_where_in("input_index", input_indices)
            .fetch_all(exec)
            .await
    }

    pub async fn mark_item_status_by_batch<'a, E>(
        exec: E,
        batch_id: &str,
        input_index: i32,
        status: ExpandItemStatus,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE expand_batch_item 
        SET 
            status = ?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE batch_id = ? AND input_index = ?
    "#;

        sqlx::query(sql)
            .bind(status)
            .bind(batch_id)
            .bind(input_index)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn mark_items_status_by_batch_from<'a, E>(
        exec: E,
        batch_id: &str,
        input_indices: &[i32],
        from: ExpandItemStatus,
        to: ExpandItemStatus,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + Copy,
    {
        if input_indices.is_empty() {
            return Ok(0);
        }

        let mut total = 0;

        for chunk in input_indices.chunks(IN_CHUNK) {
            let mut qb =
                sqlx::QueryBuilder::<Sqlite>::new("UPDATE expand_batch_item SET status = ");
            qb.push_bind(&to);
            qb.push(", updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') ");
            qb.push("WHERE batch_id = ");
            qb.push_bind(batch_id);
            qb.push(" AND status = ");
            qb.push_bind(&from);
            qb.push(" AND input_index IN (");

            let mut sep = qb.separated(", ");
            for i in chunk {
                sep.push_bind(*i);
            }
            qb.push(")");

            let res =
                qb.build().execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

            total += res.rows_affected();
        }

        Ok(total)
    }

    /// 批量更新扩容项状态，支持从多个状态更新到目标状态
    pub async fn mark_items_status_by_batch_from_multiple<'a, E>(
        exec: E,
        batch_id: &str,
        input_indices: &[i32],
        from_statuses: &[ExpandItemStatus],
        to: ExpandItemStatus,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + Copy,
    {
        if input_indices.is_empty() {
            return Ok(0);
        }

        if from_statuses.is_empty() {
            return Ok(0);
        }

        let mut total = 0;

        for chunk in input_indices.chunks(IN_CHUNK) {
            let mut qb =
                sqlx::QueryBuilder::<Sqlite>::new("UPDATE expand_batch_item SET status = ");
            qb.push_bind(&to);
            qb.push(", updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') ");
            qb.push("WHERE batch_id = ");
            qb.push_bind(batch_id);
            qb.push(" AND status IN (");

            let mut sep = qb.separated(", ");
            for status in from_statuses {
                sep.push_bind(status);
            }
            qb.push(") AND input_index IN (");

            let mut sep = qb.separated(", ");
            for i in chunk {
                sep.push_bind(*i);
            }
            qb.push(")");

            let res =
                qb.build().execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

            total += res.rows_affected();
        }

        Ok(total)
    }

    pub async fn mark_items_status_by_owner_from<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
        input_indices: &[i32],
        from: &[ExpandItemStatus],
        to: ExpandItemStatus,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + Copy,
    {
        if input_indices.is_empty() {
            return Ok(0);
        }

        let mut total = 0;

        for chunk in input_indices.chunks(IN_CHUNK) {
            let mut qb =
                sqlx::QueryBuilder::<Sqlite>::new("UPDATE expand_batch_item SET status = ");
            qb.push_bind(&to);
            qb.push(", updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') ");
            qb.push("WHERE uid = ");
            qb.push_bind(uid);
            qb.push(" AND chain_code = ");
            qb.push_bind(chain_code);

            if !from.is_empty() {
                qb.push(" AND status IN (");
                let mut sep = qb.separated(", ");
                for s in from {
                    sep.push_bind(s);
                }
                qb.push(")");
            }

            qb.push(" AND input_index IN (");
            let mut sep = qb.separated(", ");
            for i in chunk {
                sep.push_bind(*i);
            }
            qb.push(")");

            let res =
                qb.build().execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

            total += res.rows_affected();
        }

        Ok(total)
    }

    pub async fn fetch_by_status<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
        status: ExpandItemStatus,
        limit: i64,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT * FROM expand_batch_item
        WHERE uid = ? AND chain_code = ? AND status = ?
        ORDER BY batch_id, input_index
        LIMIT ?
    "#;

        sqlx::query_as(sql)
            .bind(uid)
            .bind(chain_code)
            .bind(status)
            .bind(limit)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn count_inflight<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
    ) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT COUNT(*) FROM expand_batch_item
        WHERE uid = ? AND chain_code = ?
          AND status IN (?, ?)
    "#;

        sqlx::query_scalar(sql)
            .bind(uid)
            .bind(chain_code)
            .bind(ExpandItemStatus::CreateDispatched)
            .bind(ExpandItemStatus::InitDispatched)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取批次的所有扩容项
    pub async fn get_items_by_batch_id<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM expand_batch_item 
            WHERE batch_id = ?
            ORDER BY input_index
        "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchItemEntity>(sql)
            .bind(batch_id)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 统计批次下的扩容项数量
    pub async fn count_by_batch_id<'a, E>(exec: E, batch_id: &str) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT COUNT(*) FROM expand_batch_item 
            WHERE batch_id = ?
        "#;

        sqlx::query_scalar::<_, i64>(sql)
            .bind(batch_id)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取当前 uid + chain 下，所有「曾被占用过」的 input_index
    ///
    /// ⚠️ 这是一个“永久占用集合”
    ///
    /// 一旦某个 index 出现在 expand_batch_item 中，
    /// 无论状态如何（Pending / Failed / Done），
    /// 都不得再次被分配。
    ///
    /// 用于：
    /// - index 分配
    /// - 扩容恢复（避免 index 重复）
    pub async fn get_all_occupied_indices<'a, E>(
        exec: E,
        uid: &str,
        chain: &str,
    ) -> Result<Vec<i32>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT DISTINCT input_index
            FROM expand_batch_item
            WHERE uid = ? AND chain_code = ?
            "#;

        let rows = sqlx::query_as::<_, (i32,)>(sql)
            .bind(uid)
            .bind(chain)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        let indices: Vec<i32> = rows.into_iter().map(|(i,)| i).collect();

        Ok(indices)
    }

    /// 检查某个批次的所有扩容项是否都已完成
    pub async fn is_batch_all_done<'a, E>(exec: E, batch_id: &str) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT 
                COUNT(*) > 0
                AND COUNT(*) = SUM(CASE WHEN status = ? THEN 1 ELSE 0 END)
            FROM expand_batch_item
            WHERE batch_id = ?
        "#;

        let is_done: Option<bool> = sqlx::query_scalar(sql)
            .bind(ExpandItemStatus::Done)
            .bind(batch_id)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(is_done.unwrap_or(false))
    }

    /// 获取批次的完成进度
    pub async fn get_batch_progress<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<(i32, i32), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT 
                COUNT(*) as total_count,
                SUM(CASE WHEN status = ? THEN 1 ELSE 0 END) as finished_count
            FROM expand_batch_item 
            WHERE batch_id = ?
        "#;

        let result: Option<(i32, i32)> = sqlx::query_as(sql)
            .bind(ExpandItemStatus::Done)
            .bind(batch_id)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(result.unwrap_or((0, 0)))
    }

    /// 根据链代码和输入索引查找受影响的批次，并返回每个批次命中的数量
    pub async fn find_batches_by_indices<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
        indices: &[i32],
    ) -> Result<Vec<(String, i64)>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + Copy,
    {
        if indices.is_empty() {
            return Ok(Vec::new());
        }

        use std::collections::HashMap;
        let mut acc: HashMap<String, i64> = HashMap::new();

        for chunk in indices.chunks(IN_CHUNK) {
            let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
                "SELECT batch_id, COUNT(*) as cnt
             FROM expand_batch_item
             WHERE uid = ",
            );

            qb.push_bind(uid);
            qb.push(" AND chain_code = ");
            qb.push_bind(chain_code);
            qb.push(" AND input_index IN (");

            let mut sep = qb.separated(", ");
            for idx in chunk {
                sep.push_bind(*idx);
            }
            qb.push(") GROUP BY batch_id");

            let rows = qb
                .build_query_as::<(String, i64)>()
                .fetch_all(exec)
                .await
                .map_err(|e| crate::Error::Database(e.into()))?;

            for (bid, cnt) in rows {
                *acc.entry(bid).or_insert(0) += cnt;
            }
        }

        Ok(acc.into_iter().collect())
    }

    pub async fn fetch_by_batch_and_status<'a, E>(
        exec: E,
        batch_id: &str,
        status: ExpandItemStatus,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT * FROM expand_batch_item
        WHERE batch_id = ? AND status = ?
        ORDER BY input_index
        "#;

        sqlx::query_as(sql)
            .bind(batch_id)
            .bind(status)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn fetch_retryable<'a, E>(
        exec: E,
        uid: &str,
        chain: &str,
        limit: i64,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
                SELECT * FROM expand_batch_item
        WHERE uid = ?
        AND chain_code = ?
        AND (
                    status = ?
                AND updated_at < strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-30 minutes')
        )
        ORDER BY batch_id, input_index
        LIMIT ?
    "#;

        let items = sqlx::query_as::<Sqlite, ExpandBatchItemEntity>(sql)
            .bind(uid)
            .bind(chain)
            .bind(ExpandItemStatus::Failed)
            .bind(limit)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(items)
    }

    pub async fn mark_failed_and_inc_retry<'a, E>(
        exec: E,
        uid: &str,
        chain: &str,
        indices: &[i32],
        phase: ExpandItemStatus,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + Copy,
    {
        if indices.is_empty() {
            return Ok(0);
        }

        let mut total = 0;

        for chunk in indices.chunks(IN_CHUNK) {
            let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
                "UPDATE expand_batch_item
             SET status = ?, retry_count = retry_count + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE uid = ? AND chain_code = ? AND status = ? AND input_index IN (",
            );

            qb.push_bind(ExpandItemStatus::Failed);
            qb.push_bind(uid);
            qb.push_bind(chain);
            qb.push_bind(&phase);

            let mut sep = qb.separated(", ");
            for i in chunk {
                sep.push_bind(*i);
            }
            qb.push(")");

            let res =
                qb.build().execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

            total += res.rows_affected();
        }

        Ok(total)
    }

    /// 将所有未完成的 item 重置为 CreateDispatched（用于 recover）
    ///
    /// 注意：不再重置为 Pending 状态，因为 Item 现在直接被创建为 CreateDispatched 状态
    pub async fn reset_unfinished_to_create_dispatched<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE expand_batch_item
        SET status = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE uid = ? AND chain_code = ?
          AND status != ?
    "#;

        let res = sqlx::query(sql)
            .bind(ExpandItemStatus::CreateDispatched)
            .bind(uid)
            .bind(chain_code)
            .bind(ExpandItemStatus::Done)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    /// 统计特定状态的扩容项数量
    pub async fn count_by_status<'a, E>(
        exec: E,
        status: ExpandItemStatus,
    ) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT COUNT(*) FROM expand_batch_item WHERE status = ?";

        let count = sqlx::query_scalar(sql)
            .bind(status)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(count)
    }

    /// 统计所有扩容项数量
    pub async fn count_all<'a, E>(exec: E) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT COUNT(*) FROM expand_batch_item";

        let count = sqlx::query_scalar(sql)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(count)
    }

    /// 获取所有扩容项
    pub async fn get_all<'a, E>(exec: E) -> Result<Vec<ExpandBatchItemEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM expand_batch_item";

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchItemEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取需要扫描的items
    pub async fn get_items_for_scan<'a, E>(
        exec: E,
        batch_id: &str,
        limit: i64,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT * FROM expand_batch_item
        WHERE batch_id = ?
          AND status IN (?, ?, ?)
        ORDER BY input_index
        LIMIT ?
        "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchItemEntity>(sql)
            .bind(batch_id)
            .bind(ExpandItemStatus::CreateDispatched)
            .bind(ExpandItemStatus::InitDispatched)
            .bind(ExpandItemStatus::Failed)
            .bind(limit)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 根据批次ID和多个状态获取扩容项
    pub async fn fetch_by_batch_and_statuses<'a, E>(
        exec: E,
        batch_id: &str,
        statuses: &[ExpandItemStatus],
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if statuses.is_empty() {
            return Ok(Vec::new());
        }

        let mut qb =
            sqlx::QueryBuilder::<Sqlite>::new("SELECT * FROM expand_batch_item WHERE batch_id = ");
        qb.push_bind(batch_id);
        qb.push(" AND status IN (");

        let mut sep = qb.separated(", ");
        for status in statuses {
            sep.push_bind(status);
        }
        qb.push(") ORDER BY input_index");

        qb.build_query_as().fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))
    }

    /// 根据批次ID获取所有非 Done/Failed 状态的扩容项
    pub async fn fetch_by_batch_and_not_in_statuses<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<Vec<ExpandBatchItemEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT * FROM expand_batch_item
        WHERE batch_id = ?
          AND status NOT IN (?, ?)
        ORDER BY input_index
        "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchItemEntity>(sql)
            .bind(batch_id)
            .bind(ExpandItemStatus::Done)
            .bind(ExpandItemStatus::Failed)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 统计批次下的done状态item数量
    pub async fn count_done_items<'a, E>(exec: E, batch_id: &str) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT COUNT(*) FROM expand_batch_item
        WHERE batch_id = ?
          AND status = ?
        "#;

        let count: Option<i64> = sqlx::query_scalar(sql)
            .bind(batch_id)
            .bind(ExpandItemStatus::Done)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(count.unwrap_or(0))
    }

    /// 获取带有事实状态的未完成 items
    /// 使用 LEFT JOIN 一次查询获取所有未完成 items 的事实状态
    /// 事实状态: 0=CREATE, 1=INIT, 2=DONE
    pub async fn get_items_with_fact_state<'a, E>(
        exec: E,
        batch_id: &str,
    ) -> Result<Vec<ExpandBatchItemWithFactState>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        SELECT 
            e.*,
            CASE 
                WHEN a.uid IS NULL THEN 0               -- CREATE
                WHEN a.is_init = 0 THEN 1                  -- INIT
                ELSE 2                                      -- DONE
            END AS fact_state
        FROM expand_batch_item e
        LEFT JOIN api_account a
            ON e.uid = a.uid
            AND e.chain_code = a.chain_code
            AND e.input_index = a.derivation_path_index
        WHERE e.batch_id = ?
          AND e.status NOT IN (?, ?)
        ORDER BY e.input_index ASC
        "#;

        sqlx::query_as::<sqlx::Sqlite, ExpandBatchItemWithFactState>(sql)
            .bind(batch_id)
            .bind(ExpandItemStatus::Done)
            .bind(ExpandItemStatus::Failed)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 获取按 fact_state 分组的未完成 items 索引列表
    /// 使用 LEFT JOIN 和 GROUP BY 一次查询获取所有未完成 items 并按 fact_state 分组
    /// 事实状态: 0=CREATE, 1=INIT, 2=DONE
    pub async fn get_items_grouped_by_fact_state<'a, E>(
        exec: E,
        batch_id: &str,
        init_dispatch_cooldown_sec: i64,
        max_init_per_round: i64,
        in_flight_indexes: &[i32], // 新增参数：排除正在执行的indexes
    ) -> Result<Vec<ExpandBatchItemFactGroup>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        // 使用QueryBuilder动态构建SQL，支持动态IN条件
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            "WITH fact AS (
            SELECT 
                e.input_index,
                CASE 
                    WHEN COUNT(a.uid) = 0 THEN 0        -- CREATE
                    WHEN MIN(a.is_init) = 0 THEN 1     -- INIT
                    ELSE 2                             -- DONE
                END AS fact_state,
                e.last_init_dispatched_at
            FROM expand_batch_item e
            LEFT JOIN api_account a
                ON e.uid = a.uid
                AND e.chain_code = a.chain_code
                AND e.input_index = a.derivation_path_index
            WHERE e.batch_id = ",
        );
        qb.push_bind(batch_id);
        qb.push(" AND e.status NOT IN (?, ?)");

        // 动态添加in_flight_indexes过滤条件
        if !in_flight_indexes.is_empty() {
            qb.push(" AND e.input_index NOT IN (");
            let mut sep = qb.separated(", ");
            for index in in_flight_indexes {
                sep.push_bind(index);
            }
            qb.push(")");
        }

        qb.push(
            r#"
            GROUP BY e.input_index, e.last_init_dispatched_at
        ),
        create_items AS (
            SELECT input_index, fact_state
            FROM fact
            WHERE fact_state = 0
            ORDER BY input_index ASC
        ),
        init_items AS (
            SELECT input_index, fact_state
            FROM fact
            WHERE fact_state = 1
              AND (last_init_dispatched_at IS NULL OR
                   unixepoch(last_init_dispatched_at) < unixepoch('now') - ?)
            ORDER BY last_init_dispatched_at ASC NULLS FIRST, input_index ASC
            LIMIT ?
        ),
        done_items AS (
            SELECT input_index, fact_state
            FROM fact
            WHERE fact_state = 2
            ORDER BY input_index ASC
        )
        SELECT fact_state,
               json_group_array(input_index) AS indexes
        FROM (
            SELECT * FROM create_items
            UNION ALL
            SELECT * FROM init_items
            UNION ALL
            SELECT * FROM done_items
        )
        GROUP BY fact_state
        ORDER BY fact_state
        "#,
        );

        let mut query = qb.build_query_as::<ExpandBatchItemFactGroup>();

        // 绑定剩余参数
        query = query.bind(ExpandItemStatus::Done);
        query = query.bind(ExpandItemStatus::Failed);
        query = query.bind(init_dispatch_cooldown_sec);
        query = query.bind(max_init_per_round);

        let rows = query.fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        Ok(rows)
    }

    /// 批量更新初始化派发时间
    pub async fn update_last_init_dispatched_at<'a, E>(
        exec: E,
        batch_id: &str,
        input_indices: &[i32],
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + Copy,
    {
        if input_indices.is_empty() {
            return Ok(0);
        }

        let mut total = 0;

        for chunk in input_indices.chunks(IN_CHUNK) {
            let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
                "UPDATE expand_batch_item
                 SET last_init_dispatched_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE batch_id = ",
            );

            qb.push_bind(batch_id);
            qb.push(" AND input_index IN (");

            let mut sep = qb.separated(", ");
            for i in chunk {
                sep.push_bind(*i);
            }
            qb.push(")");

            let res =
                qb.build().execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;
            total += res.rows_affected();
        }

        Ok(total)
    }

    /// 显式事实推进：将指定批次的扩容项标记为 Done，不检查之前的状态
    ///
    /// 仅用于 Scanner 在已通过 DB 事实确认完成后调用
    /// 语义："我不是在做状态流转，我是在兑现事实"
    pub async fn mark_items_done_by_fact<'a, E>(
        exec: E,
        batch_id: &str,
        input_indices: &[i32],
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + Copy,
    {
        if input_indices.is_empty() {
            return Ok(0);
        }

        let mut total = 0;

        for chunk in input_indices.chunks(IN_CHUNK) {
            let mut qb =
                sqlx::QueryBuilder::<Sqlite>::new("UPDATE expand_batch_item SET status = ");
            qb.push_bind(&ExpandItemStatus::Done);
            qb.push(", updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') ");
            qb.push("WHERE batch_id = ");
            qb.push_bind(batch_id);
            qb.push(" AND input_index IN (");

            let mut sep = qb.separated(", ");
            for i in chunk {
                sep.push_bind(*i);
            }
            qb.push(")");

            let res =
                qb.build().execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;
            total += res.rows_affected();
        }

        Ok(total)
    }
}
