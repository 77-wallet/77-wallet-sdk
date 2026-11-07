use crate::entities::account::{
    AccountEntity, AccountWalletMapping, AccountWithWalletEntity, CreateAccountVo,
};

pub struct AccountDao;

use crate::sql_utils::{
    SqlExecutableReturn as _, query_builder::DynamicQueryBuilder,
    update_builder::DynamicUpdateBuilder,
};
use sqlx::{Executor, Sqlite};

impl AccountDao {
    pub async fn detail<'a, E>(
        exec: E,
        wallet_address: Option<&str>,
        address: Option<&str>,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<Option<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT * FROM account")
            .and_where_eq_opt("wallet_address", wallet_address)
            .and_where_eq_opt("address", address)
            .and_where_eq_opt("account_id", account_id)
            .and_where_eq_opt("chain_code", chain_code)
            .and_where_eq("status", 1)
            .fetch_optional(exec)
            .await
    }

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

        query.execute(exec).await.map(|_| ()).map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn edit_account_name<'a, E>(
        executor: E,
        account_id: u32,
        wallet_address: &str,
        name: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicUpdateBuilder::new("account")
            .set("name", name)
            .set_raw("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("account_id", account_id)
            .returning("*")
            .fetch_all(executor)
            .await
    }

    pub async fn account_detail_by_max_id_and_wallet_address<'a, E>(
        executor: E,
        wallet_address: &str,
    ) -> Result<Option<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT * FROM account")
            .and_where_eq("wallet_address", wallet_address)
            .order_by("account_id DESC")
            .limit(1)
            .fetch_optional(executor)
            .await
    }

    pub async fn account_wallet_mapping<'a, E>(
        executor: E,
    ) -> Result<Vec<AccountWalletMapping>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT DISTINCT 
                account.account_id,
                account.name,
                account.wallet_address,
                wallet.uid
            FROM 
                account
            LEFT JOIN 
                wallet
            ON 
                account.wallet_address = wallet.address;
            "#;
        sqlx::query_as::<sqlx::Sqlite, AccountWalletMapping>(sql)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_in_address<'a, E>(
        executor: E,
        addresses: &[String],
        chain_code: Option<String>,
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let builder = DynamicQueryBuilder::new("SELECT * FROM account")
            .and_where_in("address", addresses)
            .and_where_eq_opt("chain_code", chain_code);

        builder.fetch_all(executor).await
    }

    pub async fn account_list_v2<'a, E>(
        executor: E,
        wallet_address: Option<&str>,
        address: Option<&str>,
        derivation_path: Option<&str>,
        chain_codes: Vec<String>,
        account_id: Option<u32>,
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT * FROM account")
            .and_where_in("chain_code", &chain_codes)
            .and_where_eq_opt("wallet_address", wallet_address)
            .and_where_eq_opt("address", address)
            .and_where_eq_opt("derivation_path", derivation_path)
            .and_where_eq_opt("account_id", account_id)
            .fetch_all(executor)
            .await
    }

    pub async fn has_account_id<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT * FROM account")
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("account_id", account_id)
            .fetch_optional(exec)
            .await
            .map(|v: Option<AccountEntity>| v.is_some())
    }

    pub async fn reset_account<'a, E>(
        exec: E,
        wallet_address: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicUpdateBuilder::new("account")
            .set("status", 2)
            .set_raw("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
            .and_where_eq("wallet_address", wallet_address)
            .returning("*")
            .fetch_all(exec)
            .await
    }

    pub async fn reset_all_account<'a, E>(exec: E) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicUpdateBuilder::new("account")
            .set("status", 2)
            .set_raw("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
            .returning("*")
            .fetch_all(exec)
            .await
    }

    pub async fn restart_account<'a, E>(
        exec: E,
        wallet_address: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicUpdateBuilder::new("account")
            .set("status", 1)
            .set_raw("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
            .and_where_eq("wallet_address", wallet_address)
            .returning("*")
            .fetch_all(exec)
            .await
    }

    pub async fn physical_delete_all<'a, E>(
        exec: E,
        wallet_addresses: &[&str],
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        // use crate::sql_utils::SqlExecutableReturn;
        crate::sql_utils::delete_builder::DynamicDeleteBuilder::new("account")
            .and_where_in("wallet_address", wallet_addresses)
            .returning("*")
            .fetch_all(exec)
            .await
    }

    pub async fn physical_delete<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        crate::sql_utils::delete_builder::DynamicDeleteBuilder::new("account")
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("account_id", account_id)
            .returning("*")
            .fetch_all(exec)
            .await
    }

    pub async fn count_unique_account_ids<'a, E>(
        exec: E,
        wallet_address: &str,
    ) -> Result<u32, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT COUNT(DISTINCT account_id) as count FROM account")
            .and_where_eq("wallet_address", wallet_address)
            .fetch_one(exec)
            .await
            .map(|(count,)| count)
    }

    pub async fn init<'a, E>(
        exec: E,
        address: &str,
        chain_code: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicUpdateBuilder::new("account")
            .set("is_init", 1)
            .set_raw("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
            .and_where_eq("address", address)
            .and_where_eq("chain_code", chain_code)
            .returning("*")
            .fetch_all(exec)
            .await
    }

    pub async fn find_one_by_address_chain_code<'a, E>(
        address: &str,
        chain_code: &str,
        exec: E,
    ) -> Result<Option<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT * FROM account")
            .and_where_eq("address", address)
            .and_where_eq("chain_code", chain_code)
            .fetch_optional(exec)
            .await
    }

    pub async fn lists_by_wallet_address<'a, E>(
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
        exec: E,
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT * FROM account")
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("status", 1)
            .and_where_eq_opt("account_id", account_id)
            .and_where_eq_opt("chain_code", chain_code)
            .fetch_all(exec)
            .await
    }

    pub async fn get_all_account_indices<'a, E>(exec: E) -> Result<Vec<u32>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT DISTINCT account_id FROM account")
            .order_by("account_id")
            .fetch_all(exec)
            .await
            .map(|rows: Vec<(u32,)>| rows.into_iter().map(|(id,)| id).collect())
        // let sql = "SELECT DISTINCT account_id FROM account ORDER BY account_id";

        // sqlx::query_as::<_, (u32,)>(sql)
        //     .fetch_all(exec)
        //     .await
        //     .map(|rows| rows.into_iter().map(|(id,)| id).collect())
        //     .map_err(|e| crate::Error::Database(e.into()))
    }

    // let builder = DynamicQueryBuilder::new(
    //     "SELECT account.*, wallet.uid, wallet.name as wallet_name FROM account",
    // )
    // .left_join("wallet ON account.wallet_address = wallet.address")
    // .and_where_eq("account.address", address)
    // .and_where_eq("account.chain_code", chain_code);

    pub async fn account_with_wallet<'a, E>(
        address: &str,
        chain_code: &str,
        executor: E,
    ) -> Result<Option<AccountWithWalletEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new(
            "SELECT account.*, wallet.uid, wallet.name as wallet_name FROM account",
        )
        .left_join("wallet ON account.wallet_address = wallet.address")
        .and_where_eq("account.address", address)
        .and_where_eq("account.chain_code", chain_code)
        .fetch_optional(executor)
        .await

        // let sql = r#"
        //     SELECT
        //         account.*,
        //         wallet.uid,wallet.name as wallet_name
        //     FROM
        //         account
        //     LEFT JOIN
        //         wallet
        //     ON
        //         account.wallet_address = wallet.address
        //     WHERE
        //         account.address = $1 AND account.chain_code = $2;
        //     "#;
        // sqlx::query_as::<_, AccountWithWalletEntity>(sql)
        //     .bind(address)
        //     .bind(chain_code)
        //     .fetch_optional(executor)
        //     .await
        //     .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn current_chain_address<'a, E>(
        address: String,
        account_id: u32,
        chain_code: &str,
        executor: E,
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT account.* FROM account")
            .inner_join("wallet ON account.wallet_address = wallet.address")
            .and_where_eq("wallet.address", address)
            .and_where_eq("account.account_id", account_id)
            .and_where_eq("account.chain_code", chain_code)
            .fetch_all(executor)
            .await

        // let sql = r#"
        //     SELECT
        //         account.*
        //     FROM
        //         account
        //     INNER JOIN
        //         wallet
        //     ON
        //         account.wallet_address = wallet.address
        //     WHERE
        //         wallet.uid = $1 AND account.account_id = $2 AND account.chain_code = $3;
        //     "#;
        // sqlx::query_as::<_, AccountEntity>(sql)
        //     .bind(uid)
        //     .bind(account_id)
        //     .bind(chain_code)
        //     .fetch_all(executor)
        //     .await
        //     .map_err(|e| crate::Error::Database(e.into()))
    }
}
