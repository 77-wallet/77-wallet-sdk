pub mod account;
pub mod address_book;
pub mod address_query_state;
pub mod announcement;
pub(crate) mod api_account;
pub mod api_asset_sync_state;
pub(crate) mod api_assets;
pub(crate) mod api_business_notification;
pub mod api_chain;
pub mod api_coin;
pub(crate) mod api_collect;
pub(crate) mod api_collect_strategy;
pub(crate) mod api_collect_strategy_chain_config;
pub(crate) mod api_fee;
pub(crate) mod api_nonce;
pub(crate) mod api_resource_delegation;
pub(crate) mod api_resource_operation;
pub(crate) mod api_tron_account_activation;
pub(crate) mod api_wallet;
pub(crate) mod api_withdraw;
pub(crate) mod api_withdraw_strategy;
pub(crate) mod api_withdraw_strategy_chain_config;
pub mod asset_query_state;
pub mod assets;
pub mod bill;
pub mod chain;
pub mod coin;
pub mod config;
pub mod device;
pub mod exchange_rate;
pub mod expand_batch;
pub mod expand_batch_item;
pub mod expand_notify_state;
pub mod multisig_account;
pub mod multisig_member;
pub mod multisig_queue;
pub mod multisig_signatures;
pub mod node;
pub mod permission;
pub mod permission_user;
pub mod stake;
pub mod system_notification;
pub mod task_queue;
pub mod wallet;

#[async_trait::async_trait]
pub trait Dao {
    type Input;
    type Output;
    type Error;
    async fn upsert<'c, E>(executor: E, input: Self::Input) -> Result<Self::Output, Self::Error>
    where
        E: sqlx::Executor<'c, Database = sqlx::Sqlite>;

    async fn list<'c, E>(executor: E) -> Result<Vec<Self::Output>, Self::Error>
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

pub mod api_app_warning_config;
