use crate::{
    DbPool,
    entities::bill::{
        BillEntity, BillKind, BillStatus, BillUpdateEntity, NewBillEntity, RecentBillListVo,
    },
    pagination::Pagination,
};
use chrono::Utc;
use serde::Serialize;
use sqlx::{Executor, QueryBuilder, Sqlite};
use std::collections::HashSet;
pub struct BillDao;

struct BillListFilters<'a> {
    addr: &'a [String],
    chain_code: Option<&'a str>,
    symbol: Option<&'a str>,
    is_multisig: Option<i64>,
    min_value: Option<f64>,
    start: Option<i64>,
    end: Option<i64>,
    transfer_type: &'a [i32],
}

impl BillListFilters<'_> {
    fn push_to<'args>(&'args self, query: &mut QueryBuilder<'args, Sqlite>) {
        query.push(" WHERE owner IN (");
        {
            let mut separated = query.separated(",");
            for address in self.addr {
                separated.push_bind(address);
            }
        }
        query.push(")");

        if !self.transfer_type.is_empty() {
            query.push(" AND tx_kind IN (");
            {
                let mut separated = query.separated(",");
                for kind in self.transfer_type {
                    separated.push_bind(kind);
                }
            }
            query.push(")");
        }
        if let Some(chain_code) = self.chain_code {
            query.push(" AND chain_code = ").push_bind(chain_code);
        }
        if let Some(is_multisig) = self.is_multisig {
            query.push(" AND is_multisig = ").push_bind(is_multisig);
        }
        if let Some(symbol) = self.symbol {
            query.push(" AND symbol = ").push_bind(symbol);
        }
        if let Some(start) = self.start {
            query.push(" AND transaction_time >= ").push_bind(start);
        }
        if let Some(end) = self.end {
            query.push(" AND transaction_time <= ").push_bind(end);
        }
        if let Some(min_value) = self.min_value {
            query.push(" AND CAST(value as REAL) > ").push_bind(min_value);
        }
    }
}

impl BillDao {
    pub async fn get_one_by_hash<'a, E>(
        hash: &str,
        exec: E,
    ) -> Result<Option<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from bill where hash = $1";

        sqlx::query_as::<_, BillEntity>(sql)
            .bind(hash)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_by_hash_and_type<'a, E>(
        executor: E,
        hash: &str,
        transfer_type: i64,
    ) -> Result<Option<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from bill where hash = $1 and transfer_type = $2";

        sqlx::query_as::<_, BillEntity>(sql)
            .bind(hash)
            .bind(transfer_type)
            .fetch_optional(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_by_hash_and_owner<'a, E>(
        executor: E,
        hash: &str,
        owner: &str,
    ) -> Result<Option<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from bill where hash = $1 and owner = $2";

        sqlx::query_as::<_, BillEntity>(sql)
            .bind(hash)
            .bind(owner)
            .fetch_optional(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn find_by_id<'a, E>(
        executor: E,
        id: &str,
    ) -> Result<Option<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from bill where id = ?";

        sqlx::query_as::<_, BillEntity>(sql)
            .bind(id)
            .fetch_optional(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // 查询某种类型的最后一笔交易
    pub async fn last_kind_bill<'a, E>(
        exec: E,
        owner_address: &str,
        bill_kind: Vec<i8>,
    ) -> Result<Option<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if bill_kind.is_empty() {
            return Ok(None);
        }

        let mut query = QueryBuilder::<Sqlite>::new("select * from bill where owner = ");
        query.push_bind(owner_address).push(" and tx_kind in (");
        {
            let mut separated = query.separated(",");
            for kind in bill_kind {
                separated.push_bind(kind);
            }
        }
        query.push(") ORDER BY datetime(transaction_time, 'unixepoch') DESC limit 1");

        query
            .build_query_as::<BillEntity>()
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn lists_by_hashs<'a, E>(
        pool: E,
        owner: &str,
        hashs: Vec<String>,
    ) -> Result<Vec<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if hashs.is_empty() {
            return Ok(Vec::new());
        }

        let mut query = QueryBuilder::<Sqlite>::new("select * from bill where owner = ");
        query.push_bind(owner).push(" and hash in (");
        {
            let mut separated = query.separated(",");
            for hash in hashs {
                separated.push_bind(hash);
            }
        }
        query.push(")");

        let res = query
            .build_query_as::<BillEntity>()
            .fetch_all(pool)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res)
    }

    pub async fn bill_count<'a, E>(pool: E) -> Result<i64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT COUNT(*) FROM bill";

        let res = sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(pool)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res)
    }

    pub async fn bill_lists<'a, E>(
        pool: &E,
        addr: &[String],
        chain_code: Option<&str>,
        symbol: Option<&str>,
        is_multisig: Option<i64>,
        min_value: Option<f64>,
        start: Option<i64>,
        end: Option<i64>,
        transfer_type: Vec<i32>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<BillEntity>, crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        let mut paginate = Pagination::<BillEntity>::init(page, page_size);
        if addr.is_empty() {
            return Ok(paginate);
        }

        let filters = BillListFilters {
            addr,
            chain_code,
            symbol,
            is_multisig,
            min_value,
            start,
            end,
            transfer_type: &transfer_type,
        };

        let mut count_query = QueryBuilder::<Sqlite>::new("SELECT count(*) FROM bill");
        filters.push_to(&mut count_query);
        paginate.total_count = count_query
            .build_query_scalar::<i64>()
            .fetch_one(pool)
            .await
            .map_err(|error| crate::Error::Database(error.into()))?;

        let mut data_query = QueryBuilder::<Sqlite>::new("SELECT * FROM bill");
        filters.push_to(&mut data_query);
        data_query
            .push(" ORDER BY datetime(transaction_time, 'unixepoch') DESC LIMIT ")
            .push_bind(page_size)
            .push(" OFFSET ")
            .push_bind(page * page_size);
        paginate.data = data_query
            .build_query_as::<BillEntity>()
            .fetch_all(pool)
            .await
            .map_err(|error| crate::Error::Database(error.into()))?;

        Ok(paginate)
    }

    // 查询5分钟内交易状态确认中的交易(用于swap本地验证余额)
    pub async fn last_swap_bill<'a, E>(
        pool: E,
        from: &str,
        chain_code: &str,
    ) -> Result<Option<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let time = Utc::now().timestamp() - 300;
        let sql = "select * from bill where from_addr = ? and chain_code = ? and tx_kind = ? and transaction_time >= ?  and status = ? ORDER BY datetime(transaction_time, 'unixepoch') DESC limit 1";
        sqlx::query_as::<_, BillEntity>(sql)
            .bind(from)
            .bind(chain_code)
            .bind(BillKind::Swap.to_i8())
            .bind(time)
            .bind(BillStatus::Pending.to_i8())
            .fetch_optional(pool)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // 最近的一笔授权交易
    pub(crate) async fn last_approve_bill<'a, E>(
        pool: E,
        from: &str,
        to: &str,
        contract: &str,
        chain_code: &str,
        tx_kind: BillKind,
    ) -> Result<Option<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let time = Utc::now().timestamp() - 300;
        let sql = r#"select * 
                from bill
                where 
                    from_addr = ? and chain_code = ? and tx_kind = ? and transaction_time >= ? and status = ? 
                    and to_addr = ? and token = ?
                ORDER BY datetime(transaction_time, 'unixepoch') DESC limit 1"#;
        sqlx::query_as::<_, BillEntity>(sql)
            .bind(from)
            .bind(chain_code)
            .bind(tx_kind.to_i8())
            .bind(time)
            .bind(BillStatus::Pending.to_i8())
            .bind(to)
            .bind(contract)
            .fetch_optional(pool)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // 最近转列
    pub async fn recent_bill(
        token: &str,
        addr: &str,
        chain_code: &str,
        min_value: Option<f64>,
        page: i64,
        page_size: i64,
        pool: DbPool,
    ) -> Result<Pagination<RecentBillListVo>, crate::Error> {
        let mut query = QueryBuilder::<Sqlite>::new(
            r#"SELECT b.* FROM bill b
            INNER JOIN (
                SELECT from_addr, to_addr, MAX(transaction_time) AS max_time
                FROM bill
                WHERE owner = "#,
        );
        query
            .push_bind(addr)
            .push(" AND chain_code = ")
            .push_bind(chain_code)
            .push(" AND to_addr <> ")
            .push_bind(addr)
            .push(" AND to_addr <> '' AND token = ")
            .push_bind(token);
        if let Some(min_value) = min_value {
            query.push(" AND CAST(value as REAL) >= ").push_bind(min_value);
        }
        query.push(
            r#" AND transfer_type = 1
                GROUP BY to_addr
            ) latest 
            ON b.from_addr = latest.from_addr 
            AND b.to_addr = latest.to_addr 
            AND b.transaction_time = latest.max_time
            AND b.transfer_type = 1
            ORDER BY b.transaction_time DESC
            "#,
        );

        // 查询1000条数据作为过滤重复的数据
        let mut paginate = Pagination::<BillEntity>::init(0, 1000);
        paginate.data = query
            .build_query_as::<BillEntity>()
            .fetch_all(pool.as_ref())
            .await
            .map_err(|error| crate::Error::Database(error.into()))?;

        let res =
            paginate.data.iter().map(RecentBillListVo::from).collect::<Vec<RecentBillListVo>>();

        let mut unique = HashSet::new();
        let mut result = vec![];
        for r in res {
            if !unique.contains(&r.address) {
                unique.insert(r.address.clone());
                result.push(r);
            }
        }
        let total_count = result.len() as i64;
        let start = page * page_size;
        let end = ((page + 1) * page_size).min(total_count);

        let res = Pagination {
            page: paginate.page,
            page_size: paginate.page_size,
            total_count,
            data: result[start as usize..end as usize].to_vec(),
        };
        Ok(res)
    }

    /// Fetches all bill details from the database with the given status.
    /// A `Vec` of `BillDetail` instances representing the fetched bill details.
    pub async fn fetch_all_by_status<'a, E>(
        executor: E,
        status: i8,
    ) -> Result<Vec<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from bill where status = $1";

        sqlx::query_as::<_, BillEntity>(sql)
            .bind(status)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // 包括需要更新交易的hash(ton链的in_msg 字段处理)
    pub async fn update_all<T>(
        pool: DbPool,
        tx: NewBillEntity<T>,
        id: i32,
    ) -> Result<(), crate::Error>
    where
        T: Serialize,
    {
        let extra_str = tx.get_extra_str()?;
        let time = sqlx::types::chrono::Utc::now().timestamp();

        let mut sql = String::from(
            r#"
        UPDATE bill SET 
            transaction_fee = $2,
            transaction_time = $3,
            status = $4,
            block_height = $5,
            updated_at = $6,
            resource_consume = $7,
            hash = $8
        "#,
        );

        // 判断 extra 是否为空，决定是否追加字段
        if !extra_str.is_empty() {
            sql.push_str(", extra = $9");
        }

        sql.push_str(" WHERE id = $1 RETURNING *");

        let mut query = sqlx::query(&sql)
            .bind(id)
            .bind(tx.transaction_fee.clone())
            .bind(tx.transaction_time.to_string())
            .bind(tx.status)
            .bind(tx.block_height.to_string())
            .bind(time)
            .bind(tx.resource_consume.clone())
            .bind(tx.hash);

        if !extra_str.is_empty() {
            query = query.bind(extra_str);
        }

        query.execute(pool.as_ref()).await.map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

    /// Creates a new bill record in the database.
    pub async fn create<'a, E, T>(tx: NewBillEntity<T>, exec: E) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
        T: serde::Serialize,
    {
        if !tx.value.is_finite() {
            return Err(crate::Error::Other("bill amount must be finite".into()));
        }
        // Bind decimal text explicitly: SQLite otherwise formats REAL values
        // as scientific notation and may shorten their significant digits.
        let value = tx.value.to_string();
        let owner = tx.get_owner();
        let time = Utc::now().timestamp();
        let signer = tx.get_singer_str();
        let (symbol, to) = tx.get_symbol_and_to();
        let transaction_time = if tx.transaction_time == 0 { time } else { tx.transaction_time };
        let token = tx.token.as_db_str().to_string();
        let multisig_tx = tx.get_multisig_i32();
        let extra = tx.get_extra_str()?;
        let sql = "insert into bill
            (
                hash,chain_code,symbol,transfer_type,tx_kind,owner,from_addr,to_addr,token,value,
                transaction_fee,resource_consume,transaction_time,status,is_multisig,block_height,queue_id,notes,
                created_at,updated_at,signer,extra
            )
                values (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
                on conflict (hash,transfer_type,owner)
                do update set
                    status = excluded.status,
                    resource_consume =excluded.resource_consume, 
                    block_height = excluded.block_height,
                    transaction_time = excluded.transaction_time,
                    transaction_fee = excluded.transaction_fee,
                    symbol = 
                        CASE WHEN bill.symbol = '' THEN excluded.symbol ELSE bill.symbol END,
                    to_addr = 
                        CASE WHEN bill.to_addr = '' THEN excluded.to_addr ELSE bill.to_addr END,
                    value =
                        CASE WHEN bill.value = '0' THEN excluded.value ELSE bill.value END,
                    extra = 
                        CASE WHEN excluded.extra != '' THEN excluded.extra ELSE bill.extra END,
                    updated_at = EXCLUDED.updated_at;
            ";

        sqlx::query(sql)
            .bind(tx.hash)
            .bind(tx.chain_code)
            .bind(symbol)
            .bind(tx.tx_type)
            .bind(tx.tx_kind.to_i8())
            .bind(owner)
            .bind(tx.from)
            .bind(to)
            .bind(token)
            .bind(value)
            .bind(tx.transaction_fee)
            .bind(tx.resource_consume)
            .bind(transaction_time)
            .bind(tx.status)
            .bind(multisig_tx)
            .bind(tx.block_height)
            .bind(tx.queue_id)
            .bind(tx.notes)
            .bind(time)
            .bind(time)
            .bind(signer)
            .bind(extra)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // 修改交易
    pub async fn update<'a, E>(
        transaction: &BillUpdateEntity,
        tx: E,
    ) -> Result<Option<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update bill set 
                transaction_fee = $2,
                transaction_time = $3,
                status = $4,
                block_height = $5,
                updated_at = $6,
                resource_consume = $7
            where hash = $1
            RETURNING *
            "#;

        let time = sqlx::types::chrono::Utc::now().timestamp();
        let mut res = sqlx::query_as::<_, BillEntity>(sql)
            .bind(transaction.hash.clone())
            .bind(transaction.format_fee.clone())
            .bind(transaction.transaction_time.to_string())
            .bind(transaction.status)
            .bind(transaction.block_height.to_string())
            .bind(time)
            .bind(transaction.resource_consume.clone())
            .fetch_all(tx)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.pop())
    }

    // if transaction not on chain update status false
    pub async fn update_fail<'a, E>(tx_hash: &str, exec: E) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"update bill set status = $2,updated_at = $3 where hash = $1 RETURNING *"#;
        let time = sqlx::types::chrono::Utc::now().timestamp();

        sqlx::query(sql)
            .bind(tx_hash)
            .bind(BillStatus::Failed.to_i8())
            .bind(time)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

    pub async fn on_going_bill<'a, E>(
        chain_code: &str,
        address: &str,
        exec: E,
    ) -> Result<Vec<BillEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        // 一个 半小时
        let time = wallet_utils::time::now().timestamp() - (90 * 60);

        let sql = "select * from bill where owner = $1 and status = $2 and chain_code = $3 and created_at > $4";

        let rs = sqlx::query_as::<_, BillEntity>(sql)
            .bind(address)
            .bind(BillStatus::Pending.to_i8())
            .bind(chain_code)
            .bind(time)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(rs)
    }

    pub async fn last_bill<'a, E>(
        chain_code: &str,
        address: &str,
        exec: E,
    ) -> Result<Option<BillEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from bill where owner = $1 and chain_code = $2 order by block_height desc limit 1";

        let rs = sqlx::query_as::<_, BillEntity>(sql)
            .bind(address)
            .bind(chain_code)
            .fetch_optional(exec)
            .await?;
        Ok(rs)
    }

    pub async fn first_transfer<'a, E>(
        address: &str,
        chain_code: &str,
        exec: E,
    ) -> Result<Option<BillEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from bill where to_addr = ? and chain_code = ?";

        let rs = sqlx::query_as::<_, BillEntity>(sql)
            .bind(address)
            .bind(chain_code)
            .fetch_optional(exec)
            .await?;
        Ok(rs)
    }
}

// let NewBillEntity {
//     hash,
//     from,
//     to,
//     token,
//     chain_code,
//     symbol,
//     status,
//     value,
//     transaction_fee,
//     resource_consume,
//     transaction_time,
//     multisig_tx,
//     tx_type,
//     tx_kind,
//     queue_id,
//     block_height,
//     notes,
//     signer,
//     extra,
// } = tx;
// let tx_kind = tx.tx_kind.to_i8();
// let multisig_tx = if tx.multisig_tx { 1 } else { 0 };

// let (symbol, to) = if tx.value == 0.0
//     && tx_kind == BillKind::Transfer.to_i8()
//     && tx.chain_code == chain_code::TRON
// {
//     ("".to_string(), "".to_string())
// } else {
//     (tx.symbol.to_uppercase(), tx.to)
// };

// let signer = tx.signer.join(",");
// let signer = signer.trim_end_matches(",");

// let values = {
//     format!(
//         "('{hash}','{chain_code}','{symbol}','{tx_type}','{tx_kind}','{owner}','{from}','{to}',
//         '{token}','{value}','{transaction_fee}','{resource_consume}','{transaction_time}','{status}',
//         '{multisig_tx}','{block_height}','{queue_id}','{notes}','{time}','{time}','{signer}'
//     )",
//     )
// };

#[cfg(test)]
mod decimal_storage_tests {
    use super::*;

    fn bill(hash: &str, value: f64) -> NewBillEntity {
        NewBillEntity::new(
            hash.into(),
            "from".into(),
            "to".into(),
            value,
            "eth".into(),
            "ETH".into(),
            false,
            BillKind::Transfer,
            "".into(),
        )
    }

    async fn pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./schema/core/migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn bill_new_amounts_are_stored_as_plain_text_without_truncation() {
        let pool = pool().await;
        for (hash, value, expected) in [
            ("small", 0.00001118, "0.00001118"),
            ("precision", 0.12345678912345678, "0.12345678912345678"),
            ("large", 1e20, "100000000000000000000"),
            ("negative", -0.00001118, "-0.00001118"),
            ("zero", 0.0, "0"),
        ] {
            BillDao::create(bill(hash, value), &pool).await.unwrap();
            let (stored, kind): (String, String) =
                sqlx::query_as("SELECT value, typeof(value) FROM bill WHERE hash = ?")
                    .bind(hash)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(stored, expected);
            assert_eq!(kind, "text");
        }
    }

    #[tokio::test]
    async fn bill_legacy_amounts_display_without_rewriting_history_or_upsert_value() {
        let pool = pool().await;
        BillDao::create(bill("legacy", 0.0), &pool).await.unwrap();
        sqlx::query("UPDATE bill SET value = '1.118e-05' WHERE hash = 'legacy'")
            .execute(&pool)
            .await
            .unwrap();
        let mut legacy = BillDao::get_one_by_hash("legacy", &pool).await.unwrap().unwrap();
        legacy.truncate_to_8_decimals();
        assert_eq!(legacy.value, "0.00001118");
        BillDao::create(bill("legacy", 2.0), &pool).await.unwrap();
        let stored: String = sqlx::query_scalar("SELECT value FROM bill WHERE hash = 'legacy'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, "1.118e-05");

        BillDao::create(bill("pending", 0.0), &pool).await.unwrap();
        BillDao::create(bill("pending", 0.00001118), &pool).await.unwrap();
        let stored: String = sqlx::query_scalar("SELECT value FROM bill WHERE hash = 'pending'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, "0.00001118");
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM bill").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn bill_nonfinite_amounts_fail_without_writing() {
        let pool = pool().await;
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(BillDao::create(bill("invalid", value), &pool).await.is_err());
        }
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM bill").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0);
    }
}
