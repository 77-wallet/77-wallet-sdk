use crate::entities::api_tron_account_activation::{
    ApiTronAccountActivationEntity, ApiTronAccountActivationRecoverStatus,
    ApiTronAccountActivationStatus, NewApiTronAccountActivation,
};
use sqlx::{Executor, Sqlite};

pub(crate) struct ApiTronAccountActivationDao;

impl ApiTronAccountActivationDao {
    pub async fn upsert<'a, E>(
        exec: E,
        input: NewApiTronAccountActivation,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            INSERT INTO api_tron_account_activation
                (uid, origin_trade_no, origin_trade_type, activation_trade_no,
                 chain_code, payer_address, target_address, amount,
                 created_at, updated_at)
            VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT(origin_trade_no, origin_trade_type, target_address) DO UPDATE SET
                uid = excluded.uid,
                chain_code = excluded.chain_code,
                payer_address = excluded.payer_address,
                amount = excluded.amount,
                updated_at = excluded.updated_at
        "#;

        sqlx::query(sql)
            .bind(input.uid)
            .bind(input.origin_trade_no)
            .bind(input.origin_trade_type)
            .bind(input.activation_trade_no)
            .bind(input.chain_code)
            .bind(input.payer_address)
            .bind(input.target_address)
            .bind(input.amount)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

    pub async fn find_by_activation_trade_no<'a, E>(
        exec: E,
        activation_trade_no: &str,
    ) -> Result<Option<ApiTronAccountActivationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiTronAccountActivationEntity>(
            "SELECT * FROM api_tron_account_activation WHERE activation_trade_no = ?",
        )
        .bind(activation_trade_no)
        .fetch_optional(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn find_by_origin_target<'a, E>(
        exec: E,
        origin_trade_no: &str,
        origin_trade_type: i64,
        target_address: &str,
    ) -> Result<Option<ApiTronAccountActivationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiTronAccountActivationEntity>(
            r#"
            SELECT * FROM api_tron_account_activation
            WHERE origin_trade_no = ?
              AND origin_trade_type = ?
              AND target_address = ?
            "#,
        )
        .bind(origin_trade_no)
        .bind(origin_trade_type)
        .bind(target_address)
        .fetch_optional(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn mark_account_verified<'a, E>(
        exec: E,
        activation_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_tron_account_activation
            SET account_verified_at = COALESCE(account_verified_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                status = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE activation_trade_no = ?1
              AND account_verified_at IS NULL
            "#,
        )
        .bind(activation_trade_no)
        .bind(ApiTronAccountActivationStatus::Success.as_i64())
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn scan_can_execute<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiTronAccountActivationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiTronAccountActivationEntity>(
            r#"
            SELECT * FROM api_tron_account_activation
            WHERE account_verified_at IS NULL
              AND tx_hash IS NULL
              AND status IN (?1, ?2)
              AND (next_retry_at IS NULL OR next_retry_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
              AND (
                    building_at IS NULL
                    OR datetime(building_at) <= datetime('now', '-5 minutes')
                  )
            ORDER BY created_at ASC, id ASC
            LIMIT ?3
            "#,
        )
        .bind(ApiTronAccountActivationStatus::Pending.as_i64())
        .bind(ApiTronAccountActivationStatus::Fail.as_i64())
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn scan_can_verify<'a, E>(
        exec: E,
        limit: usize,
    ) -> Result<Vec<ApiTronAccountActivationEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, ApiTronAccountActivationEntity>(
            r#"
            SELECT * FROM api_tron_account_activation
            WHERE account_verified_at IS NULL
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND status IN (?1, ?2)
              AND (next_retry_at IS NULL OR next_retry_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ORDER BY COALESCE(last_broadcast_at, updated_at, created_at) ASC, id ASC
            LIMIT ?3
            "#,
        )
        .bind(ApiTronAccountActivationStatus::Broadcasted.as_i64())
        .bind(ApiTronAccountActivationStatus::Fail.as_i64())
        .bind(limit as i64)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn claim_build_slot<'a, E>(
        exec: E,
        activation_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_tron_account_activation
            SET building_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                recover_status = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE activation_trade_no = ?1
              AND account_verified_at IS NULL
              AND tx_hash IS NULL
              AND (next_retry_at IS NULL OR next_retry_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
              AND (
                    building_at IS NULL
                    OR datetime(building_at) <= datetime('now', '-5 minutes')
                  )
            "#,
        )
        .bind(activation_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_broadcast_success<'a, E>(
        exec: E,
        activation_trade_no: &str,
        tx_hash: &str,
        raw_tx: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_tron_account_activation
            SET raw_tx = COALESCE(raw_tx, ?2),
                tx_hash = COALESCE(tx_hash, ?3),
                last_broadcast_at = COALESCE(last_broadcast_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                status = ?4,
                building_at = NULL,
                tx_status = NULL,
                recover_status = NULL,
                err_code = NULL,
                err_msg = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE activation_trade_no = ?1
              AND account_verified_at IS NULL
              AND building_at IS NOT NULL
              AND tx_hash IS NULL
            "#,
        )
        .bind(activation_trade_no)
        .bind(raw_tx)
        .bind(tx_hash)
        .bind(ApiTronAccountActivationStatus::Broadcasted.as_i64())
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_retry_wait<'a, E>(
        exec: E,
        activation_trade_no: &str,
        err_code: Option<&str>,
        err_msg: Option<&str>,
        recover_status: ApiTronAccountActivationRecoverStatus,
        next_retry_at: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_tron_account_activation
            SET status = ?2,
                building_at = NULL,
                err_code = ?3,
                err_msg = ?4,
                recover_status = ?5,
                next_retry_at = ?6,
                retry_count = retry_count + 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE activation_trade_no = ?1
              AND account_verified_at IS NULL
            "#,
        )
        .bind(activation_trade_no)
        .bind(ApiTronAccountActivationStatus::Fail.as_i64())
        .bind(err_code)
        .bind(err_msg)
        .bind(recover_status.as_i64())
        .bind(next_retry_at)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_chain_success<'a, E>(
        exec: E,
        activation_trade_no: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_tron_account_activation
            SET chain_success_at = COALESCE(chain_success_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                tx_status = 'success',
                recover_status = NULL,
                next_retry_at = NULL,
                err_code = NULL,
                err_msg = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE activation_trade_no = ?1
              AND account_verified_at IS NULL
              AND tx_hash IS NOT NULL
              AND trim(tx_hash) <> ''
              AND chain_success_at IS NULL
            "#,
        )
        .bind(activation_trade_no)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn mark_chain_failed_retry<'a, E>(
        exec: E,
        activation_trade_no: &str,
        err_code: Option<&str>,
        err_msg: Option<&str>,
        recover_status: ApiTronAccountActivationRecoverStatus,
        next_retry_at: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let res = sqlx::query(
            r#"
            UPDATE api_tron_account_activation
            SET chain_failed_at = COALESCE(chain_failed_at, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                tx_status = 'fail',
                status = ?2,
                raw_tx = NULL,
                tx_hash = NULL,
                last_broadcast_at = NULL,
                building_at = NULL,
                err_code = ?3,
                err_msg = ?4,
                recover_status = ?5,
                next_retry_at = ?6,
                retry_count = retry_count + 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE activation_trade_no = ?1
              AND account_verified_at IS NULL
            "#,
        )
        .bind(activation_trade_no)
        .bind(ApiTronAccountActivationStatus::Fail.as_i64())
        .bind(err_code)
        .bind(err_msg)
        .bind(recover_status.as_i64())
        .bind(next_retry_at)
        .execute(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }
}
