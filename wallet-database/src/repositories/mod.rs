pub mod account;
pub mod address_book;
pub mod announcement;
pub mod assets;
pub mod bill;
pub mod chain;
pub mod coin;
pub mod device;
pub mod exchange_rate;
pub mod multisig_account;
pub mod multisig_queue;
pub mod node;
pub mod stake;
pub mod system_notification;
pub mod task_queue;
pub mod wallet;

pub struct ResourcesRepo {
    db_pool: crate::DbPool,
    transaction: Option<sqlx::Transaction<'static, sqlx::Sqlite>>,
}

impl ResourcesRepo {
    pub fn new(db_pool: crate::DbPool) -> Self {
        Self {
            db_pool,
            transaction: None,
        }
    }
    pub fn pool(&self) -> crate::DbPool {
        self.db_pool.clone()
    }

    pub fn pool_ref(&self) -> &crate::DbPool {
        &self.db_pool
    }
}

impl chain::ChainRepoTrait for ResourcesRepo {}
impl coin::CoinRepoTrait for ResourcesRepo {}
impl account::AccountRepoTrait for ResourcesRepo {}
impl wallet::WalletRepoTrait for ResourcesRepo {}
impl bill::BillRepoTrait for ResourcesRepo {}
impl device::DeviceRepoTrait for ResourcesRepo {}
impl assets::AssetsRepoTrait for ResourcesRepo {}
impl exchange_rate::ExchangeRateRepoTrait for ResourcesRepo {}
impl announcement::AnnouncementRepoTrait for ResourcesRepo {}
impl task_queue::TaskQueueRepoTrait for ResourcesRepo {}
impl system_notification::SystemNotificationRepoTrait for ResourcesRepo {}
impl node::NodeRepoTrait for ResourcesRepo {}

#[async_trait::async_trait]
impl TransactionTrait for ResourcesRepo {
    fn get_transaction(self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>, crate::Error> {
        self.transaction.ok_or(crate::Error::Database(
            crate::DatabaseError::TransactionNotBegin,
        ))
    }

    fn get_db_pool(&self) -> &crate::DbPool {
        &self.db_pool
    }

    fn get_mut_transaction(
        &mut self,
    ) -> Result<&mut sqlx::Transaction<'static, sqlx::Sqlite>, crate::Error> {
        self.transaction.as_mut().ok_or(crate::Error::Database(
            crate::DatabaseError::TransactionNotBegin,
        ))
    }

    fn insert_transaction(&mut self, tx: sqlx::Transaction<'static, sqlx::Sqlite>) {
        self.transaction = Some(tx);
    }

    fn get_conn_or_tx(&mut self) -> Result<ExecutorWrapper<'_>, crate::Error> {
        if let Some(tx) = self.transaction.as_mut() {
            Ok(ExecutorWrapper::Transaction(tx))
        } else {
            Ok(ExecutorWrapper::Pool(&self.db_pool))
        }
    }

    async fn begin_transaction(&mut self) -> Result<(), crate::Error>
    where
        Self: Sized,
    {
        let conn = self.get_db_pool();
        let tx = conn
            .begin()
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        self.insert_transaction(tx);
        Ok(())
    }

    async fn commit_transaction(&mut self) -> Result<(), crate::Error>
    where
        Self: Sized,
    {
        if let Some(transaction) = self.transaction.take() {
            transaction
                .commit()
                .await
                .map_err(|e| crate::Error::Database(e.into()))?;
        }

        Ok(())
    }
}

pub enum ExecutorWrapper<'a> {
    Transaction(&'a mut sqlx::Transaction<'static, sqlx::Sqlite>),
    Pool(&'a sqlx::Pool<sqlx::Sqlite>),
}

impl<'a> ExecutorWrapper<'a> {
    pub async fn execute<F, Fut, T>(self, query: F) -> Result<T, crate::Error>
    where
        F: for<'c> FnOnce(&'c mut sqlx::SqliteConnection) -> Fut,
        Fut: std::future::Future<Output = Result<T, crate::Error>>,
    {
        match self {
            ExecutorWrapper::Transaction(executor) => query(executor.as_mut()).await,
            ExecutorWrapper::Pool(executor) => {
                let mut conn = executor
                    .acquire()
                    .await
                    .map_err(|e| crate::Error::Database(e.into()))?;
                query(&mut conn).await
            }
        }
    }
}

#[async_trait::async_trait]
pub trait TransactionTrait: std::marker::Send {
    async fn begin_transaction(&mut self) -> Result<(), crate::Error>
    where
        Self: Sized;
    // {
    //     let conn = self.get_db_pool();
    //     let tx = conn
    //         .begin()
    //         .await
    //         .map_err(|e| crate::Error::Database(e.into()))?;
    //     self.insert_transaction(tx);
    //     Ok(self)
    // }

    async fn commit_transaction(&mut self) -> Result<(), crate::Error>
    where
        Self: Sized;
    // {
    //     let conn = self.get_transaction()?;
    //     conn.commit()
    //         .await
    //         .map_err(|e| crate::Error::Database(e.into()))?;

    //     Ok(())
    // }

    fn get_mut_transaction(
        &mut self,
    ) -> Result<&mut sqlx::Transaction<'static, sqlx::Sqlite>, crate::Error>;

    fn get_transaction(self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>, crate::Error>;

    fn get_db_pool(&self) -> &crate::DbPool;

    fn insert_transaction(&mut self, tx: sqlx::Transaction<'static, sqlx::Sqlite>);

    fn get_conn_or_tx(&mut self) -> Result<ExecutorWrapper<'_>, crate::Error>;
}
