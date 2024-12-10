mod error;
pub use error::Error;
pub mod dao;
pub mod entities;
pub mod factory;
mod init;
pub mod pagination;
pub mod repositories;

// database pool
pub type DbPool = std::sync::Arc<sqlx::Pool<Sqlite>>;

use error::database::DatabaseError;
use sqlx::Sqlite;

#[macro_export]
macro_rules! execute_with_executor {
    ($executor:expr, $query_fn:expr, $($args:expr),*) => {
        match $executor {
            super::ExecutorWrapper::Transaction(executor) => $query_fn(executor.as_mut(), $($args),*).await,
            super::ExecutorWrapper::Pool(executor) => $query_fn(executor, $($args),*).await,
        }
    };
}

#[derive(Debug, Clone)]
pub struct SqliteContext {
    pub sqlite_provider: crate::init::SqlitePoolProvider,
}

impl SqliteContext {
    pub async fn new(db_path: &str) -> Result<Self, crate::Error> {
        let uri = format!("{db_path}/data.db");
        let provider = crate::init::SqlitePoolProvider::new(uri).await?;

        Ok(SqliteContext {
            sqlite_provider: provider,
        })
    }

    pub fn get_pool(&self) -> Result<std::sync::Arc<sqlx::SqlitePool>, crate::Error> {
        Ok(self.sqlite_provider.get_pool()?)
    }
}

pub(crate) fn any_in_collection<T, I>(collection: I, placeholder: &str) -> String
where
    T: std::fmt::Display,
    I: IntoIterator<Item = T>,
{
    let mut iter = collection.into_iter().peekable();
    let mut any = String::new();

    while let Some(item) = iter.next() {
        any.push_str(&format!("{}", item));
        if iter.peek().is_some() {
            any.push_str(placeholder);
        }
    }

    any
}
