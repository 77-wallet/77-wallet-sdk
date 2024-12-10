use crate::DbPool;
use sqlx::{migrate::MigrateDatabase as _, Pool, Sqlite};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SqlitePoolProvider {
    pub uri: String,
    pub conn: DbPool,
}

#[derive(Debug, Default, Clone)]
pub enum Migrator {
    #[default]
    Pub,
    _Pri,
}
impl Migrator {
    pub fn migrator(&self) -> sqlx::migrate::Migrator {
        match self {
            Migrator::Pub => sqlx::migrate!("./schema/migrations"),
            Migrator::_Pri => sqlx::migrate!("./schema/migrations"),
        }
    }
}

impl SqlitePoolProvider {
    pub async fn new(uri: String) -> Result<Self, crate::Error> {
        let pool = Self::init_pool(&uri).await?;

        // run migrations
        Self::run_migrate(pool.clone()).await?;

        Ok(Self { uri, conn: pool })
    }

    pub async fn run_migrate(pool: DbPool) -> Result<(), crate::Error> {
        let migrator = Migrator::default();

        // run migraor
        if let Err(e) = migrator.migrator().run(pool.as_ref()).await {
            let msg = format!("migrate filed: remove files = {e}");
            tracing::error!(msg);
            panic!("{msg}");
        }
        Ok(())
    }

    pub async fn init_pool(uri: &str) -> Result<DbPool, crate::Error> {
        if !sqlx::Sqlite::database_exists(uri).await.unwrap_or(false) {
            sqlx::Sqlite::create_database(uri)
                .await
                .map_err(|_| crate::DatabaseError::DatabaseCreateFailed)?;
        };

        tracing::debug!("[init_pool] data base uri: {uri}");

        // get database connected
        let pool = sqlx::Pool::<sqlx::Sqlite>::connect(uri)
            .await
            .map_err(|e| {
                tracing::error!("[init_ database] connect error: {e}");
                crate::DatabaseError::DatabaseConnectFailed
            })?;

        Ok(Arc::new(pool))
    }

    pub fn get_pool(&self) -> Result<std::sync::Arc<Pool<Sqlite>>, crate::DatabaseError> {
        Ok(self.conn.clone())
    }

    pub fn get_uri(&self) -> String {
        self.uri.clone()
    }
}
