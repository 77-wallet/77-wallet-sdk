use crate::entities::api_resource_delegation::{
    ApiResourceDelegationEntity, ApiResourceDelegationOperationType,
    ApiResourceDelegationResultStatus, ApiResourceDelegationSource, ApiResourceDelegationStatus,
    NewApiResourceDelegation,
};
use sqlx::{Executor, Sqlite};

pub(crate) struct ApiResourceDelegationDao;

impl ApiResourceDelegationDao {
    pub async fn upsert<'a, E>(exec: E, input: NewApiResourceDelegation) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            INSERT INTO api_resource_delegation
                (uid, source, operation_type, origin_trade_no, origin_trade_type,
                 resource_trade_no, chain_code, owner_address, receiver_address,
                 delegation_mode, permission_id, resource_type, native_amount, amount,
                 created_at, updated_at)
            VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT(resource_trade_no) DO UPDATE SET
                uid = excluded.uid,
                source = excluded.source,
                operation_type = excluded.operation_type,
                origin_trade_no = excluded.origin_trade_no,
                origin_trade_type = excluded.origin_trade_type,
                chain_code = excluded.chain_code,
                owner_address = excluded.owner_address,
                receiver_address = excluded.receiver_address,
                delegation_mode = excluded.delegation_mode,
                permission_id = excluded.permission_id,
                resource_type = excluded.resource_type,
                native_amount = excluded.native_amount,
                amount = excluded.amount,
                updated_at = excluded.updated_at
        "#;

        sqlx::query(sql)
            .bind(input.uid)
            .bind(input.source.as_i64())
            .bind(input.operation_type.as_i64())
            .bind(input.origin_trade_no)
            .bind(input.origin_trade_type)
            .bind(input.resource_trade_no)
            .bind(input.chain_code)
            .bind(input.owner_address)
            .bind(input.receiver_address)
            .bind(input.delegation_mode.as_i64())
            .bind(input.permission_id)
            .bind(input.resource_type.as_i64())
            .bind(input.native_amount)
            .bind(input.amount)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

    pub async fn get_by_resource_trade_no<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<ApiResourceDelegationEntity, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            "SELECT * FROM api_resource_delegation WHERE resource_trade_no = ?",
        )
        .bind(resource_trade_no)
        .fetch_one(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_by_origin_trade_no<'a, E>(
        exec: E,
        origin_trade_no: &str,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            "SELECT * FROM api_resource_delegation WHERE origin_trade_no = ? ORDER BY id ASC",
        )
        .bind(origin_trade_no)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_local_delegates_missing_undelegation<'a, E>(
        exec: E,
        origin_trade_type: i64,
        after_id: Option<i64>,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT delegate.*
            FROM api_resource_delegation delegate
            WHERE delegate.source = ?1
              AND delegate.operation_type = ?2
              AND delegate.origin_trade_type = ?3
              AND delegate.origin_trade_no IS NOT NULL
              AND delegate.chain_failed_at IS NULL
              AND (
                    delegate.result_status = ?4
                    OR delegate.chain_success_at IS NOT NULL
                  )
              AND NOT EXISTS (
                    SELECT 1
                    FROM api_resource_delegation undelegate
                    WHERE undelegate.source = ?1
                      AND undelegate.operation_type = ?5
                      AND undelegate.origin_trade_no = delegate.origin_trade_no
                  )
              AND (?6 IS NULL OR delegate.id > ?6)
            ORDER BY delegate.id ASC
            LIMIT ?7
            "#,
        )
        .bind(ApiResourceDelegationSource::Local.as_i64())
        .bind(ApiResourceDelegationOperationType::Delegate.as_i64())
        .bind(origin_trade_type)
        .bind(ApiResourceDelegationResultStatus::Success.as_i64())
        .bind(ApiResourceDelegationOperationType::Undelegate.as_i64())
        .bind(after_id)
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn mark_task_ack_sent<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET task_ack_sent_at = COALESCE(task_ack_sent_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND task_ack_sent_at IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_task_ack_attempted<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET task_ack_send_count = task_ack_send_count + 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND task_ack_sent_at IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn scan_need_task_ack_by_origin_type<'a, E>(
        exec: E,
        origin_trade_type: i64,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = 1
              AND operation_type = 1
              AND origin_trade_type = ?
              AND task_ack_sent_at IS NULL
            ORDER BY created_at ASC
            LIMIT ?
            "#,
        )
        .bind(origin_trade_type)
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_need_task_ack_by_origin_type_source_and_operation<'a, E>(
        exec: E,
        origin_trade_type: i64,
        source: ApiResourceDelegationSource,
        operation_type: ApiResourceDelegationOperationType,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = ?
              AND operation_type = ?
              AND origin_trade_type = ?
              AND task_ack_sent_at IS NULL
            ORDER BY created_at ASC, id ASC
            LIMIT ?
            "#,
        )
        .bind(source.as_i64())
        .bind(operation_type.as_i64())
        .bind(origin_trade_type)
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_need_result_ack_by_origin_type<'a, E>(
        exec: E,
        origin_trade_type: i64,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = 1
              AND operation_type = 1
              AND origin_trade_type = ?
              AND origin_trade_no = resource_trade_no
              AND finished_at IS NOT NULL
              AND result_payload IS NOT NULL
              AND tx_res_received_at IS NOT NULL
              AND result_ack_sent_at IS NULL
            ORDER BY finished_at ASC
            LIMIT ?
            "#,
        )
        .bind(origin_trade_type)
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn find_pending_result_ack_by_origin<'a, E>(
        exec: E,
        origin_trade_type: i64,
        origin_trade_no: &str,
    ) -> Result<Option<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = 1
              AND operation_type = 1
              AND origin_trade_type = ?
              AND origin_trade_no = ?
              AND finished_at IS NOT NULL
              AND result_payload IS NOT NULL
              AND tx_res_received_at IS NOT NULL
              AND result_ack_sent_at IS NULL
            ORDER BY finished_at ASC, id ASC
            LIMIT 1
            "#,
        )
        .bind(origin_trade_type)
        .bind(origin_trade_no)
        .fetch_optional(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_need_result_ack_by_source_and_operation<'a, E>(
        exec: E,
        source: ApiResourceDelegationSource,
        operation_type: ApiResourceDelegationOperationType,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = ?
              AND operation_type = ?
              -- Original-order projections are ACKed by the collect/withdraw
              -- flow. They are not SDK-executed platform resource tasks.
              AND (origin_trade_no IS NULL OR origin_trade_no <> resource_trade_no)
              AND finished_at IS NOT NULL
              AND result_payload IS NOT NULL
              AND tx_res_received_at IS NOT NULL
              AND result_ack_sent_at IS NULL
            ORDER BY finished_at ASC
            LIMIT ?
            "#,
        )
        .bind(source.as_i64())
        .bind(operation_type.as_i64())
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_can_execute_by_origin_type<'a, E>(
        exec: E,
        origin_trade_type: i64,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = 1
              AND operation_type = 1
              AND origin_trade_type = ?
              AND task_ack_sent_at IS NOT NULL
              AND finished_at IS NULL
              AND err_code IS NULL
              AND building_at IS NULL
              AND last_broadcast_at IS NULL
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
            ORDER BY task_ack_sent_at ASC
            LIMIT ?
            "#,
        )
        .bind(origin_trade_type)
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_can_execute_by_origin_type_and_source<'a, E>(
        exec: E,
        origin_trade_type: i64,
        source: ApiResourceDelegationSource,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let (ack_clause, order_expr) = match source {
            ApiResourceDelegationSource::Platform => {
                ("AND task_ack_sent_at IS NOT NULL", "task_ack_sent_at")
            }
            ApiResourceDelegationSource::Local => ("", "created_at"),
        };
        let sql = format!(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = ?
              AND operation_type = 1
              AND origin_trade_type = ?
              {ack_clause}
              AND finished_at IS NULL
              AND err_code IS NULL
              AND building_at IS NULL
              AND last_broadcast_at IS NULL
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
            ORDER BY {order_expr} ASC
            LIMIT ?
            "#
        );
        sqlx::query_as::<_, ApiResourceDelegationEntity>(&sql)
            .bind(source.as_i64())
            .bind(origin_trade_type)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn find_by_resource_trade_no<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<Option<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            "SELECT * FROM api_resource_delegation WHERE resource_trade_no = ?",
        )
        .bind(resource_trade_no)
        .fetch_optional(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_can_execute_by_origin_type_source_and_operation<'a, E>(
        exec: E,
        origin_trade_type: i64,
        source: ApiResourceDelegationSource,
        operation_type: ApiResourceDelegationOperationType,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let (ack_clause, order_expr) = match source {
            ApiResourceDelegationSource::Platform => {
                ("AND task_ack_sent_at IS NOT NULL", "task_ack_sent_at")
            }
            ApiResourceDelegationSource::Local => ("", "created_at"),
        };
        let sql = format!(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = ?
              AND operation_type = ?
              AND origin_trade_type = ?
              AND finished_at IS NULL
              AND err_code IS NULL
              {ack_clause}
              AND (
                    building_at IS NULL
                    OR datetime(building_at) <= datetime('now', '-5 minutes')
                  )
              AND last_broadcast_at IS NULL
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
            ORDER BY {order_expr} ASC, id ASC
            LIMIT ?
            "#
        );
        sqlx::query_as::<_, ApiResourceDelegationEntity>(&sql)
            .bind(source.as_i64())
            .bind(operation_type.as_i64())
            .bind(origin_trade_type)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_failed_by_origin_type_source_and_operation<'a, E>(
        exec: E,
        origin_trade_type: i64,
        source: ApiResourceDelegationSource,
        operation_type: ApiResourceDelegationOperationType,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = ?
              AND operation_type = ?
              AND origin_trade_type = ?
              AND err_code IS NOT NULL
            ORDER BY COALESCE(updated_at, created_at) DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(source.as_i64())
        .bind(operation_type.as_i64())
        .bind(origin_trade_type)
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_can_recover_local_undelegation_by_origin_type<'a, E>(
        exec: E,
        origin_trade_type: i64,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = 2
              AND operation_type = 2
              AND origin_trade_type = ?
              AND finished_at IS NULL
              AND err_code IS NULL
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND last_broadcast_at IS NOT NULL
            ORDER BY
                CASE
                    WHEN broadcast_uncertain_last_checked_at IS NULL THEN 0
                    ELSE 1
                END ASC,
                COALESCE(broadcast_uncertain_last_checked_at, created_at) ASC,
                id ASC
            LIMIT ?
            "#,
        )
        .bind(origin_trade_type)
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_can_recover_by_origin_type_source_and_operation<'a, E>(
        exec: E,
        origin_trade_type: i64,
        source: ApiResourceDelegationSource,
        operation_type: ApiResourceDelegationOperationType,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let (ack_clause, order_clause) = match source {
            ApiResourceDelegationSource::Platform => {
                ("AND task_ack_sent_at IS NOT NULL", "task_ack_sent_at ASC, id ASC")
            }
            ApiResourceDelegationSource::Local => (
                "",
                r#"CASE
                    WHEN broadcast_uncertain_last_checked_at IS NULL THEN 0
                    ELSE 1
                END ASC,
                COALESCE(broadcast_uncertain_last_checked_at, created_at) ASC,
                id ASC"#,
            ),
        };
        let sql = format!(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = ?
              AND operation_type = ?
              AND origin_trade_type = ?
              AND finished_at IS NULL
              AND err_code IS NULL
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND last_broadcast_at IS NOT NULL
              {ack_clause}
            ORDER BY {order_clause}
            LIMIT ?
            "#
        );
        sqlx::query_as::<_, ApiResourceDelegationEntity>(&sql)
            .bind(source.as_i64())
            .bind(operation_type.as_i64())
            .bind(origin_trade_type)
            .bind(limit as i64)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_unreconciled_terminal_broadcasts_for_recovery<'a, E>(
        exec: E,
        origin_trade_type: i64,
        operation_type: ApiResourceDelegationOperationType,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = ?1
              AND operation_type = ?2
              AND origin_trade_type = ?3
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND last_broadcast_at IS NOT NULL
              AND chain_success_at IS NULL
              AND (
                    chain_failed_at IS NOT NULL
                    OR finished_at IS NOT NULL
                    OR err_code IS NOT NULL
                    OR result_status = ?4
                  )
              AND COALESCE(result_payload, '') NOT LIKE 'historical_chain_reconciliation_confirmed_failure:%'
            ORDER BY updated_at ASC, id ASC
            LIMIT ?5
            "#,
        )
        .bind(ApiResourceDelegationSource::Local.as_i64())
        .bind(operation_type.as_i64())
        .bind(origin_trade_type)
        .bind(ApiResourceDelegationResultStatus::Fail.as_i64())
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn reopen_terminal_broadcast_for_reconciliation<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let result = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET chain_failed_at = NULL,
                result_status = NULL,
                finished_at = NULL,
                result_payload = 'historical_chain_reconciliation_pending',
                fail_type = NULL,
                err_code = NULL,
                err_msg = NULL,
                status = ?2,
                broadcast_uncertain_since_at = COALESCE(
                    broadcast_uncertain_since_at,
                    strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                ),
                broadcast_uncertain_last_checked_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND source = ?3
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND last_broadcast_at IS NOT NULL
              AND chain_success_at IS NULL
              AND (
                    chain_failed_at IS NOT NULL
                    OR finished_at IS NOT NULL
                    OR err_code IS NOT NULL
                    OR result_status = ?4
                  )
              AND COALESCE(result_payload, '') NOT LIKE 'historical_chain_reconciliation_confirmed_failure:%'
            "#,
        )
        .bind(resource_trade_no)
        .bind(ApiResourceDelegationStatus::Pending.as_i64())
        .bind(ApiResourceDelegationSource::Local.as_i64())
        .bind(ApiResourceDelegationResultStatus::Fail.as_i64())
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result.rows_affected())
    }

    pub async fn claim_build_slot<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET building_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND (
                    source = 2
                    OR task_ack_sent_at IS NOT NULL
              )
              AND finished_at IS NULL
              AND err_code IS NULL
              AND (
                    building_at IS NULL
                    OR datetime(building_at) <= datetime('now', '-5 minutes')
                  )
              AND tx_hash IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_broadcast_pending<'a, E>(
        exec: E,
        resource_trade_no: &str,
        tx_hash: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET tx_hash = ?2,
                building_at = NULL,
                last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND building_at IS NOT NULL
              AND tx_hash IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .bind(tx_hash)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_built<'a, E>(
        exec: E,
        resource_trade_no: &str,
        tx_hash: &str,
        raw_tx: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET tx_hash = ?2,
                raw_tx = ?3,
                building_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND building_at IS NOT NULL
              AND tx_hash IS NULL
              AND raw_tx IS NULL
              AND last_broadcast_at IS NULL
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .bind(tx_hash)
        .bind(raw_tx)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_broadcasted<'a, E>(
        exec: E,
        resource_trade_no: &str,
        tx_hash: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                building_at = NULL,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND tx_hash = ?2
              AND raw_tx IS NOT NULL
              AND trim(raw_tx) <> ''
              AND last_broadcast_at IS NULL
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .bind(tx_hash)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn reconcile_broadcast_pending<'a, E>(
        exec: E,
        resource_trade_no: &str,
        tx_hash: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET tx_hash = ?2,
                building_at = NULL,
                last_broadcast_at = COALESCE(last_broadcast_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND tx_hash IS NULL
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .bind(tx_hash)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_chain_success<'a, E>(
        exec: E,
        resource_trade_no: &str,
        transaction_time: Option<&str>,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET chain_success_at = COALESCE(chain_success_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                transaction_time = COALESCE(transaction_time, ?2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                building_at = NULL,
                broadcast_uncertain_reconciled_at = CASE
                    WHEN broadcast_uncertain_since_at IS NOT NULL
                    THEN COALESCE(broadcast_uncertain_reconciled_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                    ELSE broadcast_uncertain_reconciled_at
                END,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND finished_at IS NULL
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .bind(transaction_time)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn scan_need_tx_exec_receipt_upload_by_origin_type<'a, E>(
        exec: E,
        origin_trade_type: i64,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = 1
              AND operation_type = 1
              AND origin_trade_type = ?
              AND tx_exec_receipt_uploaded_at IS NULL
              AND (
                    (
                      chain_success_at IS NOT NULL
                      AND tx_hash IS NOT NULL
                      AND trim(tx_hash) <> ''
                    )
                    OR err_code IS NOT NULL
                  )
            ORDER BY updated_at ASC, id ASC
            LIMIT ?
            "#,
        )
        .bind(origin_trade_type)
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_need_tx_exec_receipt_upload_by_origin_type_source_and_operation<'a, E>(
        exec: E,
        origin_trade_type: i64,
        source: ApiResourceDelegationSource,
        operation_type: ApiResourceDelegationOperationType,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = ?
              AND operation_type = ?
              AND origin_trade_type = ?
              AND tx_exec_receipt_uploaded_at IS NULL
              AND (
                    (
                      chain_success_at IS NOT NULL
                      AND tx_hash IS NOT NULL
                      AND trim(tx_hash) <> ''
                    )
                    OR err_code IS NOT NULL
                  )
            ORDER BY updated_at ASC, id ASC
            LIMIT ?
            "#,
        )
        .bind(source.as_i64())
        .bind(operation_type.as_i64())
        .bind(origin_trade_type)
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_need_tx_exec_receipt_upload_by_source_and_operation<'a, E>(
        exec: E,
        source: ApiResourceDelegationSource,
        operation_type: ApiResourceDelegationOperationType,
        limit: usize,
    ) -> Result<Vec<ApiResourceDelegationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceDelegationEntity>(
            r#"
            SELECT * FROM api_resource_delegation
            WHERE source = ?
              AND operation_type = ?
              AND tx_exec_receipt_uploaded_at IS NULL
              AND (
                    (
                      chain_success_at IS NOT NULL
                      AND tx_hash IS NOT NULL
                      AND trim(tx_hash) <> ''
                    )
                    OR err_code IS NOT NULL
                  )
            ORDER BY updated_at ASC, id ASC
            LIMIT ?
            "#,
        )
        .bind(source.as_i64())
        .bind(operation_type.as_i64())
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn mark_tx_exec_receipt_uploaded<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET tx_exec_receipt_uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND source = 1
              AND operation_type = 1
              AND tx_exec_receipt_uploaded_at IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_tx_exec_receipt_uploaded_by_source_and_operation<'a, E>(
        exec: E,
        resource_trade_no: &str,
        source: ApiResourceDelegationSource,
        operation_type: ApiResourceDelegationOperationType,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET tx_exec_receipt_uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND source = ?
              AND operation_type = ?
              AND tx_exec_receipt_uploaded_at IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .bind(source.as_i64())
        .bind(operation_type.as_i64())
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_broadcast_uncertain_attempt<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET broadcast_uncertain_since_at = COALESCE(broadcast_uncertain_since_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                broadcast_uncertain_retry_count = broadcast_uncertain_retry_count + 1,
                broadcast_uncertain_last_checked_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_broadcast_uncertain_rebroadcast_attempted<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET broadcast_uncertain_rebroadcast_count = broadcast_uncertain_rebroadcast_count + 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_broadcast_uncertain_reconciled<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET broadcast_uncertain_reconciled_at = COALESCE(broadcast_uncertain_reconciled_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
              AND finished_at IS NULL
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_failed_if_unfinished<'a, E>(
        exec: E,
        resource_trade_no: &str,
        err_code: &str,
        err_msg: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET err_code = ?2,
                err_msg = ?3,
                chain_failed_at = COALESCE(chain_failed_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                finished_at = COALESCE(finished_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                broadcast_uncertain_reconciled_at = CASE
                    WHEN broadcast_uncertain_since_at IS NOT NULL
                    THEN COALESCE(broadcast_uncertain_reconciled_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                    ELSE broadcast_uncertain_reconciled_at
                END,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                status = 3,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND finished_at IS NULL
              AND err_code IS NULL
              AND tx_hash IS NULL
              AND last_broadcast_at IS NULL
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .bind(err_code)
        .bind(err_msg)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_failed_with_result_if_unfinished<'a, E>(
        exec: E,
        resource_trade_no: &str,
        fail_type: Option<i64>,
        err_code: &str,
        err_msg: &str,
        result_payload: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET err_code = ?3,
                err_msg = ?4,
                finished_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                result_status = ?6,
                result_payload = ?5,
                fail_type = ?2,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                status = ?7,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND finished_at IS NULL
              AND err_code IS NULL
              AND tx_hash IS NULL
              AND raw_tx IS NULL
              AND last_broadcast_at IS NULL
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .bind(fail_type)
        .bind(err_code)
        .bind(err_msg)
        .bind(result_payload)
        .bind(ApiResourceDelegationResultStatus::Fail.as_i64())
        .bind(ApiResourceDelegationStatus::Fail.as_i64())
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_recovered_chain_failure<'a, E>(
        exec: E,
        resource_trade_no: &str,
        tx_hash: &str,
        fail_type: Option<i64>,
        err_code: &str,
        err_msg: &str,
        result_payload: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET err_code = ?4,
                err_msg = ?5,
                chain_failed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                finished_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                result_status = ?7,
                result_payload = ?6,
                fail_type = ?3,
                broadcast_uncertain_reconciled_at = CASE
                    WHEN broadcast_uncertain_since_at IS NOT NULL
                    THEN strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                    ELSE broadcast_uncertain_reconciled_at
                END,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                status = ?8,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND tx_hash = ?2
              AND trim(tx_hash) <> ''
              AND last_broadcast_at IS NOT NULL
              AND finished_at IS NULL
              AND result_status IS NULL
              AND err_code IS NULL
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .bind(tx_hash)
        .bind(fail_type)
        .bind(err_code)
        .bind(err_msg)
        .bind(result_payload)
        .bind(ApiResourceDelegationResultStatus::Fail.as_i64())
        .bind(ApiResourceDelegationStatus::Fail.as_i64())
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_result_ack_attempted<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET result_ack_send_count = result_ack_send_count + 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND finished_at IS NOT NULL
              AND result_payload IS NOT NULL
              AND tx_res_received_at IS NOT NULL
              AND result_ack_sent_at IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_result_ack_sent<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET result_ack_sent_at = COALESCE(result_ack_sent_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND finished_at IS NOT NULL
              AND result_payload IS NOT NULL
              AND tx_res_received_at IS NOT NULL
              AND result_ack_sent_at IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn upsert_original_order_result_fact<'a, E>(
        exec: E,
        input: NewApiResourceDelegation,
        result_status: ApiResourceDelegationResultStatus,
        fail_type: Option<i64>,
        result_payload: Option<&str>,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let status = match result_status {
            ApiResourceDelegationResultStatus::Success => ApiResourceDelegationStatus::Success,
            ApiResourceDelegationResultStatus::Fail => ApiResourceDelegationStatus::Fail,
        };
        let res = sqlx::query(
            r#"
            INSERT INTO api_resource_delegation
                (uid, source, operation_type, origin_trade_no, origin_trade_type,
                 resource_trade_no, chain_code, owner_address, receiver_address,
                 delegation_mode, permission_id, resource_type, native_amount, amount,
                 status, task_ack_sent_at,
                 result_status, finished_at, tx_res_received_at, result_payload, fail_type,
                 created_at, updated_at)
            VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 ?15,
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                 ?16,
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                 ?17, ?18,
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT(resource_trade_no) DO UPDATE SET
                uid = excluded.uid,
                source = excluded.source,
                operation_type = excluded.operation_type,
                origin_trade_no = excluded.origin_trade_no,
                origin_trade_type = excluded.origin_trade_type,
                chain_code = excluded.chain_code,
                owner_address = excluded.owner_address,
                receiver_address = excluded.receiver_address,
                delegation_mode = excluded.delegation_mode,
                permission_id = excluded.permission_id,
                resource_type = excluded.resource_type,
                native_amount = excluded.native_amount,
                amount = excluded.amount,
                status = excluded.status,
                task_ack_sent_at = COALESCE(
                    api_resource_delegation.task_ack_sent_at,
                    excluded.task_ack_sent_at
                ),
                result_status = excluded.result_status,
                finished_at = COALESCE(
                    api_resource_delegation.finished_at,
                    excluded.finished_at
                ),
                tx_res_received_at = COALESCE(
                    api_resource_delegation.tx_res_received_at,
                    excluded.tx_res_received_at
                ),
                result_payload = COALESCE(excluded.result_payload, api_resource_delegation.result_payload),
                fail_type = excluded.fail_type,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(input.uid)
        .bind(input.source.as_i64())
        .bind(input.operation_type.as_i64())
        .bind(input.origin_trade_no)
        .bind(input.origin_trade_type)
        .bind(input.resource_trade_no)
        .bind(input.chain_code)
        .bind(input.owner_address)
        .bind(input.receiver_address)
        .bind(input.delegation_mode.as_i64())
        .bind(input.permission_id)
        .bind(input.resource_type.as_i64())
        .bind(input.native_amount)
        .bind(input.amount)
        .bind(status.as_i64())
        .bind(result_status.as_i64())
        .bind(result_payload)
        .bind(fail_type)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_result_received<'a, E>(
        exec: E,
        resource_trade_no: &str,
        result_status: ApiResourceDelegationResultStatus,
        fail_type: Option<i64>,
        err_code: Option<&str>,
        err_msg: Option<&str>,
        result_payload: Option<&str>,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET result_status = ?2,
                finished_at = COALESCE(finished_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                result_payload = COALESCE(?6, result_payload),
                fail_type = ?3,
                err_code = ?4,
                err_msg = ?5,
                status = ?7,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
            "#,
        )
        .bind(resource_trade_no)
        .bind(result_status.as_i64())
        .bind(fail_type)
        .bind(err_code)
        .bind(err_msg)
        .bind(result_payload)
        .bind(match result_status {
            ApiResourceDelegationResultStatus::Success => {
                ApiResourceDelegationStatus::Success.as_i64()
            }
            ApiResourceDelegationResultStatus::Fail => ApiResourceDelegationStatus::Fail.as_i64(),
        })
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_backend_result_received<'a, E>(
        exec: E,
        resource_trade_no: &str,
        result_status: ApiResourceDelegationResultStatus,
        fail_type: Option<i64>,
        err_code: Option<&str>,
        err_msg: Option<&str>,
        result_payload: Option<&str>,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET result_status = ?2,
                finished_at = COALESCE(finished_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                tx_res_received_at = COALESCE(tx_res_received_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                result_payload = COALESCE(?6, result_payload),
                fail_type = ?3,
                err_code = ?4,
                err_msg = ?5,
                status = ?7,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
            "#,
        )
        .bind(resource_trade_no)
        .bind(result_status.as_i64())
        .bind(fail_type)
        .bind(err_code)
        .bind(err_msg)
        .bind(result_payload)
        .bind(match result_status {
            ApiResourceDelegationResultStatus::Success => {
                ApiResourceDelegationStatus::Success.as_i64()
            }
            ApiResourceDelegationResultStatus::Fail => ApiResourceDelegationStatus::Fail.as_i64(),
        })
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn reset_for_rebuild<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET status = 1,
                building_at = NULL,
                tx_hash = NULL,
                raw_tx = NULL,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND operation_type IN (1, 2)
              AND finished_at IS NULL
              AND err_code IS NULL
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
              AND NOT (
                    raw_tx IS NOT NULL
                AND trim(raw_tx) <> ''
                AND tx_hash IS NOT NULL
                AND trim(tx_hash) <> ''
                AND last_broadcast_at IS NULL
              )
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn invalidate_for_rebroadcast<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_delegation
            SET status = 1,
                building_at = NULL,
                tx_hash = NULL,
                raw_tx = NULL,
                last_broadcast_at = NULL,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                broadcast_uncertain_rebroadcast_count = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND operation_type IN (1, 2)
              AND finished_at IS NULL
              AND err_code IS NULL
              AND chain_success_at IS NULL
              AND chain_failed_at IS NULL
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND last_broadcast_at IS NOT NULL
              AND broadcast_uncertain_since_at IS NOT NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }
}
