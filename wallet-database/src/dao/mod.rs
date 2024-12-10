pub mod account;
pub mod address_book;
pub mod announcement;
pub mod assets;
pub mod bill;
pub mod chain;
pub mod coin;
pub mod config;
pub mod device;
pub mod exchange_rate;
pub mod multisig_account;
pub mod multisig_member;
pub mod multisig_queue;
pub mod multisig_signatures;
pub mod node;
pub mod stake;
pub mod system_notification;
pub mod task_queue;
pub mod wallet;

#[async_trait::async_trait]
pub trait Dao {
    type Input;
    type Output;
    async fn upsert<'c, E>(executor: E, input: Self::Input) -> Result<Self::Output, crate::Error>
    where
        E: sqlx::Executor<'c, Database = sqlx::Sqlite>;

    async fn list<'c, E>(executor: E) -> Result<Vec<Self::Output>, crate::Error>
    where
        E: sqlx::Executor<'c, Database = sqlx::Sqlite>;
}

#[async_trait::async_trait]
pub trait UpdateDao<I, O> {
    async fn update<'a, E>(executor: E, id: I) -> Result<Vec<O>, crate::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Sqlite>;
}

#[async_trait::async_trait]
pub trait QueryOneDao<I, O> {
    async fn one<'a, E>(executor: E, id: I) -> Result<Option<O>, crate::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Sqlite>;
}

#[async_trait::async_trait]
pub trait QueryListDao<O> {
    async fn list<'c, E>(executor: E) -> Result<Vec<O>, crate::Error>
    where
        E: sqlx::Executor<'c, Database = sqlx::Sqlite>;
}
