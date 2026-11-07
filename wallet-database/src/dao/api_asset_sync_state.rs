use sqlx::{Executor, Sqlite};

use crate::entities::{
    api_asset_sync_state::{ApiAssetSyncStateEntity, ApiAssetSyncStatus},
    asset_token_key::AssetTokenKey,
};

pub struct ApiAssetSyncStateDao;

impl ApiAssetSyncStateDao {
    pub async fn upsert_pending<'a, E>(
        exec: E,
        address: &str,
        chain_code: &str,
        token_address: AssetTokenKey,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = r#"
            INSERT INTO api_asset_sync_state (
                address, chain_code, token_address, status, attempt_count, last_error, created_at, updated_at
            )
            VALUES (
                ?, ?, ?, ?, 0, NULL,
                strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            )
            ON CONFLICT (address, chain_code, token_address) DO UPDATE SET
                status = CASE
                    WHEN api_asset_sync_state.status = ? THEN api_asset_sync_state.status
                    ELSE excluded.status
                END,
                last_error = NULL,
                updated_at = excluded.updated_at
        "#;

        sqlx::query(sql)
            .bind(address)
            .bind(chain_code)
            .bind(token_address)
            .bind(ApiAssetSyncStatus::Pending)
            .bind(ApiAssetSyncStatus::Running)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn claim_next<'a, E>(
        exec: E,
        include_stuck_running: bool,
    ) -> Result<Option<ApiAssetSyncStateEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = r#"
            UPDATE api_asset_sync_state
            SET
                status = ?,
                attempt_count = attempt_count + 1,
                last_error = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE rowid = (
                SELECT rowid
                FROM api_asset_sync_state
                WHERE
                    status = ?
                    OR (
                        status = ?
                        AND (
                            updated_at IS NULL
                            OR unixepoch(updated_at) < unixepoch('now') - 30
                        )
                    )
                    OR (
                        ? = 1
                        AND status = ?
                        AND updated_at IS NOT NULL
                        AND unixepoch(updated_at) < unixepoch('now') - 600
                    )
                ORDER BY updated_at ASC, created_at ASC
                LIMIT 1
            )
            RETURNING *
        "#;

        sqlx::query_as::<_, ApiAssetSyncStateEntity>(sql)
            .bind(ApiAssetSyncStatus::Running)
            .bind(ApiAssetSyncStatus::Pending)
            .bind(ApiAssetSyncStatus::Failed)
            .bind(include_stuck_running as i32)
            .bind(ApiAssetSyncStatus::Running)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn mark_done<'a, E>(
        exec: E,
        address: &str,
        chain_code: &str,
        token_address: AssetTokenKey,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = r#"
            UPDATE api_asset_sync_state
            SET
                status = ?,
                last_error = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE address = ? AND chain_code = ? AND token_address = ?
        "#;

        sqlx::query(sql)
            .bind(ApiAssetSyncStatus::Done)
            .bind(address)
            .bind(chain_code)
            .bind(token_address)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn mark_failed<'a, E>(
        exec: E,
        address: &str,
        chain_code: &str,
        token_address: AssetTokenKey,
        err_msg: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = r#"
            UPDATE api_asset_sync_state
            SET
                status = ?,
                last_error = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE address = ? AND chain_code = ? AND token_address = ?
              AND status != ?
        "#;

        sqlx::query(sql)
            .bind(ApiAssetSyncStatus::Failed)
            .bind(err_msg)
            .bind(address)
            .bind(chain_code)
            .bind(token_address)
            .bind(ApiAssetSyncStatus::Done)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete_done_older_than<'a, E>(
        exec: E,
        retention_secs: u64,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = r#"
            DELETE FROM api_asset_sync_state
            WHERE status = ?
              AND updated_at IS NOT NULL
              AND unixepoch(updated_at) < unixepoch('now') - ?
        "#;

        sqlx::query(sql)
            .bind(ApiAssetSyncStatus::Done)
            .bind(retention_secs as i64)
            .execute(exec)
            .await
            .map(|result| result.rows_affected())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn trim_done_to_limit<'a, E>(exec: E, max_rows: u32) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = r#"
            DELETE FROM api_asset_sync_state
            WHERE rowid IN (
                SELECT rowid
                FROM api_asset_sync_state
                WHERE status = ?
                ORDER BY updated_at ASC, created_at ASC
                LIMIT (
                    SELECT max(count(*) - ?, 0)
                    FROM api_asset_sync_state
                    WHERE status = ?
                )
            )
        "#;

        sqlx::query(sql)
            .bind(ApiAssetSyncStatus::Done)
            .bind(max_rows as i64)
            .bind(ApiAssetSyncStatus::Done)
            .execute(exec)
            .await
            .map(|result| result.rows_affected())
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
