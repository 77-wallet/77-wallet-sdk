use crate::entities::api_resource_operation::{
    ApiResourceOperationEntity, NewApiResourceOperation,
};
use sqlx::{Executor, Sqlite};

pub(crate) struct ApiResourceOperationDao;

impl ApiResourceOperationDao {
    pub async fn upsert<'a, E>(exec: E, input: NewApiResourceOperation) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            INSERT INTO api_resource_operation
                (uid, task_source, operation_type, resource_trade_no, chain_code,
                 owner_address, receiver_address, resource_type, amount, created_at, updated_at)
            VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT(resource_trade_no) DO UPDATE SET
                uid = excluded.uid,
                task_source = excluded.task_source,
                operation_type = excluded.operation_type,
                chain_code = excluded.chain_code,
                owner_address = excluded.owner_address,
                receiver_address = excluded.receiver_address,
                resource_type = excluded.resource_type,
                amount = excluded.amount,
                updated_at = excluded.updated_at
        "#;

        sqlx::query(sql)
            .bind(input.uid)
            .bind(input.task_source.as_i64())
            .bind(input.operation_type.as_i64())
            .bind(input.resource_trade_no)
            .bind(input.chain_code)
            .bind(input.owner_address)
            .bind(input.receiver_address)
            .bind(input.resource_type.as_i64())
            .bind(input.amount)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

    pub async fn get_by_resource_trade_no<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<ApiResourceOperationEntity, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceOperationEntity>(
            "SELECT * FROM api_resource_operation WHERE resource_trade_no = ?",
        )
        .bind(resource_trade_no)
        .fetch_one(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_by_hash_and_owner<'a, E>(
        exec: E,
        tx_hash: &str,
        owner_address: &str,
    ) -> Result<Option<ApiResourceOperationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceOperationEntity>(
            r#"
            SELECT * FROM api_resource_operation
            WHERE tx_hash = ?
              AND owner_address = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(tx_hash)
        .bind(owner_address)
        .fetch_optional(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn record_client_broadcast_success<'a, E>(
        exec: E,
        input: NewApiResourceOperation,
        tx_hash: &str,
        raw_tx: &str,
        transaction_fee: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            INSERT INTO api_resource_operation
                (uid, task_source, operation_type, resource_trade_no, chain_code,
                 owner_address, receiver_address, resource_type, amount, raw_tx,
                 tx_hash, transaction_fee, last_broadcast_at,
                 created_at, updated_at)
            VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT(resource_trade_no) DO UPDATE SET
                uid = excluded.uid,
                task_source = excluded.task_source,
                operation_type = excluded.operation_type,
                chain_code = excluded.chain_code,
                owner_address = excluded.owner_address,
                receiver_address = excluded.receiver_address,
                resource_type = excluded.resource_type,
                amount = excluded.amount,
                raw_tx = COALESCE(api_resource_operation.raw_tx, excluded.raw_tx),
                tx_hash = COALESCE(api_resource_operation.tx_hash, excluded.tx_hash),
                transaction_fee = COALESCE(api_resource_operation.transaction_fee, excluded.transaction_fee),
                last_broadcast_at = COALESCE(api_resource_operation.last_broadcast_at, excluded.last_broadcast_at),
                updated_at = excluded.updated_at
        "#;

        sqlx::query(sql)
            .bind(input.uid)
            .bind(input.task_source.as_i64())
            .bind(input.operation_type.as_i64())
            .bind(input.resource_trade_no)
            .bind(input.chain_code)
            .bind(input.owner_address)
            .bind(input.receiver_address)
            .bind(input.resource_type.as_i64())
            .bind(input.amount)
            .bind(raw_tx)
            .bind(tx_hash)
            .bind(transaction_fee)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
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
            UPDATE api_resource_operation
            SET task_ack_sent_at = COALESCE(task_ack_sent_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn scan_need_task_ack<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceOperationEntity>(
            r#"
            SELECT * FROM api_resource_operation
            WHERE task_source = 1
              AND operation_type IN (1, 2)
              AND task_ack_sent_at IS NULL
            ORDER BY id ASC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_can_build<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceOperationEntity>(
            r#"
            SELECT * FROM api_resource_operation
            WHERE task_source = 1
              AND operation_type IN (1, 2)
              AND task_ack_sent_at IS NOT NULL
              AND building_at IS NULL
              AND raw_tx IS NULL
              AND err_code IS NULL
            ORDER BY id ASC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_can_broadcast<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceOperationEntity>(
            r#"
            SELECT * FROM api_resource_operation
            WHERE task_source = 1
              AND operation_type IN (1, 2)
              AND raw_tx IS NOT NULL
              AND trim(raw_tx) <> ''
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND last_broadcast_at IS NULL
              AND err_code IS NULL
            ORDER BY id ASC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_need_recover<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceOperationEntity>(
            r#"
            SELECT * FROM api_resource_operation
            WHERE task_source = 1
              AND operation_type IN (1, 2)
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND raw_tx IS NOT NULL
              AND trim(raw_tx) <> ''
              AND last_broadcast_at IS NOT NULL
              AND transaction_time IS NULL
              AND tx_exec_receipt_uploaded_at IS NULL
              AND err_code IS NULL
            ORDER BY id ASC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_need_tx_exec_receipt_upload<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceOperationEntity>(
            r#"
            SELECT * FROM api_resource_operation
            WHERE task_source = 1
              AND operation_type IN (1, 2)
              AND tx_exec_receipt_uploaded_at IS NULL
              AND (
                    transaction_time IS NOT NULL
                    OR err_code IS NOT NULL
                  )
            ORDER BY id ASC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_need_result_ack<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiResourceOperationEntity>(
            r#"
            SELECT * FROM api_resource_operation
            WHERE task_source = 1
              AND operation_type IN (1, 2)
              AND result_received_at IS NOT NULL
              AND result_ack_sent_at IS NULL
            ORDER BY result_received_at ASC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn claim_building_at<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET building_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND task_source = 1
              AND operation_type IN (1, 2)
              AND task_ack_sent_at IS NOT NULL
              AND building_at IS NULL
              AND raw_tx IS NULL
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Persist the irreversible build facts for a resource operation.
    ///
    /// raw_tx is the first executable chain fact. Once it exists, scanner must
    /// stop treating the operation as buildable and later broadcast/recover
    /// logic must use this stored payload instead of rebuilding implicitly.
    pub async fn update_after_build<'a, E>(
        exec: E,
        resource_trade_no: &str,
        tx_hash: &str,
        raw_tx: &str,
        transaction_fee: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET raw_tx = ?3,
                tx_hash = ?2,
                transaction_fee = ?4,
                building_at = COALESCE(building_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND task_ack_sent_at IS NOT NULL
              AND raw_tx IS NULL
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .bind(tx_hash)
        .bind(raw_tx)
        .bind(transaction_fee)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Release an in-flight build slot after a pre-raw_tx failure or early exit.
    pub async fn clear_building_at<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET building_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND raw_tx IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Mark a resource operation as broadcast to the chain node.
    ///
    /// This is only a broadcast fact. Chain confirmation must be written by a
    /// later recover/confirm step, never by this method.
    pub async fn mark_broadcast_executed<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET last_broadcast_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND task_source = 1
              AND operation_type IN (1, 2)
              AND raw_tx IS NOT NULL
              AND trim(raw_tx) <> ''
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND last_broadcast_at IS NULL
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Persist final on-chain confirmation time from chain query result.
    pub async fn confirm_transaction_time_if_absent<'a, E>(
        exec: E,
        resource_trade_no: &str,
        transaction_time: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET transaction_time = ?2,
                tx_status = COALESCE(tx_status, 'success'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND task_source = 1
              AND operation_type IN (1, 2)
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND last_broadcast_at IS NOT NULL
              AND transaction_time IS NULL
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

    /// Mark the backend execution receipt upload side effect as complete.
    pub async fn mark_tx_exec_receipt_uploaded<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET tx_exec_receipt_uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND task_source = 1
              AND operation_type IN (1, 2)
              AND tx_exec_receipt_uploaded_at IS NULL
              AND (
                    transaction_time IS NOT NULL
                    OR err_code IS NOT NULL
                  )
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Persist the backend final result push before MQTT message ACK.
    ///
    /// This fact only records that backend has delivered a result event. The
    /// later TxRes ACK remains a separate idempotent side effect.
    pub async fn mark_result_received<'a, E>(
        exec: E,
        resource_trade_no: &str,
        result_status: &str,
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
            UPDATE api_resource_operation
            SET result_status = ?2,
                result_received_at = COALESCE(result_received_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                fail_type = ?3,
                err_code = COALESCE(?4, err_code),
                err_msg = COALESCE(?5, err_msg),
                result_payload = COALESCE(?6, result_payload),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND task_source = 1
            "#,
        )
        .bind(resource_trade_no)
        .bind(result_status)
        .bind(fail_type)
        .bind(err_code)
        .bind(err_msg)
        .bind(result_payload)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Mark the backend TxRes ACK side effect as complete.
    pub async fn mark_result_ack_sent<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET result_ack_sent_at = COALESCE(result_ack_sent_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND task_source = 1
              AND result_received_at IS NOT NULL
              AND result_ack_sent_at IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Persist a terminal failure fact for backend resource operations.
    ///
    /// Failure must not overwrite a successful chain confirmation, and it must
    /// be written at most once so the original error remains auditable.
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
            UPDATE api_resource_operation
            SET err_code = ?2,
                err_msg = ?3,
                tx_status = 'fail',
                building_at = CASE WHEN raw_tx IS NULL THEN NULL ELSE building_at END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?1
              AND task_source = 1
              AND transaction_time IS NULL
              AND err_code IS NULL
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

    /// Mark a broadcast attempt as uncertain and increment retry count.
    /// This is used when broadcast returns uncertain result (None).
    pub async fn mark_broadcast_uncertain_attempt<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET broadcast_uncertain_since_at = COALESCE(broadcast_uncertain_since_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                broadcast_uncertain_retry_count = broadcast_uncertain_retry_count + 1,
                broadcast_uncertain_last_checked_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND task_source = 1
              AND raw_tx IS NOT NULL
              AND trim(raw_tx) <> ''
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    /// Invalidate raw_tx when transaction expires.
    /// This allows the operation to be rebuilt with fresh nonce/expiration.
    pub async fn invalidate_raw_tx<'a, E>(
        exec: E,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET raw_tx = NULL,
                tx_hash = NULL,
                transaction_fee = NULL,
                last_broadcast_at = NULL,
                building_at = NULL,
                broadcast_uncertain_since_at = NULL,
                broadcast_uncertain_retry_count = 0,
                broadcast_uncertain_last_checked_at = NULL,
                broadcast_uncertain_reconciled_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE resource_trade_no = ?
              AND task_source = 1
              AND err_code IS NULL
            "#,
        )
        .bind(resource_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }
}
