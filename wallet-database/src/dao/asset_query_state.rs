use sqlx::{Executor, Sqlite};

use crate::entities::asset_query_state::{AssetQueryStateEntity, AssetQueryStatus};

pub struct AssetQueryStateDao {}

impl AssetQueryStateDao {
    /// Upsert a pending asset query task for a (uid, chain_code, page).
    ///
    /// Semantics:
    /// - Inserts new row as Pending
    /// - If row exists:
    ///   - Updates index_list_json
    ///   - Sets status to Pending unless current status is Running/Done
    ///   - Clears last_error
    ///   - Updates updated_at
    pub async fn upsert_pending<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
        page: i64,
        index_list_json: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = r#"
            INSERT INTO asset_query_state (
                uid, chain_code, page, status, index_list_json, attempt_count, last_error, created_at, updated_at
            )
            VALUES (
                ?, ?, ?, ?, ?, 0, NULL,
                strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            )
            ON CONFLICT (uid, chain_code, page) DO UPDATE SET
                index_list_json = excluded.index_list_json,
                status = CASE
                    WHEN asset_query_state.status IN (?, ?) THEN asset_query_state.status
                    ELSE excluded.status
                END,
                last_error = NULL,
                updated_at = excluded.updated_at
        "#;

        sqlx::query(sql)
            .bind(uid)
            .bind(chain_code)
            .bind(page)
            .bind(AssetQueryStatus::Pending)
            .bind(index_list_json)
            .bind(AssetQueryStatus::Done)
            .bind(AssetQueryStatus::Running)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// Atomically claim one task and mark it Running.
    ///
    /// Rules:
    /// - Prefer Pending/Failed tasks.
    /// - Failed tasks must respect a minimal retry interval (30s).
    /// - If include_stuck_running=true, allow reclaiming Running tasks that are stale (10min).
    pub async fn claim_next<'a, E>(
        exec: E,
        include_stuck_running: bool,
    ) -> Result<Option<AssetQueryStateEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = r#"
            UPDATE asset_query_state
            SET
                status = ?,
                attempt_count = attempt_count + 1,
                last_error = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE rowid = (
                SELECT rowid
                FROM asset_query_state
                WHERE
                    (
                        status = ?
                        OR (
                            status = ?
                            AND (
                                updated_at IS NULL
                                OR unixepoch(updated_at) < unixepoch('now') - 30
                            )
                        )
                    )
                    OR (
                        ? = 1
                        AND status = ?
                        AND updated_at IS NOT NULL
                        AND unixepoch(updated_at) < unixepoch('now') - 600
                    )
                ORDER BY created_at ASC
                LIMIT 1
            )
            RETURNING *
        "#;

        sqlx::query_as::<_, AssetQueryStateEntity>(sql)
            .bind(AssetQueryStatus::Running)
            .bind(AssetQueryStatus::Pending)
            .bind(AssetQueryStatus::Failed)
            .bind(include_stuck_running as i32)
            .bind(AssetQueryStatus::Running)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn mark_done<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
        page: i64,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = r#"
            UPDATE asset_query_state
            SET
                status = ?,
                last_error = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE uid = ?
              AND chain_code = ?
              AND page = ?
        "#;

        sqlx::query(sql)
            .bind(AssetQueryStatus::Done)
            .bind(uid)
            .bind(chain_code)
            .bind(page)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn mark_failed<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
        page: i64,
        err_msg: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = r#"
            UPDATE asset_query_state
            SET
                status = ?,
                last_error = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE uid = ?
              AND chain_code = ?
              AND page = ?
              AND status != ?
        "#;

        sqlx::query(sql)
            .bind(AssetQueryStatus::Failed)
            .bind(err_msg)
            .bind(uid)
            .bind(chain_code)
            .bind(page)
            .bind(AssetQueryStatus::Done)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_temp_dir(prefix: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "{}_{}_{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().to_string()
    }

    #[tokio::test]
    async fn claim_next_is_exclusive() {
        let dir = make_temp_dir("wallet_db_asset_query_state_claim");
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();

        AssetQueryStateDao::upsert_pending(pool.as_ref(), "u", "tron", 0, "[1,2,3]").await.unwrap();

        let t1 = AssetQueryStateDao::claim_next(pool.as_ref(), true).await.unwrap();
        let t2 = AssetQueryStateDao::claim_next(pool.as_ref(), true).await.unwrap();

        assert!(t1.is_some());
        assert!(t2.is_none());
    }
}
