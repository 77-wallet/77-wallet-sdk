mod error;
pub use error::{DatabaseError, Error};
pub mod dao;
pub mod db;
pub mod db_pool;
pub use db_pool::{
    ApiResourceDbPool, ApiTransactionDbPool, ApiWalletDbPool, CoreDbPool, DbPool, TaskDbPool,
};
pub mod entities;
mod init;
pub use init::SqlitePoolConfig;
pub mod pagination;
pub mod repositories;
pub(crate) mod sql_utils;

// database pool
pub use wallet_tree::KdfAlgorithm;

#[derive(Debug, Clone)]
pub struct SqliteContext {
    pub sqlite_provider: crate::init::SqlitePoolProvider,
}

impl SqliteContext {
    pub async fn new(db_path: &str, db_name: Option<&str>) -> Result<Self, crate::Error> {
        Self::new_with_config(db_path, db_name, SqlitePoolConfig::default()).await
    }

    pub async fn new_with_config(
        db_path: &str,
        db_name: Option<&str>,
        config: SqlitePoolConfig,
    ) -> Result<Self, crate::Error> {
        let db_name = db_name.unwrap_or("data.db");
        let uri = format!("{db_path}/{db_name}");

        // 根据db_name选择对应的Migrator
        let migrator = match db_name {
            "data.db" => crate::init::Migrator::Core,
            "api_transaction.db" => crate::init::Migrator::ApiTransaction,
            "api_resource.db" => crate::init::Migrator::ApiResource,
            "api_wallet.db" => crate::init::Migrator::ApiWallet,
            "task.db" => crate::init::Migrator::Task,
            _ => {
                return Err(crate::Error::Database(
                    crate::error::database::DatabaseError::InvalidDatabaseName(db_name.to_string()),
                ));
            }
        };

        let provider =
            crate::init::SqlitePoolProvider::new_with_config(uri, migrator, config).await?;

        Ok(SqliteContext { sqlite_provider: provider })
    }

    pub fn get_pool(&self) -> Result<std::sync::Arc<sqlx::SqlitePool>, crate::Error> {
        Ok(self.sqlite_provider.get_pool()?)
    }

    pub fn into_core_db_pool(self) -> Result<CoreDbPool, crate::Error> {
        Ok(CoreDbPool::new_split(
            self.sqlite_provider.get_read_pool()?,
            self.sqlite_provider.get_write_pool()?,
        ))
    }

    pub fn into_task_db_pool(self) -> Result<TaskDbPool, crate::Error> {
        Ok(TaskDbPool::new_split(
            self.sqlite_provider.get_read_pool()?,
            self.sqlite_provider.get_write_pool()?,
        ))
    }

    pub fn into_transaction_db_pool(self) -> Result<ApiTransactionDbPool, crate::Error> {
        Ok(ApiTransactionDbPool::new_split(
            self.sqlite_provider.get_read_pool()?,
            self.sqlite_provider.get_write_pool()?,
        ))
    }

    pub fn into_api_resource_db_pool(self) -> Result<ApiResourceDbPool, crate::Error> {
        Ok(ApiResourceDbPool::new_split(
            self.sqlite_provider.get_read_pool()?,
            self.sqlite_provider.get_write_pool()?,
        ))
    }

    pub fn into_api_wallet_db_pool(self) -> Result<ApiWalletDbPool, crate::Error> {
        Ok(ApiWalletDbPool::new_split(
            self.sqlite_provider.get_read_pool()?,
            self.sqlite_provider.get_write_pool()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn resource_db_migration_creates_resource_tables() {
        let dir = crate::repositories::test_helper::make_temp_dir("resource_db_migration");
        let ctx = crate::SqliteContext::new(&dir, Some("api_resource.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();

        for table in ["api_resource_delegation", "api_resource_operation"] {
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?",
            )
            .bind(table)
            .fetch_one(pool.as_ref())
            .await
            .unwrap();
            assert_eq!(exists, 1, "{table} should be created in api_resource.db");
        }
    }

    #[tokio::test]
    async fn api_wallet_migration_drops_redundant_asset_index() {
        let dir = crate::repositories::test_helper::make_temp_dir(
            "api_wallet_drop_redundant_asset_index",
        );
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();

        let redundant_index_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'index' AND name = ?",
        )
        .bind("api_assets_symbol_address_chain_idx")
        .fetch_one(pool.as_ref())
        .await
        .unwrap();
        assert_eq!(redundant_index_count, 0);

        let unique_index_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'index' AND name = ?",
        )
        .bind("api_assets_symbol_address_chain_token_idx")
        .fetch_one(pool.as_ref())
        .await
        .unwrap();
        assert_eq!(unique_index_count, 1);

        let insert = || {
            sqlx::query(
                r#"
INSERT INTO api_assets ( decimals, address, token_address, protocol, chain_code,
    status, is_multisig, balance, created_at
) VALUES ( 18, 'address-1', '', NULL, 'eth', 1, 0, '0', CURRENT_TIMESTAMP)
"#,
            )
        };

        insert().execute(pool.as_ref()).await.unwrap();
        let duplicate = insert().execute(pool.as_ref()).await;
        assert!(duplicate.is_err(), "the unique asset key must remain enforced");
    }

    #[tokio::test]
    async fn api_wallet_migration_creates_nonzero_asset_total_index() {
        let dir =
            crate::repositories::test_helper::make_temp_dir("api_wallet_nonzero_asset_total_index");
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();

        let index_sql: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?")
                .bind("api_assets_nonzero_wallet_total_idx")
                .fetch_one(pool.as_ref())
                .await
                .unwrap();

        assert!(index_sql.contains("address, chain_code, token_address, balance"));
        assert!(index_sql.contains("WHERE status = 1 AND balance != '0'"));
    }
}
