pub mod account;
pub mod address_book;
pub mod announcement;
pub mod api_business_notification;

pub mod api_wallet;

pub mod assets;
pub mod bill;
pub mod chain;
pub mod coin;
pub mod config;
pub mod device;
pub mod device_protection;
pub mod exchange_rate;
pub mod multisig_account;
pub mod multisig_member;
pub mod multisig_queue;
pub mod multisig_signature;
pub mod node;
pub mod permission;
pub mod stake;
pub mod system_notification;
pub mod task_queue;
pub mod wallet;

#[cfg(test)]
pub(crate) mod test_helper;

#[cfg(test)]
pub async fn with_tx<T, F>(pool: &crate::DbPool, f: F) -> Result<T, crate::Error>
where
    F: for<'a> FnOnce(
        &'a mut sqlx::SqliteConnection,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<T, crate::Error>> + 'a>,
    >,
{
    let mut tx = pool.begin().await.map_err(|e| crate::Error::Database(e.into()))?;
    let result = f(tx.as_mut()).await;
    match result {
        Ok(value) => {
            tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
            Ok(value)
        }
        Err(err) => {
            tx.rollback().await.map_err(|e| crate::Error::Database(e.into()))?;
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::with_tx;
    use crate::Error;
    use sqlx::Row;
    use std::sync::Arc;

    async fn setup_pool() -> Arc<sqlx::SqlitePool> {
        Arc::new(sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap())
    }

    #[tokio::test]
    async fn with_tx_commits_on_success() {
        let pool = setup_pool().await;
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
            .execute(pool.as_ref())
            .await
            .unwrap();

        with_tx(&pool, |conn| {
            Box::pin(async move {
                sqlx::query("INSERT INTO t(name) VALUES ('ok')")
                    .execute(conn)
                    .await
                    .map_err(|e| Error::Database(e.into()))?;
                Ok::<(), Error>(())
            })
        })
        .await
        .unwrap();

        let row =
            sqlx::query("SELECT COUNT(1) AS c FROM t").fetch_one(pool.as_ref()).await.unwrap();
        let count: i64 = row.get("c");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn with_tx_rolls_back_on_error() {
        let pool = setup_pool().await;
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
            .execute(pool.as_ref())
            .await
            .unwrap();

        let err = with_tx(&pool, |conn| {
            Box::pin(async move {
                sqlx::query("INSERT INTO t(name) VALUES ('will_rollback')")
                    .execute(conn)
                    .await
                    .map_err(|e| Error::Database(e.into()))?;
                Err::<(), Error>(Error::Other("rollback".to_string()))
            })
        })
        .await
        .unwrap_err();
        assert!(matches!(err, Error::Other(_)));

        let row =
            sqlx::query("SELECT COUNT(1) AS c FROM t").fetch_one(pool.as_ref()).await.unwrap();
        let count: i64 = row.get("c");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn with_tx_returns_db_error_on_constraint_violation() {
        let pool = setup_pool().await;
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
            .execute(pool.as_ref())
            .await
            .unwrap();

        let err = with_tx(&pool, |conn| {
            Box::pin(async move {
                sqlx::query("INSERT INTO t(name) VALUES (NULL)")
                    .execute(conn)
                    .await
                    .map_err(|e| Error::Database(e.into()))?;
                Ok::<(), Error>(())
            })
        })
        .await
        .unwrap_err();
        assert!(matches!(err, Error::Database(_)));

        let row =
            sqlx::query("SELECT COUNT(1) AS c FROM t").fetch_one(pool.as_ref()).await.unwrap();
        let count: i64 = row.get("c");
        assert_eq!(count, 0);
    }
}
