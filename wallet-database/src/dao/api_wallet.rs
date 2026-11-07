use crate::{
    entities::api_wallet::{ApiWalletEntity, ApiWalletType},
    sql_utils::{
        SqlExecutableNoReturn, SqlExecutableReturn as _, query_builder::DynamicQueryBuilder,
        update_builder::DynamicUpdateBuilder,
    },
};
use sqlx::{Executor, QueryBuilder, Sqlite};

/// DAO 层设计原则：
///
/// 1. DAO 只负责“事实写入 / 事实读取”，不负责状态机逻辑
/// 2. UPDATE / DELETE 等状态推进方法：
///    - 不使用 RETURNING
///    - 不返回 Entity
///    - 0 行更新是合法状态，不是错误
/// 3. 如需 Entity，调用方必须显式 SELECT

pub(crate) struct ApiWalletDao;

impl ApiWalletDao {
    /// Connection-local commit token; compare only on the same retained connection.
    pub async fn data_version(conn: &mut sqlx::SqliteConnection) -> Result<i64, crate::Error> {
        sqlx::query_scalar("PRAGMA data_version")
            .fetch_one(conn)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn upsert<'a, E>(
        exec: E,
        address: &str,
        uid: &str,
        name: &str,
        phrase: &[u8],
        seed: &[u8],
        status: u8,
        api_wallet_type: ApiWalletType,
        binding_address: Option<&str>,
        sn: &str,
        import_stage: u8,
        // merchant_id: &str,
        // app_id: &str,
    ) -> Result<ApiWalletEntity, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + Clone,
    {
        let sql = r#"
            INSERT INTO api_wallet (
                address, uid, name, phrase, seed,
                status, is_init, import_stage, api_wallet_type, binding_address, sn,
                created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT(address)
            DO UPDATE SET
                uid = excluded.uid,
                name = excluded.name,
                phrase = excluded.phrase,
                seed = excluded.seed,
                status = excluded.status,
                import_stage = MAX(api_wallet.import_stage, excluded.import_stage),
                api_wallet_type = excluded.api_wallet_type,
                binding_address = excluded.binding_address,
                sn = excluded.sn,
                updated_at = excluded.updated_at
        "#;

        // 执行 INSERT/UPDATE，不依赖 RETURNING
        sqlx::query(sql)
            .bind(address)
            .bind(uid)
            .bind(name)
            .bind(phrase)
            .bind(seed)
            .bind(status)
            .bind(import_stage)
            .bind(api_wallet_type)
            .bind(binding_address)
            .bind(sn)
            .execute(exec.clone())
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        // 单独查询钱包信息，确保返回最新数据
        let select_sql = "SELECT * FROM api_wallet WHERE address = ?";
        sqlx::query_as::<sqlx::Sqlite, ApiWalletEntity>(select_sql)
            .bind(address)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn detail<'a, E>(
        exec: E,
        address: &str,
    ) -> Result<Option<ApiWalletEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM api_wallet WHERE address = ? AND status = 1;";
        sqlx::query_as::<sqlx::Sqlite, ApiWalletEntity>(sql)
            .bind(address)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn wallet_latest<'a, E>(exec: E) -> Result<Option<ApiWalletEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM api_wallet WHERE status = 1
                   ORDER BY updated_at DESC
                   LIMIT 1;";

        sqlx::query_as::<sqlx::Sqlite, ApiWalletEntity>(sql)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn uid_list<'a, E>(exec: E) -> Result<Vec<(String,)>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT DISTINCT uid FROM api_wallet WHERE status = 1;";

        sqlx::query_as::<sqlx::Sqlite, (String,)>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// Only active withdrawal-wallet owners, without loading wallet material.
    pub(crate) async fn active_warning_config_bindings<'a, E>(
        exec: E,
    ) -> Result<Vec<(String, String)>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, (String, String)>(
            "SELECT DISTINCT merchant_id, app_id FROM api_wallet WHERE status = 1 AND api_wallet_type = ? AND merchant_id IS NOT NULL AND TRIM(merchant_id) <> '' AND app_id IS NOT NULL AND TRIM(app_id) <> '' ORDER BY merchant_id, app_id",
        ).bind(ApiWalletType::Withdrawal).fetch_all(exec).await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn active_withdrawal_uids<'a, E>(exec: E) -> Result<Vec<(String,)>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query_as::<_, (String,)>(
            "SELECT DISTINCT uid FROM api_wallet WHERE status = 1 AND api_wallet_type = ? AND TRIM(uid) <> '' ORDER BY uid",
        )
        .bind(ApiWalletType::Withdrawal)
        .fetch_all(exec)
        .await
        .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn detail_by_uid<'a, E>(
        exec: E,
        uid: &str,
    ) -> Result<Option<ApiWalletEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        DynamicQueryBuilder::new("SELECT * FROM api_wallet")
            .and_where_eq("uid", uid)
            .and_where_eq("status", "1")
            .fetch_optional(exec)
            .await
    }

    pub async fn list<'a, E>(
        exec: E,
        api_wallet_type: Option<ApiWalletType>,
    ) -> Result<Vec<ApiWalletEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut sql = "SELECT * FROM api_wallet".to_string();
        let mut conditions = Vec::new();

        if api_wallet_type.is_some() {
            conditions.push("api_wallet_type = ?".to_string());
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        let mut query = sqlx::query_as::<_, ApiWalletEntity>(&sql);

        if let Some(api_wallet_type) = api_wallet_type {
            query = query.bind(api_wallet_type);
        }

        query.fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_merchain_id<'a, E>(
        exec: E,
        address: &str,
        merchant_id: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_wallet SET
                merchant_id = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE address = ? AND status = 1
        "#;

        let res = sqlx::query(sql)
            .bind(merchant_id)
            .bind(address)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() == 1)
    }

    pub async fn bind_withdraw_and_subaccount_relation<'a, E>(
        exec: E,
        wallet_address: &str,
        binding_address: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let builder = DynamicUpdateBuilder::new("api_wallet")
            .set("binding_address", binding_address)
            .set_raw("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
            .and_where_eq("address", wallet_address);
        SqlExecutableNoReturn::execute(builder, exec).await
    }

    pub async fn update_app_id<'a, E>(
        exec: E,
        address: &str,
        app_id: Option<&str>,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_wallet SET
                app_id = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE address = ? AND status = 1
        "#;

        let res = sqlx::query(sql)
            .bind(app_id)
            .bind(address)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() == 1)
    }

    pub async fn update_import_stage<'a, E>(
        exec: E,
        uid: &str,
        import_stage: u8,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_wallet SET
                import_stage = MAX(import_stage, ?),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE uid = ? AND status = 1
        "#;

        let res = sqlx::query(sql)
            .bind(import_stage)
            .bind(uid)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() == 1)
    }

    pub async fn update_sn<'a, E>(exec: E, address: &str, sn: &str) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_wallet SET
                sn = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE address = ? AND status = 1
        "#;

        let res = sqlx::query(sql)
            .bind(sn)
            .bind(address)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() == 1)
    }

    pub async fn clear_binding_address<'a, E>(exec: E, address: &str) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_wallet SET
                binding_address = null,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE address = ? AND status = 1
        "#;

        let res = sqlx::query(sql)
            .bind(address)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() == 1)
    }

    pub async fn clear_binding_relation<'a, E>(
        exec: E,
        wallet_address: &str,
        binding_address: &str,
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_wallet SET
                binding_address = null,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE address IN (?, ?) AND status = 1
        "#;

        let res = sqlx::query(sql)
            .bind(wallet_address)
            .bind(binding_address)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    pub async fn update_seed_and_phrase<'a, E>(
        exec: E,
        uid: &str,
        phrase: &[u8],
        seed: &[u8],
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_wallet SET
                seed = ?,
                phrase = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE uid = ?
        "#;

        let res = sqlx::query(sql)
            .bind(seed)
            .bind(phrase)
            .bind(uid)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() == 1)
    }

    pub async fn unbind_uid<'a, E>(exec: E, address: &str) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_wallet SET
                app_id = null,
                merchant_id = null,
                sn = null,
                binding_address = null,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE address = ? AND status = 1
        "#;

        let res = sqlx::query(sql)
            .bind(address)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() == 1)
    }

    pub async fn edit_name<'a, E>(exec: E, address: &str, name: &str) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            UPDATE api_wallet SET
                name = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE address = ? AND status = 1
        "#;

        let res = sqlx::query(sql)
            .bind(name)
            .bind(address)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected() == 1)
    }

    pub async fn mark_init<'a, E>(exec: E, uid: &str) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let now = sqlx::types::chrono::Utc::now();
        let sql = r#"
            UPDATE api_wallet SET
                is_init = 1,
                updated_at = ?
            WHERE uid = ? AND status = 1
        "#;

        let result = sqlx::query(sql)
            .bind(now)
            .bind(uid)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        // 返回是否更新成功，0行更新是合法状态，不是错误
        Ok(result.rows_affected() == 1)
    }

    pub async fn physical_delete<'a, E>(
        exec: E,
        addresses: &[&str],
    ) -> Result<Vec<ApiWalletEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if addresses.is_empty() {
            return Ok(Vec::new());
        }

        let mut query = QueryBuilder::<Sqlite>::new("DELETE FROM api_wallet WHERE address IN (");
        let mut values = query.separated(", ");
        for address in addresses {
            values.push_bind(*address);
        }
        values.push_unseparated(") RETURNING *");

        query
            .build_query_as::<ApiWalletEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn physical_delete_all_wallet<'a, E>(exec: E) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"DELETE FROM api_wallet"#;
        let result =
            sqlx::query(sql).execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        Ok(result.rows_affected())
    }
}
