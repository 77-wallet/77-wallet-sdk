use crate::{
    entities::{
        account::AccountEntity,
        api_account::{
            AccountToWalletAddress, ApiAccountAddressPageRow, ApiAccountEntity,
            ApiAccountWalletMapping, CreateApiAccountVo,
        },
        api_wallet::ApiWalletType,
    },
    sql_utils::{
        SqlExecutableReturn as _, query_builder::DynamicQueryBuilder,
        update_builder::DynamicUpdateBuilder,
    },
};
use sqlx::{Executor, Sqlite, SqliteConnection};

pub(crate) struct ApiAccountDao;

impl ApiAccountDao {
    /// Deletion must include inactive addresses that may still be device-bound.
    pub async fn list_for_wallet_deletion<'a, E>(
        exec: E,
        wallet_addresses: &[&str],
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if wallet_addresses.is_empty() {
            return Ok(Vec::new());
        }
        DynamicQueryBuilder::new("SELECT * FROM api_account")
            .and_where_in("wallet_address", wallet_addresses)
            .fetch_all(exec)
            .await
    }

    pub async fn deletion_page(
        conn: &mut SqliteConnection,
        wallet_addresses: &[&str],
        after_id: i64,
        through_id: i64,
    ) -> Result<Vec<crate::entities::api_account::ApiAccountDeletionRow>, crate::Error> {
        const PAGE_SIZE: usize = 1000;
        let wallets: std::collections::BTreeSet<_> = wallet_addresses.iter().copied().collect();
        let mut rows: Vec<crate::entities::api_account::ApiAccountDeletionRow> = Vec::new();
        for wallet in wallets {
            // Seek only this wallet's next page. Keep the bounded JSON transfer
            // rather than sending every account separately through SQLx.
            let page: String = sqlx::query_scalar("SELECT json_group_array(json_array(id,chain_code,address)) FROM (SELECT id,chain_code,address FROM api_account INDEXED BY api_account_wallet_id_idx WHERE wallet_address=? AND id>? AND id<=? ORDER BY id ASC LIMIT ?)")
                .bind(wallet).bind(after_id).bind(through_id).bind(PAGE_SIZE as i64)
                .fetch_one(&mut *conn).await.map_err(|e| crate::Error::Database(e.into()))?;
            let page: Vec<(i64, String, String)> = serde_json::from_str(&page).map_err(|_| {
                crate::Error::Other("invalid API wallet deletion identity page".into())
            })?;
            rows.extend(page.into_iter().map(|(id, chain_code, address)| {
                crate::entities::api_account::ApiAccountDeletionRow { id, chain_code, address }
            }));
            // The global first page must be among each wallet's first page.
            // Preserve the original cursor/hash order with at most two pages
            // temporarily held, even if an internal caller supplies many wallets.
            rows.sort_unstable_by_key(|row| row.id);
            rows.truncate(PAGE_SIZE);
        }
        Ok(rows)
    }

    /// Caller holds BEGIN IMMEDIATE so the ownership check and deletion see
    /// the same rows, including accounts without a corresponding wallet row.
    pub async fn delete_for_wallets(
        conn: &mut SqliteConnection,
        wallet_addresses: &[&str],
    ) -> Result<u64, crate::Error> {
        use crate::sql_utils::SqlQueryBuilder as _;
        if wallet_addresses.is_empty() {
            return Ok(0);
        }
        let mut outside = sqlx::QueryBuilder::<Sqlite>::new(
            "SELECT EXISTS(SELECT 1 FROM api_account INDEXED BY api_account_wallet_id_idx WHERE wallet_address NOT IN (",
        );
        {
            let mut values = outside.separated(",");
            for address in wallet_addresses {
                values.push_bind(*address);
            }
        }
        outside.push("))");
        let has_other_accounts: bool = outside
            .build_query_scalar()
            .fetch_one(&mut *conn)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        if !has_other_accounts {
            // No WHERE/RETURNING lets SQLite clear the table/index B-trees.
            // SQLite itself retains normal per-row behavior if DELETE triggers exist.
            return sqlx::query("DELETE FROM api_account")
                .execute(conn)
                .await
                .map(|result| result.rows_affected())
                .map_err(|e| crate::Error::Database(e.into()));
        }
        let (sql, args) =
            crate::sql_utils::delete_builder::DynamicDeleteBuilder::new("api_account")
                .and_where_in("wallet_address", wallet_addresses)
                .build_sql()?;
        sqlx::query_with(&sql, args)
            .execute(conn)
            .await
            .map(|result| result.rows_affected())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn account_high_watermark<'a, E>(exec: E) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM api_account")
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// Walk original persisted facts by rowid, returning one active source per
    /// logical account. A fixed cutoff excludes derived rows inserted mid-walk.
    /// Indexed EXISTS probes avoid materializing or grouping the complete table.
    pub async fn missing_chain_account_page<'a, E>(
        exec: E,
        chain: &str,
        after_id: i64,
        through_id: i64,
        limit: u32,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        Self::missing_chain_account_page_for_variants(exec, chain, after_id, through_id, limit, 1)
            .await
    }

    pub async fn missing_chain_account_page_for_variants<'a, E>(
        exec: E,
        chain: &str,
        after_id: i64,
        through_id: i64,
        limit: u32,
        expected_variants: u32,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if limit == 0 {
            return Ok(Vec::new());
        }
        sqlx::query_as::<_, ApiAccountEntity>(
            "SELECT source.* FROM api_account AS source NOT INDEXED
             WHERE source.id > ? AND source.id <= ? AND source.status = 1
             AND NOT EXISTS (
                 SELECT 1 FROM api_account AS earlier
                 WHERE earlier.wallet_address = source.wallet_address
                   AND earlier.account_id = source.account_id
                   AND earlier.api_wallet_type = source.api_wallet_type
                   AND earlier.status = 1 AND earlier.id < source.id
             )
             AND (
                 (SELECT COUNT(DISTINCT COALESCE(resolved.address_type, ''))
                  FROM api_account AS resolved
                  WHERE resolved.wallet_address = source.wallet_address
                    AND resolved.account_id = source.account_id
                    AND resolved.api_wallet_type = source.api_wallet_type
                    AND resolved.chain_code = ?
                    AND (resolved.status != 1 OR resolved.is_init = 1)) < ?
                 OR EXISTS (
                     SELECT 1 FROM api_account AS pending
                     WHERE pending.wallet_address = source.wallet_address
                       AND pending.account_id = source.account_id
                       AND pending.api_wallet_type = source.api_wallet_type
                       AND pending.chain_code = ?
                       AND pending.status = 1 AND pending.is_init = 0
                 )
             ) ORDER BY source.id ASC LIMIT ?",
        )
        .bind(after_id)
        .bind(through_id)
        .bind(chain)
        .bind(i64::from(expected_variants))
        .bind(chain)
        .bind(i64::from(limit))
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn target_uninitialized_for_logical<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: u32,
        api_wallet_type: ApiWalletType,
        chain: &str,
        limit: u32,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if limit == 0 {
            return Ok(Vec::new());
        }
        sqlx::query_as::<_, ApiAccountEntity>(
            "SELECT * FROM api_account WHERE wallet_address = ? AND account_id = ?
             AND api_wallet_type = ? AND chain_code = ? AND status = 1 AND is_init = 0
             ORDER BY id ASC LIMIT ?",
        )
        .bind(wallet_address)
        .bind(account_id)
        .bind(api_wallet_type)
        .bind(chain)
        .bind(i64::from(limit))
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_active_address_page_by_wallet<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: Option<u32>,
        after_id: i64,
        limit: u32,
    ) -> Result<Vec<ApiAccountAddressPageRow>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            // Force a rowid-range walk. Existing wallet indexes do not end in `id`,
            // so letting SQLite choose one can require sorting every remaining row
            // for each page before applying LIMIT.
            "SELECT id, address FROM api_account NOT INDEXED WHERE wallet_address = ",
        );
        qb.push_bind(wallet_address).push(" AND status = 1 AND id > ").push_bind(after_id);
        if let Some(account_id) = account_id {
            qb.push(" AND account_id = ").push_bind(account_id);
        }
        qb.push(" ORDER BY id ASC LIMIT ").push_bind(i64::from(limit));

        qb.build_query_as::<ApiAccountAddressPageRow>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 插入多个账户（存在则更新 updated_at）
    pub async fn upsert_multi<'a>(
        exec: &mut SqliteConnection,
        reqs: Vec<CreateApiAccountVo>,
    ) -> Result<(), crate::Error> {
        if reqs.is_empty() {
            return Ok(());
        }

        const BATCH_SIZE: usize = 1000;
        tracing::info!(count = %reqs.len(), "ApiAccountDao: starting upsert_multi");

        for (batch_idx, chunk) in reqs.chunks(BATCH_SIZE).enumerate() {
            tracing::debug!(batch_idx = %batch_idx, batch_size = %chunk.len(), "ApiAccountDao: processing batch");

            let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
                "INSERT INTO api_account ( 
                    account_id, name, address, pubkey, address_type,
                    wallet_address, uid, derivation_path, derivation_path_index,
                    chain_code, api_wallet_type, status, is_init, is_used, 
                    created_at, updated_at
                ) ",
            );

            qb.push_values(chunk, |mut b, item| {
                b.push_bind(item.account_id)
                    .push_bind(item.name.clone())
                    .push_bind(item.address.clone())
                    .push_bind(item.pubkey.clone())
                    .push_bind(item.address_type.clone())
                    .push_bind(item.wallet_address.clone())
                    .push_bind(item.uid.clone())
                    .push_bind(item.derivation_path.clone())
                    .push_bind(item.derivation_path_index)
                    .push_bind(item.chain_code.clone())
                    .push_bind(item.api_wallet_type)
                    .push_bind(1)
                    .push_bind(item.is_init)
                    .push_bind(false)
                    .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
                    .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')");
            });

            qb.push(
                " ON CONFLICT(address, chain_code, address_type)
                  DO UPDATE SET updated_at = excluded.updated_at",
            );

            let result = qb
                .build()
                .execute(&mut *exec)
                .await
                .map_err(|e| crate::Error::Database(e.into()))?;

            tracing::debug!(batch_idx = %batch_idx, rows_affected = %result.rows_affected(), "ApiAccountDao: batch completed");
        }

        tracing::info!(count = %reqs.len(), "ApiAccountDao: upsert_multi completed");
        Ok(())
    }

    pub async fn lists_by_wallet_address<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        tracing::info!(
            wallet_address = %wallet_address,
            account_id = ?account_id,
            chain_code = ?chain_code,
            sql = "SELECT * FROM api_account WHERE wallet_address = ? AND status = 1 ...",
            "API_ACCOUNT_QUERY::lists_by_wallet_address"
        );
        DynamicQueryBuilder::new("SELECT * FROM api_account")
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("status", 1)
            .and_where_eq_opt("account_id", account_id)
            .and_where_eq_opt("chain_code", chain_code)
            .fetch_all(exec)
            .await
    }

    pub async fn count_by_wallet_address<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(*) as count FROM api_account WHERE wallet_address = ",
        );
        qb.push_bind(wallet_address);
        qb.push(" AND status = 1");
        if let Some(account_id) = account_id {
            qb.push(" AND account_id = ").push_bind(account_id);
        }
        if let Some(chain_code) = chain_code {
            qb.push(" AND chain_code = ").push_bind(chain_code);
        }

        qb.build_query_as::<(i64,)>()
            .fetch_one(exec)
            .await
            .map(|(count,)| count)
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn count_by_wallet_address_up_to<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
        limit: i64,
    ) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(*) FROM (SELECT 1 FROM api_account INDEXED BY api_account_wallet_status_idx WHERE wallet_address = ",
        );
        qb.push_bind(wallet_address);
        qb.push(" AND status = 1");
        if let Some(account_id) = account_id {
            qb.push(" AND account_id = ").push_bind(account_id);
        }
        if let Some(chain_code) = chain_code {
            qb.push(" AND chain_code = ").push_bind(chain_code);
        }
        qb.push(" LIMIT ").push_bind(limit.max(0));
        qb.push(")");

        qb.build_query_as::<(i64,)>()
            .fetch_one(exec)
            .await
            .map(|(count,)| count)
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn count_by_status<'a, E>(exec: E, status: i32) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT COUNT(1) as count FROM api_account WHERE status = ?";

        sqlx::query_as::<sqlx::Sqlite, (i64,)>(sql)
            .bind(status)
            .fetch_one(exec)
            .await
            .map(|(count,)| count)
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn exists_by_chain_code<'a, E>(
        exec: E,
        chain_code: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT 1 FROM api_account WHERE chain_code = ? LIMIT 1";

        let row = sqlx::query_as::<sqlx::Sqlite, (i64,)>(sql)
            .bind(chain_code)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(row.is_some())
    }

    pub async fn physical_delete_all<'a, E>(
        exec: E,
        wallet_addresses: &[&str],
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        // use crate::sql_utils::SqlExecutableReturn;
        crate::sql_utils::delete_builder::DynamicDeleteBuilder::new("api_account")
            .and_where_in("wallet_address", wallet_addresses)
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
        DynamicQueryBuilder::new("SELECT COUNT(DISTINCT account_id) as count FROM api_account")
            .and_where_eq("wallet_address", wallet_address)
            .fetch_one(exec)
            .await
            .map(|(count,)| count)
    }

    pub async fn physical_delete<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        crate::sql_utils::delete_builder::DynamicDeleteBuilder::new("api_account")
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("account_id", account_id)
            .returning("*")
            .fetch_all(exec)
            .await
    }

    /// 标记is_used
    pub async fn update_is_used<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: u32,
        chain_code: &str,
        is_used: bool,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_account SET 
                is_used = $3,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE wallet_address = $1 AND account_id = $2 AND chain_code = $4
            RETURNING *
        "#;

        sqlx::query_as::<_, ApiAccountEntity>(sql)
            .bind(wallet_address)
            .bind(account_id)
            .bind(is_used)
            .bind(chain_code)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_inited_indices<'a, E>(
        exec: E,
        wallet_address: &str,
        chain_code: &str,
    ) -> Result<Vec<(i32,)>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT DISTINCT derivation_path_index FROM api_account")
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("chain_code", chain_code)
            .and_where_eq("is_init", 1)
            .fetch_all(exec)
            .await
    }

    pub async fn list_inited_indices_by_candidates<'a, E>(
        exec: E,
        wallet_address: &str,
        chain_code: &str,
        candidates: &[i32],
    ) -> Result<Vec<(i32,)>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            "SELECT DISTINCT derivation_path_index FROM api_account WHERE wallet_address = ",
        );
        qb.push_bind(wallet_address);
        qb.push(" AND chain_code = ").push_bind(chain_code);
        qb.push(" AND is_init = 1 AND derivation_path_index IN (");
        {
            let mut separated = qb.separated(", ");
            for idx in candidates {
                separated.push_bind(idx);
            }
        }
        qb.push(")");

        qb.build_query_as::<(i32,)>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 根据 address + chain_code + address_type 精确查找
    pub async fn find_one<'a, E>(
        exec: E,
        address: &str,
        chain_code: &str,
        address_type: &str,
        api_wallet_type: ApiWalletType,
    ) -> Result<Option<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT * FROM api_account")
            .and_where_eq("address", address)
            .and_where_eq("chain_code", chain_code)
            .and_where_eq("address_type", address_type)
            .and_where_eq("api_wallet_type", api_wallet_type)
            .fetch_optional(exec)
            .await
    }

    pub async fn api_account_list<'a, E>(
        executor: E,
        wallet_address: Option<String>,
        account_id: Option<u32>,
        chain_codes: Vec<String>,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        tracing::info!(
            wallet_address = ?wallet_address,
            account_id = ?account_id,
            chain_codes = ?chain_codes,
            sql = "SELECT id, account_id, name, address, pubkey, address_type, wallet_address, uid, derivation_path, derivation_path_index, chain_code, api_wallet_type, status, is_init, is_expand, is_used, created_at, updated_at FROM api_account WHERE chain_code IN (...) AND status = 1 ...",
            "API_ACCOUNT_QUERY::api_account_list"
        );
        DynamicQueryBuilder::new("SELECT id, account_id, name, address, pubkey, address_type, wallet_address, uid, derivation_path, derivation_path_index, chain_code, api_wallet_type, status, is_init, is_expand, is_used, created_at, updated_at FROM api_account")
            .and_where_in("chain_code", &chain_codes)
            .and_where_eq("status", 1)
            .and_where_eq_opt("wallet_address", wallet_address)
            .and_where_eq_opt("account_id", account_id)
            .fetch_all(executor)
            .await
    }

    pub async fn find_all_by_wallet_address_index<'a, E>(
        exec: E,
        wallet_address: &str,
        chain_code: &str,
        account_id: u32,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let builder = DynamicQueryBuilder::new(
            "SELECT id, account_id, name, address, pubkey, address_type, wallet_address, uid, derivation_path, derivation_path_index, chain_code, api_wallet_type, status, is_init, is_expand, is_used, created_at, updated_at FROM api_account",
        );

        builder
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("chain_code", chain_code)
            .and_where_eq("account_id", account_id)
            .fetch_all(exec)
            .await
    }

    pub async fn has_account_id<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: u32,
        api_wallet_type: ApiWalletType,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        tracing::info!(
            wallet_address = %wallet_address,
            account_id,
            api_wallet_type = ?api_wallet_type,
            "API_ACCOUNT_QUERY::has_account_id"
        );
        DynamicQueryBuilder::new("SELECT * FROM api_account")
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("account_id", account_id)
            .and_where_eq("api_wallet_type", api_wallet_type)
            .fetch_optional(exec)
            .await
            .map(|v: Option<ApiAccountEntity>| v.is_some())
    }

    pub async fn account_detail_by_max_id_and_wallet_address<'a, E>(
        executor: E,
        wallet_address: &str,
        api_wallet_type: ApiWalletType,
    ) -> Result<Option<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        tracing::info!(
            wallet_address = %wallet_address,
            api_wallet_type = ?api_wallet_type,
            "API_ACCOUNT_QUERY::account_detail_by_max_id_and_wallet_address"
        );
        DynamicQueryBuilder::new("SELECT * FROM api_account")
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("api_wallet_type", api_wallet_type)
            .order_by("account_id DESC")
            .limit(1)
            .fetch_optional(executor)
            .await
    }

    pub async fn find_one_by_address_chain_code<'a, E>(
        address: &str,
        chain_code: &str,
        exec: E,
    ) -> Result<Option<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        tracing::debug!(
            address = %address,
            chain_code = %chain_code,
            "API_ACCOUNT_QUERY::find_one_by_address_chain_code"
        );
        DynamicQueryBuilder::new("SELECT * FROM api_account")
            .and_where_eq("address", address)
            .and_where_eq("chain_code", chain_code)
            .and_where_eq("status", 1)
            .fetch_optional(exec)
            .await
    }

    pub async fn find_one_by_address<'a, E>(
        address: &str,
        chain_code: &str,
        exec: E,
    ) -> Result<Option<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        tracing::debug!(
            address = %address,
            chain_code = %chain_code,
            "API_ACCOUNT_QUERY::find_one_by_address"
        );
        DynamicQueryBuilder::new("SELECT * FROM api_account")
            .and_where_eq("address", address)
            .and_where_eq("chain_code", chain_code)
            .and_where_eq("status", 1)
            .fetch_optional(exec)
            .await
    }

    /// 批量查询账户（通过地址列表）
    pub async fn find_by_addresses<'a, E>(
        addresses: &[String],
        exec: E,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if addresses.is_empty() {
            return Ok(Vec::new());
        }

        tracing::info!(address_count = addresses.len(), "API_ACCOUNT_QUERY::find_by_addresses");
        DynamicQueryBuilder::new("SELECT id, account_id, name, address, pubkey, address_type, wallet_address, uid, derivation_path, derivation_path_index, chain_code, api_wallet_type, status, is_init, is_expand, is_used, created_at, updated_at FROM api_account")
            .and_where_in("address", addresses)
            .and_where_eq("status", 1)
            .fetch_all(exec)
            .await
    }

    pub async fn find_one_by_wallet_address_account_id_chain_code<'a, E>(
        wallet_address: &str,
        account_id: u32,
        chain_code: &str,
        exec: E,
    ) -> Result<Option<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        tracing::debug!(
            wallet_address = %wallet_address,
            account_id,
            chain_code = %chain_code,
            "API_ACCOUNT_QUERY::find_one_by_wallet_address_account_id_chain_code"
        );
        DynamicQueryBuilder::new("SELECT * FROM api_account")
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("account_id", account_id)
            .and_where_eq("chain_code", chain_code)
            .fetch_optional(exec)
            .await
    }

    pub async fn get_all_account_indices<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
    ) -> Result<Vec<u32>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new(
            "SELECT DISTINCT account_id FROM api_account
            LEFT JOIN api_wallet ON
               api_account.wallet_address = api_wallet.address
        ",
        )
        .and_where_eq("api_account.api_wallet_type", ApiWalletType::SubAccount)
        .and_where_eq("api_wallet.uid", uid)
        .and_where_eq("chain_code", chain_code)
        .order_by("account_id")
        .fetch_all(exec)
        .await
        .map(|rows: Vec<(u32,)>| rows.into_iter().map(|(id,)| id).collect())
    }

    pub async fn init<'a, E>(
        exec: E,
        address: &str,
        chain_code: &str,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicUpdateBuilder::new("api_account")
            .set("is_init", 1)
            .set_raw("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
            .and_where_eq("address", address)
            .and_where_eq("chain_code", chain_code)
            .returning("*")
            .fetch_all(exec)
            .await
    }

    pub async fn init_many<'a, E>(exec: E, pairs: &[(String, String)]) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if pairs.is_empty() {
            return Ok(0);
        }

        let placeholders = pairs.iter().map(|_| "(?, ?)").collect::<Vec<_>>().join(", ");

        let sql = format!(
            r#"
        UPDATE api_account
        SET
            is_init = 1,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE (address, chain_code) IN ({})
        "#,
            placeholders
        );

        let mut query = sqlx::query(&sql);

        for (address, chain_code) in pairs {
            query = query.bind(address).bind(chain_code);
        }

        let res = query.execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        tracing::info!(rows = %res.rows_affected(), "api_account init batch");

        Ok(res.rows_affected())
    }

    pub async fn expand<'a, E>(
        exec: E,
        address: &str,
        chain_code: &str,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicUpdateBuilder::new("api_account")
            .set("is_expand", 1)
            .set_raw("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
            .and_where_eq("address", address)
            .and_where_eq("chain_code", chain_code)
            .returning("*")
            .fetch_all(exec)
            .await
    }

    pub async fn account_list<'a, E>(
        executor: E,
        wallet_address: Option<&str>,
        address: Option<&str>,
        derivation_path: Option<&str>,
        chain_codes: Vec<String>,
        account_id: Option<u32>,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        tracing::info!("API_ACCOUNT_QUERY::account_list");
        DynamicQueryBuilder::new("SELECT * FROM api_account")
            .and_where_in("chain_code", &chain_codes)
            .and_where_eq_opt("wallet_address", wallet_address)
            .and_where_eq_opt("address", address)
            .and_where_eq_opt("derivation_path", derivation_path)
            .and_where_eq_opt("account_id", account_id)
            .fetch_all(executor)
            .await
    }

    pub async fn account_to_wallet<'a, E>(
        executor: E,
    ) -> Result<Vec<AccountToWalletAddress>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        tracing::info!("API_ACCOUNT_QUERY::account_to_wallet");
        DynamicQueryBuilder::new("SELECT address, wallet_address FROM api_account")
            .fetch_all(executor)
            .await
    }

    pub async fn account_wallet_mapping<'a, E>(
        executor: E,
        api_wallet_type: Option<ApiWalletType>,
    ) -> Result<Vec<ApiAccountWalletMapping>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        // let sql = r#"
        //     SELECT DISTINCT
        //         api_account.account_id,
        //         api_account.name,
        //         api_account.address,
        //         api_account.wallet_address,
        //         api_wallet.uid
        //     FROM
        //         api_account
        //     LEFT JOIN
        //         api_wallet
        //     ON
        //         api_account.wallet_address = api_wallet.address;
        //     "#;
        // sqlx::query_as::<sqlx::Sqlite, AccountWalletMapping>(sql)
        //     .fetch_all(executor)
        //     .await
        //     .map_err(|e| crate::Error::Database(e.into()))

        DynamicQueryBuilder::new(
            "SELECT DISTINCT 
                api_account.account_id,
                api_account.name,
                api_account.address,
                api_account.wallet_address,
                api_wallet.uid,
                api_wallet.seed,
                api_wallet.api_wallet_type
            FROM 
                api_account
            LEFT JOIN 
                api_wallet
            ON 
                api_account.wallet_address = api_wallet.address
            ",
        )
        .and_where_eq_opt("api_wallet.api_wallet_type", api_wallet_type)
        .fetch_all(executor)
        .await
    }

    pub async fn edit_account_name<'a, E>(
        executor: E,
        wallet_address: &str,
        account_id: u32,
        name: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicUpdateBuilder::new("api_account")
            .set("name", name)
            .set_raw("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
            .and_where_eq("wallet_address", wallet_address)
            .and_where_eq("account_id", account_id)
            .returning("*")
            .fetch_all(executor)
            .await
    }

    pub async fn lists_by_wallet_address_v2<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        page: i64,
        page_size: i64,
    ) -> Result<Vec<ApiAccountSummeryEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let limit = page_size;
        let mut offset = page_size * page;
        if offset < 0 {
            offset = 0;
        }

        let start = std::time::Instant::now();
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            r#"
select * from(
SELECT
all_data.account_id 				                    AS account_id,
all_data.name 							                AS account_name,
all_data.api_wallet_type 		                        AS api_wallet_type,
all_data.wallet_address 		                        AS wallet_address,
CAST(SUM(all_data.total_coin_quantity) AS REAL) 		AS total_coins_quantity,
CAST(all_data.coin_unit_price AS REAL) AS coin_unit_price,
CAST(SUM(total_coin_amount) AS REAL)			AS total_account_amount,
JSON_GROUP_ARRAY(
    JSON_OBJECT(
        'account_address', all_data.address,
        'wallet_address', all_data.wallet_address,
        'derivation_path', all_data.derivation_path,
				'chain_code', all_data.chain_code,
				'chain_name', all_data.chain_name,
				'address_type', all_data.address_type,
				'coin_name', all_data.coin_name,
				'created_at', all_data.created_at,
				'updated_at', all_data.updated_at
    )
) AS chain_info_list
FROM
(
SELECT
api_account.account_id,api_account.name,api_account.address,api_account.derivation_path,api_account.address_type,
api_account.api_wallet_type,api_account.wallet_address,api_account.address, api_account.chain_code,api_account.created_at,
api_account.updated_at,
api_assets.token_address,api_assets.balance,
api_chain.name 														AS chain_name,
api_coin.price 														AS coin_unit_price,
api_coin.name 														AS coin_name,
SUM(api_assets.balance)  									AS total_coin_quantity,
api_coin.price * SUM(api_assets.balance)  AS total_coin_amount
FROM  api_assets
LEFT JOIN api_account
ON api_assets.address = api_account.address AND api_account.chain_code = api_assets.chain_code
LEFT JOIN api_coin
ON api_coin.chain_code=api_assets.chain_code AND api_coin.token_address=api_assets.token_address
LEFT JOIN  api_chain
ON api_chain.chain_code=api_assets.chain_code
WHERE api_chain.status =1
"#,
        );

        qb.push(" AND api_account.wallet_address = ").push_bind(wallet_address);
        if let Some(account_id) = account_id {
            qb.push(" AND api_account.account_id = ").push_bind(account_id);
        }
        if let Some(chain_code) = chain_code {
            qb.push(" AND api_account.chain_code = ").push_bind(chain_code);
        }

        qb.push(
            r#"
GROUP BY api_account.wallet_address,api_account.account_id,api_account.chain_code,api_assets.token_address
)AS all_data
GROUP BY all_data.wallet_address,all_data.account_id
) as all_datas
ORDER BY total_account_amount DESC
LIMIT "#,
        )
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);

        let res = qb
            .build_query_as::<ApiAccountSummeryEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()));

        tracing::info!(
            elapsed_ms = start.elapsed().as_millis(),
            "ApiAccountDao: lists_by_wallet_address_v2"
        );
        res
    }

    pub async fn count_by_wallet_address_v2<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
    ) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let start = std::time::Instant::now();
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            r#"
SELECT
count(1) as total_count
FROM
(
SELECT
all_data.account_id 				AS account_id,
all_data.name 							AS account_name,
all_data.api_wallet_type 		AS api_wallet_type,
all_data.wallet_address 		AS wallet_address,
CAST(SUM(all_data.total_coin_quantity) AS REAL) 		AS total_coins_quantity,
CAST(all_data.coin_unit_price AS REAL) AS coin_unit_price,
CAST(SUM(total_coin_amount) AS REAL)			AS total_account_amount
FROM
(
SELECT
api_account.account_id,api_account.name,api_account.api_wallet_type,api_account.wallet_address,api_account.address, api_account.chain_code,
api_assets.token_address,api_assets.balance,
api_chain.name 														AS api_chain_name,
api_coin.price 														AS coin_unit_price,
SUM(api_assets.balance)  									AS total_coin_quantity,
api_coin.price * SUM(api_assets.balance)  AS total_coin_amount
FROM api_assets
LEFT JOIN api_account
ON api_assets.address = api_account.address AND api_account.chain_code = api_assets.chain_code
LEFT JOIN api_coin
ON api_coin.chain_code=api_assets.chain_code AND api_coin.token_address=api_assets.token_address
LEFT JOIN api_chain
ON api_chain.chain_code=api_assets.chain_code
WHERE api_chain.status =1
"#,
        );

        qb.push(" AND api_account.wallet_address = ").push_bind(wallet_address);
        if let Some(account_id) = account_id {
            qb.push(" AND api_account.account_id = ").push_bind(account_id);
        }
        if let Some(chain_code) = chain_code {
            qb.push(" AND api_account.chain_code = ").push_bind(chain_code);
        }

        qb.push(
            r#"
GROUP BY api_account.wallet_address,api_account.account_id,api_account.chain_code,api_assets.token_address
)AS all_data
GROUP BY all_data.wallet_address,all_data.account_id
)AS all_data2
        "#,
        );

        #[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
        struct CountResult {
            total_count: i64,
        }

        let res = qb
            .build_query_as::<CountResult>()
            .fetch_one(exec)
            .await
            .map(|o| o.total_count)
            .map_err(|e| crate::Error::Database(e.into()));

        tracing::info!(
            elapsed_ms = start.elapsed().as_millis(),
            "ApiAccountDao: count_by_wallet_address_v2"
        );
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{api_account::CreateApiAccountVo, api_wallet::ApiWalletType};

    fn make_temp_dir(prefix: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "{}_{}_{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().to_string()
    }

    async fn insert_accounts(
        pool: &sqlx::Pool<sqlx::Sqlite>,
        reqs: Vec<CreateApiAccountVo>,
    ) -> Result<(), crate::Error> {
        let mut tx = pool.begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        ApiAccountDao::upsert_multi(tx.as_mut(), reqs).await?;
        tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

    #[tokio::test]
    async fn list_inited_indices_by_candidates_filters_by_candidates_and_is_init() {
        let dir = make_temp_dir("wallet_db_api_account_inited_candidates");
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();

        let reqs = vec![
            CreateApiAccountVo::new(
                0,
                "0xaddr0",
                "pk0",
                "wallet_1",
                "uid_1",
                "m/44'/60'/0'/0/0",
                0,
                "eth",
                "账户0",
                ApiWalletType::SubAccount,
            )
            .with_is_init(true),
            CreateApiAccountVo::new(
                1,
                "0xaddr1",
                "pk1",
                "wallet_1",
                "uid_1",
                "m/44'/60'/0'/0/1",
                1,
                "eth",
                "账户1",
                ApiWalletType::SubAccount,
            )
            .with_is_init(false),
            CreateApiAccountVo::new(
                2,
                "0xaddr2",
                "pk2",
                "wallet_1",
                "uid_1",
                "m/44'/60'/0'/0/2",
                2,
                "eth",
                "账户2",
                ApiWalletType::SubAccount,
            )
            .with_is_init(true),
            CreateApiAccountVo::new(
                3,
                "0xaddr3",
                "pk3",
                "wallet_1",
                "uid_1",
                "m/44'/60'/0'/0/3",
                3,
                "bsc",
                "账户3",
                ApiWalletType::SubAccount,
            )
            .with_is_init(true),
        ];
        insert_accounts(pool.as_ref(), reqs).await.unwrap();

        let mut rows = ApiAccountDao::list_inited_indices_by_candidates(
            pool.as_ref(),
            "wallet_1",
            "eth",
            &[0, 1, 3],
        )
        .await
        .unwrap();
        rows.sort_by_key(|(idx,)| *idx);

        assert_eq!(rows, vec![(0,)]);
    }

    #[tokio::test]
    async fn list_inited_indices_by_candidates_returns_empty_on_empty_candidates() {
        let dir = make_temp_dir("wallet_db_api_account_inited_empty_candidates");
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();

        let rows =
            ApiAccountDao::list_inited_indices_by_candidates(pool.as_ref(), "wallet_1", "eth", &[])
                .await
                .unwrap();

        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn api_account_list_only_returns_active_status() {
        let dir = make_temp_dir("wallet_db_api_account_list_status");
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();

        let reqs = vec![
            CreateApiAccountVo::new(
                0,
                "0xactive",
                "pk0",
                "wallet_1",
                "uid_1",
                "m/44'/60'/0'/0/0",
                0,
                "eth",
                "active",
                ApiWalletType::SubAccount,
            ),
            CreateApiAccountVo::new(
                1,
                "0xinactive",
                "pk1",
                "wallet_1",
                "uid_1",
                "m/44'/60'/0'/0/1",
                1,
                "eth",
                "inactive",
                ApiWalletType::SubAccount,
            ),
        ];
        insert_accounts(pool.as_ref(), reqs).await.unwrap();

        sqlx::query("UPDATE api_account SET status = 0 WHERE address = ?")
            .bind("0xinactive")
            .execute(pool.as_ref())
            .await
            .unwrap();

        let rows = ApiAccountDao::api_account_list(
            pool.as_ref(),
            Some("wallet_1".to_string()),
            None,
            vec!["eth".to_string()],
        )
        .await
        .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].address, "0xactive");
        assert_eq!(rows[0].status, 1);

        let active_count = ApiAccountDao::count_by_status(pool.as_ref(), 1).await.unwrap();
        let inactive_count = ApiAccountDao::count_by_status(pool.as_ref(), 0).await.unwrap();
        assert_eq!(active_count, 1);
        assert_eq!(inactive_count, 1);
    }

    #[tokio::test]
    async fn search_address_by_uid_filters_scope_and_status() {
        let dir = make_temp_dir("wallet_db_api_account_search_uid");
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();
        let target_address = "0x17f6a199862FD0ffb2d5C79f3DBBE37597162A24";

        let reqs = vec![
            CreateApiAccountVo::new(
                0,
                target_address,
                "pk0",
                "wallet_1",
                "uid_1",
                "m/44'/60'/0'/0/0",
                0,
                "eth",
                "match",
                ApiWalletType::SubAccount,
            ),
            CreateApiAccountVo::new(
                1,
                target_address,
                "pk1",
                "wallet_2",
                "uid_2",
                "m/44'/60'/0'/0/1",
                1,
                "eth",
                "wrong_uid",
                ApiWalletType::SubAccount,
            ),
            CreateApiAccountVo::new(
                2,
                "0x27f6a199862FD0ffb2d5C79f3DBBE37597162A24",
                "pk2",
                "wallet_1",
                "uid_1",
                "m/44'/60'/0'/0/2",
                2,
                "eth",
                "inactive",
                ApiWalletType::SubAccount,
            ),
        ];
        insert_accounts(pool.as_ref(), reqs).await.unwrap();

        sqlx::query("UPDATE api_account SET status = 0 WHERE account_id = ?")
            .bind(2_u32)
            .execute(pool.as_ref())
            .await
            .unwrap();

        let rows = ApiAccountDao::search_address_by_uid(
            pool.as_ref(),
            "uid_1",
            &target_address.to_lowercase(),
        )
        .await
        .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].uid, "uid_1");
        assert_eq!(rows[0].wallet_address, "wallet_1");
        assert_eq!(rows[0].address, target_address);
        assert_eq!(rows[0].status, 1);

        let inactive_rows = ApiAccountDao::search_address_by_uid(
            pool.as_ref(),
            "uid_1",
            "0x27f6a199862fd0ffb2d5c79f3dbbe37597162a24",
        )
        .await
        .unwrap();
        assert!(inactive_rows.is_empty());
    }
}
// all_data.account_id 				        AS account_id,
// all_data.name 							AS account_name,
// all_data.api_wallet_type 		        AS api_wallet_type,
// all_data.wallet_address 		            AS wallet_address,
// SUM(all_data.total_coin_quantity) 		AS total_coins_quantity,
// all_data.coin_unit_price 		        AS coin_unit_price,
// SUM(total_coin_amount)			        AS total_account_amount
#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ApiAccountSummeryEntity {
    pub account_id: u32,
    pub account_name: String,
    pub api_wallet_type: ApiWalletType,
    pub coin_unit_price: Option<f64>,
    pub total_coins_quantity: f64,
    pub total_account_amount: Option<f64>,
    pub chain_info_list: serde_json::Value,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ChainInfoEntity {
    pub account_address: String,
    pub wallet_address: String,
    pub derivation_path: String,
    pub chain_code: String,
    pub chain_name: Option<String>,
    pub coin_name: Option<String>,
    pub address_type: String,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}
impl ChainInfoEntity {
    pub fn address_type(&self) -> Option<String> {
        (!self.address_type.is_empty()).then(|| self.address_type.clone())
    }
}

impl ApiAccountSummeryEntity {
    pub fn get_chain_info_list(&self) -> Result<Vec<ChainInfoEntity>, crate::Error> {
        if let serde_json::Value::Array(arr) = &self.chain_info_list {
            let mut result: Vec<ChainInfoEntity> = Vec::new();
            for item in arr {
                // tracing::info!("item: {:?}", item);
                let chain_info: Result<ChainInfoEntity, _> = serde_json::from_value(item.clone());

                match chain_info {
                    Ok(info) => {
                        result.push(info);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to deserialize chain info: {}", e)
                    }
                }
            }
            Ok(result)
        } else {
            Ok(vec![])
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApiAccountEntitySummer {
    pub account_id: u32,
}

impl ApiAccountDao {
    pub async fn lists_acc_by_wallet_address_v3<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        page: i64,
        page_size: i64,
    ) -> Result<Vec<ApiAccountEntitySummer>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let limit = page_size;
        let mut offset = page_size * page;
        if offset < 0 {
            offset = 0;
        }
        let start = std::time::Instant::now();
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            r#"
select account_id from
api_account
WHERE api_account.wallet_address =
"#,
        );
        qb.push_bind(wallet_address);
        qb.push(" AND api_account.api_wallet_type IN (1, 2)");
        if let Some(account_id) = account_id {
            qb.push(" AND api_account.account_id = ").push_bind(account_id);
        }
        if let Some(chain_code) = chain_code {
            qb.push(" AND api_account.chain_code = ").push_bind(chain_code);
        }
        qb.push(
            r#"
GROUP BY api_account.account_id
ORDER BY api_account.account_id ASC
LIMIT "#,
        )
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);

        let res = qb
            .build_query_as::<ApiAccountEntitySummer>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()));

        tracing::debug!(
            elapsed_ms = start.elapsed().as_millis(),
            "ApiAccountDao: lists_acc_by_wallet_address_v3"
        );
        res
    }

    pub async fn lists_by_wallet_address_v3<'a, E>(
        exec: E,
        wallet_address: &str,
        account_ids: Vec<u32>,
        chain_code: Option<String>,
    ) -> Result<Vec<ApiAccountSummeryEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if account_ids.is_empty() {
            return Ok(vec![]);
        }
        let start = std::time::Instant::now();
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            r#"
select * from(
SELECT
all_data.account_id 				                    AS account_id,
all_data.name 							                AS account_name,
all_data.api_wallet_type 		                        AS api_wallet_type,
all_data.wallet_address 		                        AS wallet_address,
CAST(SUM(all_data.total_coin_quantity) AS REAL) 		AS total_coins_quantity,
CAST(all_data.coin_unit_price AS REAL) AS coin_unit_price,
CAST(SUM(total_coin_amount) AS REAL)			AS total_account_amount,
JSON_GROUP_ARRAY(
    JSON_OBJECT(
        'account_address', all_data.address,
        'wallet_address', all_data.wallet_address,
        'derivation_path', all_data.derivation_path,
				'chain_code', all_data.chain_code,
				'chain_name', all_data.chain_name,
				'address_type', all_data.address_type,
				'coin_name', all_data.coin_name,
				'created_at', all_data.created_at,
				'updated_at', all_data.updated_at
    )
) AS chain_info_list
FROM
(
SELECT
api_account.account_id,api_account.name,api_account.address,api_account.derivation_path,api_account.address_type,
api_account.api_wallet_type,api_account.wallet_address,api_account.address, api_account.chain_code,api_account.created_at,
api_account.updated_at,
api_assets.token_address,api_assets.balance,
api_chain.name 														AS chain_name,
api_coin.price 														AS coin_unit_price,
api_coin.name 														AS coin_name,
SUM(api_assets.balance)  									AS total_coin_quantity,
api_coin.price * SUM(api_assets.balance)  AS total_coin_amount
FROM  api_assets
LEFT JOIN api_account
ON api_assets.address = api_account.address AND api_account.chain_code = api_assets.chain_code
LEFT JOIN api_coin
ON api_coin.chain_code=api_assets.chain_code AND api_coin.token_address=api_assets.token_address
LEFT JOIN  api_chain
ON api_chain.chain_code=api_assets.chain_code
WHERE api_chain.status =1
AND api_account.api_wallet_type IN (1, 2)
"#,
        );

        qb.push(" AND api_account.wallet_address = ").push_bind(wallet_address);
        qb.push(" AND api_account.account_id in (");
        let mut separated = qb.separated(", ");
        for account_id in account_ids {
            separated.push_bind(account_id);
        }
        separated.push_unseparated(")");

        if let Some(chain_code) = chain_code {
            qb.push(" AND api_account.chain_code = ").push_bind(chain_code);
        }

        qb.push(
            r#"
GROUP BY api_account.wallet_address,api_account.account_id,api_account.chain_code,api_assets.token_address
)AS all_data
GROUP BY all_data.wallet_address,all_data.account_id
) as all_datas
ORDER BY account_id ASC
        "#,
        );

        let res = qb
            .build_query_as::<ApiAccountSummeryEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()));

        tracing::debug!(
            elapsed_ms = start.elapsed().as_millis(),
            "ApiAccountDao: lists_by_wallet_address_v3"
        );
        res
    }

    pub async fn count_by_wallet_address_v3<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
    ) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let start = std::time::Instant::now();
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            r#"
SELECT
count(1) as total_count
FROM
(
select account_id from
api_account
WHERE api_account.wallet_address =
"#,
        );
        qb.push_bind(wallet_address);
        qb.push(" AND api_account.api_wallet_type IN (1, 2)");
        if let Some(account_id) = account_id {
            qb.push(" AND api_account.account_id = ").push_bind(account_id);
        }
        if let Some(chain_code) = chain_code {
            qb.push(" AND api_account.chain_code = ").push_bind(chain_code);
        }
        qb.push(
            r#"
GROUP BY api_account.account_id
) as all_data
        "#,
        );
        #[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
        struct CountResult {
            total_count: i64,
        }

        let res = qb
            .build_query_as::<CountResult>()
            .fetch_one(exec)
            .await
            .map(|o| o.total_count)
            .map_err(|e| crate::Error::Database(e.into()));

        tracing::info!(
            elapsed_ms = start.elapsed().as_millis(),
            "ApiAccountDao: count_by_wallet_address_v3"
        );
        res
    }

    /// 检查指定的 wallet_address、chain_code 和 account_id 是否存在
    pub async fn exists_address<'a, E>(
        exec: E,
        wallet_address: &str,
        chain_code: &str,
        account_id: u32,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"SELECT 1 FROM api_account WHERE wallet_address = $1 AND chain_code = $2 AND account_id = $3 LIMIT 1"#;

        let result = sqlx::query_scalar::<_, i32>(sql)
            .bind(wallet_address)
            .bind(chain_code)
            .bind(account_id)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(result.is_some())
    }

    /// 地址搜索：在指定 API 钱包 uid 范围内搜索地址，支持大小写不敏感匹配
    pub async fn search_address_by_uid<'a, E>(
        exec: E,
        uid: &str,
        keyword: &str,
    ) -> Result<Vec<ApiAccountEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        tracing::info!(
            uid = %uid,
            keyword = %keyword,
            "API_ACCOUNT_QUERY::search_address_by_uid"
        );

        // 对关键词进行规范化处理（转为小写用于 EVM/TRON 地址匹配）
        let keyword_lower = keyword.to_lowercase();

        // 使用 LIKE 进行模糊匹配，但由于需求是精确匹配，我们使用 = 和 LOWER()
        // 支持：
        // 1. 精确匹配原地址
        // 2. 大小写不敏感匹配（通过 LOWER() 转换）
        let sql = r#"
            SELECT * FROM api_account 
            WHERE uid = ?
              AND status = 1
              AND (address = ? OR LOWER(address) = ?)
        "#;

        let results = sqlx::query_as::<_, ApiAccountEntity>(sql)
            .bind(uid)
            .bind(keyword)
            .bind(keyword_lower)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(results)
    }
}

#[cfg(test)]
#[path = "api_account/deletion_tests.rs"]
mod deletion_tests;
