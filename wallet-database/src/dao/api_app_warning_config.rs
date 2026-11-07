use crate::Error;
use sqlx::{Executor, Sqlite, SqliteConnection};

pub struct ApiAppWarningConfigDao;

impl ApiAppWarningConfigDao {
    pub async fn find_snapshot<'e, E>(
        exec: E,
        org: &str,
        app: &str,
    ) -> Result<Option<(String, String)>, Error>
    where
        E: Executor<'e, Database = Sqlite>,
    {
        sqlx::query_as("SELECT generation, value FROM api_app_warning_config WHERE org_id = ? AND app_id = ? AND value IS NOT NULL")
            .bind(org).bind(app).fetch_optional(exec).await.map_err(|e| Error::Database(e.into()))
    }

    /// Caller owns the transaction: retain good data and reserve only a bound scope.
    pub async fn reserve_generation(
        conn: &mut SqliteConnection,
        org: &str,
        app: &str,
    ) -> Result<Option<String>, Error> {
        sqlx::query("INSERT OR IGNORE INTO api_app_warning_config (org_id, app_id) SELECT ?, ? WHERE EXISTS (SELECT 1 FROM api_wallet WHERE merchant_id = ? AND app_id = ? AND status = 1 AND api_wallet_type = 2)")
            .bind(org).bind(app).bind(org).bind(app).execute(&mut *conn).await.map_err(|e| Error::Database(e.into()))?;
        sqlx::query_scalar(
            "SELECT generation FROM api_app_warning_config WHERE org_id = ? AND app_id = ?",
        )
        .bind(org)
        .bind(app)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| Error::Database(e.into()))
    }

    pub async fn publish<'e, E>(
        exec: E,
        org: &str,
        app: &str,
        generation: &str,
        value: &str,
    ) -> Result<bool, Error>
    where
        E: Executor<'e, Database = Sqlite>,
    {
        let result = sqlx::query("UPDATE api_app_warning_config SET value = ? WHERE org_id = ? AND app_id = ? AND generation = ? AND EXISTS (SELECT 1 FROM api_wallet WHERE merchant_id = ? AND app_id = ? AND status = 1 AND api_wallet_type = 2)")
            .bind(value).bind(org).bind(app).bind(generation).bind(org).bind(app).execute(exec).await.map_err(|e| Error::Database(e.into()))?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn generation_exists<'e, E>(
        exec: E,
        org: &str,
        app: &str,
        generation: &str,
    ) -> Result<bool, Error>
    where
        E: Executor<'e, Database = Sqlite>,
    {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM api_app_warning_config WHERE org_id = ? AND app_id = ? AND generation = ? AND value IS NOT NULL)")
            .bind(org).bind(app).bind(generation).fetch_one(exec).await.map_err(|e| Error::Database(e.into()))
    }
}
