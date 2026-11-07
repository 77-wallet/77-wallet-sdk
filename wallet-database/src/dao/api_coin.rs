use crate::{
    DbPool,
    entities::{
        api_coin::{ApiCoinData, ApiCoinEntity},
        asset_token_key::AssetTokenKey,
        coin::{BatchCoinSwappable, CoinId, CoinWithAssets},
    },
    pagination::Pagination,
};
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{Executor, QueryBuilder, Sqlite};

pub(crate) struct ApiCoinDao;

fn build_api_coin_list_query(
    symbol: Option<String>,
    chain_code: Option<String>,
    is_default: Option<u8>,
) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::<Sqlite>::new("SELECT * FROM api_coin WHERE is_del = 0");
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
}

fn push_api_excluded_coin_filters<'a>(
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
            let token = coin_id.token_address.normalized_for_chain(&coin_id.chain_code);
            tuple.push_bind(&coin_id.symbol).push_bind(&coin_id.chain_code).push_bind(token);
        });
    }
}

fn push_api_coin_with_assets_from_and_filters<'a>(
    query: &mut QueryBuilder<'a, Sqlite>,
    search: &'a str,
    exclude_token: &'a [String],
    chain_code: &'a str,
    address: &'a [String],
) {
    query.push(
        " FROM api_coin LEFT JOIN api_assets ON api_coin.chain_code = api_assets.chain_code \
         AND api_coin.token_address = api_assets.token_address AND ",
    );
    if address.is_empty() {
        query.push("0");
    } else {
        query.push("api_assets.address IN (");
        let mut addresses = query.separated(", ");
        for value in address {
            addresses.push_bind(value);
        }
        addresses.push_unseparated(")");
    }

    query.push(" WHERE 1 = 1");
    if chain_code.is_empty() {
        query.push(" AND api_coin.chain_code IN ('tron','bnb','eth')");
    } else {
        query.push(" AND api_coin.chain_code = ").push_bind(chain_code);
    }

    if !exclude_token.is_empty() {
        query.push(" AND api_coin.token_address NOT IN (");
        let mut tokens = query.separated(", ");
        for token in exclude_token {
            tokens.push_bind(AssetTokenKey::from_raw(Some(token)).normalized_for_chain(chain_code));
        }
        tokens.push_unseparated(")");
    }

    if !search.is_empty() {
        let pattern = format!("%{}%", search.to_lowercase());
        query
            .push(" AND (LOWER(api_coin.symbol) LIKE ")
            .push_bind(pattern.clone())
            .push(" OR LOWER(api_coin.token_address) LIKE ")
            .push_bind(pattern)
            .push(")");
    }
}

impl ApiCoinDao {
    pub async fn upsert_multi_coin<'a, E>(
        tx: E,
        coins: Vec<ApiCoinData>,
    ) -> Result<Vec<ApiCoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if coins.is_empty() {
            return Ok(vec![]);
        }
        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "insert into api_coin (
                name, symbol, chain_code, token_address, price, protocol, decimals, is_default, is_popular, is_custom, status, created_at, updated_at) ",
        );
        let mut seen = std::collections::HashSet::new();
        let coins = coins.into_iter().filter_map(|mut coin| {
            coin.token_address = coin.token_address.normalized_for_chain(&coin.chain_code);
            seen.insert((coin.chain_code.clone(), coin.token_address.clone())).then_some(coin)
        });
        query_builder.push_values(coins, |mut b, coin| {
            b.push_bind(coin.name)
                .push_bind(coin.symbol)
                .push_bind(coin.chain_code)
                .push_bind(coin.token_address)
                .push_bind(coin.price)
                .push_bind(coin.protocol)
                .push_bind(coin.decimals)
                .push_bind(coin.is_default)
                .push_bind(coin.is_popular)
                .push_bind(coin.is_custom)
                .push_bind(coin.status)
                .push_bind(coin.created_at.to_rfc3339_opts(SecondsFormat::Secs, true))
                .push_bind(coin.created_at.to_rfc3339_opts(SecondsFormat::Secs, true));
        });
        // query_builder.push(" on conflict (id) do update set updated_at = excluded.updated_at");
        query_builder.push(
            " on conflict (chain_code, token_address) do update set name = EXCLUDED.name, 
            symbol = EXCLUDED.symbol,
            price = EXCLUDED.price,
            decimals = EXCLUDED.decimals,
            is_custom = EXCLUDED.is_custom,
            is_default = EXCLUDED.is_default,
            status = EXCLUDED.status, 
            updated_at = EXCLUDED.updated_at, 
            is_del = EXCLUDED.is_del
            RETURNING *",
        );

        let query = query_builder.build_query_as::<ApiCoinEntity>();
        query.fetch_all(tx).await.map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn insert_missing_native_coin_for_bound_chain<'a, E>(
        tx: E,
        coin: ApiCoinData,
        node_id: &str,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if !coin.token_address.is_native() {
            return Ok(false);
        }
        let result = sqlx::query("INSERT INTO api_coin (name, symbol, chain_code, token_address, price, protocol, decimals, is_default, is_popular, is_custom, status, created_at, updated_at) SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13 WHERE EXISTS (SELECT 1 FROM api_chain WHERE chain_code = $3 AND status = 1 AND main_symbol = $2 AND node_id = $14) ON CONFLICT(chain_code, token_address) DO NOTHING")
            .bind(coin.name).bind(coin.symbol).bind(coin.chain_code).bind(coin.token_address)
            .bind(coin.price).bind(coin.protocol).bind(coin.decimals).bind(coin.is_default)
            .bind(coin.is_popular).bind(coin.is_custom).bind(coin.status)
            .bind(coin.created_at.to_rfc3339_opts(SecondsFormat::Secs, true))
            .bind(coin.created_at.to_rfc3339_opts(SecondsFormat::Secs, true)).bind(node_id)
            .execute(tx).await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result.rows_affected() != 0)
    }

    pub async fn list_v2<'a, E>(
        exec: E,
        symbol: Option<String>,
        chain_code: Option<String>,
        is_default: Option<u8>,
    ) -> Result<Vec<ApiCoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        build_api_coin_list_query(symbol, chain_code, is_default)
            .build_query_as::<ApiCoinEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list<'a, E>(
        exec: E,
        symbol: Option<String>,
        chain_code: Option<String>,
        is_default: Option<u8>,
    ) -> Result<Vec<ApiCoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        build_api_coin_list_query(symbol, chain_code, is_default)
            .build_query_as::<ApiCoinEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn main_coin<'a, E>(
        chain_code: &str,
        exec: E,
    ) -> Result<Option<ApiCoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM api_coin WHERE is_del = 0 AND token_address = '' and chain_code = $1 and status = 1";

        let res = sqlx::query_as::<_, ApiCoinEntity>(sql)
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
    ) -> Result<Option<ApiCoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM api_coin WHERE is_del = 0 AND chain_code = $1
            AND (token_address = $2 OR ($3 AND token_address = $2 COLLATE NOCASE))
            AND status = 1 ORDER BY (token_address = $2) DESC LIMIT 1";

        let res = sqlx::query_as::<_, ApiCoinEntity>(sql)
            .bind(chain_code)
            .bind(token_address)
            .bind(crate::entities::api_withdraw::is_signed_evm_chain(chain_code))
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res)
    }

    // 币种管理列表(不查询status = 1)
    pub async fn coin_list_symbol_not_in<'a, E>(
        exec: &E,
        exclude: &[CoinId],
        chain_code: Option<String>,
        keyword: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiCoinEntity>, crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        let base = " FROM api_coin WHERE is_del = 0 AND status = 1";
        let chain_code = chain_code.as_deref();

        let mut count_query = QueryBuilder::<Sqlite>::new(format!("SELECT COUNT(*){base}"));
        push_api_excluded_coin_filters(&mut count_query, exclude, chain_code, keyword);
        let total_count = count_query
            .build_query_scalar::<i64>()
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        let mut data_query = QueryBuilder::<Sqlite>::new(format!("SELECT *{base}"));
        push_api_excluded_coin_filters(&mut data_query, exclude, chain_code, keyword);
        data_query
            .push(" ORDER BY updated_at DESC, created_at DESC LIMIT ")
            .push_bind(page_size)
            .push(" OFFSET ")
            .push_bind(page * page_size);
        let data = data_query
            .build_query_as::<ApiCoinEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(Pagination { page, page_size, total_count, data })
    }

    pub async fn update_price_unit<'a, E>(
        exec: E,
        coin_id: &CoinId,
        price: &str,
        unit: Option<u8>,
        status: Option<i32>,
        time: Option<DateTime<Utc>>,
        symbol: Option<String>,
        name: Option<String>,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        // 基础 SQL 语句，设置 price
        let mut sql = "UPDATE api_coin SET price = ?".to_string();

        if unit.is_some() {
            sql.push_str(", decimals = ?");
        }

        if status.is_some() {
            sql.push_str(", status = ?");
        }

        if time.is_some() {
            sql.push_str(", updated_at = ?");
        }

        if symbol.is_some() {
            sql.push_str(", symbol = ?");
        }

        if name.is_some() {
            sql.push_str(", name = ?");
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
        if let Some(time_val) = time {
            query = query.bind(time_val.to_rfc3339_opts(SecondsFormat::Secs, true));
        }
        if let Some(symbol_val) = symbol {
            query = query.bind(symbol_val);
        }

        if let Some(name) = name {
            query = query.bind(name);
        }

        // 处理 token_address，如果为空，设置为空字符串
        let token_address = coin_id.token_address.normalized_for_chain(&coin_id.chain_code);
        // 绑定 WHERE 子句的参数
        query = query
            .bind(token_address)
            // .bind(&coin_id.symbol)
            .bind(&coin_id.chain_code);

        let _res = query.execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

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
            "UPDATE api_coin SET price = ? where token_address = ? and chain_code = ?".to_string();

        let _r = sqlx::query::<_>(&sql)
            .bind(price)
            .bind(AssetTokenKey::from_raw(Some(token_address)).normalized_for_chain(chain_code))
            .bind(chain_code)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

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

        let mut qb = QueryBuilder::<Sqlite>::new("UPDATE api_coin SET swappable = CASE ");

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

    pub async fn get_last_coin<'a, E>(
        exec: E,
        is_create: bool,
    ) -> Result<Option<ApiCoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let time = if is_create { "created_at" } else { "updated_at" };

        let sql =
            format!("select * from api_coin where is_custom = 0 order by {} desc limit 1", time);

        let res = sqlx::query_as::<_, ApiCoinEntity>(&sql)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res)
    }

    pub async fn coin_count<'a, E>(exec: E) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT COUNT(*) FROM api_coin";
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn same_coin_num<'a, E>(
        exec: E,
        symbol: &str,
        chain_code: &str,
    ) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT COUNT(*) FROM api_coin where symbol = ? and chain_code = ?";
        sqlx::query_scalar::<_, i64>(sql)
            .bind(symbol)
            .bind(chain_code)
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
        push_api_coin_with_assets_from_and_filters(
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

        let mut data_query = QueryBuilder::<Sqlite>::new(
            "SELECT api_coin.*, COALESCE(api_assets.balance, '0') AS balance",
        );
        push_api_coin_with_assets_from_and_filters(
            &mut data_query,
            search,
            &exclude_token,
            &chain_code,
            &address,
        );
        data_query
            .push(" ORDER BY CAST(api_assets.balance AS NUMERIC) DESC LIMIT ")
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

    pub async fn drop_coin_just_null_token_address<'a, E>(exec: E) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "DELETE FROM api_coin WHERE token_address IS NULL";

        sqlx::query(sql)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_by_chain_token_map_batch<'a, E>(
        exec: E,
        chain_list: &std::collections::HashMap<String, String>,
    ) -> Result<Vec<ApiCoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if chain_list.is_empty() {
            return Ok(vec![]);
        }

        let mut where_parts: Vec<String> = Vec::new();
        let mut params: Vec<String> = Vec::new();

        for (chain_code, token_address) in chain_list {
            where_parts.push("(chain_code = ? AND token_address = ?)".to_string());
            params.push(chain_code.clone());
            params.push(
                AssetTokenKey::from_raw(Some(token_address))
                    .normalized_for_chain(chain_code)
                    .as_db_str()
                    .to_string(),
            );
        }

        let where_clause = where_parts.join(" OR ");
        let sql = format!("SELECT * FROM api_coin WHERE is_del = 0 AND ({})", where_clause);

        let mut query = sqlx::query_as::<_, ApiCoinEntity>(&sql);
        for param in params {
            query = query.bind(param);
        }

        query.fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_by_chain_token_pairs_batch<'a, E>(
        exec: E,
        pairs: &[(String, String)],
    ) -> Result<Vec<ApiCoinEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if pairs.is_empty() {
            return Ok(vec![]);
        }

        let mut qb = QueryBuilder::<Sqlite>::new("SELECT * FROM api_coin WHERE is_del = 0 AND (");
        for (i, (chain_code, token_address)) in pairs.iter().enumerate() {
            if i > 0 {
                qb.push(" OR ");
            }
            qb.push("(chain_code = ")
                .push_bind(chain_code)
                .push(" AND token_address = ")
                .push_bind(
                    AssetTokenKey::from_raw(Some(token_address)).normalized_for_chain(chain_code),
                )
                .push(")");
        }
        qb.push(")");

        qb.build_query_as::<ApiCoinEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
