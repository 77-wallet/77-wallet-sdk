use crate::{
    DbPool,
    entities::{
        asset_token_key::AssetTokenKey,
        coin::{BatchCoinSwappable, CoinData, CoinEntity, CoinId, CoinWithAssets, SymbolId},
    },
    pagination::Pagination,
};
use chrono::SecondsFormat;
use sqlx::{
    Executor, Pool, QueryBuilder, Sqlite,
    types::chrono::{DateTime, Utc},
};
use std::{collections::HashSet, sync::Arc};

fn push_coin_list_page_filters<'a>(
    query: &mut QueryBuilder<'a, Sqlite>,
    chain_code: Option<&'a str>,
    keyword: Option<&'a str>,
    symbol_list: &'a [String],
) {
    if let Some(chain_code) = chain_code {
        query.push(" AND chain_code = ").push_bind(chain_code);
    }

    if !symbol_list.is_empty() {
        query.push(" AND symbol IN (");
        let mut separated = query.separated(", ");
        for symbol in symbol_list {
            separated.push_bind(symbol);
        }
        separated.push_unseparated(")");
    }

    if let Some(keyword) = keyword {
        query.push(" AND symbol LIKE ").push_bind(format!("%{keyword}%"));
    }
}

fn push_excluded_coin_page_filters<'a>(
    query: &mut QueryBuilder<'a, Sqlite>,
    exclude: &'a [CoinId],
    chain_code: Option<&'a str>,
    keyword: Option<&'a str>,
) {
    if let Some(chain_code) = chain_code {
        query.push(" AND chain_code = ").push_bind(chain_code);
    }

    if let Some(keyword) = keyword {
        query.push(" AND symbol LIKE ").push_bind(format!("%{keyword}%"));
    }

    if !exclude.is_empty() {
        query.push(" AND (symbol, chain_code, token_address) NOT IN ");
        query.push_tuples(exclude, |mut tuple, coin_id| {
            tuple
                .push_bind(&coin_id.symbol)
                .push_bind(&coin_id.chain_code)
                .push_bind(coin_id.token_address.normalized_for_chain(&coin_id.chain_code));
        });
    }
}

fn push_coin_with_assets_from_and_filters<'a>(
    query: &mut QueryBuilder<'a, Sqlite>,
    search: &'a str,
    exclude_token: &'a [String],
    chain_code: &'a str,
    address: &'a [String],
) {
    query.push(
        " FROM coin LEFT JOIN assets ON coin.chain_code = assets.chain_code \
         AND coin.token_address = assets.token_address AND ",
    );
    if address.is_empty() {
        query.push("0");
    } else {
        query.push("assets.address IN (");
        let mut addresses = query.separated(", ");
        for value in address {
            addresses.push_bind(value);
        }
        addresses.push_unseparated(")");
    }

    query.push(" WHERE swappable = 1");
    if chain_code.is_empty() {
        query.push(" AND coin.chain_code IN ('tron','bnb','eth','sol')");
    } else {
        query.push(" AND coin.chain_code = ").push_bind(chain_code);
    }

    if !exclude_token.is_empty() {
        query.push(" AND coin.token_address NOT IN (");
        let mut tokens = query.separated(", ");
        for token in exclude_token {
            tokens.push_bind(token);
        }
        tokens.push_unseparated(")");
    }

    if !search.is_empty() {
        let pattern = format!("%{}%", search.to_lowercase());
        query
            .push(" AND (LOWER(coin.symbol) LIKE ")
            .push_bind(pattern.clone())
            .push(" OR LOWER(coin.token_address) LIKE ")
            .push_bind(pattern)
            .push(")");
    }
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct CoinDao {
    pub name: String,
    pub symbol: String,
    pub chain_code: String,
    pub token_address: AssetTokenKey,
    pub status: i8,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}
impl CoinDao {
    pub async fn update_price_unit<'a, E>(
        exec: E,
        coin_id: &CoinId,
        price: &str,
        unit: Option<u8>,
        status: Option<i32>,
        swappable: Option<bool>,
        time: Option<DateTime<Utc>>,
        symbol: Option<String>,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        // 基础 SQL 语句，设置 price
        let mut sql = "UPDATE coin SET price = ?".to_string();

        if unit.is_some() {
            sql.push_str(", decimals = ?");
        }

        if status.is_some() {
            sql.push_str(", status = ?");
        }

        if swappable.is_some() {
            sql.push_str(", swappable = ?");
        }

        if time.is_some() {
            sql.push_str(", updated_at = ?");
        }

        if symbol.is_some() {
            sql.push_str(", symbol = ?");
        }

        sql.push_str(" WHERE token_address = ?  AND chain_code = ?");

        // let mut query = sqlx::query_as::<sqlx::Sqlite, Self>(&sql).bind(price); // 绑定 price 参数
        let mut query = sqlx::query(&sql).bind(price); // 绑定 price 参数

        if let Some(unit_val) = unit {
            query = query.bind(unit_val);
        }
        if let Some(status_val) = status {
            query = query.bind(status_val);
        }
        if let Some(swappable_val) = swappable {
            let swappable_val = if swappable_val { 1 } else { 0 };
            query = query.bind(swappable_val);
        }
        if let Some(time_val) = time {
            query = query.bind(time_val.to_rfc3339_opts(SecondsFormat::Secs, true));
        }
        if let Some(symbol_val) = symbol {
            query = query.bind(symbol_val);
        }

        // 处理 token_address，如果为空，设置为空字符串
        let token_address =
            coin_id.token_address.normalized_for_chain(&coin_id.chain_code).as_db_str().to_string();
        // 绑定 WHERE 子句的参数
        query = query
            .bind(token_address)
            // .bind(&coin_id.symbol)
            .bind(&coin_id.chain_code);

        let _res = query.execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

    // only update price
    pub async fn update_price_unit1<'a, E>(
        exec: E,
        chain_code: &str,
        token_address: &str,
        price: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql =
            "UPDATE coin SET price = ? where token_address = ? and chain_code = ?".to_string();

        let _r = sqlx::query::<_>(&sql)
            .bind(price)
            .bind(AssetTokenKey::from_raw(Some(token_address)).normalized_for_chain(chain_code))
            .bind(chain_code)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

    pub async fn chain_code_list<'a, E>(exec: E) -> Result<Vec<String>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT DISTINCT chain_code FROM coin WHERE is_del = 0";

        sqlx::query_scalar::<_, String>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_by_chain_token_map_batch<'a, E>(
        exec: E,
        chain_list: &std::collections::HashMap<String, String>,
    ) -> Result<Vec<CoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if chain_list.is_empty() {
            return Ok(vec![]);
        }

        let mut where_parts: Vec<String> = Vec::new();
        let mut params: Vec<String> = Vec::new();

        for (chain_code, token_address) in chain_list {
            let comparison = if crate::entities::api_withdraw::is_signed_evm_chain(chain_code) {
                "(chain_code = ? AND token_address = ? COLLATE NOCASE)"
            } else {
                "(chain_code = ? AND token_address = ?)"
            };
            where_parts.push(comparison.to_string());
            params.push(chain_code.clone());
            params.push(token_address.clone());
        }

        let where_clause = where_parts.join(" OR ");
        let sql = format!("SELECT * FROM coin WHERE is_del = 0 AND ({})", where_clause);

        let mut query = sqlx::query_as::<_, CoinEntity>(&sql);
        for param in params {
            query = query.bind(param);
        }

        query.fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_v2<'a, E>(
        exec: E,
        symbol: Option<String>,
        chain_code: Option<String>,
        is_default: Option<u8>,
    ) -> Result<Vec<CoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut query = QueryBuilder::<Sqlite>::new("SELECT * FROM coin WHERE is_del = 0");

        if let Some(symbol) = symbol {
            query.push(" AND symbol = ").push_bind(symbol);
        }

        if let Some(chain_code) = chain_code {
            query.push(" AND chain_code = ").push_bind(chain_code);
        }

        if let Some(is_default) = is_default {
            query.push(" AND is_default = ").push_bind(is_default);
        }

        query
            .build_query_as::<CoinEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list<'a, E>(
        exec: E,
        symbol_list: &[String],
        chain_code: Option<String>,
        is_default: Option<u8>,
    ) -> Result<Vec<CoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut query = QueryBuilder::<Sqlite>::new("SELECT * FROM coin WHERE is_del = 0");

        if !symbol_list.is_empty() {
            query.push(" AND symbol IN (");
            let mut separated = query.separated(", ");
            for symbol in symbol_list {
                separated.push_bind(symbol);
            }
            separated.push_unseparated(")");
        }

        if let Some(chain_code) = chain_code {
            query.push(" AND chain_code = ").push_bind(chain_code);
        }

        if let Some(is_default) = is_default {
            query.push(" AND is_default = ").push_bind(is_default);
        }

        query
            .build_query_as::<CoinEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // 币种管理列表(不查询status = 1)
    pub async fn coin_list_symbol_not_in<'a, E>(
        exec: &E,
        exclude: &[CoinId],
        chain_code: Option<String>,
        keyword: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<CoinEntity>, crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        let base = " FROM coin WHERE is_del = 0 AND status = 1";
        let chain_code = chain_code.as_deref();

        let mut count_query = QueryBuilder::<Sqlite>::new(format!("SELECT COUNT(*){base}"));
        push_excluded_coin_page_filters(&mut count_query, exclude, chain_code, keyword);
        let total_count = count_query
            .build_query_scalar::<i64>()
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        let mut data_query = QueryBuilder::<Sqlite>::new(format!("SELECT *{base}"));
        push_excluded_coin_page_filters(&mut data_query, exclude, chain_code, keyword);
        data_query
            .push(" ORDER BY updated_at DESC, created_at DESC LIMIT ")
            .push_bind(page_size)
            .push(" OFFSET ")
            .push_bind(page * page_size);
        let data = data_query
            .build_query_as::<CoinEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(Pagination { page, page_size, total_count, data })
    }

    pub async fn coin_list_page(
        pool: Arc<Pool<Sqlite>>,
        chain_code: Option<&str>,
        keyword: Option<&str>,
        symbol_list: Vec<String>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<CoinEntity>, crate::Error> {
        let base = " FROM coin WHERE is_del = 0 AND status = 1";

        let mut count_query = QueryBuilder::<Sqlite>::new(format!("SELECT COUNT(*){base}"));
        push_coin_list_page_filters(&mut count_query, chain_code, keyword, &symbol_list);
        let total_count = count_query
            .build_query_scalar::<i64>()
            .fetch_one(&*pool)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        let mut data_query = QueryBuilder::<Sqlite>::new(format!("SELECT *{base}"));
        push_coin_list_page_filters(&mut data_query, chain_code, keyword, &symbol_list);
        data_query
            .push(" LIMIT ")
            .push_bind(page_size)
            .push(" OFFSET ")
            .push_bind(page * page_size);
        let data = data_query
            .build_query_as::<CoinEntity>()
            .fetch_all(&*pool)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(Pagination { page, page_size, total_count, data })
    }

    /// Native identity is chain + empty contract; a catalog rename must not add a symbol alias.
    pub(crate) async fn rename_native_symbol(
        conn: &mut sqlx::SqliteConnection,
        chain_code: &str,
        symbol: &str,
    ) -> Result<(), crate::Error> {
        sqlx::query(
            "UPDATE coin SET symbol=? WHERE chain_code=? AND token_address='' AND symbol<>?",
        )
        .bind(symbol)
        .bind(chain_code)
        .bind(symbol)
        .execute(conn)
        .await
        .map_err(crate::DatabaseError::from)?;
        Ok(())
    }

    pub async fn upsert_multi_coin<'a, E>(tx: E, coins: Vec<CoinData>) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if coins.is_empty() {
            return Ok(());
        }
        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "insert into coin (
                name, symbol, chain_code, token_address, price, protocol, decimals, is_default, is_popular, is_custom, status,swappable, created_at, updated_at) ",
        );
        query_builder.push_values(coins, |mut b, coin| {
            let token_address = coin.token_address.normalized_for_chain(&coin.chain_code);
            let swappable = if coin.swappable { 1 } else { 0 };
            b.push_bind(coin.name)
                .push_bind(coin.symbol)
                .push_bind(coin.chain_code)
                .push_bind(token_address)
                .push_bind(coin.price)
                .push_bind(coin.protocol)
                .push_bind(coin.decimals)
                .push_bind(coin.is_default)
                .push_bind(coin.is_popular)
                .push_bind(coin.is_custom)
                .push_bind(coin.status)
                .push_bind(swappable)
                .push_bind(coin.created_at.to_rfc3339_opts(SecondsFormat::Secs, true))
                .push_bind(coin.updated_at.to_rfc3339_opts(SecondsFormat::Secs, true));
        });
        // query_builder.push(" on conflict (id) do update set updated_at = excluded.updated_at");
        query_builder.push(
            " on conflict do update set name = EXCLUDED.name,
            decimals = EXCLUDED.decimals,
            is_custom = EXCLUDED.is_custom,
            is_default = EXCLUDED.is_default,
            status = EXCLUDED.status, 
            swappable = EXCLUDED.swappable,
            updated_at = EXCLUDED.updated_at, 
            is_del = EXCLUDED.is_del",
        );

        let query = query_builder.build();
        query.execute(tx).await.map(|_| ()).map_err(|e| crate::Error::Database(e.into()))
    }

    // 批量更新swap开关
    pub async fn multi_update_swappable<'a, E>(
        coins: Vec<BatchCoinSwappable>,
        tx: E,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if coins.is_empty() {
            return Ok(());
        }

        let mut qb = QueryBuilder::<Sqlite>::new("UPDATE coin SET swappable = CASE ");

        for p in coins.iter() {
            qb.push("WHEN chain_code=")
                .push_bind(p.chain_code.clone())
                .push(" AND token_address=")
                .push_bind(
                    AssetTokenKey::from_raw(Some(&p.token_address))
                        .normalized_for_chain(&p.chain_code),
                )
                .push(" THEN ")
                .push_bind(1)
                .push(" ");
        }
        qb.push("ELSE 0 END");

        qb.build().execute(tx).await.map(|_| ()).map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn main_coin<'a, E>(
        chain_code: &str,
        exec: E,
    ) -> Result<Option<CoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM coin WHERE is_del = 0 AND token_address = '' and chain_code = $1";

        let res = sqlx::query_as::<_, CoinEntity>(sql)
            .bind(chain_code)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res)
    }

    pub async fn get_coin_by_chain_code_token_address<'a, E>(
        exec: E,
        chain_code: &str,
        token_address: AssetTokenKey,
    ) -> Result<Option<CoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM coin WHERE is_del = 0 AND chain_code = $1 and token_address = $2";

        let res = sqlx::query_as::<_, CoinEntity>(sql)
            .bind(chain_code)
            .bind(token_address.normalized_for_chain(chain_code))
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res)
    }

    pub async fn drop_coin_just_null_token_address<'a, E>(exec: E) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "DELETE FROM coin WHERE token_address IS NULL";

        sqlx::query(sql)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn drop_custom_coin<'a, E>(exec: E, coin_id: &SymbolId) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "DELETE FROM coin WHERE chain_code = $1 and symbol = $2 AND is_custom = 1";

        sqlx::query(sql)
            .bind(&coin_id.chain_code)
            .bind(&coin_id.symbol)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // 查询coin表中is_custom为1，并且assets表中chain_code、symbol和coin表中chain_code、symbol相同的记录
    pub async fn get_custom_coin_and_assets<'a, E>(exec: E) -> Result<Vec<CoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM coin WHERE is_custom = 1 AND EXISTS 
        (SELECT 1 FROM assets WHERE assets.chain_code = coin.chain_code AND assets.symbol = coin.symbol)";
        sqlx::query_as::<_, CoinEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn drop_multi_custom_coin<'a, E>(
        tx: E,
        coin_ids: HashSet<SymbolId>,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if coin_ids.is_empty() {
            return Ok(());
        }

        // 创建动态 SQL 语句
        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "DELETE FROM coin WHERE is_custom = 1 AND (chain_code, symbol) IN ",
        );

        // 使用 push_tuples 方法添加所有 coin_id 参数
        query_builder.push_tuples(coin_ids, |mut b, coin_id| {
            b.push_bind(coin_id.chain_code).push_bind(coin_id.symbol);
        });

        // 构建并执行查询
        let query = query_builder.build();
        query.execute(tx).await.map(|_| ()).map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn clean_table<'a, E>(exec: E) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "DELETE FROM coin";
        sqlx::query(sql)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn batch_update_default_coin_status<'a, E>(
        exec: E,
        coin_ids: &[CoinId],
        status: u8,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if coin_ids.is_empty() {
            return Ok(());
        }

        let mut query_builder = QueryBuilder::<Sqlite>::new("UPDATE coin SET status = ");
        query_builder.push_bind(status);
        query_builder.push(
            ", updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE is_custom = 0 AND is_default = 1 AND ",
        );
        query_builder.push(" (chain_code, symbol, token_address) IN ");
        query_builder.push_tuples(coin_ids.iter(), |mut b, coin_id| {
            b.push_bind(&coin_id.chain_code).push_bind(&coin_id.symbol).push_bind(
                coin_id
                    .token_address
                    .normalized_for_chain(&coin_id.chain_code)
                    .as_db_str()
                    .to_string(),
            );
        });

        query_builder
            .build()
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_last_coin<'a, E>(
        exec: E,
        is_create: bool,
    ) -> Result<Option<CoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let time = if is_create { "created_at" } else { "updated_at" };

        let sql = format!("select * from coin where is_custom = 0 order by {} desc limit 1", time);

        let res = sqlx::query_as::<_, CoinEntity>(&sql)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res)
    }

    pub async fn coin_count<'a, E>(exec: E) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT COUNT(*) FROM coin";
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn coin_list_with_assets(
        search: &str,
        exclude_token: Vec<String>,
        chain_code: String,
        address: Vec<String>,
        page: i64,
        page_size: i64,
        pool: DbPool,
    ) -> Result<Pagination<CoinWithAssets>, crate::Error> {
        let mut count_query = QueryBuilder::<Sqlite>::new("SELECT COUNT(*)");
        push_coin_with_assets_from_and_filters(
            &mut count_query,
            search,
            &exclude_token,
            &chain_code,
            &address,
        );
        let total_count = count_query
            .build_query_scalar::<i64>()
            .fetch_one(&*pool)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        let mut data_query =
            QueryBuilder::<Sqlite>::new("SELECT coin.*, COALESCE(assets.balance, '0') AS balance");
        push_coin_with_assets_from_and_filters(
            &mut data_query,
            search,
            &exclude_token,
            &chain_code,
            &address,
        );
        data_query
            .push(" ORDER BY CAST(assets.balance AS NUMERIC) DESC LIMIT ")
            .push_bind(page_size)
            .push(" OFFSET ")
            .push_bind(page * page_size);
        let data = data_query
            .build_query_as::<CoinWithAssets>()
            .fetch_all(&*pool)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(Pagination { page, page_size, total_count, data })
    }

    // 相同币符号的数量
    pub async fn same_coin_num<'a, E>(
        exec: E,
        symbol: &str,
        chain_code: &str,
    ) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT COUNT(*) FROM coin where symbol = ? and chain_code = ?";
        sqlx::query_scalar::<_, i64>(sql)
            .bind(symbol)
            .bind(chain_code)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete_wsol_error<'a, E>(exec: E) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "delete from coin where symbol = 'wSOL' and chain_code = 'sol' and token_address = 'So11111111111111111111111111111111111111112';";
        let _r = sqlx::query::<_>(&sql)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }
}
