use crate::{
    entities::{
        api_resource_gate::{
            ApiResourceBlockReason, ApiResourceDependencyType, ApiResourceGateResult,
        },
        api_trade_type::ApiTradeType,
        api_withdraw::{
            ApiWithdrawEntity, ApiWithdrawStatus, ErrCode, PendingWithdrawResourceBudget,
            SelfTransferReceipt, SignedSelfTransfer, WithdrawCreatedFact, WithdrawFailureStage,
            signed_evm_chain_codes_json,
        },
    },
    pagination::Pagination,
};
use chrono::{DateTime, Utc};
use sqlx::{Executor, QueryBuilder, Row, Sqlite};

// ⚠️ finished_at 为链终态事实字段
// ⚠️ 除 mark_chain_finished / mark_tx_res_ack_sent_and_chain_finished 外，禁止任何 UPDATE 语句写入 finished_at
// ⚠️ 未来 code review 时，搜索 `finished_at =` 并拒绝除上述方法外的所有情况

pub(crate) struct ApiWithdrawDao;

#[allow(dead_code)]
impl ApiWithdrawDao {
    /// A manual-review error is reportable, but its signed nonce remains fenced.
    pub async fn mark_uncertain_manual_review<'a, E>(
        exec: E,
        trade_no: &str,
        message: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"UPDATE api_withdraws SET err_code=$3,err_msg=$2,status=$4,failure_stage=$5,
            updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
            WHERE trade_no=$1 AND trade_type=1 AND err_code IS NULL AND finished_at IS NULL
              AND raw_tx IS NOT NULL AND broadcast_uncertain_since_at IS NOT NULL
              AND last_broadcast_at IS NULL AND transaction_time IS NULL
              AND chain_success_at IS NULL AND chain_failed_at IS NULL
              AND chain_code IN (SELECT value FROM json_each($6))"#;
        Ok(sqlx::query(sql)
            .bind(trade_no)
            .bind(message)
            .bind(ErrCode::TransactionOnChainException)
            .bind(ApiWithdrawStatus::SendingTxFailed)
            .bind(WithdrawFailureStage::Chain)
            .bind(signed_evm_chain_codes_json())
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?
            .rows_affected())
    }

    pub async fn mark_terminal_nonce_gap<'a, E>(
        exec: E,
        trade_no: &str,
        hash: &str,
        chain_next: i64,
        message: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"UPDATE api_withdraws SET err_code=6008,err_msg=$4,failure_stage=2,building_at=NULL,
            updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
            WHERE trade_no=$1 AND trade_type=1 AND tx_hash=$2 AND nonce>$3
              AND raw_tx IS NOT NULL AND err_code IS NULL AND finished_at IS NULL
              AND tx_exec_receipt_uploaded_at IS NULL AND transaction_time IS NULL
              AND chain_success_at IS NULL AND chain_failed_at IS NULL AND last_broadcast_at IS NULL
              AND broadcast_uncertain_since_at IS NULL AND broadcast_uncertain_retry_count=0
              AND broadcast_uncertain_rebroadcast_count=0 AND broadcast_uncertain_last_checked_at IS NULL
              AND broadcast_uncertain_reconciled_at IS NULL"#;
        Ok(sqlx::query(sql)
            .bind(trade_no)
            .bind(hash)
            .bind(chain_next)
            .bind(message)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?
            .rows_affected())
    }

    /// Insert ordinary signed facts without entering the backend withdrawal flow.
    /// The repository owns the transaction and validates any conflicting identity.
    pub async fn insert_signed_self_transfer<'a, E>(
        exec: E,
        input: &SignedSelfTransfer<'_>,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query(
            r#"INSERT INTO api_withdraws
            (trade_no, uid, name, from_addr, to_addr, chain_code, token_addr,
             symbol, value, nonce, tx_hash, raw_tx, transaction_fee, notes,
             validate, trade_type, init_status, status, building_at,
             broadcast_uncertain_since_at, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '', 4, 0, 0,
                strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            ON CONFLICT(trade_no) DO NOTHING"#,
        )
        .bind(input.trade_no)
        .bind(input.uid)
        .bind(input.name)
        .bind(input.from_addr)
        .bind(input.to_addr)
        .bind(input.chain_code)
        .bind(&input.token_addr)
        .bind(input.symbol)
        .bind(input.value)
        .bind(input.nonce)
        .bind(input.tx_hash)
        .bind(input.raw_tx)
        .bind(input.transaction_fee)
        .bind(input.notes)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

    pub async fn mark_self_transfer_accepted<'a, E>(
        exec: E,
        trade_no: &str,
        tx_hash: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let rows = sqlx::query(
            r#"UPDATE api_withdraws SET
            last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
            broadcast_uncertain_since_at = NULL,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
            WHERE trade_no = ? AND tx_hash = ? AND trade_type = 4
              AND chain_code IN (SELECT value FROM json_each(?)) AND raw_tx IS NOT NULL
              AND (last_broadcast_at IS NULL OR broadcast_uncertain_since_at IS NOT NULL)
              AND transaction_time IS NULL AND finished_at IS NULL AND err_code IS NULL
              AND chain_success_at IS NULL AND chain_failed_at IS NULL"#,
        )
        .bind(trade_no)
        .bind(tx_hash)
        .bind(signed_evm_chain_codes_json())
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(rows.rows_affected() == 1)
    }

    /// Persist the retry budget before any network send. A cancelled attempt is
    /// not refunded, so process restart cannot create a broadcast retry storm.
    pub async fn claim_self_transfer_rebroadcast<'a, E>(
        exec: E,
        trade_no: &str,
        tx_hash: &str,
        raw_tx: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let rows = sqlx::query(r#"UPDATE api_withdraws SET
            broadcast_uncertain_rebroadcast_count = broadcast_uncertain_rebroadcast_count + 1,
            broadcast_uncertain_since_at = COALESCE(broadcast_uncertain_since_at, strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            broadcast_uncertain_last_checked_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
            WHERE trade_no = ? AND tx_hash = ? AND raw_tx = ?
              AND raw_tx != '' AND tx_hash != '' AND trade_type = 4 AND chain_code IN (SELECT value FROM json_each(?))
              AND transaction_time IS NULL AND finished_at IS NULL AND err_code IS NULL
              AND chain_success_at IS NULL AND chain_failed_at IS NULL
              AND broadcast_uncertain_rebroadcast_count < 3
              AND julianday(COALESCE(broadcast_uncertain_last_checked_at,
                  broadcast_uncertain_since_at, last_broadcast_at, created_at)) <= julianday('now', '-60 seconds')"#)
            .bind(trade_no).bind(tx_hash).bind(raw_tx)
        .bind(signed_evm_chain_codes_json()).execute(exec).await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(rows.rows_affected() == 1)
    }

    pub async fn pending_self_transfers_for_sender<'a, E>(
        exec: E,
        from_addr: &str,
        chain_code: &str,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as(
            r#"SELECT * FROM api_withdraws WHERE from_addr = ? AND chain_code = ?
            AND trade_type = 4 AND chain_code IN (SELECT value FROM json_each(?)) AND raw_tx IS NOT NULL
            AND transaction_time IS NULL AND finished_at IS NULL AND err_code IS NULL
            AND chain_success_at IS NULL AND chain_failed_at IS NULL
            ORDER BY nonce, id"#,
        )
        .bind(from_addr)
        .bind(chain_code)
        .bind(signed_evm_chain_codes_json())
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    /// Stable cursor over pending signed ordinary transfers; finalizing earlier
    /// rows must not shift later rows out of the next page.
    pub async fn scan_pending_self_transfers<'a, E>(
        exec: E,
        after_id: i64,
        limit: i64,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as(
            r#"SELECT * FROM api_withdraws WHERE id > ?
            AND trade_type = 4 AND chain_code IN (SELECT value FROM json_each(?))
            AND raw_tx IS NOT NULL AND raw_tx != ''
            AND tx_hash IS NOT NULL AND tx_hash != ''
            AND transaction_time IS NULL AND finished_at IS NULL AND err_code IS NULL
            AND chain_success_at IS NULL AND chain_failed_at IS NULL
            ORDER BY id LIMIT ?"#,
        )
        .bind(after_id)
        .bind(signed_evm_chain_codes_json())
        .bind(limit)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    /// Record a receipt only for the same signed SelfWithdraw. The repository
    /// completes the terminal fact in the same transaction via mark_chain_finished.
    pub async fn confirm_self_transfer_receipt<'a, E>(
        exec: E,
        receipt: &SelfTransferReceipt<'_>,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let status =
            if receipt.success { ApiWithdrawStatus::Success } else { ApiWithdrawStatus::Failure };
        let rows = sqlx::query(
            r#"UPDATE api_withdraws SET
            status = ?, transaction_time = ?, block_height = ?, receipt_refresh_pending = 1,
            transaction_fee = ?, resource_consume = ?,
            chain_success_at = CASE WHEN ? THEN strftime('%Y-%m-%dT%H:%M:%SZ','now') ELSE NULL END,
            chain_failed_at = CASE WHEN ? THEN NULL ELSE strftime('%Y-%m-%dT%H:%M:%SZ','now') END,
            broadcast_uncertain_since_at = NULL,
            broadcast_uncertain_retry_count = 0,
            broadcast_uncertain_last_checked_at = NULL,
            broadcast_uncertain_reconciled_at = NULL,
            broadcast_uncertain_rebroadcast_count = 0,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
            WHERE trade_no = ? AND tx_hash = ? AND chain_code = ?
              AND trade_type = 4 AND chain_code IN (SELECT value FROM json_each(?))
              AND raw_tx IS NOT NULL AND raw_tx != ''
              AND transaction_time IS NULL AND finished_at IS NULL AND err_code IS NULL
              AND chain_success_at IS NULL AND chain_failed_at IS NULL"#,
        )
        .bind(status)
        .bind(receipt.transaction_time)
        .bind(receipt.block_height)
        .bind(receipt.transaction_fee)
        .bind(receipt.resource_consume)
        .bind(receipt.success)
        .bind(receipt.success)
        .bind(receipt.trade_no)
        .bind(receipt.tx_hash)
        .bind(receipt.chain_code)
        .bind(signed_evm_chain_codes_json())
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(rows.rows_affected())
    }

    pub async fn all_api_withdraw<'a, E>(
        exec: E,
        uid: &str,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"SELECT * FROM api_withdraws WHERE uid = ? AND trade_type = ?"#;
        let result = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(uid)
            .bind(ApiTradeType::Withdraw)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    pub async fn list_api_withdraw_with_status<'a, E>(
        exec: E,
        vec_status: Vec<ApiWithdrawStatus>,
        page: i64,
        page_size: i64,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut qb = QueryBuilder::<Sqlite>::new("SELECT * FROM api_withdraws WHERE trade_type = ");
        qb.push_bind(ApiTradeType::Withdraw as u8);
        if !vec_status.is_empty() {
            qb.push(" AND status IN (");
            let mut separated = qb.separated(", "); // 自动在元素间加逗号
            for status in &vec_status {
                separated.push_bind(status);
            }
            qb.push(")");
        }

        qb.push(" ORDER BY updated_at ASC, created_at ASC");
        qb.push(" LIMIT ").push_bind(page_size);
        qb.push(" OFFSET ").push_bind(page * page_size);
        let query = qb.build_query_as::<ApiWithdrawEntity>();
        let rows = query.fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(rows)
    }

    pub async fn page_api_withdraw<'a, E>(
        pool: &E,
        uid: &str,
        vec_status: Vec<ApiWithdrawStatus>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiWithdrawEntity>, crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        let mut count_qb =
            QueryBuilder::<Sqlite>::new("SELECT count(*) FROM api_withdraws WHERE trade_type = ");
        count_qb.push_bind(ApiTradeType::Withdraw as u8);
        let mut qb = QueryBuilder::<Sqlite>::new("SELECT * FROM api_withdraws WHERE trade_type = ");
        qb.push_bind(ApiTradeType::Withdraw as u8);
        if !uid.is_empty() {
            count_qb.push(" AND uid = ").push_bind(uid);
            qb.push(" AND uid = ").push_bind(uid);
        }
        if !vec_status.is_empty() {
            count_qb.push(" AND status IN (");
            qb.push(" AND status IN (");
            let mut count_separated = count_qb.separated(", ");
            let mut separated = qb.separated(", "); // 自动在元素间加逗号
            for status in &vec_status {
                count_separated.push_bind(status);
                separated.push_bind(status);
            }
            count_qb.push(")");
            qb.push(")");
        }
        let count_query = count_qb.build_query_scalar();
        let total_count =
            count_query.fetch_one(pool).await.map_err(|e| crate::Error::Database(e.into()))?;

        qb.push(" ORDER BY updated_at DESC, created_at DESC");
        qb.push(" LIMIT ").push_bind(page_size);
        qb.push(" OFFSET ").push_bind(page * page_size);
        let query = qb.build_query_as::<ApiWithdrawEntity>();
        let rows = query.fetch_all(pool).await.map_err(|e| crate::Error::Database(e.into()))?;

        let mut paginate = Pagination::<ApiWithdrawEntity>::init(page, page_size);
        paginate.data = rows;
        paginate.total_count = total_count;
        Ok(paginate)
    }

    pub async fn page_api_withdraw_with_status<'a, E>(
        exec: E,
        _page: i64,
        page_size: i64,
        vec_status: &[ApiWithdrawStatus],
    ) -> Result<(i64, Vec<ApiWithdrawEntity>), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + Clone,
    {
        let placeholders = vec_status.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let count_sql = format!(
            "SELECT count(*) FROM api_withdraws where status in ({})
",
            placeholders
        );
        let sql = format!(
            "SELECT * FROM api_withdraws where status in ({}) ORDER BY id ASC LIMIT ?",
            placeholders
        );

        let mut query = sqlx::query_scalar::<_, i64>(&count_sql);
        for status in vec_status {
            query = query.bind(status);
        }
        let count =
            query.fetch_one(exec.clone()).await.map_err(|e| crate::Error::Database(e.into()))?;

        let mut query = sqlx::query_as::<_, ApiWithdrawEntity>(&sql);
        for status in vec_status {
            query = query.bind(status);
        }
        let res = query
            .bind(page_size)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok((count, res))
    }

    pub async fn page_api_withdraw_with_init_status<'a, E>(
        pool: &E,
        uid: &str,
        init_status: ApiWithdrawStatus,
        vec_status: Vec<ApiWithdrawStatus>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiWithdrawEntity>, crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        let mut count_qb =
            QueryBuilder::<Sqlite>::new("SELECT count(*) FROM api_withdraws WHERE trade_type = ");
        let mut qb = QueryBuilder::<Sqlite>::new("SELECT * FROM api_withdraws WHERE trade_type = ");
        count_qb.push_bind(ApiTradeType::Withdraw);
        count_qb.push(" AND init_status = ").push_bind(init_status);
        qb.push_bind(ApiTradeType::Withdraw);
        qb.push(" AND init_status = ").push_bind(init_status);
        if !uid.is_empty() {
            count_qb.push(" AND uid = ").push_bind(uid);
            qb.push(" AND uid = ").push_bind(uid);
        }
        if !vec_status.is_empty() {
            if vec_status.len() == 1 {
                count_qb.push(" AND status = ").push_bind(&vec_status[0]);
                qb.push(" AND status =  ").push_bind(&vec_status[0]);
            } else {
                count_qb.push(" AND status IN (");
                qb.push(" AND status IN (");
                let mut count_separated = count_qb.separated(", ");
                let mut separated = qb.separated(", "); // 自动在元素间加逗号
                for status in &vec_status {
                    count_separated.push_bind(status);
                    separated.push_bind(status);
                }
                count_qb.push(")");
                qb.push(")");
            }
        }

        let count_query = count_qb.build_query_scalar();
        let total_count =
            count_query.fetch_one(pool).await.map_err(|e| crate::Error::Database(e.into()))?;

        qb.push(" ORDER BY updated_at DESC, created_at DESC");
        qb.push(" LIMIT ").push_bind(page_size);
        qb.push(" OFFSET ").push_bind(page * page_size);
        let query = qb.build_query_as::<ApiWithdrawEntity>();
        let rows = query.fetch_all(pool).await.map_err(|e| crate::Error::Database(e.into()))?;

        let mut paginate = Pagination::<ApiWithdrawEntity>::init(page, page_size);
        paginate.total_count = total_count;
        paginate.data = rows;
        Ok(paginate)
    }

    pub async fn get_api_withdraw_by_id<'a, E>(
        exec: E,
        id: &str,
    ) -> Result<ApiWithdrawEntity, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM api_withdraws WHERE id = ?";
        let res = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(id)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res)
    }

    pub async fn get_api_withdraw_by_trade_no<'a, E>(
        exec: E,
        trade_no: &str,
        trade_type: ApiTradeType,
    ) -> Result<ApiWithdrawEntity, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM api_withdraws WHERE trade_no = ? AND trade_type = ?";
        let res = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(trade_no)
            .bind(trade_type)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res)
    }

    pub async fn find_api_withdraw_by_trade_no<'a, E>(
        exec: E,
        trade_no: &str,
        trade_type: ApiTradeType,
    ) -> Result<Option<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM api_withdraws WHERE trade_no = ? AND trade_type = ?";
        sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(trade_no)
            .bind(trade_type)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn find_by_resource_dependency_trade_no<'a, E>(
        exec: E,
        dependency_trade_no: &str,
    ) -> Result<Option<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM api_withdraws WHERE resource_dependency_trade_no = ? AND trade_type = ? ORDER BY id ASC LIMIT 1";
        sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(dependency_trade_no)
            .bind(ApiTradeType::Withdraw)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_api_withdraw_by_trade_no_status<'a, E>(
        exec: E,
        trade_no: &str,
        vec_status: &[ApiWithdrawStatus],
    ) -> Result<ApiWithdrawEntity, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let placeholders = vec_status.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT * FROM api_withdraws where trade_no = ? AND trade_type = ? AND status in ({})",
            placeholders
        );
        let mut query = sqlx::query_as::<_, ApiWithdrawEntity>(&sql)
            .bind(trade_no)
            .bind(ApiTradeType::Withdraw);
        for status in vec_status {
            query = query.bind(status);
        }
        let res = query.fetch_one(exec).await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res)
    }

    pub async fn mark_resource_blocked<'a, E>(
        exec: E,
        trade_no: &str,
        block_reason: ApiResourceBlockReason,
        dependency_trade_no: Option<&str>,
        dependency_type: Option<ApiResourceDependencyType>,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET resource_check_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                resource_gate_released_at = NULL,
                resource_gate_result = NULL,
                resource_block_reason = ?2,
                resource_dependency_trade_no = ?3,
                resource_dependency_type = ?4,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?1
        "#;

        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(block_reason.as_i64())
            .bind(dependency_trade_no)
            .bind(dependency_type.map(ApiResourceDependencyType::as_i64))
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Freeze the platform-delegation decision and its exact request amount
    /// before the idempotent backend call is attempted.
    pub async fn mark_platform_delegation_required<'a, E>(
        exec: E,
        trade_no: &str,
        estimated_resource_consume: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET resource_check_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                resource_gate_released_at = NULL,
                resource_gate_result = NULL,
                resource_block_reason = ?2,
                resource_dependency_trade_no = NULL,
                resource_dependency_type = ?3,
                estimated_resource_consume = ?4,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?1
              AND resource_gate_released_at IS NULL
              AND resource_gate_result IS NULL
              AND resource_block_reason IS NULL
              AND raw_tx IS NULL
              AND transaction_time IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
        "#;

        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(ApiResourceBlockReason::NeedPlatformDelegate.as_i64())
            .bind(ApiResourceDependencyType::PlatformDelegate.as_i64())
            .bind(estimated_resource_consume)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn update_platform_delegation_dependency_trade_no<'a, E>(
        exec: E,
        trade_no: &str,
        dependency_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET resource_dependency_trade_no = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?1
              AND resource_gate_released_at IS NULL
              AND resource_gate_result IS NULL
              AND resource_block_reason = ?3
              AND resource_dependency_type = ?4
              AND resource_dependency_trade_no IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(dependency_trade_no)
            .bind(ApiResourceBlockReason::NeedPlatformDelegate.as_i64())
            .bind(ApiResourceDependencyType::PlatformDelegate.as_i64())
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_resource_released<'a, E>(
        exec: E,
        trade_no: &str,
        gate_result: ApiResourceGateResult,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET resource_check_at = COALESCE(resource_check_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                resource_gate_released_at = COALESCE(resource_gate_released_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                resource_gate_result = ?2,
                resource_block_reason = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?1
        "#;

        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(gate_result.as_i64())
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_resource_delegation_confirmed<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET resource_check_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                resource_gate_released_at = NULL,
                resource_gate_result = ?2,
                resource_block_reason = NULL,
                resource_dependency_trade_no = NULL,
                resource_dependency_type = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?1
              AND resource_gate_released_at IS NULL
        "#;

        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(ApiResourceGateResult::ResourceDelegationSuccess.as_i64())
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Persist the platform-delegation result without releasing the withdraw
    /// resource gate. The result ACK is a separate fact and remains the hard
    /// prerequisite for gate release.
    pub async fn mark_platform_delegation_result_received<'a, E>(
        exec: E,
        trade_no: &str,
        gate_result: ApiResourceGateResult,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET resource_check_at = COALESCE(resource_check_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                resource_gate_result = COALESCE(resource_gate_result, ?2),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?1
              AND resource_gate_released_at IS NULL
              AND resource_gate_result IS NULL
              AND resource_block_reason = ?3
              AND resource_dependency_type = ?4
        "#;

        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(gate_result.as_i64())
            .bind(ApiResourceBlockReason::NeedPlatformDelegate.as_i64())
            .bind(ApiResourceDependencyType::PlatformDelegate.as_i64())
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Scan withdraws whose resource decision is already frozen at platform
    /// delegation but whose result fact has not arrived yet. Re-running the
    /// idempotent apply call fills this fact gap; it must not re-enter resource
    /// evaluation.
    pub async fn scan_need_platform_delegation_apply<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM api_withdraws
            WHERE trade_type = ?1
              AND lower(chain_code) = 'tron'
              AND resource_gate_released_at IS NULL
              AND resource_gate_result IS NULL
              AND resource_block_reason = ?2
              AND resource_dependency_type = ?3
              AND resource_dependency_trade_no IS NULL
              AND raw_tx IS NULL
              AND transaction_time IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
            ORDER BY created_at ASC
            LIMIT ?4
        "#;
        sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiTradeType::Withdraw)
            .bind(ApiResourceBlockReason::NeedPlatformDelegate.as_i64())
            .bind(ApiResourceDependencyType::PlatformDelegate.as_i64())
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_need_resource_gate<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM api_withdraws
            WHERE tx_ack_sent_at IS NOT NULL
              AND audit_passed_at IS NOT NULL
              AND lower(chain_code) = 'tron'
              AND resource_gate_released_at IS NULL
              AND (
                  resource_block_reason IS NULL
                  OR (
                      resource_block_reason = ?
                      AND resource_dependency_trade_no IS NULL
                  )
              )
              AND raw_tx IS NULL
              AND transaction_time IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
              AND trade_type = ?
            ORDER BY created_at ASC
            LIMIT ?
        "#;
        let result = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiResourceBlockReason::NeedLocalDelegate.as_i64())
            .bind(ApiTradeType::Withdraw)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    pub async fn scan_need_fee_estimate<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM api_withdraws
            WHERE trade_type = ?
              AND fee_estimated_at IS NULL
              AND raw_tx IS NULL
              AND transaction_time IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
            ORDER BY created_at ASC
            LIMIT ?
        "#;
        let result = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiTradeType::Withdraw)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    pub async fn scan_withdraw_resource_recovery<'a, E>(
        exec: E,
        after_id: Option<i64>,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM api_withdraws
            WHERE trade_type = ?
              AND lower(chain_code) = 'tron'
              AND status IN (?, ?)
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND transaction_time IS NOT NULL
              AND (
                    CASE
                        WHEN json_valid(resource_consume)
                            THEN json_type(resource_consume) <> 'object'
                        ELSE 1
                    END
                    OR (
                        token_addr IS NOT NULL
                        AND trim(token_addr) <> ''
                        AND json_valid(resource_consume)
                        AND COALESCE(
                            json_extract(resource_consume, '$.bandwidth'),
                            json_extract(resource_consume, '$.net_used'),
                            0
                        ) = 0
                        AND COALESCE(
                            json_extract(resource_consume, '$.energy'),
                            json_extract(resource_consume, '$.energy_used'),
                            0
                        ) = 0
                    )
              )
              AND (? IS NULL OR id > ?)
            ORDER BY id ASC
            LIMIT ?
        "#;
        sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiTradeType::Withdraw)
            .bind(ApiWithdrawStatus::Success)
            .bind(ApiWithdrawStatus::ConfirmSuccessReport)
            .bind(after_id)
            .bind(after_id)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_resource_consume_if_unchanged<'a, E>(
        exec: E,
        trade_no: &str,
        observed: &str,
        replacement: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let result = sqlx::query(
            "UPDATE api_withdraws SET resource_consume = ? WHERE trade_no = ? AND resource_consume = ?",
        )
        .bind(replacement)
        .bind(trade_no)
        .bind(observed)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result.rows_affected())
    }

    pub async fn pending_resource_budget_by_owner<'a, E>(
        exec: E,
        chain_code: &str,
        from_addr: &str,
        exclude_trade_no: &str,
    ) -> Result<PendingWithdrawResourceBudget, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT
                COALESCE(SUM(
                    CASE
                        WHEN json_valid(estimated_resource_consume)
                        THEN CAST(COALESCE(
                            json_extract(estimated_resource_consume, '$.energy'),
                            json_extract(estimated_resource_consume, '$.energy_used'),
                            json_extract(estimated_resource_consume, '$.energyUsed'),
                            0
                        ) AS INTEGER)
                        ELSE 0
                    END
                ), 0) AS energy,
                COALESCE(SUM(
                    CASE
                        WHEN json_valid(estimated_resource_consume)
                        THEN CAST(COALESCE(
                            json_extract(estimated_resource_consume, '$.bandwidth'),
                            json_extract(estimated_resource_consume, '$.net_used'),
                            json_extract(estimated_resource_consume, '$.netUsed'),
                            0
                        ) AS INTEGER)
                        ELSE 0
                    END
                ), 0) AS bandwidth
            FROM api_withdraws
            WHERE trade_type = ?
              AND lower(chain_code) = lower(?)
              AND from_addr = ?
              AND trade_no <> ?
              AND estimated_resource_consume IS NOT NULL
              AND transaction_time IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
        "#;

        let row = sqlx::query(sql)
            .bind(ApiTradeType::Withdraw)
            .bind(chain_code)
            .bind(from_addr)
            .bind(exclude_trade_no)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        let energy = row.try_get::<i64, _>("energy").unwrap_or_default().max(0) as u64;
        let bandwidth = row.try_get::<i64, _>("bandwidth").unwrap_or_default().max(0) as u64;
        Ok(PendingWithdrawResourceBudget { energy, bandwidth })
    }

    pub async fn pending_resource_orders_by_owner<'a, E>(
        exec: E,
        chain_code: &str,
        from_addr: &str,
        exclude_trade_no: &str,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM api_withdraws
            WHERE trade_type = ?
              AND lower(chain_code) = lower(?)
              AND from_addr = ?
              AND trade_no <> ?
              AND estimated_resource_consume IS NOT NULL
              AND transaction_time IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
            ORDER BY created_at ASC, id ASC
            LIMIT ?
        "#;

        sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiTradeType::Withdraw)
            .bind(chain_code)
            .bind(from_addr)
            .bind(exclude_trade_no)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// Find withdraw candidates for acct_change-driven tx_hash backfill.
    ///
    /// Notes:
    /// - Only applies to normal withdraw orders (not self-withdraw/recharge)
    /// - Requires success-path execution evidence before backfill
    /// - Filters out rows that already uploaded tx_exec_receipt or entered failure path
    pub async fn find_candidates_for_acct_change_hash_backfill<'a, E>(
        exec: E,
        chain_code: &str,
        from_addr: &str,
        to_addr: &str,
        token_addr: Option<&str>,
        symbol: &str,
        limit: i64,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let base_sql = r#"
            SELECT * FROM api_withdraws
            WHERE trade_type = ?
              AND chain_code = ?
              AND from_addr = ?
              AND to_addr = ?
              AND symbol = ?
              AND finished_at IS NULL
              AND tx_exec_receipt_uploaded_at IS NULL
              AND err_code IS NULL
              AND chain_failed_at IS NULL
              AND (
                    chain_success_at IS NOT NULL
                 OR transaction_time IS NOT NULL
                 OR last_broadcast_at IS NOT NULL
              )
              AND (tx_hash IS NULL OR trim(tx_hash) = '')
        "#;

        let (sql, token_to_bind) = match token_addr {
            Some(token) => (
                format!(
                    "{} AND token_addr = ? ORDER BY COALESCE(transaction_time, chain_success_at, last_broadcast_at, updated_at, created_at) DESC LIMIT ?",
                    base_sql
                ),
                Some(token),
            ),
            None => (
                format!(
                    "{} AND (token_addr IS NULL OR trim(token_addr) = '') ORDER BY COALESCE(transaction_time, chain_success_at, last_broadcast_at, updated_at, created_at) DESC LIMIT ?",
                    base_sql
                ),
                None,
            ),
        };

        let mut query = sqlx::query_as::<_, ApiWithdrawEntity>(&sql)
            .bind(ApiTradeType::Withdraw)
            .bind(chain_code)
            .bind(from_addr)
            .bind(to_addr)
            .bind(symbol);
        if let Some(token) = token_to_bind {
            query = query.bind(token);
        }

        let result = query
            .bind(limit)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    pub async fn get_by_hash_and_owner<'a, E>(
        exec: E,
        owner: &str,
        tx_hash: &str,
    ) -> Result<ApiWithdrawEntity, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM api_withdraws WHERE from_addr = ? AND tx_hash = ?";
        let res = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(owner)
            .bind(tx_hash)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res)
    }

    pub async fn lists_by_hashs<'a, E>(
        exec: E,
        owner: &str,
        hashs: Vec<String>,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut qb =
            QueryBuilder::<Sqlite>::new("SELECT * FROM api_withdraws WHERE `from_addr` = ");

        qb.push_bind(owner);
        qb.push(" AND trade_type = ").push_bind(ApiTradeType::SelfWithdraw);
        // 绑定多个 hash
        qb.push(" AND tx_hash IN (");
        qb.push_values(hashs.iter(), |mut b, h| {
            b.push_bind(h);
        });
        qb.push(")");

        let query = qb.build_query_as::<ApiWithdrawEntity>();

        let res = query.fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res)
    }

    pub async fn recent_bill<'a, E>(
        exec: &E,
        token: &str,
        from_addr: &str,
        chain_code: &str,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiWithdrawEntity>, crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        let mut count_qb =
            QueryBuilder::<Sqlite>::new("SELECT count(*) FROM api_withdraws WHERE trade_type=4 ");
        let mut qb = QueryBuilder::<Sqlite>::new("SELECT * FROM api_withdraws WHERE trade_type=4 ");
        if !from_addr.is_empty() {
            count_qb.push("AND from_addr = ").push_bind(from_addr);
            qb.push("AND from_addr = ").push_bind(from_addr);
        }
        if !chain_code.is_empty() {
            count_qb.push("AND chain_code = ").push_bind(chain_code);
            qb.push("AND chain_code = ").push_bind(chain_code);
        }
        if !token.is_empty() {
            count_qb.push("AND token_addr = ").push_bind(token);
            qb.push("AND token_addr = ").push_bind(token);
        }

        // count
        count_qb.push(" GROUP BY to_addr");
        let count_query = count_qb.build_query_scalar();
        let total_count =
            count_query.fetch_one(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        // list
        qb.push(" GROUP BY to_addr");
        qb.push(" ORDER BY updated_at DESC, created_at DESC");
        qb.push(" LIMIT ").push_bind(page_size);
        qb.push(" OFFSET ").push_bind(page * page_size);
        let query = qb.build_query_as::<ApiWithdrawEntity>();
        let rows = query.fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        let mut paginate = Pagination::<ApiWithdrawEntity>::init(page, page_size);
        paginate.total_count = total_count;
        paginate.data = rows;
        Ok(paginate)
    }

    pub async fn bill_lists<'a, E>(
        exec: &E,
        uid: &str,
        addr: &[String],
        chain_code: Option<&str>,
        symbol: Option<&str>,
        _is_multisig: Option<i64>,
        min_value: Option<f64>,
        start: Option<i64>,
        end: Option<i64>,
        transfer_type: Vec<i32>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiWithdrawEntity>, crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        let (mut count_qb, mut qb) = if transfer_type.len() > 0 {
            let count_qb_s = "SELECT count(*) FROM api_withdraws WHERE ".to_string();
            let qb_s = "SELECT * FROM api_withdraws WHERE ".to_string();
            let mut conds: Vec<&str> = vec![];
            for tt in transfer_type {
                if tt == ApiTradeType::Withdraw as i32 {
                    conds.push("(trade_type = 1 AND init_status = 0 AND status in (3,5,7,8,9,10))");
                } else if tt == ApiTradeType::SelfWithdraw as i32 {
                    conds.push("trade_type = 4");
                } else if tt == ApiTradeType::SelfRecharge as i32 {
                    conds.push("trade_type = 5");
                }
            }
            let s = conds.join(" OR ");
            (
                QueryBuilder::<Sqlite>::new(count_qb_s + " ( " + s.as_str() + " ) "),
                QueryBuilder::<Sqlite>::new(qb_s + " ( " + s.as_str() + " ) "),
            )
        } else {
            (
                QueryBuilder::<Sqlite>::new(
                    "SELECT count(*) FROM api_withdraws WHERE ((trade_type = 1 AND init_status = 0 AND status in (3,5,7,8,9,10)) OR trade_type IN (4,5)) ",
                ),
                QueryBuilder::<Sqlite>::new(
                    "SELECT * FROM api_withdraws WHERE ((trade_type = 1 AND init_status = 0 AND status in (3,5,7,8,9,10)) OR trade_type IN (4,5)) ",
                ),
            )
        };
        if !uid.is_empty() {
            count_qb.push(" AND uid = ").push_bind(uid);
            qb.push(" AND uid = ").push_bind(uid);
        }
        let filtered_addrs = addr.iter().filter(|a| !a.is_empty()).collect::<Vec<_>>();
        if !filtered_addrs.is_empty() {
            count_qb.push(" AND (from_addr IN (");
            {
                let mut count_sep = count_qb.separated(", ");
                for a in &filtered_addrs {
                    count_sep.push_bind(*a);
                }
            }
            count_qb.push(") OR to_addr IN (");
            {
                let mut count_sep = count_qb.separated(", ");
                for a in &filtered_addrs {
                    count_sep.push_bind(*a);
                }
            }
            count_qb.push("))");

            qb.push(" AND (from_addr IN (");
            {
                let mut sep = qb.separated(", ");
                for a in &filtered_addrs {
                    sep.push_bind(*a);
                }
            }
            qb.push(") OR to_addr IN (");
            {
                let mut sep = qb.separated(", ");
                for a in &filtered_addrs {
                    sep.push_bind(*a);
                }
            }
            qb.push("))");
        }
        if let Some(c) = chain_code {
            // tracing::info!("chain code: {}", c);
            count_qb.push(" AND chain_code = ").push_bind(c);
            qb.push(" AND chain_code = ").push_bind(c);
        }
        if let Some(c) = symbol {
            count_qb.push(" AND symbol = ").push_bind(c);
            qb.push(" AND symbol = ").push_bind(c);
        }
        if let Some(c) = min_value {
            count_qb.push(" AND CAST(value AS REAL) >= ").push_bind(c);
            qb.push(" AND CAST(value AS REAL) >= ").push_bind(c);
        }
        if let Some(c) = start {
            let dt = DateTime::<Utc>::from_timestamp(c, 0)
                .ok_or_else(|| crate::Error::Other(format!("invalid unix timestamp: {c}")))?;
            tracing::info!(" ==== start {:?}", dt);
            count_qb.push(" AND transaction_time >= ").push_bind(dt);
            qb.push(" AND transaction_time >= ").push_bind(dt);
        }
        if let Some(c) = end {
            let dt = DateTime::<Utc>::from_timestamp(c, 0)
                .ok_or_else(|| crate::Error::Other(format!("invalid unix timestamp: {c}")))?;
            tracing::info!(" ==== end {:?}", dt);
            count_qb.push(" AND transaction_time <= ").push_bind(dt);
            qb.push(" AND transaction_time <= ").push_bind(dt);
        }

        tracing::info!("query_count={:?}", count_qb.sql());
        let count_query = count_qb.build_query_scalar();
        let total_count =
            count_query.fetch_one(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        qb.push(" ORDER BY updated_at DESC, created_at DESC");
        qb.push(" LIMIT ").push_bind(page_size);
        qb.push(" OFFSET ").push_bind(page * page_size);
        let query = qb.build_query_as::<ApiWithdrawEntity>();
        let rows = query.fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        let mut paginate = Pagination::<ApiWithdrawEntity>::init(page, page_size);
        paginate.total_count = total_count;
        paginate.data = rows;
        Ok(paginate)
    }

    pub async fn upsert<'a, E>(exec: E, api_withdraw: ApiWithdrawEntity) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            INSERT INTO api_withdraws
                (uid,
                name,
                from_addr,
                to_addr,
                value,
                validate,
                chain_code,
                token_addr,
                symbol,
                trade_no,
                out_order_id,
                client_id,
                create_time,
                trade_type,
                init_status,
                status,
                tx_hash,
                resource_consume,
                transaction_fee,
                transaction_time,
                block_height,
                notes,
                created_at,
                updated_at)
            VALUES
                ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,
                 $11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            ON CONFLICT(trade_no) DO UPDATE SET
                updated_at          = strftime('%Y-%m-%dT%H:%M:%SZ','now')
        "#;

        let res = sqlx::query(sql)
            .bind(&api_withdraw.uid)
            .bind(&api_withdraw.name)
            .bind(&api_withdraw.from_addr)
            .bind(&api_withdraw.to_addr)
            .bind(&api_withdraw.value)
            .bind(&api_withdraw.validate)
            .bind(&api_withdraw.chain_code)
            .bind(&api_withdraw.token_addr)
            .bind(&api_withdraw.symbol)
            .bind(&api_withdraw.trade_no)
            .bind(&api_withdraw.out_order_id)
            .bind(&api_withdraw.client_id)
            .bind(&api_withdraw.create_time)
            .bind(&api_withdraw.trade_type)
            .bind(&api_withdraw.init_status)
            .bind(&api_withdraw.status)
            .bind(&api_withdraw.tx_hash) // hash
            .bind(&api_withdraw.resource_consume) // consume
            .bind(&api_withdraw.transaction_fee) // fee
            .bind(api_withdraw.transaction_time) // time
            .bind(api_withdraw.block_height) // block height
            .bind(&api_withdraw.notes)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        tracing::info!(rows_affected=%res.rows_affected(), "withdraw api");
        Ok(())
    }

    pub async fn add<'a, E>(exec: E, api_withdraw: WithdrawCreatedFact) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            INSERT INTO api_withdraws
                (uid,
                name,
                from_addr,
                to_addr,
                value,
                validate,
                chain_code,
                token_addr,
                symbol,
                trade_no,
                trade_type,
                status,
                created_at)
            VALUES
                ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT(trade_no) DO UPDATE SET
                updated_at          = strftime('%Y-%m-%dT%H:%M:%SZ','now')
        "#;

        let res = sqlx::query(sql)
            .bind(&api_withdraw.uid)
            .bind(&api_withdraw.name)
            .bind(&api_withdraw.from_addr)
            .bind(&api_withdraw.to_addr)
            .bind(&api_withdraw.value)
            .bind(&api_withdraw.validate)
            .bind(&api_withdraw.chain_code)
            .bind(api_withdraw.token_addr)
            .bind(&api_withdraw.symbol)
            .bind(&api_withdraw.trade_no)
            .bind(api_withdraw.trade_type)
            .bind(api_withdraw.status)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        tracing::info!(xx=%res.rows_affected(), "tx withdraw api");
        Ok(())
    }

    pub async fn update_status_and_err<'a, E>(
        exec: E,
        trade_no: &str,
        status: ApiWithdrawStatus,
        err_code: ErrCode,
        err_msg: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                status = $2,
                err_code = $3,
                err_msg = $4,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND err_code IS NULL
        "#;

        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(&status)
            .bind(err_code)
            .bind(err_msg)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    /// 批量失败指定钱包的所有 pending 提币订单（设备被顶掉时调用）。
    /// 写 err_code / err_msg / status（设为发送失败报告终态），释放资源门禁。
    pub async fn batch_fail_pending_by_uid<'a, E>(
        exec: E,
        uid: &str,
        err_code: ErrCode,
        err_msg: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                err_code = $2,
                err_msg = $3,
                status = $5,
                resource_block_reason = NULL,
                resource_dependency_trade_no = NULL,
                resource_dependency_type = NULL,
                resource_gate_released_at = COALESCE(resource_gate_released_at, strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
            WHERE uid = $1
              AND trade_type = $4
              AND transaction_time IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
        "#;

        let res = sqlx::query(sql)
            .bind(uid)
            .bind(err_code)
            .bind(err_msg)
            .bind(ApiTradeType::Withdraw)
            .bind(ApiWithdrawStatus::SendingTxFailedReport)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    pub async fn mark_build_failed_if_unfinished<'a, E>(
        exec: E,
        trade_no: &str,
        err_code: ErrCode,
        err_msg: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                err_code = $2,
                err_msg = $3,
                failure_stage = $4,
                building_at = NULL,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND raw_tx IS NULL
              AND transaction_time IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
        "#;

        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(err_code)
            .bind(err_msg)
            .bind(WithdrawFailureStage::Build)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    pub async fn update_next_status<'a, E>(
        exec: E,
        trade_no: &str,
        status: ApiWithdrawStatus,
        next_status: ApiWithdrawStatus,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                status = $3,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1 and status = $2
              AND err_code IS NULL
        "#;

        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(&status)
            .bind(&next_status)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    pub async fn update_tx_status<'a, E>(
        exec: E,
        trade_no: &str,
        nonce: i64,
        tx_hash: &str,
        resource_consume: &str,
        transaction_fee: &str,
        transaction_time: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
        block_height: &str,
        status: ApiWithdrawStatus,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                status = $2,
                nonce = $3,
                tx_hash = $4,
                resource_consume = $5,
                transaction_fee = $6,
                transaction_time = $7,
                block_height = $8,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              -- Signed EVM SelfWithdraw is completed by receipt observation;
              -- legacy account-change status must not overwrite its signed facts.
              AND NOT (trade_type = 4 AND chain_code IN (SELECT value FROM json_each($9))
                       AND COALESCE(raw_tx, '') != '')
        "#;

        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(status)
            .bind(nonce)
            .bind(tx_hash)
            .bind(resource_consume)
            .bind(transaction_fee)
            .bind(transaction_time)
            .bind(block_height)
            .bind(signed_evm_chain_codes_json())
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    pub async fn update_tx<'a, E>(
        exec: E,
        trade_no: &str,
        resource_consume: &str,
        transaction_fee: &str,
        transaction_time: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
        block_height: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                resource_consume = $2,
                transaction_fee = $3,
                transaction_time = $4,
                block_height = $5,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
        "#;

        sqlx::query(sql)
            .bind(trade_no)
            .bind(resource_consume)
            .bind(transaction_fee)
            .bind(transaction_time)
            .bind(block_height)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

    pub async fn update_actual_fee<'a, E>(
        exec: E,
        trade_no: &str,
        transaction_fee: Option<&str>,
        resource_consume: Option<&str>,
        block_height: Option<&str>,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                transaction_fee = CASE WHEN $2 IS NULL THEN transaction_fee ELSE $2 END,
                resource_consume = CASE WHEN $3 IS NULL THEN resource_consume ELSE $3 END,
                block_height = CASE WHEN $4 IS NULL THEN block_height ELSE $4 END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND ($2 IS NOT NULL OR $3 IS NOT NULL OR $4 IS NOT NULL)
        "#;

        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(transaction_fee)
            .bind(resource_consume)
            .bind(block_height)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    pub async fn update_post_tx_count<'a, E>(
        exec: E,
        trade_no: &str,
        status: ApiWithdrawStatus,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                post_tx_count = MIN(post_tx_count + 1, 63),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1 and status = $2
        "#;

        sqlx::query(sql)
            .bind(trade_no)
            .bind(&status)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

    pub async fn update_post_confirm_tx_count<'a, E>(
        exec: E,
        trade_no: &str,
        status: ApiWithdrawStatus,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                post_confirm_tx_count = MIN(post_confirm_tx_count + 1, 63),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1 and status = $2
        "#;
        sqlx::query(sql)
            .bind(trade_no)
            .bind(&status)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

    pub async fn get_ack_times<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<
        (
            Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
            Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
        ),
        crate::Error,
    >
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT tx_ack_sent_at, tx_res_ack_sent_at
            FROM api_withdraws
            WHERE trade_no = $1
        "#;
        let row = sqlx::query(sql)
            .bind(trade_no)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        if let Some(row) = row {
            let tx_ack_sent_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>> =
                row.try_get(0).map_err(|e| crate::Error::Database(e.into()))?;
            let tx_res_ack_sent_at: Option<
                sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
            > = row.try_get(1).map_err(|e| crate::Error::Database(e.into()))?;

            Ok((tx_ack_sent_at, tx_res_ack_sent_at))
        } else {
            Ok((None, None))
        }
    }

    /// 保留历史调用入口（attempted 字段已移除）
    ///
    /// 当前 attempted_at 字段已移除，此方法仅刷新 updated_at 以保持调用兼容。
    pub async fn mark_tx_ack_attempted<'a, E>(exec: E, trade_no: &str) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND tx_ack_sent_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 标记交易 ACK 已发送（推进事实）
    ///
    /// 语义：
    /// - 交易 ACK 已成功发送到后端
    /// - 这是副作用完成的事实
    ///
    /// ⚠️ 调用约束：
    /// - 仅允许调用一次（tx_ack_sent_at IS NULL）
    /// - 由 SideEffectWorker 调用
    pub async fn mark_tx_ack_sent<'a, E>(exec: E, trade_no: &str) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                tx_ack_sent_at = COALESCE(
                    tx_ack_sent_at,
                    strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                ),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND tx_ack_sent_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 保留历史调用入口（attempted 字段已移除）
    ///
    /// 当前 attempted_at 字段已移除，此方法仅刷新 updated_at 以保持调用兼容。
    pub async fn mark_tx_exec_receipt_attempted<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND tx_exec_receipt_uploaded_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 标记交易执行回执已上传
    ///
    /// 语义：
    /// - 交易执行回执已成功上传到后端
    /// - 这是副作用完成的事实
    ///
    /// ⚠️ 调用约束：
    /// - 仅允许调用一次（tx_exec_receipt_uploaded_at IS NULL）
    /// - 由 SideEffectWorker 调用
    pub async fn mark_tx_exec_receipt_uploaded<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                tx_exec_receipt_uploaded_at = COALESCE(
                    tx_exec_receipt_uploaded_at,
                    strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                ),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND tx_exec_receipt_uploaded_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 保留历史调用入口（attempted 字段已移除）
    ///
    /// 当前 attempted_at 字段已移除，此方法仅刷新 updated_at 以保持调用兼容。
    pub async fn mark_tx_res_ack_attempted<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND tx_res_ack_sent_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 标记交易结果 ACK 已发送（推进事实）
    ///
    /// 语义：
    /// - 交易结果 ACK 已成功发送到后端
    /// - 这是副作用完成的事实
    ///
    /// ⚠️ 调用约束：
    /// - 仅允许调用一次（tx_res_ack_sent_at IS NULL）
    /// - 由 SideEffectWorker 调用
    pub async fn mark_tx_res_ack_sent<'a, E>(exec: E, trade_no: &str) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                tx_res_ack_sent_at = COALESCE(
                    tx_res_ack_sent_at,
                    strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                ),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND tx_res_ack_sent_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 原子标记链上终态（唯一合法 finished_at 写入口）
    ///
    /// 语义：
    /// - 这是"广播成功 → 链上确认"的不可逆事实跃迁
    /// - 单条 SQL 原子更新，防止 kill -9 产生"半完成事实"
    /// - WHERE 带旧事实约束，保证并发安全
    ///
    /// 写入顺序约束（不可逆）：
    /// raw_tx → tx_hash → transaction_time → finished_at
    pub async fn mark_chain_finished<'a, E>(exec: E, trade_no: &str) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                finished_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND finished_at IS NULL
              AND (
                    transaction_time IS NOT NULL
                    OR chain_success_at IS NOT NULL
                    OR (
                        transaction_time IS NULL
                        AND chain_success_at IS NULL
                        AND (chain_failed_at IS NOT NULL OR err_code IS NOT NULL)
                        AND tx_exec_receipt_uploaded_at IS NOT NULL
                    )
                    OR (
                        transaction_time IS NOT NULL
                        AND chain_success_at IS NULL
                        AND chain_failed_at IS NOT NULL
                        AND last_broadcast_at IS NULL
                        AND tx_exec_receipt_uploaded_at IS NULL
                    )
              )
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 原子标记交易结果 ACK 已发送并标记链上终态
    ///
    /// 语义：
    /// - 交易结果 ACK 已成功发送到后端
    /// - 同时标记链上终态
    /// - 单条 SQL 原子更新，防止 kill -9 产生"半完成事实"
    /// - WHERE 带旧事实约束，保证并发安全
    ///
    /// 写入顺序约束（不可逆）：
    /// raw_tx → tx_hash → transaction_time → finished_at
    pub async fn mark_tx_res_ack_sent_and_chain_finished<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                tx_res_ack_sent_at = COALESCE(
                    tx_res_ack_sent_at,
                    strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                ),
                finished_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND tx_res_ack_sent_at IS NULL
              AND finished_at IS NULL
              AND transaction_time IS NOT NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 扫描需要发送交易 ACK 的交易
    ///
    /// 事实条件直接翻译：
    /// - tx_ack_sent_at IS NULL：尚未发送交易 ACK
    /// - id IS NOT NULL：记录已存在
    ///
    /// ⚠️ 注意：
    /// - 仅基于推进事实判断，不依赖 attempted 行为中间态
    pub async fn scan_need_tx_ack<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM api_withdraws 
            WHERE tx_ack_sent_at IS NULL
            AND finished_at IS NULL
            AND err_code IS NULL
            AND trade_type = ?
            ORDER BY created_at ASC
            LIMIT ?
        "#;
        let result = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiTradeType::Withdraw)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    /// 扫描需要恢复交易的记录
    ///
    /// 事实条件：
    /// - tx_hash IS NOT NULL：交易已构建
    /// - transaction_time IS NULL：链上结果未确认
    /// - last_broadcast_at IS NOT NULL：广播可见但未确认的记录仍应进入 recover
    /// - finished_at IS NULL：系统生命周期未结束
    /// - err_code IS NULL：无终止错误
    /// - tx_exec_receipt_uploaded_at IS NULL：回执上传后禁止自动恢复
    ///
    /// ⚠️ 重要约束：
    /// - SQL必须100%等价于scanner中的need_recover predicate
    ///
    /// ============================================================================
    /// ⚠️ EVM uncertain recover model:
    ///
    /// - EVM may enter "broadcast uncertain" (sendRaw returned hash, same-rpc not visible)
    /// - These records use broadcast_uncertain_* facts + worker backoff/timeout/reconcile
    /// - Recover is allowed only after uncertain marker is written
    ///
    /// To avoid pre-broadcast Recover/Broadcast races:
    /// - EVM rows with raw_tx present, no last_broadcast_at, and NO uncertain marker
    ///   are excluded from recover (Broadcast should own progression)
    /// ============================================================================
    pub async fn scan_need_recover<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM api_withdraws 
            WHERE tx_hash IS NOT NULL
            AND (
                (
                    transaction_time IS NULL
                    AND tx_exec_receipt_uploaded_at IS NULL
                    AND finished_at IS NULL
                )
                OR (
                    lower(chain_code) = 'tron'
                    AND token_addr IS NOT NULL
                    AND trim(token_addr) <> ''
                    AND status IN (?, ?)
                    AND (
                        trim(resource_consume) = ''
                        OR NOT json_valid(resource_consume)
                        OR (
                            COALESCE(json_extract(resource_consume, '$.bandwidth'), json_extract(resource_consume, '$.net_used'), 0) = 0
                            AND COALESCE(json_extract(resource_consume, '$.energy'), json_extract(resource_consume, '$.energy_used'), 0) = 0
                        )
                    )
                )
            )
            AND err_code IS NULL
            AND NOT (
                chain_code IN ('bnb','eth')
                AND raw_tx IS NOT NULL
                AND last_broadcast_at IS NULL
                AND broadcast_uncertain_since_at IS NULL
            )
            AND trade_type = ?
            ORDER BY created_at ASC
            LIMIT ?
        "#;
        let result = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiWithdrawStatus::Success)
            .bind(ApiWithdrawStatus::ConfirmSuccessReport)
            .bind(ApiTradeType::Withdraw)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    // ============================================================================
    // STRONG ORDER GATE — BuildTx 不可逆事实屏障
    // ============================================================================
    //
    // BuildTx 只能在以下事实全部满足时发生：
    //
    // [FACT REQUIRED]
    // ✔ tx_ack_sent_at IS NOT NULL   — 后端确认已发送
    // ✔ audit_passed_at IS NOT NULL  — 审计通过（强顺序屏障）
    //
    // [FACT MUST NOT EXIST]
    // ✘ raw_tx IS NOT NULL           — 防止重复构建
    // ✘ finished_at IS NOT NULL      — 已终态
    // ✘ err_code IS NOT NULL         — 终止错误
    //
    // ⚠️ DO NOT REMOVE ANY CONDITION
    // ⚠️ Scanner 与 DAO predicate 必须完全一致
    // ============================================================================
    // ============================================================================
    // ⚠️ Scanner / DAO Predicate Symmetry Rule
    //
    // 本方法 SQL predicate 必须与：
    // wallet_api::infrastructure::withdraw::shadow::scanner::can_build 完全一致
    //
    // 修改任一侧时必须同步修改另一侧，否则会导致：
    // - Phantom Task
    // - Double Build
    // - 永久卡死
    //
    // DAO 是事实来源，Scanner 是安全网
    // ============================================================================
    pub async fn scan_can_build<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            -- ============================================================================
            -- STRONG ORDER GATE — BuildTx 不可逆事实屏障
            -- ============================================================================
            --
            -- BuildTx 只能在以下事实全部满足时发生：
            --
            -- [FACT REQUIRED]
            -- ✔ tx_ack_sent_at IS NOT NULL   — 后端确认已发送
            -- ✔ audit_passed_at IS NOT NULL  — 审计通过（强顺序屏障）
            -- ✔ 非 TRON 或 resource_gate_released_at IS NOT NULL
            --
            -- [FACT MUST NOT EXIST]
            -- ✘ raw_tx IS NOT NULL           — 防止重复构建
            -- ✘ transaction_time IS NOT NULL — 链上事实已确认
            -- ✘ finished_at IS NOT NULL      — 已终态
            -- ✘ err_code IS NOT NULL         — 终止错误
            --
            -- ⚠️ DO NOT REMOVE ANY CONDITION
            -- ⚠️ Scanner 与 DAO predicate 必须完全一致
            -- ============================================================================
            SELECT * FROM api_withdraws 
            WHERE tx_ack_sent_at IS NOT NULL
            AND audit_passed_at IS NOT NULL
            AND (lower(chain_code) <> 'tron' OR resource_gate_released_at IS NOT NULL)
            AND raw_tx IS NULL 
            AND transaction_time IS NULL
            AND finished_at IS NULL
            AND err_code IS NULL
            AND trade_type = ?
            ORDER BY created_at ASC
            LIMIT ?
        "#;
        let result = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiTradeType::Withdraw)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    /// 扫描可广播的交易
    ///
    /// 事实条件：
    /// - raw_tx IS NOT NULL：交易已构建
    /// - last_broadcast_at IS NULL：广播行为尚未发生
    /// - finished_at IS NULL：系统生命周期未结束
    /// - err_code IS NULL：无终止错误
    /// - tx_ack_sent_at IS NOT NULL：后端确认已发送
    ///
    /// ⚠️ 核心事实驱动原则：
    /// - 只基于不可逆事实字段决策
    /// - EVM uncertain（broadcast_uncertain_*）由 Recover 独占推进
    /// - 并发通过事实写入唯一性保证
    ///
    /// ============================================================================
    /// Broadcast model:
    ///
    /// - last_broadcast_at is written ONLY after successful submission
    /// - Failed attempts DO NOT write broadcast fact
    /// - Scanner relies on this to allow retry
    ///
    /// EVM uncertain rows are excluded here and owned by Recover.
    /// ============================================================================
    pub async fn scan_can_broadcast<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM api_withdraws 
            WHERE raw_tx IS NOT NULL 
            AND last_broadcast_at IS NULL 
            AND transaction_time IS NULL
            AND finished_at IS NULL
            AND err_code IS NULL
            AND tx_ack_sent_at IS NOT NULL
            AND (chain_code NOT IN ('bnb','eth') OR broadcast_uncertain_since_at IS NULL)
            AND trade_type = ?
            ORDER BY created_at ASC
            LIMIT ?
        "#;
        let result = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiTradeType::Withdraw)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    /// 扫描需要上传交易执行回执的交易
    ///
    /// 事实条件：
    /// - tx_exec_receipt_uploaded_at IS NULL：尚未上传执行回执
    /// - finished_at IS NULL：系统生命周期未结束
    /// - (chain_success_at IS NOT NULL OR transaction_time IS NOT NULL OR chain_failed_at IS NOT NULL OR err_code IS NOT NULL)：
    ///     - 已确认链上成功
    ///     - 或已确认链上结果
    ///     - 或出现终止型错误
    ///
    /// ⚠️ 架构铁律：
    /// - UploadTxExecReceipt =【执行行为回执】
    /// - 表示系统已执行 SendRawTx 并收到节点响应
    /// - 不代表链确认
    /// - last_broadcast_at 只是广播可见事实，不能作为上报门槛
    /// - 不依赖 transaction_time
    /// - tx_hash 只是构建事实，不能作为执行回执 gate
    /// - broadcast visible 但未确认的记录，必须留在 scanner 外
    ///
    /// ⚠️ err_code 仍允许上传：
    /// - 属于行为事实补齐副作用
    /// - 不属于推进，不受 err_code 冻结
    pub async fn scan_need_tx_exec_receipt_upload<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM api_withdraws 
            WHERE finished_at IS NULL
            AND tx_exec_receipt_uploaded_at IS NULL
            AND trade_type = ?
            AND (
                chain_success_at IS NOT NULL
                OR transaction_time IS NOT NULL
                OR chain_failed_at IS NOT NULL
                OR err_code IS NOT NULL
            )
            AND NOT (
                err_code IS NULL
                AND chain_failed_at IS NULL
                AND (
                    chain_success_at IS NOT NULL
                    OR transaction_time IS NOT NULL
                    OR last_broadcast_at IS NOT NULL
                )
                AND (
                    tx_hash IS NULL
                    OR trim(tx_hash) = ''
                )
            )
            ORDER BY created_at ASC
            LIMIT ?
        "#;
        let result = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiTradeType::Withdraw)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    /// 扫描需要发送交易结果 ACK 的交易
    ///
    /// 事实条件：
    /// - tx_exec_receipt_uploaded_at IS NOT NULL：交易执行回执已上传
    /// - finished_at IS NULL：系统生命周期未结束
    /// - transaction_time IS NOT NULL：链上结果已确认
    /// - tx_res_ack_sent_at IS NULL：尚未发送交易结果 ACK（推进事实）
    /// - err_code IS NULL：无终止错误
    ///
    /// ⚠️ 强顺序屏障：
    /// - TxResAck 必须发生在 TxExecReceipt 上传之后
    /// - TxResAck 必须发生在链上结果确认之后
    /// - 使用 transaction_time 作为前置条件是必要的，确保只对已确认的交易发送 ACK
    ///
    /// ⚠️ 注意：
    /// - 仅基于推进事实判断，不依赖 attempted 行为中间态
    pub async fn scan_need_tx_res_ack<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            -- ⚠️ 强顺序屏障：
            -- TxResAck 必须发生在 TxExecReceipt 上传之后
            -- TxResAck 必须发生在链上结果确认之后
            -- 使用 transaction_time 作为前置条件是必要的，确保只对已确认的交易发送 ACK
            SELECT * FROM api_withdraws 
            WHERE tx_exec_receipt_uploaded_at IS NOT NULL
            AND finished_at IS NULL
            AND transaction_time IS NOT NULL
            AND tx_res_received_at IS NOT NULL
            AND tx_res_ack_sent_at IS NULL
            AND err_code IS NULL
            AND trade_type = ?
            ORDER BY tx_exec_receipt_uploaded_at ASC
            LIMIT ?
        "#;
        let result = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiTradeType::Withdraw)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    /// 周期性卡单预筛选：扫描“可能卡住”的交易（低成本）
    ///
    /// 设计目标：
    /// - 尽可能便宜：只做粗筛
    /// - 宁可多报：后续由 wallet-api 的 DiagnoseEngine 二次判断
    ///
    /// 粗筛条件：
    /// - finished_at IS NULL：尚未终态
    /// - created_at < now - 5 minutes：避免刚创建的正常单刷屏
    /// - 已有进展事实（任意一个存在）：
    ///   - tx_ack_sent_at / raw_tx / last_broadcast_at / transaction_time
    ///
    /// ⚠️ 注意：
    /// - 不排除 err_code：失败冻结态仍可能需要补齐 receipt 等行为事实
    pub async fn scan_possible_stuck<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiWithdrawEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM api_withdraws
            WHERE finished_at IS NULL
              AND trade_type = ?
              AND created_at < strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-5 minutes')
              AND (
                tx_ack_sent_at IS NOT NULL
                OR raw_tx IS NOT NULL
                OR last_broadcast_at IS NOT NULL
                OR transaction_time IS NOT NULL
              )
            ORDER BY created_at ASC
            LIMIT ?
        "#;
        let result = sqlx::query_as::<_, ApiWithdrawEntity>(sql)
            .bind(ApiTradeType::Withdraw)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result)
    }

    /// 更新构建完成后的交易信息，包括raw_tx、tx_hash、transaction_fee、nonce和building_at
    ///
    /// ⚠️ 写入顺序约束（不可逆）：
    /// raw_tx → tx_hash → transaction_time → finished_at
    /// - 不允许写tx_hash时raw_tx还是NULL
    /// - 不允许写transaction_time时tx_hash是NULL
    /// - 不允许写finished_at时transaction_time是NULL
    ///
    /// ⚠️ nonce 语义：
    /// - nonce 是在 phase 1 分配的已裁决事实
    /// - 一旦写入，不允许修改
    /// - recover_tx 依赖此值进行交易恢复
    pub async fn update_after_build<'a, E>(
        exec: E,
        trade_no: &str,
        tx_hash: &str,
        raw_tx: &str,
        transaction_fee: &str,
        nonce: i64,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                raw_tx = $3,
                tx_hash = $2,
                transaction_fee = $4,
                nonce = $5,
                building_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND raw_tx IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(tx_hash)
            .bind(raw_tx)
            .bind(transaction_fee)
            .bind(nonce)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 作废当前 raw_tx 及其 tx_hash
    ///
    /// ⚠️ 设计铁律：
    /// - 一旦 raw_tx 被判定为不可再广播 / 不可再构建（如手续费不足、前置条件变化）
    /// - 必须同时清空 tx_hash
    /// - 确保 scanner / recover 只基于有效事实工作
    ///
    /// 本方法是"事实回滚"，不是状态流转。
    /// 该方法不是重试控制，而是事实作废。
    ///
    /// ⚠️ 调用约束：
    /// - 仅允许对尚未广播的交易调用（transaction_time IS NULL）
    /// - status 仅用于错误标注，不得用于流程推进
    /// - 📌 必须检查返回值 rows_affected()：
    ///   * rows_affected() == 0：表示事实已变更，无需处理
    ///   * rows_affected() == 1：表示成功作废事实
    ///   * 不建议直接忽略返回值
    pub async fn invalidate_raw_tx<'a, E>(
        exec: E,
        trade_no: &str,
        status: Option<ApiWithdrawStatus>,
        err_code: Option<u32>,
        err_msg: Option<&str>,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                raw_tx = NULL,
                tx_hash = NULL,
                building_at = NULL,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                status = COALESCE($2, status),
                err_code = COALESCE($3, err_code),
                err_msg = COALESCE($4, err_msg),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND transaction_time IS NULL
              AND raw_tx IS NOT NULL
        "#;

        let mut query = sqlx::query(sql).bind(trade_no);
        query = query.bind(status);
        query = query.bind(err_code);
        query = query.bind(err_msg);

        let res = query.execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    /// ⚠️ OBSERVATION ONLY
    /// This field is NOT used for:
    /// - execution decision
    /// - scanner logic
    /// Scanner MUST NOT depend on this field
    ///
    /// 建议只把它当成短期 build-slot 占位字段。
    pub async fn update_building_at<'a, E>(exec: E, trade_no: &str) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                building_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND building_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 清理 build slot
    pub async fn clear_building_at<'a, E>(exec: E, trade_no: &str) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                building_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND building_at IS NOT NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 更新last_broadcast_at时间
    pub async fn update_last_broadcast_at<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 标记广播已执行
    pub async fn mark_broadcast_executed<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND last_broadcast_at IS NULL
              AND transaction_time IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Record EVM broadcast/recover uncertain observation.
    pub async fn mark_broadcast_uncertain_attempt<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                broadcast_uncertain_since_at = COALESCE(
                    broadcast_uncertain_since_at,
                    strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                ),
                broadcast_uncertain_retry_count = COALESCE(broadcast_uncertain_retry_count, 0) + 1,
                broadcast_uncertain_last_checked_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND finished_at IS NULL
              AND err_code IS NULL
              AND transaction_time IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Mark uncertain timeout reconcile execution (at most once per lifecycle).
    pub async fn mark_broadcast_uncertain_reconciled<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                broadcast_uncertain_reconciled_at = COALESCE(
                    broadcast_uncertain_reconciled_at,
                    strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                ),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND finished_at IS NULL
              AND err_code IS NULL
              AND transaction_time IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Record one automatic rebuild/rebroadcast retry after uncertain timeout.
    pub async fn mark_broadcast_uncertain_rebroadcast_attempted<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                broadcast_uncertain_rebroadcast_count = COALESCE(broadcast_uncertain_rebroadcast_count, 0) + 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND finished_at IS NULL
              AND err_code IS NULL
              AND transaction_time IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 作废当前 raw_tx 及其 tx_hash（用于广播后判定失联后的重建）
    pub async fn invalidate_raw_tx_for_rebroadcast<'a, E>(
        exec: E,
        trade_no: &str,
        status: Option<ApiWithdrawStatus>,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                raw_tx = NULL,
                tx_hash = NULL,
                last_broadcast_at = NULL,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                status = COALESCE($2, status),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND transaction_time IS NULL
              AND last_broadcast_at IS NOT NULL
        "#;

        let mut query = sqlx::query(sql).bind(trade_no);
        query = query.bind(status);

        let res = query.execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    /// Clear uncertain tracking facts when stronger facts are established.
    pub async fn clear_broadcast_uncertain_tracking<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 确认链上交易事实
    pub async fn confirm_onchain_transaction_fact<'a, E>(
        exec: E,
        trade_no: &str,
        tx_hash: &str,
        transaction_time: &str,
        transaction_fee: &str,
        resource_consume: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                tx_hash = $2,
                transaction_time = $3,
                chain_success_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                chain_failed_at = NULL,
                err_code = NULL,
                err_msg = NULL,
                resource_consume = $4,
                transaction_fee = $5,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND transaction_time IS NULL
              AND (tx_hash IS NULL OR tx_hash = $2)
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(tx_hash)
            .bind(transaction_time)
            .bind(resource_consume)
            .bind(transaction_fee)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 确认链上交易事实（带恢复）
    pub async fn confirm_onchain_transaction_fact_with_recover<'a, E>(
        exec: E,
        trade_no: &str,
        tx_hash: &str,
        last_broadcast_at: &str,
        transaction_time: &str,
        transaction_fee: &str,
        resource_consume: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                last_broadcast_at = COALESCE(last_broadcast_at, $3),
                tx_hash = $2,
                transaction_time = $4,
                chain_success_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                chain_failed_at = NULL,
                err_code = NULL,
                err_msg = NULL,
                resource_consume = $5,
                transaction_fee = $6,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND transaction_time IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(tx_hash)
            .bind(last_broadcast_at)
            .bind(transaction_time)
            .bind(resource_consume)
            .bind(transaction_fee)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 确认交易时间（如果不存在）
    pub async fn confirm_transaction_time_if_absent<'a, E>(
        exec: E,
        trade_no: &str,
        transaction_time: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                transaction_time = $2,
                chain_failed_at = NULL,
                err_code = NULL,
                err_msg = NULL,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND transaction_time IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(transaction_time)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Backfill tx_hash only when it is missing and withdraw execution has already progressed.
    ///
    /// Safety rules:
    /// - only writes when tx_hash is NULL/empty
    /// - restricted to normal withdraw trade_type
    /// - requires success-path execution evidence
    /// - never overwrites non-empty tx_hash
    pub async fn backfill_tx_hash_if_missing<'a, E>(
        exec: E,
        trade_no: &str,
        tx_hash: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                tx_hash = $2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND trade_type = $3
              AND (tx_hash IS NULL OR trim(tx_hash) = '')
              AND (
                    chain_success_at IS NOT NULL
                 OR transaction_time IS NOT NULL
                 OR last_broadcast_at IS NOT NULL
              )
        "#;

        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(tx_hash)
            .bind(ApiTradeType::Withdraw)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 设置审核通过事实
    pub async fn set_audit_passed<'a, E>(exec: E, trade_no: &str) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                audit_passed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                audit_rejected_at = NULL,
                audit_reason = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND audit_passed_at IS NULL
              AND audit_rejected_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 设置审核拒绝事实
    pub async fn set_audit_rejected<'a, E>(
        exec: E,
        trade_no: &str,
        reason: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                audit_rejected_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                audit_passed_at = NULL,
                audit_reason = $2,
                err_code = $3,
                err_msg = $4,
                finished_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND audit_passed_at IS NULL
              AND audit_rejected_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(reason)
            .bind(ErrCode::UnknownError)
            .bind(reason)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 设置链成功事实
    pub async fn set_chain_success<'a, E>(exec: E, trade_no: &str) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                chain_success_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                chain_failed_at = NULL,
                failure_stage = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 设置链失败事实
    pub async fn set_chain_failed<'a, E>(exec: E, trade_no: &str) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                chain_failed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                chain_success_at = NULL,
                failure_stage = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 设置失败阶段事实
    pub async fn set_failure_stage<'a, E>(
        exec: E,
        trade_no: &str,
        stage: WithdrawFailureStage,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                failure_stage = $2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(stage)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 更新状态
    pub async fn update_status<'a, E>(
        exec: E,
        trade_no: &str,
        status: ApiWithdrawStatus,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                status = $2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(status)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// 标记 MQTT TxRes 已接收（外部事实）
    ///
    /// 语义：
    /// - 记录业务结果已就绪（来自 MQTT）
    /// - 只写入一次（幂等）
    /// - 不推进链、不修改状态
    ///
    /// ⚠️ 设计约束：
    /// - 禁止写 finished_at
    /// - 禁止修改 status
    /// - Scanner 只在 ResultAck 阶段读取该字段
    pub async fn update_tx_res_received_at<'a, E>(
        exec: E,
        trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                tx_res_received_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = $1
              AND tx_res_received_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn update_fee_estimate<'a, E>(
        exec: E,
        trade_no: &str,
        estimated_transaction_fee: &str,
        estimated_resource_consume: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_withdraws
            SET
                estimated_transaction_fee = ?2,
                estimated_resource_consume = ?3,
                fee_estimated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?1
              AND trade_type = ?4
              AND fee_estimated_at IS NULL
        "#;
        let res = sqlx::query(sql)
            .bind(trade_no)
            .bind(estimated_transaction_fee)
            .bind(estimated_resource_consume)
            .bind(ApiTradeType::Withdraw)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::ApiWithdrawDao;
    use crate::{
        SqliteContext,
        entities::{
            api_resource_gate::ApiResourceGateResult,
            api_trade_type::ApiTradeType,
            api_withdraw::{ApiWithdrawStatus, ErrCode, WithdrawFailureStage},
            asset_token_key::AssetTokenKey,
        },
        repositories::api_wallet::withdraw::ApiWithdrawRepo,
    };

    fn make_temp_dir(prefix: &str) -> String {
        let now =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("{prefix}_{pid}_{now}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().to_string()
    }

    async fn seed_withdraw_for_build_slot(pool: &crate::ApiTransactionDbPool, trade_no: &str) {
        ApiWithdrawRepo::upsert_api_withdraw(
            pool,
            "uid_build_slot",
            "withdraw",
            "FROM_BUILD_SLOT",
            "TO_BUILD_SLOT",
            "1",
            "validate",
            "tron",
            AssetTokenKey::Native,
            "TRX",
            trade_no,
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::SendingTx,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
    }

    async fn seed_withdraw_for_fee_estimate(
        pool: &crate::ApiTransactionDbPool,
        trade_no: &str,
        chain_code: &str,
        trade_type: ApiTradeType,
    ) {
        ApiWithdrawRepo::upsert_api_withdraw(
            pool,
            "uid_fee_estimate",
            "withdraw",
            "FROM_FEE_ESTIMATE",
            "TO_FEE_ESTIMATE",
            "8.88",
            "validate",
            chain_code,
            AssetTokenKey::Native,
            if chain_code.eq_ignore_ascii_case("tron") { "TRX" } else { "ETH" },
            trade_no,
            None,
            None,
            None,
            trade_type,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::InitOrder,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn upsert_api_withdraw_persists_order_metadata() {
        let dir = make_temp_dir("wallet_db_api_withdraw_order_metadata");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_meta",
            "withdraw",
            "FROM_META",
            "TO_META",
            "12.34",
            "validate",
            "tron",
            AssetTokenKey::Native,
            "TRX",
            "W_ORDER_METADATA",
            Some("merchant-order-1".to_string()),
            Some("client-1".to_string()),
            Some("2026-05-19 10:11:12".to_string()),
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::InitOrder,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_meta",
            "withdraw",
            "FROM_META",
            "TO_META",
            "12.34",
            "validate",
            "tron",
            AssetTokenKey::Native,
            "TRX",
            "W_ORDER_METADATA",
            Some("merchant-order-2".to_string()),
            Some("client-2".to_string()),
            Some("2026-05-19 13:14:15".to_string()),
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::InitOrder,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        let after = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            "W_ORDER_METADATA",
            ApiTradeType::Withdraw,
        )
        .await
        .unwrap();

        assert_eq!(after.out_order_id.as_deref(), Some("merchant-order-1"));
        assert_eq!(after.client_id.as_deref(), Some("client-1"));
        assert_eq!(after.create_time.as_deref(), Some("2026-05-19 10:11:12"));
    }

    #[tokio::test]
    async fn withdraw_fee_estimate_snapshot_update_is_first_write_wins() {
        let dir = make_temp_dir("wallet_db_api_withdraw_fee_estimate");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        seed_withdraw_for_fee_estimate(&pool, "W_FEE_ESTIMATE", "tron", ApiTradeType::Withdraw)
            .await;

        let affected = ApiWithdrawRepo::update_fee_estimate(
            &pool,
            "W_FEE_ESTIMATE",
            "0.456",
            r#"{"bandwidth":10,"energy":20}"#,
        )
        .await
        .unwrap();

        let retry_affected = ApiWithdrawRepo::update_fee_estimate(
            &pool,
            "W_FEE_ESTIMATE",
            "9.999",
            r#"{"bandwidth":99,"energy":99}"#,
        )
        .await
        .unwrap();

        let after = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            "W_FEE_ESTIMATE",
            ApiTradeType::Withdraw,
        )
        .await
        .unwrap();

        assert_eq!(affected, 1);
        assert_eq!(retry_affected, 0);
        assert_eq!(after.estimated_transaction_fee.as_deref(), Some("0.456"));
        assert_eq!(
            after.estimated_resource_consume.as_deref(),
            Some(r#"{"bandwidth":10,"energy":20}"#)
        );
        assert!(after.fee_estimated_at.is_some());
        assert_eq!(after.transaction_fee, "0");
        assert_eq!(after.resource_consume, "0");
    }

    #[tokio::test]
    async fn withdraw_fee_estimate_snapshot_scan_finds_supported_pending_chains() {
        let dir = make_temp_dir("wallet_db_api_withdraw_fee_estimate_scan");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        seed_withdraw_for_fee_estimate(
            &pool,
            "W_FEE_ESTIMATE_PENDING",
            "TRON",
            ApiTradeType::Withdraw,
        )
        .await;
        seed_withdraw_for_fee_estimate(&pool, "W_FEE_ESTIMATE_ETH", "eth", ApiTradeType::Withdraw)
            .await;
        seed_withdraw_for_fee_estimate(&pool, "W_FEE_ESTIMATE_BNB", "bnb", ApiTradeType::Withdraw)
            .await;
        seed_withdraw_for_fee_estimate(&pool, "W_FEE_ESTIMATE_SOL", "sol", ApiTradeType::Withdraw)
            .await;
        seed_withdraw_for_fee_estimate(
            &pool,
            "W_FEE_ESTIMATE_SOLANA",
            "solana",
            ApiTradeType::Withdraw,
        )
        .await;
        seed_withdraw_for_fee_estimate(&pool, "W_FEE_ESTIMATE_BTC", "btc", ApiTradeType::Withdraw)
            .await;
        seed_withdraw_for_fee_estimate(&pool, "W_FEE_ESTIMATE_SUI", "sui", ApiTradeType::Withdraw)
            .await;
        seed_withdraw_for_fee_estimate(
            &pool,
            "W_FEE_ESTIMATE_SELF",
            "tron",
            ApiTradeType::SelfWithdraw,
        )
        .await;
        seed_withdraw_for_fee_estimate(
            &pool,
            "W_FEE_ESTIMATE_EXISTING",
            "tron",
            ApiTradeType::Withdraw,
        )
        .await;
        seed_withdraw_for_fee_estimate(
            &pool,
            "W_FEE_ESTIMATE_RAW_TX",
            "tron",
            ApiTradeType::Withdraw,
        )
        .await;

        ApiWithdrawRepo::update_fee_estimate(
            &pool,
            "W_FEE_ESTIMATE_EXISTING",
            "0.456",
            r#"{"bandwidth":10,"energy":20}"#,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE api_withdraws SET raw_tx = ? WHERE trade_no = ?")
            .bind("raw")
            .bind("W_FEE_ESTIMATE_RAW_TX")
            .execute(pool.as_ref())
            .await
            .unwrap();

        let rows = ApiWithdrawRepo::scan_need_fee_estimate(&pool, 100).await.unwrap();
        let mut trade_nos = rows.into_iter().map(|row| row.trade_no).collect::<Vec<_>>();
        trade_nos.sort();

        assert_eq!(
            trade_nos,
            vec![
                "W_FEE_ESTIMATE_BNB",
                "W_FEE_ESTIMATE_BTC",
                "W_FEE_ESTIMATE_ETH",
                "W_FEE_ESTIMATE_PENDING",
                "W_FEE_ESTIMATE_SOL",
                "W_FEE_ESTIMATE_SOLANA",
                "W_FEE_ESTIMATE_SUI",
            ]
        );
    }

    #[tokio::test]
    async fn withdraw_resource_recovery_scans_scalar_success_and_updates_conditionally() {
        let dir = make_temp_dir("wallet_db_withdraw_resource_recovery");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        for trade_no in [
            "W_RECOVER_SCALAR",
            "W_RECOVER_JSON",
            "W_RECOVER_ZERO_TOKEN",
            "W_RECOVER_ZERO_NATIVE",
            "W_RECOVER_PENDING",
        ] {
            seed_withdraw_for_fee_estimate(&pool, trade_no, "tron", ApiTradeType::Withdraw).await;
        }

        for (trade_no, status, resource_consume, transaction_time) in [
            ("W_RECOVER_SCALAR", ApiWithdrawStatus::Success, "70070", Some(chrono::Utc::now())),
            (
                "W_RECOVER_JSON",
                ApiWithdrawStatus::Success,
                r#"{"net_used":4891,"energy_used":70070}"#,
                Some(chrono::Utc::now()),
            ),
            (
                "W_RECOVER_ZERO_TOKEN",
                ApiWithdrawStatus::Success,
                r#"{"bandwidth":0,"energy":0}"#,
                Some(chrono::Utc::now()),
            ),
            (
                "W_RECOVER_ZERO_NATIVE",
                ApiWithdrawStatus::Success,
                r#"{"bandwidth":0,"energy":0}"#,
                Some(chrono::Utc::now()),
            ),
            ("W_RECOVER_PENDING", ApiWithdrawStatus::SendingTx, "70070", None),
        ] {
            sqlx::query(
                "UPDATE api_withdraws SET status = ?, tx_hash = ?, transaction_time = ?, resource_consume = ? WHERE trade_no = ?",
            )
            .bind(status)
            .bind(format!("HASH_{trade_no}"))
            .bind(transaction_time)
            .bind(resource_consume)
            .bind(trade_no)
            .execute(pool.as_ref())
            .await
            .unwrap();
        }

        sqlx::query("UPDATE api_withdraws SET token_addr = ? WHERE trade_no = ?")
            .bind("TTestTokenContract")
            .bind("W_RECOVER_ZERO_TOKEN")
            .execute(pool.as_ref())
            .await
            .unwrap();

        let candidates =
            ApiWithdrawRepo::scan_withdraw_resource_recovery(&pool, None, 20).await.unwrap();
        assert_eq!(
            candidates.iter().map(|row| row.trade_no.as_str()).collect::<Vec<_>>(),
            vec!["W_RECOVER_SCALAR", "W_RECOVER_ZERO_TOKEN"]
        );
        let next_page =
            ApiWithdrawRepo::scan_withdraw_resource_recovery(&pool, Some(candidates[1].id), 20)
                .await
                .unwrap();
        assert!(next_page.is_empty());

        let replacement = r#"{"net_used":4891,"energy_used":70070}"#;
        let affected = ApiWithdrawRepo::update_resource_consume_if_unchanged(
            &pool,
            "W_RECOVER_SCALAR",
            "70070",
            replacement,
        )
        .await
        .unwrap();
        let stale_retry = ApiWithdrawRepo::update_resource_consume_if_unchanged(
            &pool,
            "W_RECOVER_SCALAR",
            "70070",
            r#"{"net_used":1,"energy_used":1}"#,
        )
        .await
        .unwrap();
        let after = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            "W_RECOVER_SCALAR",
            ApiTradeType::Withdraw,
        )
        .await
        .unwrap();

        assert_eq!(affected, 1);
        assert_eq!(stale_retry, 0);
        assert_eq!(after.resource_consume, replacement);
    }

    #[tokio::test]
    async fn pending_withdraw_resource_budget_sums_unfinished_same_owner_orders() {
        let dir = make_temp_dir("wallet_db_api_withdraw_pending_resource_budget");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        for trade_no in [
            "W_RESOURCE_CURRENT",
            "W_RESOURCE_PENDING",
            "W_RESOURCE_ESTIMATED_ONLY",
            "W_RESOURCE_FINISHED",
            "W_RESOURCE_OTHER_OWNER",
        ] {
            seed_withdraw_for_fee_estimate(&pool, trade_no, "tron", ApiTradeType::Withdraw).await;
            ApiWithdrawRepo::update_fee_estimate(
                &pool,
                trade_no,
                "1.23",
                r#"{"bandwidth":345,"energy":13532}"#,
            )
            .await
            .unwrap();
        }

        ApiWithdrawRepo::mark_resource_released(
            &pool,
            "W_RESOURCE_PENDING",
            ApiResourceGateResult::ResourceReady,
        )
        .await
        .unwrap();
        ApiWithdrawRepo::mark_resource_released(
            &pool,
            "W_RESOURCE_FINISHED",
            ApiResourceGateResult::ResourceReady,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE api_withdraws SET transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE trade_no = ?")
            .bind("W_RESOURCE_FINISHED")
            .execute(pool.as_ref())
            .await
            .unwrap();
        sqlx::query("UPDATE api_withdraws SET from_addr = ? WHERE trade_no = ?")
            .bind("OTHER_OWNER")
            .bind("W_RESOURCE_OTHER_OWNER")
            .execute(pool.as_ref())
            .await
            .unwrap();
        ApiWithdrawRepo::mark_resource_released(
            &pool,
            "W_RESOURCE_OTHER_OWNER",
            ApiResourceGateResult::ResourceReady,
        )
        .await
        .unwrap();

        let budget = ApiWithdrawRepo::pending_resource_budget_by_owner(
            &pool,
            "TRON",
            "FROM_FEE_ESTIMATE",
            "W_RESOURCE_CURRENT",
        )
        .await
        .unwrap();

        assert_eq!(budget.energy, 27_064);
        assert_eq!(budget.bandwidth, 690);
    }

    #[tokio::test]
    async fn pending_withdraw_resource_budget_counts_estimated_order_before_gate_release() {
        let dir = make_temp_dir("wallet_db_api_withdraw_pending_resource_estimated_only");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        seed_withdraw_for_fee_estimate(&pool, "W_RESOURCE_CURRENT", "tron", ApiTradeType::Withdraw)
            .await;
        seed_withdraw_for_fee_estimate(
            &pool,
            "W_RESOURCE_ESTIMATED_ONLY",
            "tron",
            ApiTradeType::Withdraw,
        )
        .await;
        ApiWithdrawRepo::update_fee_estimate(
            &pool,
            "W_RESOURCE_ESTIMATED_ONLY",
            "1.23",
            r#"{"bandwidth":345,"energy":13532}"#,
        )
        .await
        .unwrap();

        let budget = ApiWithdrawRepo::pending_resource_budget_by_owner(
            &pool,
            "TRON",
            "FROM_FEE_ESTIMATE",
            "W_RESOURCE_CURRENT",
        )
        .await
        .unwrap();

        assert_eq!(budget.energy, 13_532);
        assert_eq!(budget.bandwidth, 345);
    }

    #[tokio::test]
    async fn update_building_at_claims_slot_once() {
        let dir = make_temp_dir("wallet_db_api_withdraw_update_building_at");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        seed_withdraw_for_build_slot(&pool, "W_BUILDING_CLAIM").await;

        let first =
            ApiWithdrawDao::update_building_at(pool.as_ref(), "W_BUILDING_CLAIM").await.unwrap();
        let second =
            ApiWithdrawDao::update_building_at(pool.as_ref(), "W_BUILDING_CLAIM").await.unwrap();

        assert_eq!(first, 1);
        assert_eq!(second, 0);

        let after = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            "W_BUILDING_CLAIM",
            ApiTradeType::Withdraw,
        )
        .await
        .unwrap();
        assert!(after.building_at.is_some());
    }

    #[tokio::test]
    async fn clear_building_at_releases_slot() {
        let dir = make_temp_dir("wallet_db_api_withdraw_clear_building_at");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        seed_withdraw_for_build_slot(&pool, "W_BUILDING_CLEAR").await;

        let claimed =
            ApiWithdrawDao::update_building_at(pool.as_ref(), "W_BUILDING_CLEAR").await.unwrap();
        assert_eq!(claimed, 1);

        let cleared =
            ApiWithdrawDao::clear_building_at(pool.as_ref(), "W_BUILDING_CLEAR").await.unwrap();
        assert_eq!(cleared, 1);

        let reclaimed =
            ApiWithdrawDao::update_building_at(pool.as_ref(), "W_BUILDING_CLEAR").await.unwrap();
        assert_eq!(reclaimed, 1);

        let after = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            "W_BUILDING_CLEAR",
            ApiTradeType::Withdraw,
        )
        .await
        .unwrap();
        assert!(after.building_at.is_some());
    }

    #[tokio::test]
    async fn scan_possible_stuck_prefilter_works() {
        let dir = make_temp_dir("wallet_db_api_withdraw_scan_possible_stuck");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        // old + progressed => included
        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "0",
            "v",
            "c",
            None,
            "s",
            "W_STUCK_1",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_withdraws SET created_at = strftime('%Y-%m-%dT%H:%M:%SZ','now','-10 minutes'), tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE trade_no = ?",
        )
        .bind("W_STUCK_1")
        .execute(pool.as_ref())
        .await
        .unwrap();

        // too new => excluded
        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "0",
            "v",
            "c",
            None,
            "s",
            "W_STUCK_2",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_withdraws SET created_at = strftime('%Y-%m-%dT%H:%M:%SZ','now','-1 minutes'), tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE trade_no = ?",
        )
        .bind("W_STUCK_2")
        .execute(pool.as_ref())
        .await
        .unwrap();

        // finished => excluded
        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "0",
            "v",
            "c",
            None,
            "s",
            "W_STUCK_3",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_withdraws SET created_at = strftime('%Y-%m-%dT%H:%M:%SZ','now','-10 minutes'), tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), finished_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE trade_no = ?",
        )
        .bind("W_STUCK_3")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let rows = ApiWithdrawDao::scan_possible_stuck(pool.as_ref(), 100).await.unwrap();
        let trade_nos: std::collections::HashSet<_> =
            rows.into_iter().map(|r| r.trade_no).collect();

        assert!(trade_nos.contains("W_STUCK_1"));
        assert!(!trade_nos.contains("W_STUCK_2"));
        assert!(!trade_nos.contains("W_STUCK_3"));
    }

    #[tokio::test]
    async fn scan_need_tx_res_ack_requires_tx_res_received_at() {
        let dir = make_temp_dir("wallet_db_api_withdraw_scan_need_tx_res_ack_gate");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        // record A: eligible
        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "0",
            "v",
            "c",
            None,
            "s",
            "W_TX_RES_A",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_withdraws SET tx_exec_receipt_uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ','now'), tx_res_received_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE trade_no = ?",
        )
        .bind("W_TX_RES_A")
        .execute(pool.as_ref())
        .await
        .unwrap();

        // record B: missing tx_res_received_at => must be excluded
        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "0",
            "v",
            "c",
            None,
            "s",
            "W_TX_RES_B",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_withdraws SET tx_exec_receipt_uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE trade_no = ?",
        )
        .bind("W_TX_RES_B")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let records = ApiWithdrawDao::scan_need_tx_res_ack(pool.as_ref(), 100).await.unwrap();
        let trade_nos: Vec<String> = records.into_iter().map(|r| r.trade_no).collect();

        assert!(trade_nos.contains(&"W_TX_RES_A".to_string()));
        assert!(!trade_nos.contains(&"W_TX_RES_B".to_string()));
    }

    #[tokio::test]
    async fn scan_need_recover_allows_broadcast_visible_pending_result() {
        let dir = make_temp_dir("wallet_db_api_withdraw_scan_need_recover_broadcast_visible");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "0",
            "v",
            "eth",
            None,
            "s",
            "W_RECOVER_BROADCAST_VISIBLE",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET tx_hash = '0xrecover',
                 raw_tx = '{}',
                 last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_RECOVER_BROADCAST_VISIBLE")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let records = ApiWithdrawDao::scan_need_recover(pool.as_ref(), 100).await.unwrap();
        assert!(records.iter().any(|r| r.trade_no == "W_RECOVER_BROADCAST_VISIBLE"));
    }

    #[tokio::test]
    async fn scan_need_recover_allows_confirmed_tron_token_with_zero_resources() {
        let dir = make_temp_dir("wallet_db_api_withdraw_scan_confirmed_tron_resource");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "1",
            "v",
            "tron",
            "TToken",
            "USDT",
            "W_RECOVER_CONFIRMED_TRON_RESOURCE",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            Some("tx-hash".to_string()),
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Success,
            r#"{"bandwidth":0,"energy":0}"#,
            "0.344",
            Some(chrono::Utc::now()),
            Some("69342697".to_string()),
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_withdraws
             SET tx_exec_receipt_uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 finished_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_RECOVER_CONFIRMED_TRON_RESOURCE")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let records = ApiWithdrawDao::scan_need_recover(pool.as_ref(), 100).await.unwrap();

        assert!(records.iter().any(|r| r.trade_no == "W_RECOVER_CONFIRMED_TRON_RESOURCE"));
    }

    #[tokio::test]
    async fn scan_need_recover_excludes_pre_broadcast_pending_result() {
        let dir = make_temp_dir("wallet_db_api_withdraw_scan_need_recover_pre_broadcast");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "0",
            "v",
            "eth",
            None,
            "s",
            "W_RECOVER_PRE_BROADCAST",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET tx_hash = '0xrecover2',
                 raw_tx = '{}'
             WHERE trade_no = ?",
        )
        .bind("W_RECOVER_PRE_BROADCAST")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let records = ApiWithdrawDao::scan_need_recover(pool.as_ref(), 100).await.unwrap();
        assert!(!records.iter().any(|r| r.trade_no == "W_RECOVER_PRE_BROADCAST"));
    }

    #[tokio::test]
    async fn scan_need_tx_exec_receipt_upload_allows_transaction_time_without_last_broadcast() {
        let dir = make_temp_dir("wallet_db_api_withdraw_scan_need_receipt_tx_time");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "0",
            "v",
            "c",
            None,
            "s",
            "W_RECEIPT_TX_TIME",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        // Chain fact exists but broadcast fact is missing (uncertain broadcast).
        sqlx::query(
            "UPDATE api_withdraws
             SET transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 tx_hash = '0xtesthash'
             WHERE trade_no = ?",
        )
        .bind("W_RECEIPT_TX_TIME")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let records =
            ApiWithdrawDao::scan_need_tx_exec_receipt_upload(pool.as_ref(), 100).await.unwrap();
        let trade_nos: Vec<String> = records.into_iter().map(|r| r.trade_no).collect();

        assert!(trade_nos.contains(&"W_RECEIPT_TX_TIME".to_string()));
    }

    #[tokio::test]
    async fn scan_need_tx_exec_receipt_upload_excludes_broadcast_visible_pending() {
        let dir = make_temp_dir("wallet_db_api_withdraw_scan_receipt_pending");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "0",
            "v",
            "tron",
            None,
            "USDT",
            "W_RECEIPT_PENDING_BROADCAST_VISIBLE",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 tx_hash = '0xpendinghash'
             WHERE trade_no = ?",
        )
        .bind("W_RECEIPT_PENDING_BROADCAST_VISIBLE")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let records =
            ApiWithdrawDao::scan_need_tx_exec_receipt_upload(pool.as_ref(), 100).await.unwrap();
        assert!(!records.iter().any(|r| r.trade_no == "W_RECEIPT_PENDING_BROADCAST_VISIBLE"));
    }

    #[tokio::test]
    async fn scan_need_tx_exec_receipt_upload_freezes_success_missing_hash_on_chain_success() {
        let dir = make_temp_dir("wallet_db_api_withdraw_scan_receipt_freeze_chain_success");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "0",
            "v",
            "tron",
            None,
            "USDT",
            "W_RECEIPT_FREEZE_CHAIN_SUCCESS",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET chain_success_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 tx_hash = ''
             WHERE trade_no = ?",
        )
        .bind("W_RECEIPT_FREEZE_CHAIN_SUCCESS")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let records =
            ApiWithdrawDao::scan_need_tx_exec_receipt_upload(pool.as_ref(), 100).await.unwrap();
        assert!(!records.iter().any(|r| r.trade_no == "W_RECEIPT_FREEZE_CHAIN_SUCCESS"));
    }

    #[tokio::test]
    async fn scan_need_tx_exec_receipt_upload_fail_path_not_frozen_by_chain_failed() {
        let dir = make_temp_dir("wallet_db_api_withdraw_scan_receipt_fail_chain_failed");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "0",
            "v",
            "tron",
            None,
            "USDT",
            "W_RECEIPT_FAIL_CHAIN_FAILED_ALLOWED",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET chain_failed_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 tx_hash = ''
             WHERE trade_no = ?",
        )
        .bind("W_RECEIPT_FAIL_CHAIN_FAILED_ALLOWED")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let records =
            ApiWithdrawDao::scan_need_tx_exec_receipt_upload(pool.as_ref(), 100).await.unwrap();
        assert!(records.iter().any(|r| r.trade_no == "W_RECEIPT_FAIL_CHAIN_FAILED_ALLOWED"));
    }

    #[tokio::test]
    async fn find_candidates_for_acct_change_hash_backfill_matches_chain_success_without_tx_time() {
        let dir = make_temp_dir("wallet_db_api_withdraw_find_candidates_chain_success");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "WFrom",
            "WTo",
            "499",
            "v",
            "tron",
            Some("WToken".to_string()),
            "USDT",
            "W_ACCT_CHANGE_HASH_BACKFILL_CHAIN_SUCCESS",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET chain_success_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 transaction_time = NULL,
                 tx_hash = ''
             WHERE trade_no = ?",
        )
        .bind("W_ACCT_CHANGE_HASH_BACKFILL_CHAIN_SUCCESS")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let recs = ApiWithdrawDao::find_candidates_for_acct_change_hash_backfill(
            pool.as_ref(),
            "tron",
            "WFrom",
            "WTo",
            Some("WToken"),
            "USDT",
            10,
        )
        .await
        .unwrap();

        assert!(recs.iter().any(|r| r.trade_no == "W_ACCT_CHANGE_HASH_BACKFILL_CHAIN_SUCCESS"));
    }

    #[tokio::test]
    async fn find_candidates_for_acct_change_hash_backfill_excludes_non_withdraw_trade_type() {
        let dir = make_temp_dir("wallet_db_api_withdraw_find_candidates_trade_type");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "WFrom2",
            "WTo2",
            "499",
            "v",
            "tron",
            Some("WToken2".to_string()),
            "USDT",
            "W_ACCT_CHANGE_HASH_BACKFILL_SELF_WITHDRAW",
            None,
            None,
            None,
            ApiTradeType::SelfWithdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 tx_hash = ''
             WHERE trade_no = ?",
        )
        .bind("W_ACCT_CHANGE_HASH_BACKFILL_SELF_WITHDRAW")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let recs = ApiWithdrawDao::find_candidates_for_acct_change_hash_backfill(
            pool.as_ref(),
            "tron",
            "WFrom2",
            "WTo2",
            Some("WToken2"),
            "USDT",
            10,
        )
        .await
        .unwrap();

        assert!(recs.is_empty());
    }

    #[tokio::test]
    async fn backfill_tx_hash_if_missing_updates_when_chain_success_exists() {
        let dir = make_temp_dir("wallet_db_api_withdraw_backfill_hash_ok");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "1",
            "v",
            "tron",
            None,
            "TRX",
            "W_BACKFILL_TX_HASH_OK",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET chain_success_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 tx_hash = ''
             WHERE trade_no = ?",
        )
        .bind("W_BACKFILL_TX_HASH_OK")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let rows = ApiWithdrawDao::backfill_tx_hash_if_missing(
            pool.as_ref(),
            "W_BACKFILL_TX_HASH_OK",
            "0xwithdrawhash",
        )
        .await
        .unwrap();
        assert_eq!(rows, 1);
    }

    #[tokio::test]
    async fn backfill_tx_hash_if_missing_requires_execution_evidence() {
        let dir = make_temp_dir("wallet_db_api_withdraw_backfill_hash_needs_fact");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "1",
            "v",
            "tron",
            None,
            "TRX",
            "W_BACKFILL_TX_HASH_NEEDS_FACT",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query("UPDATE api_withdraws SET tx_hash = '' WHERE trade_no = ?")
            .bind("W_BACKFILL_TX_HASH_NEEDS_FACT")
            .execute(pool.as_ref())
            .await
            .unwrap();

        let rows = ApiWithdrawDao::backfill_tx_hash_if_missing(
            pool.as_ref(),
            "W_BACKFILL_TX_HASH_NEEDS_FACT",
            "0xwithdrawhash",
        )
        .await
        .unwrap();
        assert_eq!(rows, 0);
    }

    #[tokio::test]
    async fn backfill_tx_hash_if_missing_does_not_override_existing_non_empty_hash() {
        let dir = make_temp_dir("wallet_db_api_withdraw_backfill_hash_no_override");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "1",
            "v",
            "tron",
            None,
            "TRX",
            "W_BACKFILL_TX_HASH_NO_OVERRIDE",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 tx_hash = '0xexisting'
             WHERE trade_no = ?",
        )
        .bind("W_BACKFILL_TX_HASH_NO_OVERRIDE")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let rows = ApiWithdrawDao::backfill_tx_hash_if_missing(
            pool.as_ref(),
            "W_BACKFILL_TX_HASH_NO_OVERRIDE",
            "0xnewhash",
        )
        .await
        .unwrap();
        assert_eq!(rows, 0);
    }

    #[tokio::test]
    async fn mark_chain_finished_requires_terminal_evidence() {
        let dir = make_temp_dir("wallet_db_api_withdraw_mark_chain_finished_gate");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "1",
            "v",
            "sol",
            None,
            "USDC",
            "W_FINISH_GATE",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        let rows =
            ApiWithdrawDao::mark_chain_finished(pool.as_ref(), "W_FINISH_GATE").await.unwrap();
        assert_eq!(rows, 0, "no evidence should not finish");

        sqlx::query(
            "UPDATE api_withdraws
             SET chain_failed_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_FINISH_GATE")
        .execute(pool.as_ref())
        .await
        .unwrap();
        let rows =
            ApiWithdrawDao::mark_chain_finished(pool.as_ref(), "W_FINISH_GATE").await.unwrap();
        assert_eq!(rows, 0, "failure evidence without uploaded receipt should not finish");

        sqlx::query(
            "UPDATE api_withdraws
             SET tx_exec_receipt_uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_FINISH_GATE")
        .execute(pool.as_ref())
        .await
        .unwrap();
        let rows =
            ApiWithdrawDao::mark_chain_finished(pool.as_ref(), "W_FINISH_GATE").await.unwrap();
        assert_eq!(rows, 1, "failure evidence + uploaded receipt can finish");
    }

    #[tokio::test]
    async fn scan_can_broadcast_blocks_confirmed_withdraw_rows() {
        let dir = make_temp_dir("wallet_db_api_withdraw_scan_can_broadcast_confirmed");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "1",
            "v",
            "tron",
            None,
            "TRX",
            "W_CAN_BROADCAST_CONFIRMED_BLOCKED",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::SendingTx,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 raw_tx = '{\"tx\":true}',
                 tx_hash = '0xplatform_cancelled_before_broadcast',
                 transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 chain_failed_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_CAN_BROADCAST_CONFIRMED_BLOCKED")
        .execute(pool.as_ref())
        .await
        .unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "1",
            "v",
            "tron",
            None,
            "TRX",
            "W_CAN_BROADCAST_PENDING_READY",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::SendingTx,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 raw_tx = '{\"tx\":true}',
                 tx_hash = '0xpending_ready',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_CAN_BROADCAST_PENDING_READY")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let rows = ApiWithdrawRepo::scan_can_broadcast(&pool, 10).await.unwrap();
        let trade_nos: Vec<_> = rows.into_iter().map(|r| r.trade_no).collect();

        assert!(!trade_nos.contains(&"W_CAN_BROADCAST_CONFIRMED_BLOCKED".to_string()));
        assert!(trade_nos.contains(&"W_CAN_BROADCAST_PENDING_READY".to_string()));
    }

    #[tokio::test]
    async fn confirm_onchain_transaction_fact_clears_stale_error_fields() {
        let dir = make_temp_dir("wallet_db_api_withdraw_confirm_clears_stale_error");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "1",
            "v",
            "sol",
            None,
            "USDC",
            "W_CONFIRM_CLEAR_ERR",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        sqlx::query(
            "UPDATE api_withdraws
             SET err_code = 6004,
                 err_msg = 'old err',
                 chain_failed_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_CONFIRM_CLEAR_ERR")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let now = chrono::Utc::now().to_rfc3339();
        let rows = ApiWithdrawDao::confirm_onchain_transaction_fact(
            pool.as_ref(),
            "W_CONFIRM_CLEAR_ERR",
            "withdraw_hash",
            &now,
            "0.00001",
            "0",
        )
        .await
        .unwrap();
        assert_eq!(rows, 1);

        let after = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            "W_CONFIRM_CLEAR_ERR",
            ApiTradeType::Withdraw,
        )
        .await
        .unwrap();
        assert!(after.err_code.is_none());
        assert!(after.err_msg.is_none());
        assert!(after.chain_failed_at.is_none());
    }

    #[tokio::test]
    async fn recover_and_late_broadcast_cannot_overwrite_confirmed_fact() {
        let dir = make_temp_dir("wallet_db_api_withdraw_fact_cas");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid",
            "n",
            "from",
            "to",
            "1",
            "v",
            "tron",
            None,
            "TRX",
            "W_FACT_CAS",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        let now = chrono::Utc::now().to_rfc3339();
        let rows = ApiWithdrawDao::confirm_onchain_transaction_fact_with_recover(
            pool.as_ref(),
            "W_FACT_CAS",
            "hash-first",
            &now,
            &now,
            "0.00001",
            "0",
        )
        .await
        .unwrap();
        assert_eq!(rows, 1);

        let rows = ApiWithdrawDao::confirm_onchain_transaction_fact_with_recover(
            pool.as_ref(),
            "W_FACT_CAS",
            "hash-conflict",
            &now,
            &now,
            "0.00002",
            "1",
        )
        .await
        .unwrap();
        assert_eq!(rows, 0, "a conflicting recover hash must not overwrite facts");

        let rows =
            ApiWithdrawDao::mark_broadcast_executed(pool.as_ref(), "W_FACT_CAS").await.unwrap();
        assert_eq!(rows, 0, "late broadcast must not clear confirmed facts");
    }

    #[tokio::test]
    async fn bill_lists_filters_by_addr_on_from_or_to() {
        let dir = make_temp_dir("wallet_db_api_withdraw_bill_lists_addr_filter");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_bill_addr",
            "n",
            "ADDR_A",
            "TO_1",
            "1",
            "v",
            "tron",
            None,
            "TRX",
            "W_BILL_ADDR_FROM",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::SendingTx,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_bill_addr",
            "n",
            "FROM_2",
            "ADDR_A",
            "1",
            "v",
            "tron",
            None,
            "TRX",
            "W_BILL_ADDR_TO",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::SendingTx,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_bill_addr",
            "n",
            "FROM_X",
            "TO_X",
            "1",
            "v",
            "tron",
            None,
            "TRX",
            "W_BILL_ADDR_OTHER",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::SendingTx,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();

        let res = ApiWithdrawDao::bill_lists(
            pool.as_ref(),
            "uid_bill_addr",
            &["ADDR_A".to_string()],
            Some("tron"),
            Some("TRX"),
            None,
            None,
            None,
            None,
            vec![ApiTradeType::Withdraw as i32],
            0,
            50,
        )
        .await
        .unwrap();

        let trade_nos = res.data.iter().map(|v| v.trade_no.clone()).collect::<Vec<_>>();
        assert_eq!(res.total_count, 2);
        assert!(trade_nos.contains(&"W_BILL_ADDR_FROM".to_string()));
        assert!(trade_nos.contains(&"W_BILL_ADDR_TO".to_string()));
        assert!(!trade_nos.contains(&"W_BILL_ADDR_OTHER".to_string()));
    }

    #[tokio::test]
    async fn scan_can_build_rejects_committed_or_finished_rows() {
        let dir = make_temp_dir("wallet_db_api_withdraw_scan_can_build_terminal_guard");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_guard",
            "withdraw",
            "FROM_READY",
            "TO_READY",
            "1",
            "validate",
            "tron",
            AssetTokenKey::Native,
            "TRX",
            "W_CAN_BUILD_READY",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::SendingTx,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_withdraws
             SET tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 audit_passed_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 resource_gate_released_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_CAN_BUILD_READY")
        .execute(pool.as_ref())
        .await
        .unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_guard",
            "withdraw",
            "FROM_RECOVERED",
            "TO_RECOVERED",
            "1",
            "validate",
            "tron",
            AssetTokenKey::Native,
            "TRX",
            "W_CAN_BUILD_RECOVERED",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::SendingTx,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_withdraws
             SET tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 audit_passed_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_CAN_BUILD_RECOVERED")
        .execute(pool.as_ref())
        .await
        .unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_guard",
            "withdraw",
            "FROM_FINISHED",
            "TO_FINISHED",
            "1",
            "validate",
            "tron",
            AssetTokenKey::Native,
            "TRX",
            "W_CAN_BUILD_FINISHED",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::SendingTx,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_withdraws
             SET tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 audit_passed_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 finished_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_CAN_BUILD_FINISHED")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let rows = ApiWithdrawDao::scan_can_build(pool.as_ref(), 10).await.unwrap();
        let trade_nos: Vec<_> = rows.into_iter().map(|r| r.trade_no).collect();

        assert!(trade_nos.contains(&"W_CAN_BUILD_READY".to_string()));
        assert!(!trade_nos.contains(&"W_CAN_BUILD_RECOVERED".to_string()));
        assert!(!trade_nos.contains(&"W_CAN_BUILD_FINISHED".to_string()));
    }

    #[tokio::test]
    async fn withdraw_build_failure_marks_error_and_blocks_can_build_scan() {
        let dir = make_temp_dir("wallet_db_api_withdraw_build_failure");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_build_failure",
            "withdraw",
            "FROM_BUILD_FAILURE",
            "TO_BUILD_FAILURE",
            "5",
            "validate",
            "tron",
            AssetTokenKey::Native,
            "TRX",
            "W_BUILD_FAILURE",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::SendingTx,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_withdraws
             SET tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 audit_passed_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 resource_gate_released_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 building_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_BUILD_FAILURE")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let before = ApiWithdrawDao::scan_can_build(pool.as_ref(), 10).await.unwrap();
        assert!(before.iter().any(|row| row.trade_no == "W_BUILD_FAILURE"));

        let rows = ApiWithdrawRepo::mark_build_failed_if_unfinished(
            &pool,
            "W_BUILD_FAILURE",
            ErrCode::BalanceInsufficient,
            "withdraw amount is insufficient",
        )
        .await
        .unwrap();
        assert_eq!(rows, 1);

        let after = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            "W_BUILD_FAILURE",
            ApiTradeType::Withdraw,
        )
        .await
        .unwrap();
        assert_eq!(after.err_code, Some(ErrCode::BalanceInsufficient));
        assert_eq!(after.failure_stage, Some(WithdrawFailureStage::Build));
        assert_eq!(after.status, ApiWithdrawStatus::SendingTxFailed);
        assert!(after.building_at.is_none());

        let scan_after = ApiWithdrawDao::scan_can_build(pool.as_ref(), 10).await.unwrap();
        assert!(!scan_after.iter().any(|row| row.trade_no == "W_BUILD_FAILURE"));

        let second = ApiWithdrawRepo::mark_build_failed_if_unfinished(
            &pool,
            "W_BUILD_FAILURE",
            ErrCode::BalanceInsufficient,
            "withdraw amount is insufficient",
        )
        .await
        .unwrap();
        assert_eq!(second, 0);
    }

    #[tokio::test]
    async fn withdraw_build_failure_blocks_resource_gate_scan() {
        let dir = make_temp_dir("wallet_db_api_withdraw_resource_gate_build_failure");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_resource_gate_failure",
            "withdraw",
            "FROM_RESOURCE_GATE_FAILURE",
            "TO_RESOURCE_GATE_FAILURE",
            "5",
            "validate",
            "tron",
            AssetTokenKey::Native,
            "TRX",
            "W_RESOURCE_GATE_FAILURE",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::SendingTx,
            "0",
            "0",
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE api_withdraws
             SET tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 audit_passed_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE trade_no = ?",
        )
        .bind("W_RESOURCE_GATE_FAILURE")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let before = ApiWithdrawDao::scan_need_resource_gate(pool.as_ref(), 10).await.unwrap();
        assert!(before.iter().any(|row| row.trade_no == "W_RESOURCE_GATE_FAILURE"));

        let rows = ApiWithdrawRepo::mark_build_failed_if_unfinished(
            &pool,
            "W_RESOURCE_GATE_FAILURE",
            ErrCode::BalanceInsufficient,
            "token balance is insufficient for tron estimate fee",
        )
        .await
        .unwrap();
        assert_eq!(rows, 1);

        let scan_after = ApiWithdrawDao::scan_need_resource_gate(pool.as_ref(), 10).await.unwrap();
        assert!(!scan_after.iter().any(|row| row.trade_no == "W_RESOURCE_GATE_FAILURE"));
    }

    #[tokio::test]
    async fn batch_fail_pending_by_uid_releases_pending_energy() {
        let dir = make_temp_dir("wallet_db_api_withdraw_batch_fail_pending");
        let ctx = SqliteContext::new(&dir, Some("api_transaction.db")).await.unwrap();
        let pool = ctx.into_transaction_db_pool().unwrap();

        let uid = "uid_batch_fail";
        let from_addr = "FROM_BATCH_FAIL";
        let chain_code = "tron";

        // Seed 3 pending withdraw orders with the same uid + from_addr
        for i in 0..3u32 {
            let trade_no = format!("W_BATCH_FAIL_{i}");
            ApiWithdrawRepo::upsert_api_withdraw(
                &pool,
                uid,
                "withdraw",
                from_addr,
                "TO_BATCH_FAIL",
                "1",
                "validate",
                chain_code,
                AssetTokenKey::Native,
                "TRX",
                &trade_no,
                None,
                None,
                None,
                ApiTradeType::Withdraw,
                0,
                None,
                ApiWithdrawStatus::Init,
                ApiWithdrawStatus::InitOrder,
                "0",
                "0",
                None,
                None,
            )
            .await
            .unwrap();

            // Set estimated_resource_consume to simulate pending energy reservation
            ApiWithdrawRepo::update_fee_estimate(
                &pool,
                &trade_no,
                "0.456",
                r#"{"energy":13532,"bandwidth":345}"#,
            )
            .await
            .unwrap();
        }

        // Verify pending budget is non-zero before batch fail
        let before = ApiWithdrawRepo::pending_resource_budget_by_owner(
            &pool,
            chain_code,
            from_addr,
            "DUMMY_EXCLUDE",
        )
        .await
        .unwrap();
        assert_eq!(before.energy, 13532 * 3);
        assert_eq!(before.bandwidth, 345 * 3);

        // Batch fail all pending orders for this uid
        let affected = ApiWithdrawRepo::batch_fail_pending_by_uid(
            &pool,
            uid,
            ErrCode::SDKInternalError,
            "device takeover",
        )
        .await
        .unwrap();
        assert_eq!(affected, 3);

        // Verify pending budget is now 0
        let after = ApiWithdrawRepo::pending_resource_budget_by_owner(
            &pool,
            chain_code,
            from_addr,
            "DUMMY_EXCLUDE",
        )
        .await
        .unwrap();
        assert_eq!(after.energy, 0);
        assert_eq!(after.bandwidth, 0);

        // Verify each order has err_code set and status changed to failed terminal
        for i in 0..3u32 {
            let trade_no = format!("W_BATCH_FAIL_{i}");
            let row = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
                &pool,
                &trade_no,
                ApiTradeType::Withdraw,
            )
            .await
            .unwrap();
            assert_eq!(row.err_code, Some(ErrCode::SDKInternalError));
            assert_eq!(row.err_msg.as_deref(), Some("device takeover"));
            assert_eq!(row.status, ApiWithdrawStatus::SendingTxFailedReport);
            // finished_at must NOT be set (chain terminal fact constraint)
            assert!(row.finished_at.is_none());
        }
    }
}
