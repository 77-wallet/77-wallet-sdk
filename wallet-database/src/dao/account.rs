use sqlx::{Executor, Sqlite};

use crate::entities::account::{AccountEntity, CreateAccountVo};

impl AccountEntity {
    pub async fn upsert_multi_account<'a, E>(
        exec: E,
        reqs: Vec<CreateAccountVo>,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if reqs.is_empty() {
            return Ok(());
        }

        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "INSERT INTO account (
                account_id, address, pubkey, address_type, wallet_address, derivation_path, 
                chain_code, name, status, is_init, created_at, updated_at) ",
        );

        query_builder.push_values(reqs, |mut b, req| {
            b.push_bind(req.account_id)
                .push_bind(req.address)
                .push_bind(req.pubkey)
                .push_bind(req.address_type)
                .push_bind(req.wallet_address)
                .push_bind(req.derivation_path)
                .push_bind(req.chain_code)
                .push_bind(req.name)
                .push_bind(1)
                .push_bind(0)
                .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
                .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')");
        });

        query_builder.push(
            " ON CONFLICT (address, chain_code) DO UPDATE SET 
            updated_at = excluded.updated_at",
        );

        let query = query_builder.build();

        // tracing::warn!("sql: {}", query.sql());

        query
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn edit_account_name<'a, E>(
        exec: E,
        account_id: u32,
        wallet_address: &str,
        name: &str,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE account SET 
                name = $3,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE wallet_address = $1 AND account_id = $2
            RETURNING *
        "#;

        sqlx::query_as::<sqlx::Sqlite, AccountEntity>(sql)
            .bind(wallet_address)
            .bind(account_id)
            .bind(name)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn account_detail_by_max_id_and_wallet_address<'a, E>(
        executor: E,
        wallet_address: &str,
    ) -> Result<Option<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM account where wallet_address = $1
                   ORDER BY account_id DESC
                   LIMIT 1;";

        sqlx::query_as::<sqlx::Sqlite, AccountEntity>(sql)
            .bind(wallet_address)
            .fetch_optional(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_in_address<'a, E>(
        executor: E,
        addresses: &[String],
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let addresses = crate::any_in_collection(addresses, "','");
        let sql = format!("SELECT * FROM account WHERE address IN ('{}')", addresses);

        sqlx::query_as::<sqlx::Sqlite, AccountEntity>(&sql)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn account_list<'a, E>(
        executor: E,
        wallet_address: Option<&str>,
        address: Option<&str>,
        derivation_path: Option<&str>,
        chain_codes: Vec<String>,
        account_id: Option<u32>,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let chain_codes = crate::any_in_collection(chain_codes, "','");
        let mut sql = "SELECT * FROM account".to_string();
        let mut conditions = Vec::new();

        if !chain_codes.is_empty() {
            let str = format!("chain_code in ('{}')", chain_codes);
            conditions.push(str)
        }

        if wallet_address.is_some() {
            conditions.push("wallet_address = ?".to_string());
        }
        if address.is_some() {
            conditions.push("address = ?".to_string());
        }
        if derivation_path.is_some() {
            conditions.push("derivation_path = ?".to_string());
        }

        if account_id.is_some() {
            conditions.push("account_id = ?".to_string());
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        let mut query = sqlx::query_as::<_, AccountEntity>(&sql);

        // 绑定参数
        if let Some(wallet_address) = wallet_address {
            query = query.bind(wallet_address);
        }
        if let Some(address) = address {
            query = query.bind(address);
        }
        if let Some(derivation_path) = derivation_path {
            query = query.bind(derivation_path);
        }
        if let Some(account_id) = account_id {
            query = query.bind(account_id);
        }
        // 执行查询并返回结果
        let result = query.fetch_all(executor).await;

        result.map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn has_account_id<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM account WHERE wallet_address = $1 AND account_id = $2
            "#;

        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(wallet_address)
            .bind(account_id)
            .fetch_optional(exec)
            .await
            .map(|v| v.is_some())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn reset_account<'a, E>(
        exec: E,
        wallet_address: &str,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update account set 
                status = 2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            where wallet_address = $1
            RETURNING *
            "#;

        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(wallet_address)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn reset_all_account<'a, E>(exec: E) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update account set
                status = 2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            RETURNING *
            "#;
        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn restart_account<'a, E>(
        exec: E,
        wallet_address: &str,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update account set 
                status = 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            where wallet_address = $1
            RETURNING *
            "#;

        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(wallet_address)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn physical_delete_all<'a, E>(
        exec: E,
        wallet_addresses: &[&str],
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = if wallet_addresses.is_empty() {
            "DELETE FROM account RETURNING *".to_string()
        } else {
            let addresses = crate::any_in_collection(wallet_addresses, "','");
            format!(
                r#"
                DELETE FROM account
                WHERE wallet_address IN ('{}')
                RETURNING *
                "#,
                addresses
            )
        };

        sqlx::query_as::<sqlx::Sqlite, Self>(&sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn physical_delete<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        DELETE FROM account
        WHERE wallet_address = $1 AND account_id = $2
        RETURNING *
        "#;

        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(wallet_address)
            .bind(account_id)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn count_unique_account_ids<'a, E>(
        exec: E,
        wallet_address: &str,
    ) -> Result<u32, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT COUNT(DISTINCT account_id) as count
            FROM account
            WHERE wallet_address = $1
            "#;
        sqlx::query_as::<_, (u32,)>(sql)
            .bind(wallet_address)
            .fetch_one(exec)
            .await
            .map(|(count,)| count)
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn init<'a, E>(
        exec: E,
        address: &str,
        chain_code: &str,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update account set 
                is_init = 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            where address = $1 AND chain_code = $2
            RETURNING *
            "#;

        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(address)
            .bind(chain_code)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn find_one_by_address_chain_code<'a, E>(
        address: &str,
        chain_code: &str,
        exec: E,
    ) -> Result<Option<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from account where chain_code = $1 and address = $2";

        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .bind(chain_code)
            .bind(address)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn lists_by_wallet_address<'a, E>(
        wallet_address: &str,
        account_id: Option<u32>,
        exec: E,
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut sql = "select * from account where wallet_address = ? and status = 1".to_string();
        if let Some(account_id) = account_id {
            let a = format!(" and account_id = '{}'", account_id);
            sql.push_str(&a);
        }

        sqlx::query_as::<sqlx::Sqlite, Self>(&sql)
            .bind(wallet_address)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
