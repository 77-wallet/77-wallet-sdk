use crate::{
    entities::{
        api_assets::{ApiAssetsEntity, ApiAssetsEntityWithAddressType, AssetWithWalletAddress},
        asset_token_key::AssetTokenKey,
        assets::AssetsId,
    },
    error::DatabaseError,
    sql_utils::{SqlExecutableNoReturn, update_builder::DynamicUpdateBuilder},
};
use std::collections::HashMap;

use crate::entities::api_assets::ApiCreateAssetsVo;
use sqlx::{Executor, QueryBuilder, Sqlite};

pub(crate) struct ApiAssetsDao;

#[cfg(test)]
#[path = "api_assets/visibility_tests.rs"]
mod api_asset_visibility_tests;

impl ApiAssetsDao {
    pub async fn list<'a, E>(
        exec: E,
        addr: Vec<String>,
        chain_code: Option<String>,
    ) -> Result<Vec<ApiAssetsEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = format!(
            "SELECT * FROM api_assets_with_coin_labels AS api_assets WHERE status = 1 AND EXISTS (
                    SELECT 1
                    FROM api_chain
                    WHERE api_chain.chain_code = api_assets.chain_code
                    AND api_chain.status = 1
                ) AND EXISTS (
                    SELECT 1
                    FROM api_coin
                    WHERE api_coin.chain_code = api_assets.chain_code
                    AND (api_coin.token_address = api_assets.token_address OR (
                        api_assets.chain_code IN (SELECT value FROM json_each('{}'))
                        AND api_coin.token_address = api_assets.token_address COLLATE NOCASE))
                    AND api_coin.status = 1
                )",
            crate::entities::api_withdraw::signed_evm_chain_codes_json(),
        );
        let mut query = QueryBuilder::<Sqlite>::new(sql);

        if let Some(chain_code) = chain_code {
            query.push(" AND chain_code = ").push_bind(chain_code);
        }

        if !addr.is_empty() {
            query.push(" AND address IN (");
            let mut values = query.separated(", ");
            for address in &addr {
                values.push_bind(address);
            }
            values.push_unseparated(")");
        }

        query
            .build_query_as::<ApiAssetsEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_balance<'a, E>(
        exec: E,
        address: &str,
        chain_code: &str,
        token_address: AssetTokenKey,
        balance: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let builder = DynamicUpdateBuilder::new("api_assets")
            .set("balance", balance)
            .set_raw("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
            .and_where_eq("address", &address)
            .and_where_eq("chain_code", chain_code)
            .and_where_eq(
                if crate::entities::api_withdraw::is_signed_evm_chain(chain_code) {
                    "token_address COLLATE NOCASE"
                } else {
                    "token_address"
                },
                token_address,
            );
        SqlExecutableNoReturn::execute(builder, exec).await
    }

    /// 批量更新余额（在事务中执行）
    /// 使用 sqlx::query 直接执行，避免 Executor 所有权问题
    pub async fn batch_update_balance_in_tx<'a>(
        exec: &mut sqlx::Transaction<'a, Sqlite>,
        updates: &[(String, String, AssetTokenKey, String)], // (address, chain_code, token_key, balance)
    ) -> Result<(), crate::Error> {
        if updates.is_empty() {
            return Ok(());
        }

        // 在事务中批量执行更新，减少数据库往返次数
        for (address, chain_code, token_key, balance) in updates {
            let sql = r#"
                UPDATE api_assets 
                SET balance = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                WHERE address = ? AND chain_code = ? AND (token_address = ? OR (? AND token_address = ? COLLATE NOCASE))
            "#;

            sqlx::query(sql)
                .bind(balance)
                .bind(address)
                .bind(chain_code)
                .bind(token_key)
                .bind(crate::entities::api_withdraw::is_signed_evm_chain(chain_code))
                .bind(token_key)
                .execute(exec.as_mut())
                .await
                .map_err(|e| crate::Error::Database(e.into()))?;
        }

        Ok(())
    }

    pub async fn upsert_assets<'a, E>(
        exec: E,
        assets: ApiCreateAssetsVo,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let ApiCreateAssetsVo {
            assets_id,
            symbol: _,
            name: _,
            decimals,
            protocol,
            status,
            is_multisig,
            balance,
        } = assets;

        let token_address = assets_id.token_address.normalized_for_chain(&assets_id.chain_code);
        let protocol = protocol.unwrap_or_default();

        let sql = r#"
        INSERT INTO api_assets
        (
            decimals, address, chain_code, token_address, protocol, status, balance, is_multisig, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        ON CONFLICT (address, chain_code, token_address)
        DO UPDATE SET
            status = EXCLUDED.status,
            is_multisig = EXCLUDED.is_multisig,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now');
    "#;

        sqlx::query(sql)
            .bind(decimals)
            .bind(assets_id.address)
            .bind(assets_id.chain_code)
            .bind(token_address)
            .bind(protocol)
            .bind(status)
            .bind(balance)
            .bind(is_multisig)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(DatabaseError::UpdateFailed(e.to_string())))
    }

    /// 批量插入或更新资产
    pub async fn upsert_assets_multi(
        exec: &mut sqlx::SqliteConnection,
        assets: Vec<ApiCreateAssetsVo>,
    ) -> Result<(), crate::Error> {
        Self::write_assets_multi(exec, assets, false).await
    }

    pub async fn insert_missing_assets_multi(
        exec: &mut sqlx::SqliteConnection,
        assets: Vec<ApiCreateAssetsVo>,
    ) -> Result<(), crate::Error> {
        Self::write_assets_multi(exec, assets, true).await
    }

    async fn write_assets_multi(
        exec: &mut sqlx::SqliteConnection,
        assets: Vec<ApiCreateAssetsVo>,
        preserve_existing: bool,
    ) -> Result<(), crate::Error> {
        if assets.is_empty() {
            return Ok(());
        }

        // Keep each SQL statement smaller to reduce sqlite write-lock hold time.
        const BATCH_SIZE: usize = 200;
        tracing::debug!(count = %assets.len(), "ApiAssetsDao: starting upsert_assets_multi");

        for (batch_idx, chunk) in assets.chunks(BATCH_SIZE).enumerate() {
            tracing::debug!(batch_idx = %batch_idx, batch_size = %chunk.len(), "ApiAssetsDao: processing batch");

            let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "INSERT INTO api_assets (
                    decimals, address, chain_code, token_address, protocol, status, balance, is_multisig, created_at, updated_at
                ) ",
            );

            qb.push_values(chunk, |mut b, item| {
                let token_address =
                    item.assets_id.token_address.normalized_for_chain(&item.assets_id.chain_code);
                let protocol = item.protocol.clone().unwrap_or_default();

                b.push_bind(item.decimals)
                    .push_bind(item.assets_id.address.clone())
                    .push_bind(item.assets_id.chain_code.clone())
                    .push_bind(token_address)
                    .push_bind(protocol)
                    .push_bind(item.status)
                    .push_bind(item.balance.clone())
                    .push_bind(item.is_multisig)
                    .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
                    .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')");
            });

            if preserve_existing {
                qb.push(" ON CONFLICT(address, chain_code, token_address) DO NOTHING");
            } else {
                qb.push(
                    " ON CONFLICT(address, chain_code, token_address)
                  DO UPDATE SET
                      status = excluded.status,
                      is_multisig = excluded.is_multisig,
                      updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
                );
            }

            let result = qb
                .build()
                .execute(&mut *exec)
                .await
                .map_err(|e| crate::Error::Database(e.into()))?;

            tracing::debug!(batch_idx = %batch_idx, rows_affected = %result.rows_affected(), "ApiAssetsDao: batch completed");
        }

        tracing::debug!(count = %assets.len(), "ApiAssetsDao: upsert_assets_multi completed");
        Ok(())
    }

    /// 批量插入或更新资产（用于“余额同步”场景）
    ///
    /// 与 `upsert_assets_multi` 的关键区别：
    /// - ON CONFLICT 时仅更新 `balance` + `updated_at`
    /// - 这样不会被默认资产初始化（balance=0）覆盖链上同步到的真实余额
    pub async fn upsert_assets_multi_update_balance(
        exec: &mut sqlx::SqliteConnection,
        assets: Vec<ApiCreateAssetsVo>,
    ) -> Result<(), crate::Error> {
        if assets.is_empty() {
            return Ok(());
        }

        // Keep each SQL statement smaller to reduce sqlite write-lock hold time.
        const BATCH_SIZE: usize = 200;
        tracing::debug!(count = %assets.len(), "ApiAssetsDao: starting upsert_assets_multi_update_balance");

        for (batch_idx, chunk) in assets.chunks(BATCH_SIZE).enumerate() {
            tracing::debug!(batch_idx = %batch_idx, batch_size = %chunk.len(), "ApiAssetsDao: processing balance-upsert batch");

            let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "INSERT INTO api_assets (
                    decimals, address, chain_code, token_address, protocol, status, balance, is_multisig, created_at, updated_at
                ) ",
            );

            qb.push_values(chunk, |mut b, item| {
                let token_address =
                    item.assets_id.token_address.normalized_for_chain(&item.assets_id.chain_code);
                let protocol = item.protocol.clone().unwrap_or_default();

                b.push_bind(item.decimals)
                    .push_bind(item.assets_id.address.clone())
                    .push_bind(item.assets_id.chain_code.clone())
                    .push_bind(token_address)
                    .push_bind(protocol)
                    .push_bind(item.status)
                    .push_bind(item.balance.clone())
                    .push_bind(item.is_multisig)
                    .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
                    .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')");
            });

            qb.push(
                " ON CONFLICT(address, chain_code, token_address)
                  DO UPDATE SET
                      balance = excluded.balance,
                      updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
            );

            let result = qb
                .build()
                .execute(&mut *exec)
                .await
                .map_err(|e| crate::Error::Database(e.into()))?;

            tracing::debug!(batch_idx = %batch_idx, rows_affected = %result.rows_affected(), "ApiAssetsDao: balance-upsert batch completed");
        }

        tracing::debug!(count = %assets.len(), "ApiAssetsDao: upsert_assets_multi_update_balance completed");
        Ok(())
    }

    pub async fn delete_assets<'a, E>(
        exec: E,
        address: &str,
        chain_code: &str,
        token_address: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE api_assets 
            SET status = $4, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE address = $1 AND chain_code = $2 AND token_address = $3"#;

        sqlx::query(sql)
            .bind(address)
            .bind(chain_code)
            .bind(AssetTokenKey::from_raw(Some(token_address)).normalized_for_chain(chain_code))
            .bind(0) // Assuming 0 is the status for deletion
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(DatabaseError::UpdateFailed(e.to_string())))
    }

    pub async fn update_status<'a, E>(
        exec: E,
        chain_code: &str,
        symbol: &str,
        token_address: AssetTokenKey,
        status: u8,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE api_assets
        SET status = $4, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE chain_code = $1 AND token_address = $3
            AND EXISTS (
                SELECT 1
                FROM api_chain
                WHERE api_chain.chain_code = api_assets.chain_code
                AND api_chain.status = 1
            )
            AND EXISTS (
                SELECT 1
                FROM api_coin
                WHERE api_coin.chain_code = api_assets.chain_code
                AND api_coin.token_address = api_assets.token_address
                AND LOWER(api_coin.symbol) = LOWER($2)
                AND api_coin.status = 1
            );
        "#;

        sqlx::query(sql)
            .bind(chain_code)
            .bind(symbol)
            .bind(token_address.normalized_for_chain(chain_code))
            .bind(status)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(DatabaseError::UpdateFailed(e.to_string())))?;

        Ok(())
    }

    pub async fn assets_by_id<'a, E>(
        exec: E,
        assets_id: &AssetsId,
    ) -> Result<Option<ApiAssetsEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT * FROM 
                api_assets_with_coin_labels AS api_assets
            WHERE status = 1 AND address =$1 AND chain_code = $2 AND (token_address = $3 OR ($4 AND token_address = $3 COLLATE NOCASE))
                AND EXISTS (
                    SELECT 1
                    FROM api_chain
                    WHERE api_chain.chain_code = api_assets.chain_code
                    AND api_chain.status = 1
                )
                AND EXISTS (
                    SELECT 1
                    FROM api_coin
                    WHERE api_coin.chain_code = api_assets.chain_code
                    AND (api_coin.token_address = api_assets.token_address OR ($4 AND api_coin.token_address = api_assets.token_address COLLATE NOCASE))
                    AND api_coin.status = 1
                ) ORDER BY (token_address = $3) DESC LIMIT 1;"#;

        let rs = sqlx::query_as::<sqlx::Sqlite, ApiAssetsEntity>(sql)
            .bind(&assets_id.address)
            .bind(&assets_id.chain_code)
            .bind(&assets_id.token_address)
            .bind(crate::entities::api_withdraw::is_signed_evm_chain(&assets_id.chain_code))
            .fetch_optional(exec)
            .await;

        match rs {
            Ok(rs) => Ok(rs),
            Err(_e) => Err(crate::Error::Database(DatabaseError::QueryFailed)),
        }
    }

    pub async fn get_chain_assets_by_address_chain_code_symbol<'a, E>(
        exec: E,
        address: Vec<String>,
        chain_code: Option<String>,
        symbol: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<ApiAssetsEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT * FROM api_assets_with_coin_labels AS api_assets
        WHERE status = 1
            AND EXISTS (
                SELECT 1
                FROM api_chain
                WHERE api_chain.chain_code = api_assets.chain_code
                AND api_chain.status = 1
            )
            AND EXISTS (
                SELECT 1
                FROM api_coin
                WHERE api_coin.chain_code = api_assets.chain_code
                AND api_coin.token_address = api_assets.token_address
                AND api_coin.status = 1
            )",
        );

        if !address.is_empty() {
            query.push(" AND address IN (");
            let mut values = query.separated(", ");
            for item in &address {
                values.push_bind(item);
            }
            values.push_unseparated(")");
        }

        if let Some(code) = chain_code.as_deref() {
            query.push(" AND chain_code = ").push_bind(code);
        }

        if let Some(symbol) = symbol {
            query.push(" AND symbol = ").push_bind(symbol);
        }

        if let Some(is_multisig) = is_multisig {
            if is_multisig {
                query.push(" AND is_multisig = ").push_bind(1_i8);
            } else {
                query.push(" AND is_multisig IN (").push_bind(0_i8);
                query.push(", ").push_bind(2_i8).push(")");
            }
        }

        query
            .build_query_as::<ApiAssetsEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub(crate) async fn get_api_assets_by_address<'a, E>(
        exec: E,
        address: Vec<String>,
        chain_code: Option<String>,
        symbol: Option<&str>,
        token_address: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<ApiAssetsEntityWithAddressType>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        // `api_assets` lives in `api_wallet.db`. Multisig tables (e.g. `multisig_account`) are in
        // the core db (`data.db`) and must NOT be joined here. This API wallet path has no multisig
        // business, so we always join `api_account`.
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT a.name, a.symbol, a.decimals, a.address, a.chain_code,
                    a.token_address, a.protocol, a.status, a.balance, a.is_multisig,
                a.created_at, a.updated_at, acc.address_type
                FROM api_assets_with_coin_labels AS a
                JOIN api_account AS acc
                ON a.address = acc.address AND a.chain_code = acc.chain_code
                WHERE a.status = 1
                    AND EXISTS (
                        SELECT 1
                        FROM api_chain
                        WHERE api_chain.chain_code = a.chain_code
                        AND api_chain.status = 1
                    )
                    AND EXISTS (
                        SELECT 1
                        FROM api_coin
                        WHERE api_coin.chain_code = a.chain_code
                        AND api_coin.token_address = a.token_address
                        AND api_coin.status = 1
                    )",
        );

        if !address.is_empty() {
            query.push(" AND a.address IN (");
            let mut values = query.separated(", ");
            for item in &address {
                values.push_bind(item);
            }
            values.push_unseparated(")");
        }
        if let Some(code) = chain_code.as_deref() {
            query.push(" AND a.chain_code = ").push_bind(code);
        }
        if let Some(symbol) = symbol {
            query.push(" AND a.symbol = ").push_bind(symbol);
        }
        if let Some(token_address) = token_address {
            if let Some(chain) = chain_code.as_deref() {
                query.push(" AND a.token_address = ").push_bind(
                    AssetTokenKey::from_raw(Some(token_address)).normalized_for_chain(chain),
                );
            } else {
                query
                    .push(" AND (a.token_address = ")
                    .push_bind(token_address)
                    .push(" OR (a.chain_code IN (SELECT value FROM json_each(")
                    .push_bind(crate::entities::api_withdraw::signed_evm_chain_codes_json())
                    .push(")) AND a.token_address = ")
                    .push_bind(token_address)
                    .push(" COLLATE NOCASE))");
            }
        }
        if let Some(is_multisig) = is_multisig {
            if is_multisig {
                query.push(" AND a.is_multisig = ").push_bind(1_i8);
            } else {
                query.push(" AND a.is_multisig IN (").push_bind(0_i8);
                query.push(", ").push_bind(2_i8).push(")");
            }
        }

        query
            .build_query_as::<ApiAssetsEntityWithAddressType>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn assets_with_wallet_address_by_address<'a, 'b, E>(
        exec: E,
        keys: &[String],
    ) -> Result<Vec<AssetWithWalletAddress>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        // 兼容两种调用模式：
        // 1) address（全量刷新）
        // 2) address:chain:token（脏资产增量刷新）
        let has_composite_key = keys.iter().any(|k| k.contains(':'));
        let mut composite_keys: Vec<(String, String, String)> = Vec::new();
        let mut addresses: Vec<String> = Vec::new();
        for key in keys {
            if has_composite_key {
                let mut parts = key.splitn(3, ':');
                let address = parts.next().unwrap_or_default();
                let chain_code = parts.next().unwrap_or_default();
                let token_address = parts.next().unwrap_or_default();
                if !address.is_empty() && !chain_code.is_empty() {
                    composite_keys.push((
                        address.to_string(),
                        chain_code.to_string(),
                        AssetTokenKey::from_raw(Some(token_address))
                            .normalized_for_chain(chain_code)
                            .as_db_str()
                            .to_string(),
                    ));
                } else {
                    tracing::warn!("invalid address-chain-token key: {}", key);
                }
            } else if !key.is_empty() {
                addresses.push(key.clone());
            }
        }

        if has_composite_key {
            if composite_keys.is_empty() {
                return Ok(Vec::new());
            }
            // 用 VALUES/CTE + JOIN 替代字符串拼接 IN，便于 SQLite 使用索引并减少表达式计算。
            let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
                "WITH keyset(address, chain_code, token_address) AS (",
            );
            qb.push_values(&composite_keys, |mut row, (address, chain_code, token_address)| {
                row.push_bind(address).push_bind(chain_code).push_bind(token_address);
            });
            qb.push(
                ") \
                 SELECT DISTINCT \
                    a.address, \
                    aa.wallet_address, \
                    a.symbol, \
                    a.chain_code, \
                    a.token_address, \
                    a.balance, \
                    a.decimals \
                 FROM keyset k \
                 JOIN api_assets_with_coin_labels a \
                    ON a.address = k.address \
                    AND a.chain_code = k.chain_code \
                    AND a.token_address = k.token_address \
                    AND a.status = 1 \
                 JOIN api_account aa \
                    ON aa.address = a.address \
                    AND aa.chain_code = a.chain_code \
                    AND aa.status = 1",
            );

            return qb
                .build_query_as::<AssetWithWalletAddress>()
                .fetch_all(exec)
                .await
                .map_err(|e| crate::Error::Database(e.into()));
        }

        if addresses.is_empty() {
            return Ok(Vec::new());
        }

        let mut qb = sqlx::QueryBuilder::<Sqlite>::new("WITH addrset(address) AS (");
        qb.push_values(&addresses, |mut row, address| {
            row.push_bind(address);
        });
        // 地址全量刷新路径同样走 CTE + JOIN，并显式过滤 status，避免无效行参与聚合计算。
        qb.push(
            ") \
             SELECT DISTINCT \
                a.address, \
                aa.wallet_address, \
                a.symbol, \
                a.chain_code, \
                a.token_address, \
                a.balance, \
                a.decimals \
             FROM addrset s \
             JOIN api_assets_with_coin_labels a \
                ON a.address = s.address \
                AND a.status = 1 \
             JOIN api_account aa \
                ON aa.address = a.address \
                AND aa.chain_code = a.chain_code \
                AND aa.status = 1",
        );

        qb.build_query_as::<AssetWithWalletAddress>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn assets_with_wallet_address_by_token<'a, 'b, E>(
        exec: E,
        keys: &[String],
    ) -> Result<Vec<AssetWithWalletAddress>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        let mut token_keys = Vec::<(String, String, String)>::new();
        for key in keys {
            let mut parts = key.splitn(3, ':');
            let symbol = parts.next().unwrap_or_default();
            let chain_code = parts.next().unwrap_or_default();
            let token_address = parts.next().unwrap_or_default();
            if symbol.is_empty() || chain_code.is_empty() {
                tracing::warn!("invalid token key: {}", key);
                continue;
            }
            token_keys.push((
                symbol.to_ascii_uppercase(),
                chain_code.to_string(),
                AssetTokenKey::from_raw(Some(token_address))
                    .normalized_for_chain(chain_code)
                    .as_db_str()
                    .to_string(),
            ));
        }

        if token_keys.is_empty() {
            return Ok(Vec::new());
        }

        // token 脏数据刷新路径：按 (symbol, chain_code, token_address) 构造 keyset JOIN，
        // 避免 (symbol||':'||chain||':'||token) IN (...) 导致索引难命中。
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            "WITH keyset(symbol, chain_code, token_address) AS (",
        );
        qb.push_values(&token_keys, |mut row, (symbol, chain_code, token_address)| {
            row.push_bind(symbol).push_bind(chain_code).push_bind(token_address);
        });
        qb.push(
            ") \
             SELECT DISTINCT \
                a.address, \
                aa.wallet_address, \
                a.symbol, \
                a.chain_code, \
                a.token_address, \
                a.balance, \
                a.decimals \
             FROM keyset k \
             JOIN api_assets_with_coin_labels a \
                ON a.symbol = k.symbol \
                AND a.chain_code = k.chain_code \
                AND a.token_address = k.token_address \
                AND a.status = 1 \
             JOIN api_account aa \
                ON aa.address = a.address \
                AND aa.chain_code = a.chain_code \
                AND aa.status = 1",
        );

        qb.build_query_as::<AssetWithWalletAddress>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // TODO: 慢sql，需要优化
    pub async fn get_api_wallet_total_assets_v2<'a, E>(
        exec: E,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<SumResult, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let start = std::time::Instant::now();
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            r#"
SELECT
CAST(SUM(total_account_amount) AS REAL) as total_amount,
CAST(SUM(total_coins_quantity) AS REAL) as total_coins_quantity
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

        if let Some(wallet_address) = wallet_address {
            qb.push(" AND api_account.wallet_address = ").push_bind(wallet_address);
        }
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

        let res = qb
            .build_query_as::<SumResult>()
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()));

        tracing::info!(
            metric = "api_wallet_total_assets_v2_query",
            wallet_address = wallet_address.unwrap_or("all"),
            account_id = ?account_id,
            chain_code = chain_code.unwrap_or("all"),
            elapsed_ms = start.elapsed().as_millis(),
            ok = res.is_ok(),
            "ApiAssetsDao: get_api_wallet_total_assets_v2"
        );
        res
    }

    pub async fn get_api_wallet_total_assets_v3<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<Vec<AssetBalanceEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let start = std::time::Instant::now();
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            r#"
SELECT
    a.balance,
    a.chain_code,
    a.token_address
FROM api_assets a INDEXED BY api_assets_nonzero_wallet_total_idx
CROSS JOIN api_account acc INDEXED BY api_account_address_chain_code_idx
    ON acc.address = a.address
    AND a.chain_code = acc.chain_code
CROSS JOIN api_chain c INDEXED BY api_chain_chain_code_idx
    ON c.chain_code = a.chain_code
WHERE a.status = 1
    AND a.balance != '0'
    AND acc.wallet_address =
"#,
        );

        qb.push_bind(wallet_address);
        qb.push(" AND acc.status = 1");
        qb.push(" AND c.status = 1");

        if let Some(account_id) = account_id {
            qb.push(" AND acc.account_id = ").push_bind(account_id);
        }
        if let Some(chain_code) = chain_code {
            qb.push(" AND acc.chain_code = ").push_bind(chain_code);
        }

        let query_start = std::time::Instant::now();
        let res = qb
            .build_query_as::<AssetBalanceEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()));
        let query_elapsed_ms = query_start.elapsed().as_millis();

        if let Ok(ref rows) = res {
            tracing::info!(
                metric = "api_assets_v3_rows",
                wallet_address = wallet_address,
                row_count = rows.len(),
                query_elapsed_ms,
                "ApiAssetsDao: get_api_wallet_total_assets_v3 rows fetched"
            );
        }
        tracing::debug!(
            elapsed_ms = start.elapsed().as_millis(),
            query_elapsed_ms,
            wallet_address = wallet_address,
            "ApiAssetsDao: get_api_wallet_total_assets_v3"
        );
        res
    }

    pub async fn get_api_wallet_assets_v2<'a, E>(
        exec: E,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
        hide_zero_balance: bool,
    ) -> Result<Vec<ApiAssertSummeryEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let start = std::time::Instant::now();
        let mut qb = sqlx::QueryBuilder::<Sqlite>::new(
            r#"
SELECT
all_data.chain_code                                                 AS chain_code,
all_data.symbol                                                     AS symbol,
all_data.api_assets_name 							                AS api_assets_name,
CAST(SUM(all_data.total_coin_quantity) AS REAL) 		            AS total_coins_quantity,
CAST(all_data.coin_unit_price AS REAL)                              AS coin_unit_price,
CAST(SUM(total_coin_amount) AS REAL)			                    AS total_account_amount,
all_data.is_default 				                                AS coin_is_default,
all_data.is_multisig 				                                AS assets_is_multisig,
JSON_GROUP_OBJECT(all_data.chain_code, all_data.token_address)      AS chain_token_map
FROM
(
SELECT
api_account.account_id,api_account.name,api_account.api_wallet_type,api_account.wallet_address,api_account.address, api_account.chain_code,
api_assets.token_address,api_assets.balance,api_coin.symbol,api_coin.name as api_assets_name,api_assets.is_multisig,
api_chain.name 														AS api_chain_name,
api_coin.price 														AS coin_unit_price,
api_coin.is_default,
SUM(api_assets.balance)  									AS total_coin_quantity,
api_coin.price * SUM(api_assets.balance)  AS total_coin_amount
FROM api_assets
LEFT JOIN api_account
ON api_assets.address = api_account.address AND api_account.chain_code = api_assets.chain_code
LEFT JOIN api_coin
ON api_coin.chain_code=api_assets.chain_code AND api_coin.token_address=api_assets.token_address
LEFT JOIN api_chain
ON api_chain.chain_code=api_assets.chain_code
WHERE api_assets.status =1 AND api_chain.status =1 AND api_coin.status =1
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
where 1=1
"#,
        );
        if hide_zero_balance {
            qb.push(" AND all_data.total_coin_quantity > 0");
        }

        qb.push(
            r#"
GROUP BY all_data.wallet_address,all_data.account_id,all_data.symbol
ORDER BY total_account_amount DESC
        "#,
        );

        let res = qb
            .build_query_as::<ApiAssertSummeryEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()));

        tracing::debug!(
            elapsed_ms = start.elapsed().as_millis(),
            "ApiAssetsDao: get_api_wallet_assets_v2"
        );
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn make_asset(
        address: &str,
        chain_code: &str,
        symbol: &str,
        token_address: &str,
        balance: &str,
    ) -> ApiCreateAssetsVo {
        let assets_id = crate::entities::assets::AssetsId::new(
            address,
            chain_code,
            Some(token_address.to_string()).into(),
        );

        crate::entities::api_assets::ApiCreateAssetsVo::new(assets_id, symbol, 18, None, 0)
            .with_name("t")
            .with_balance(balance)
    }

    async fn seed_queryable_asset(conn: &mut sqlx::SqliteConnection) {
        sqlx::query(
            r#"
            INSERT INTO api_chain (name, chain_code, protocols, status, main_symbol, created_at)
            VALUES ('Ethereum', 'eth', '[]', 1, 'ETH', CURRENT_TIMESTAMP);
            INSERT INTO api_coin (
                name, symbol, chain_code, token_address, price, protocol, decimals,
                is_default, is_popular, is_custom, status, created_at
            ) VALUES ('Token', 'TOK', 'eth', '0xtoken', '1', '', 18, 1, 0, 0, 1, CURRENT_TIMESTAMP);
            INSERT INTO api_account (
                account_id, name, address, pubkey, address_type, wallet_address, uid,
                derivation_path, derivation_path_index, chain_code, api_wallet_type,
                status, is_init, is_used, created_at
            ) VALUES (1, 'main', '0xasset', '', '', '0xwallet', 'uid', '', 0, 'eth', 1, 1, 1, 0, CURRENT_TIMESTAMP);
            INSERT INTO api_assets (
    decimals, address, chain_code, token_address, protocol,
                status, balance, is_multisig, created_at
            ) VALUES (18, '0xasset', 'eth', '0xtoken', '', 1, '1', 0, CURRENT_TIMESTAMP);
            "#,
        )
        .execute(conn)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn api_asset_collection_reads_bind_external_selectors() {
        let dir = make_temp_dir("wallet_db_api_assets_query_bind");
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();
        let mut conn = pool.acquire().await.unwrap();
        seed_queryable_asset(&mut conn).await;

        let exact =
            ApiAssetsDao::list(&mut *conn, vec!["0xasset".to_string()], Some("eth".to_string()))
                .await
                .unwrap();
        let sql_address = vec!["missing') OR 1=1 --".to_string()];
        let listed = ApiAssetsDao::list(&mut *conn, sql_address.clone(), None).await.unwrap();
        let chain = ApiAssetsDao::get_chain_assets_by_address_chain_code_symbol(
            &mut *conn,
            sql_address.clone(),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let joined = ApiAssetsDao::get_api_assets_by_address(
            &mut *conn,
            sql_address,
            None,
            None,
            None,
            Some(false),
        )
        .await
        .unwrap();
        let wrong_chain =
            ApiAssetsDao::list(&mut *conn, vec![], Some("missing' OR 1=1 --".to_string()))
                .await
                .unwrap();

        assert_eq!(exact.len(), 1);
        assert!(listed.is_empty());
        assert!(chain.is_empty());
        assert!(joined.is_empty());
        assert!(wrong_chain.is_empty());
    }

    #[tokio::test]
    async fn init_first_then_sync_updates_balance() {
        let dir = make_temp_dir("wallet_db_api_assets_balance_upsert_1");
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();

        let mut conn = pool.acquire().await.unwrap();

        let init_asset = make_asset("addr1", "eth", "ETH", "0xtoken", "0");
        ApiAssetsDao::upsert_assets_multi(&mut *conn, vec![init_asset]).await.unwrap();

        let sync_asset = make_asset("addr1", "eth", "ETH", "0xtoken", "123");
        ApiAssetsDao::upsert_assets_multi_update_balance(&mut *conn, vec![sync_asset])
            .await
            .unwrap();

        let balance: String = sqlx::query_scalar(
            "SELECT balance FROM api_assets WHERE address = ? AND chain_code = ? AND token_address = ?",
        )
        .bind("addr1")
        .bind("eth")
        .bind("0xtoken")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(balance, "123");
    }

    #[tokio::test]
    async fn sync_first_then_init_does_not_overwrite_balance() {
        let dir = make_temp_dir("wallet_db_api_assets_balance_upsert_2");
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();

        let mut conn = pool.acquire().await.unwrap();

        let sync_asset = make_asset("addr1", "eth", "ETH", "0xtoken", "123");
        ApiAssetsDao::upsert_assets_multi_update_balance(&mut *conn, vec![sync_asset])
            .await
            .unwrap();

        let init_asset = make_asset("addr1", "eth", "ETH", "0xtoken", "0");
        ApiAssetsDao::upsert_assets_multi(&mut *conn, vec![init_asset]).await.unwrap();

        let balance: String = sqlx::query_scalar(
            "SELECT balance FROM api_assets WHERE address = ? AND chain_code = ? AND token_address = ?",
        )
        .bind("addr1")
        .bind("eth")
        .bind("0xtoken")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
        assert_eq!(balance, "123");
    }

    #[tokio::test]
    async fn get_api_wallet_assets_v2_hides_disabled_coins() {
        let dir = make_temp_dir("wallet_db_api_assets_disabled_coin");
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();
        let mut conn = pool.acquire().await.unwrap();

        sqlx::query(
            "INSERT INTO api_chain (name, chain_code, protocols, status, main_symbol, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
        )
        .bind("BNB")
        .bind("bnb")
        .bind("[]")
        .bind(1u8)
        .bind("BNB")
        .execute(&mut *conn)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO api_account (account_id, name, address, pubkey, address_type, wallet_address, uid, derivation_path, derivation_path_index, chain_code, api_wallet_type, status, is_init, is_used, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
        )
        .bind(1u32)
        .bind("main")
        .bind("0x123")
        .bind("")
        .bind("")
        .bind("0xwallet")
        .bind("uid")
        .bind("")
        .bind(0i32)
        .bind("bnb")
        .bind(1i32)
        .bind(1i32)
        .bind(0i32)
        .bind(false)
        .execute(&mut *conn)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO api_coin (name, symbol, chain_code, token_address, price, protocol, decimals, is_default, is_popular, is_custom, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
        )
        .bind("BNB")
        .bind("BNB")
        .bind("bnb")
        .bind("")
        .bind("1")
        .bind("")
        .bind(18u8)
        .bind(1u8)
        .bind(0u8)
        .bind(0u8)
        .bind(0u8)
        .execute(&mut *conn)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO api_assets (
    decimals, address, chain_code, token_address, protocol, status, balance, is_multisig, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
        )
        .bind(18u8)
        .bind("0x123")
        .bind("bnb")
        .bind("")
        .bind("")
        .bind(1u8)
        .bind("0")
        .bind(0i8)
        .execute(&mut *conn)
        .await
        .unwrap();

        let rows =
            ApiAssetsDao::get_api_wallet_assets_v2(&mut *conn, "0xwallet", Some(1), None, false)
                .await
                .unwrap();

        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn api_wallet_total_assets_v3_uses_sparse_nonzero_assets() {
        let dir = make_temp_dir("wallet_db_api_assets_v3_sparse_nonzero");
        let ctx = crate::SqliteContext::new(&dir, Some("api_wallet.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();
        let mut conn = pool.acquire().await.unwrap();

        sqlx::query(
            r#"
INSERT INTO api_chain (name, chain_code, protocols, main_symbol, status, created_at)
VALUES
    ('Ethereum', 'eth', '[]', 'ETH', 1, CURRENT_TIMESTAMP),
    ('Solana', 'sol', '[]', 'SOL', 0, CURRENT_TIMESTAMP);

INSERT INTO api_account (
    account_id, name, address, address_type, wallet_address, chain_code, uid,
    api_wallet_type, status, is_init, created_at
) VALUES
    (1, 'target', 'target-active', '', 'wallet-target', 'eth', 'uid-target', 1, 1, 1, CURRENT_TIMESTAMP),
    (2, 'inactive', 'target-inactive', '', 'wallet-target', 'eth', 'uid-target', 1, 0, 1, CURRENT_TIMESTAMP),
    (1, 'disabled-chain', 'target-sol', '', 'wallet-target', 'sol', 'uid-target', 1, 1, 1, CURRENT_TIMESTAMP),
    (1, 'other', 'other-active', '', 'wallet-other', 'eth', 'uid-other', 1, 1, 1, CURRENT_TIMESTAMP);

INSERT INTO api_assets (
    decimals, address, token_address, chain_code, status, balance, created_at
) VALUES
    (18, 'target-active', 'token-live', 'eth', 1, '12', CURRENT_TIMESTAMP),
    (18, 'target-active', 'token-zero', 'eth', 1, '0', CURRENT_TIMESTAMP),
    (18, 'target-active', 'token-off', 'eth', 0, '99', CURRENT_TIMESTAMP),
    (18, 'target-inactive', 'token-ina', 'eth', 1, '77', CURRENT_TIMESTAMP),
    (9, 'target-sol', 'token-sol', 'sol', 1, '66', CURRENT_TIMESTAMP),
    (18, 'other-active', 'token-other', 'eth', 1, '88', CURRENT_TIMESTAMP);
"#,
        )
        .execute(&mut *conn)
        .await
        .unwrap();

        let rows =
            ApiAssetsDao::get_api_wallet_total_assets_v3(&mut *conn, "wallet-target", None, None)
                .await
                .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].balance, "12");
        assert_eq!(rows[0].chain_code, "eth");
        assert_eq!(rows[0].token_address, "token-live");

        let filtered = ApiAssetsDao::get_api_wallet_total_assets_v3(
            &mut *conn,
            "wallet-target",
            Some(2),
            None,
        )
        .await
        .unwrap();
        assert!(filtered.is_empty());

        let plan: Vec<(i64, i64, i64, String)> = sqlx::query_as(
            r#"
EXPLAIN QUERY PLAN
SELECT a.balance, a.chain_code, a.token_address
FROM api_assets a INDEXED BY api_assets_nonzero_wallet_total_idx
CROSS JOIN api_account acc INDEXED BY api_account_address_chain_code_idx
    ON acc.address = a.address AND a.chain_code = acc.chain_code
CROSS JOIN api_chain c INDEXED BY api_chain_chain_code_idx
    ON c.chain_code = a.chain_code
WHERE a.status = 1 AND a.balance != '0'
    AND acc.wallet_address = 'wallet-target'
    AND acc.status = 1
    AND c.status = 1
"#,
        )
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        assert!(plan.iter().any(|(_, _, _, detail)| {
            detail.contains("SCAN a USING COVERING INDEX api_assets_nonzero_wallet_total_idx")
        }));
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ApiAssertSummeryEntity {
    pub chain_code: String,
    pub symbol: String,
    pub api_assets_name: String,
    pub total_coins_quantity: f64,
    pub coin_unit_price: Option<f64>,
    pub total_account_amount: Option<f64>,
    pub coin_is_default: bool,
    pub assets_is_multisig: i8,
    /// 示例：{"eth": "0x...", "bsc": "0x..."}
    pub chain_token_map: serde_json::Value,
}
impl ApiAssertSummeryEntity {
    pub fn get_chain_token_map(&self) -> Result<HashMap<String, String>, crate::Error> {
        if let serde_json::Value::Object(map) = &self.chain_token_map {
            let mut result: HashMap<String, String> = HashMap::new();
            for (key, val) in map {
                if let serde_json::Value::String(s) = val {
                    result.insert(key.to_string(), s.to_string());
                } else {
                    // 如果不是字符串，可以转为字符串或跳过
                    result.insert(key.to_string(), val.to_string());
                }
            }
            Ok(result)
        } else {
            tracing::warn!("chain_token_map json map is not object: {:?}", self.chain_token_map);
            Ok(HashMap::new())
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct SumResult {
    pub total_coins_quantity: f64,
    pub total_amount: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct AssetBalanceEntity {
    pub balance: String,
    pub chain_code: String,
    pub token_address: String,
}

#[cfg(test)]
#[path = "api_assets/evm_token_identity_tests.rs"]
mod evm_token_identity_tests;

#[cfg(test)]
#[path = "api_assets/metadata_access_tests.rs"]
mod metadata_access_tests;

#[cfg(test)]
#[path = "api_assets/catalog_labels_read_tests.rs"]
mod catalog_labels_read_tests;
#[cfg(test)]
#[path = "api_assets/catalog_labels_test_support.rs"]
mod catalog_labels_test_support;

#[cfg(test)]
#[path = "api_assets/catalog_labels_refresh_tests.rs"]
mod catalog_labels_refresh_tests;

#[cfg(test)]
#[path = "api_assets/catalog_labels_policy_tests.rs"]
mod catalog_labels_policy_tests;

#[cfg(test)]
#[path = "api_assets/catalog_labels_visibility_tests.rs"]
mod catalog_labels_visibility_tests;

#[cfg(test)]
#[path = "api_assets/catalog_labels_schema_tests.rs"]
mod catalog_labels_schema_tests;

#[cfg(test)]
#[path = "api_assets/catalog_labels_write_tests.rs"]
mod catalog_labels_write_tests;

#[cfg(test)]
#[path = "api_assets/catalog_labels_scale_tests.rs"]
mod catalog_labels_scale_tests;

#[cfg(test)]
#[path = "api_assets/catalog_labels_removed_write_tests.rs"]
mod catalog_labels_removed_write_tests;

#[cfg(test)]
#[path = "api_assets/catalog_labels_removal_migration_tests.rs"]
mod catalog_labels_removal_migration_tests;
