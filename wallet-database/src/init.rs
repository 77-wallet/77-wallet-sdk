use crate::DbPool;
use chrono::Utc;
use sqlx::{Pool, Sqlite, migrate::MigrateDatabase as _};
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub struct SqlitePoolConfig {
    pub reader_max_connections: u32,
    pub writer_max_connections: u32,
}

impl Default for SqlitePoolConfig {
    fn default() -> Self {
        Self { reader_max_connections: 4, writer_max_connections: 1 }
    }
}

#[derive(Debug, Clone)]
pub struct SqlitePoolProvider {
    pub uri: String,
    pub read_conn: DbPool,
    pub write_conn: DbPool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Migrator {
    Core,           // data.db
    ApiTransaction, // api_transaction.db
    ApiResource,    // api_resource.db
    ApiWallet,      // api_wallet.db
    Task,           // task.db
}
impl Migrator {
    pub fn migrator(&self) -> Result<sqlx::migrate::Migrator, crate::Error> {
        // Keep this match explicit so sqlx::migrate! paths stay discoverable at compile time.
        match self {
            Migrator::Core => Ok(sqlx::migrate!("./schema/core/migrations")),
            Migrator::ApiTransaction => Ok(sqlx::migrate!("./schema/api_transaction/migrations")),
            Migrator::ApiResource => Ok(sqlx::migrate!("./schema/api_resource/migrations")),
            Migrator::ApiWallet => Ok(sqlx::migrate!("./schema/api_wallet/migrations")),
            Migrator::Task => Ok(sqlx::migrate!("./schema/task/migrations")),
        }
    }
}

impl SqlitePoolProvider {
    const ANALYZE_REFRESH_DAYS: i64 = 1;

    pub async fn new(uri: String, migrator: Migrator) -> Result<Self, crate::Error> {
        Self::new_with_config(uri, migrator, SqlitePoolConfig::default()).await
    }

    pub async fn new_with_config(
        uri: String,
        migrator: Migrator,
        config: SqlitePoolConfig,
    ) -> Result<Self, crate::Error> {
        let (write_pool, write_created) =
            Self::init_pool(&uri, config.writer_max_connections).await?;
        let (read_pool, _) = Self::init_pool(&uri, config.reader_max_connections).await?;

        // run migrations
        Self::run_migrate(write_pool.clone(), migrator).await?;
        if migrator == Migrator::ApiWallet && !write_created {
            Self::checkpoint_api_wallet_wal_on_startup(write_pool.clone()).await;
        }
        Self::spawn_analyze_if_needed(write_pool.clone(), write_created).await;

        Ok(Self { uri, read_conn: read_pool, write_conn: write_pool })
    }

    pub async fn run_migrate(pool: DbPool, migrator: Migrator) -> Result<(), crate::Error> {
        Self::run_migrate_internal(pool, migrator).await
    }

    async fn run_migrate_internal(pool: DbPool, migrator: Migrator) -> Result<(), crate::Error> {
        // run migrator
        if let Err(e) = migrator.migrator()?.run(pool.as_ref()).await {
            let msg = format!("migrate filed: remove files = {e}");
            tracing::error!(msg);
            panic!("{msg}");
        }

        Ok(())
    }

    async fn checkpoint_api_wallet_wal_on_startup(pool: DbPool) {
        let started_at = std::time::Instant::now();
        let passive = sqlx::query_as::<_, (i64, i64, i64)>("PRAGMA wal_checkpoint(PASSIVE)")
            .fetch_one(pool.as_ref())
            .await;

        let (busy, log_pages, checkpointed_pages) = match passive {
            Ok(result) => result,
            Err(error) => {
                tracing::warn!(
                    metric = "api_wallet_startup_wal_checkpoint",
                    mode = "passive",
                    elapsed_ms = started_at.elapsed().as_millis(),
                    error = %error,
                    "API wallet startup WAL checkpoint failed"
                );
                return;
            }
        };

        tracing::info!(
            metric = "api_wallet_startup_wal_checkpoint",
            mode = "passive",
            busy,
            log_pages,
            checkpointed_pages,
            elapsed_ms = started_at.elapsed().as_millis(),
            "API wallet startup WAL checkpoint completed"
        );

        if busy != 0 || checkpointed_pages != log_pages {
            tracing::info!(
                metric = "api_wallet_startup_wal_checkpoint",
                busy,
                log_pages,
                checkpointed_pages,
                "API wallet startup WAL truncate skipped because readers still need WAL pages"
            );
            return;
        }

        let truncate_started_at = std::time::Instant::now();
        match sqlx::query_as::<_, (i64, i64, i64)>("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(pool.as_ref())
            .await
        {
            Ok((truncate_busy, truncate_log_pages, truncate_checkpointed_pages)) => {
                tracing::info!(
                    metric = "api_wallet_startup_wal_checkpoint",
                    mode = "truncate",
                    busy = truncate_busy,
                    log_pages = truncate_log_pages,
                    checkpointed_pages = truncate_checkpointed_pages,
                    elapsed_ms = truncate_started_at.elapsed().as_millis(),
                    "API wallet startup WAL truncate completed"
                );
            }
            Err(error) => {
                tracing::warn!(
                    metric = "api_wallet_startup_wal_checkpoint",
                    mode = "truncate",
                    elapsed_ms = truncate_started_at.elapsed().as_millis(),
                    error = %error,
                    "API wallet startup WAL truncate failed"
                );
            }
        }
    }

    async fn spawn_analyze_if_needed(pool: DbPool, database_was_created: bool) {
        let should_analyze = database_was_created
            || match Self::analyze_is_due(pool.clone()).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::warn!("[spawn_analyze_if_needed] ANALYZE schedule check failed: {e}");
                    false
                }
            };

        if !should_analyze {
            return;
        }

        tracing::info!("[spawn_analyze_if_needed] scheduling ANALYZE in background");
        tokio::spawn(async move {
            tracing::info!("[spawn_analyze_if_needed] ANALYZE started");
            if let Err(e) = sqlx::query("ANALYZE").execute(pool.as_ref()).await {
                tracing::warn!("[spawn_analyze_if_needed] ANALYZE failed: {e}");
            } else {
                tracing::info!("[spawn_analyze_if_needed] ANALYZE completed");
                if let Err(e) = Self::mark_analyze_day(pool).await {
                    tracing::warn!(
                        "[spawn_analyze_if_needed] failed to persist ANALYZE watermark: {e}"
                    );
                }
            }
        });
    }

    async fn analyze_is_due(pool: DbPool) -> Result<bool, sqlx::Error> {
        let day = Self::current_day();
        let last_day =
            sqlx::query_scalar::<_, i64>("PRAGMA user_version").fetch_one(pool.as_ref()).await?;
        Ok(day.saturating_sub(last_day) >= Self::ANALYZE_REFRESH_DAYS)
    }

    async fn mark_analyze_day(pool: DbPool) -> Result<(), sqlx::Error> {
        let day = Self::current_day();
        sqlx::query(&format!("PRAGMA user_version = {day}"))
            .execute(pool.as_ref())
            .await
            .map(|_| ())
    }

    fn current_day() -> i64 {
        Utc::now().timestamp() / 86_400
    }

    pub async fn init_pool(
        uri: &str,
        max_connections: u32,
    ) -> Result<(DbPool, bool), crate::Error> {
        use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
        use std::{str::FromStr, time::Duration};

        let created = !sqlx::Sqlite::database_exists(uri).await.unwrap_or(false);
        if created {
            sqlx::Sqlite::create_database(uri)
                .await
                .map_err(|_| crate::DatabaseError::DatabaseCreateFailed)?;
        };

        tracing::debug!("[init_pool] data base uri: {uri}");

        // 配置 WAL + busy_timeout + NORMAL
        let opts = SqliteConnectOptions::from_str(uri)
            .map_err(|_| crate::DatabaseError::DatabaseConnectFailed)?
            .journal_mode(SqliteJournalMode::Wal) // 🚀 启用 WAL 模式
            .synchronous(SqliteSynchronous::Normal) // 🚀 更快的写，可靠性仍够用
            .busy_timeout(Duration::from_secs(5)) // 🚀 等锁最长 5 秒
            .create_if_missing(true);

        // 用连接池管理连接，避免单连接锁竞争
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(max_connections)
            .min_connections(1)
            .acquire_timeout(Duration::from_secs(60)) // 🔥 增加获取连接等待时间
            .connect_with(opts)
            .await
            .map_err(|e| {
                tracing::error!("[init_database] connect error: {e}");
                crate::DatabaseError::DatabaseConnectFailed
            })?;

        Ok((Arc::new(pool), created))
    }

    pub fn get_pool(&self) -> Result<std::sync::Arc<Pool<Sqlite>>, crate::DatabaseError> {
        Ok(self.read_conn.clone())
    }

    pub fn get_read_pool(&self) -> Result<std::sync::Arc<Pool<Sqlite>>, crate::DatabaseError> {
        Ok(self.read_conn.clone())
    }

    pub fn get_write_pool(&self) -> Result<std::sync::Arc<Pool<Sqlite>>, crate::DatabaseError> {
        Ok(self.write_conn.clone())
    }

    pub fn get_uri(&self) -> String {
        self.uri.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_migrate_skips_analyze() {
        let pool = Arc::new(
            sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("open sqlite memory"),
        );

        let result = SqlitePoolProvider::run_migrate_internal(pool, Migrator::Core).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn init_pool_reports_creation_state() {
        let uri = format!(
            "{}/init_pool_reports_creation_state-{}.db",
            std::env::temp_dir().display(),
            std::process::id()
        );

        let (_, created) =
            SqlitePoolProvider::init_pool(&uri, 1).await.expect("create sqlite pool");

        assert!(created);
    }

    #[tokio::test]
    async fn startup_checkpoint_merges_and_truncates_api_wallet_wal() {
        let uri = format!(
            "{}/startup_checkpoint_merges_and_truncates_api_wallet_wal-{}.db",
            std::env::temp_dir().display(),
            std::process::id()
        );
        let (pool, _) = SqlitePoolProvider::init_pool(&uri, 1).await.expect("create sqlite pool");

        sqlx::query("PRAGMA wal_autocheckpoint = 0")
            .execute(pool.as_ref())
            .await
            .expect("disable automatic checkpoint");
        sqlx::query("CREATE TABLE checkpoint_probe (id INTEGER PRIMARY KEY, value TEXT NOT NULL)")
            .execute(pool.as_ref())
            .await
            .expect("create checkpoint probe");
        sqlx::query("INSERT INTO checkpoint_probe(value) VALUES (?)")
            .bind("x".repeat(64 * 1024))
            .execute(pool.as_ref())
            .await
            .expect("write WAL frames");

        SqlitePoolProvider::checkpoint_api_wallet_wal_on_startup(pool.clone()).await;

        let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM checkpoint_probe")
            .fetch_one(pool.as_ref())
            .await
            .expect("read checkpointed data");
        let (busy, log_pages, checkpointed_pages): (i64, i64, i64) =
            sqlx::query_as("PRAGMA wal_checkpoint(PASSIVE)")
                .fetch_one(pool.as_ref())
                .await
                .expect("inspect WAL after truncate");

        assert_eq!(row_count, 1);
        assert_eq!(busy, 0);
        assert_eq!(log_pages, 0);
        assert_eq!(checkpointed_pages, 0);
    }

    #[test]
    fn default_sqlite_pool_config_keeps_single_sqlite_writer() {
        let config = SqlitePoolConfig::default();

        assert_eq!(config.reader_max_connections, 4);
        assert_eq!(config.writer_max_connections, 1);
    }

    #[tokio::test]
    async fn analyze_is_due_uses_user_version_watermark() {
        let pool = Arc::new(
            sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .expect("open sqlite memory"),
        );

        let current_day = SqlitePoolProvider::current_day();
        sqlx::query(&format!("PRAGMA user_version = {current_day}"))
            .execute(pool.as_ref())
            .await
            .expect("set watermark");

        assert!(!SqlitePoolProvider::analyze_is_due(pool.clone()).await.expect("read watermark"));

        let stale_day = current_day.saturating_sub(2);
        sqlx::query(&format!("PRAGMA user_version = {stale_day}"))
            .execute(pool.as_ref())
            .await
            .expect("set stale watermark");

        assert!(SqlitePoolProvider::analyze_is_due(pool).await.expect("read stale watermark"));
    }
}
