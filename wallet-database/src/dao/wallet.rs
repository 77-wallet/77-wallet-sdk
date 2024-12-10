use sqlx::{Executor, Sqlite};

use crate::entities::wallet::WalletEntity;

impl WalletEntity {
    pub async fn upsert<'a, E>(
        exec: E,
        address: &str,
        uid: &str,
        name: &str,
        status: u8,
    ) -> Result<Self, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "INSERT INTO wallet (address, uid, name, status, is_init, created_at, updated_at)
            VALUES (?, ?, ?, ?, 0, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT (address) DO UPDATE SET
                uid = excluded.uid,
                name = excluded.name,
                status = excluded.status,
                updated_at = excluded.updated_at
            RETURNING *;";
        let mut res = sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .bind(address)
            .bind(uid)
            .bind(name)
            .bind(status)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.pop().ok_or(crate::DatabaseError::ReturningNone)?)
    }

    pub async fn detail<'a, E>(exec: E, address: &str) -> Result<Option<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT *
                FROM wallet
                WHERE address =? AND status = 1;";
        sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .bind(address)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn detail_all_status<'a, E>(
        exec: E,
        address: &str,
    ) -> Result<Option<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT *
                FROM wallet
                WHERE address =$1;";
        sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .bind(address)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list<'a, E>(exec: E) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT *
        FROM wallet WHERE status = 1;";

        sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn wallet_detail_by_wallet_address<'a, E>(
        exec: E,
        address: &str,
    ) -> Result<Option<WalletEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM wallet WHERE address = ? AND status = 1;";

        sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .bind(address)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn wallet_detail_by_wallet_name<'a, E>(
        exec: E,

        name: Option<String>,
    ) -> Result<Option<WalletEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = if name.is_some() {
            "SELECT * FROM wallet WHERE name = ? AND status = 1;"
        } else {
            "SELECT * FROM wallet WHERE status = 1;"
        };

        let query = sqlx::query_as::<_, WalletEntity>(sql);

        let result = if let Some(name) = name {
            query.bind(name).fetch_optional(exec).await
        } else {
            query.fetch_optional(exec).await
        };

        result.map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn wallet_latest<'a, E>(exec: E) -> Result<Option<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM wallet WHERE status = 1
                   ORDER BY updated_at DESC
                   LIMIT 1;";

        sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn uid_list<'a, E>(exec: E) -> Result<Vec<(String,)>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT DISTINCT uid FROM wallet WHERE status = 1;";

        sqlx::query_as::<sqlx::Sqlite, (String,)>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn edit_wallet_name<'a, E>(
        exec: E,

        wallet_address: &str,
        name: &str,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update wallet set 
                name = $2,
                updated_at = $3
            where address = $1 AND status = 1
            RETURNING *
            "#;
        let time = sqlx::types::chrono::Utc::now().timestamp();

        sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .bind(wallet_address)
            .bind(name)
            .bind(time)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn init<'a, E>(exec: E, uid: &str) -> Result<Self, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update wallet set 
                is_init = 1,
                updated_at = $2
            where uid = $1 AND status = 1
            RETURNING *
            "#;
        let time = sqlx::types::chrono::Utc::now().timestamp();

        let mut res = sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .bind(uid)
            .bind(time)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.pop().ok_or(crate::DatabaseError::ReturningNone)?)
    }

    pub async fn update_wallet_update_at<'a, E>(
        exec: E,
        wallet_address: &str,
    ) -> Result<Option<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update wallet set 
                updated_at = $3
            where address = $1 AND status = 1
            RETURNING *
            "#;
        let time = sqlx::types::chrono::Utc::now().timestamp();

        let mut res = sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .bind(wallet_address)
            .bind(time)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.pop())
    }

    pub async fn reset_wallet<'a, E>(
        exec: E,
        wallet_address: &str,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update wallet set 
                status = 2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            where address = $1
            RETURNING *
            "#;

        sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .bind(wallet_address)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn reset_all_wallet<'a, E>(exec: E) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update wallet set
                status = 2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            where status = 1
            RETURNING *
            "#;
        sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn restart_wallet<'a, E>(
        exec: E,
        wallet_addresses: &[&str],
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let addresses = crate::any_in_collection(wallet_addresses, "','");
        let sql = format!(
            r#"
            update wallet set 
                status = 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            where address IN ('{}')
            RETURNING *
            "#,
            addresses
        );

        sqlx::query_as::<sqlx::Sqlite, WalletEntity>(&sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete_wallet<'a, E>(
        exec: E,
        wallet_addresses: &[&str],
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let addresses = crate::any_in_collection(wallet_addresses, "','");
        let sql = format!(
            r#"
            DELETE FROM wallet
            WHERE address IN ('{}')
            RETURNING *
            "#,
            addresses
        );

        sqlx::query_as::<sqlx::Sqlite, WalletEntity>(&sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete_all_wallet<'a, E>(exec: E) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            DELETE FROM wallet
            RETURNING *
            "#;
        sqlx::query_as::<sqlx::Sqlite, WalletEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
