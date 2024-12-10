use crate::{
    entities::bill::{
        BillEntity, BillKind, BillStatus, BillUpdateEntity, NewBillEntity, RecentBillListVo,
    },
    pagination::Pagination,
};
use sqlx::{Executor, Pool, Sqlite};
use std::{collections::HashSet, sync::Arc};
use wallet_types::constant::chain_code;
pub struct BillDao;

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

    pub async fn bill_lists<'a, E>(
        pool: &E,
        addr: &[String],
        chain_code: Option<&str>,
        symbol: Option<&str>,
        is_multisig: Option<i64>,
        min_value: Option<f64>,
        start: Option<i64>,
        end: Option<i64>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<BillEntity>, crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        let mut sql = String::from("SELECT * FROM bill WHERE owner IN (");
        let placeholders: Vec<String> = addr.iter().map(|item| format!("'{}'", item)).collect();
        sql.push_str(&placeholders.join(","));
        sql.push(')');

        if let Some(chain_code) = chain_code {
            sql.push_str(format!(" AND chain_code = '{}'", chain_code).as_str());
        }
        if let Some(is_multisig) = is_multisig {
            sql.push_str(format!(" AND is_multisig = '{}'", is_multisig).as_str());
        }

        if let Some(symbol) = symbol {
            sql.push_str(format!(" AND symbol = '{}'", symbol).as_str());
        }
        if let Some(start) = start {
            sql.push_str(format!(" AND transaction_time >= '{}'", start).as_str());
        }
        if let Some(end) = end {
            sql.push_str(format!(" AND transaction_time <= '{}'", end).as_str());
        }

        if let Some(min_value) = min_value {
            sql.push_str(format!(" AND CAST(value as REAL) > {}", min_value).as_str());
        }

        sql.push_str(" ORDER BY datetime(transaction_time, 'unixepoch') DESC");
        tracing::warn!("{}", sql);

        let paginate = Pagination::<BillEntity>::init(page, page_size);
        Ok(paginate.page(pool, &sql).await?)
    }

    pub async fn recent_bill(
        symbol: &str,
        addr: &str,
        chain_code: &str,
        min_value: Option<f64>,
        page: i64,
        page_size: i64,
        pool: Arc<Pool<Sqlite>>,
    ) -> Result<Pagination<RecentBillListVo>, crate::Error> {
        let min_value_condition = if let Some(value) = min_value {
            format!("AND CAST(value as REAL) >= {}", value)
        } else {
            String::new()
        };

        let sql = format!(
            r#"SELECT b.* FROM bill b
            INNER JOIN (
                SELECT from_addr, to_addr, MAX(transaction_time) AS max_time
                FROM bill
                WHERE owner = '{}'
                AND chain_code = '{}'
                AND to_addr <> '{}'
                AND symbol = '{}'
                {}
                AND transfer_type = 1
                GROUP BY to_addr
            ) latest 
            ON b.from_addr = latest.from_addr 
            AND b.to_addr = latest.to_addr 
            AND b.transaction_time = latest.max_time
            AND b.transfer_type = 1
            ORDER BY b.transaction_time DESC
            "#,
            addr, chain_code, addr, symbol, min_value_condition
        );

        let count_sql = format!(r#" SELECT count(*) FROM ({}) AS subquery;"#, sql);

        // 查询1000条数据作为过滤重复的数据
        let mut paginate = Pagination::<BillEntity>::init(0, 1000);
        paginate.total_count = paginate.group_count(&count_sql, pool.as_ref()).await?;
        paginate.data = paginate.data(&sql, pool.as_ref()).await?;

        let res = paginate
            .data
            .iter()
            .map(RecentBillListVo::from)
            .collect::<Vec<RecentBillListVo>>();

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

    /// Creates a new bill record in the database.
    pub async fn create<'a, E>(tx: NewBillEntity, exec: E) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let owner = tx.get_owner();
        let NewBillEntity {
            hash,
            from,
            to,
            token,
            chain_code,
            symbol,
            status,
            value,
            transaction_fee,
            resource_consume,
            transaction_time,
            multisig_tx,
            tx_type,
            tx_kind,
            queue_id,
            block_height,
            notes,
        } = tx;
        let token = token.unwrap_or_default();
        let tx_kind = tx_kind.to_i8();
        let multisig_tx = if multisig_tx { 1 } else { 0 };
        let time = sqlx::types::chrono::Utc::now().timestamp();

        // 一笔代币转账两次账变通知、如果第一次是手续费的通知，symbol 为空，等第二次代币的通知在修改symbol
        // TODO 此处需要优化 不能简单的判断value = 0,在部署多签账号的时候有问题
        let (symbol, to) = if value == 0.0
            && tx_kind == BillKind::Transfer.to_i8()
            && chain_code == chain_code::TRON
        {
            ("".to_string(), "".to_string())
        } else {
            (symbol, to)
        };

        let transaction_time = if transaction_time == 0 {
            time
        } else {
            transaction_time
        };

        let values = {
            format!(
                "('{hash}','{chain_code}','{symbol}','{tx_type}','{tx_kind}','{owner}','{from}','{to}',
                '{token}','{value}','{transaction_fee}','{resource_consume}','{transaction_time}','{status}',
                '{multisig_tx}','{block_height}','{queue_id}','{notes}','{time}','{time}'
            )",
            )
        };
        let sql = format!(
            "insert into bill 
            (
                hash,chain_code,symbol,transfer_type,tx_kind,owner,from_addr,to_addr,token,value,
                transaction_fee,resource_consume,transaction_time,status,is_multisig,block_height,queue_id,notes,
                created_at,updated_at
            ) 
                values {}
                on conflict (hash,transfer_type)
                do update set
                    status = excluded.status,
                    resource_consume =excluded.resource_consume, 
                    block_height = excluded.block_height,
                    transaction_time = excluded.transaction_time,
                    symbol = 
                        CASE WHEN bill.symbol = '' THEN excluded.symbol ELSE bill.symbol END,
                    to_addr = 
                        CASE WHEN bill.to_addr = '' THEN excluded.to_addr ELSE bill.to_addr END,
                    transaction_fee = 
                        CASE WHEN bill.transaction_fee = '0' THEN excluded.transaction_fee ELSE bill.transaction_fee END,
                    value =
                        CASE WHEN bill.value = '0' THEN excluded.value ELSE bill.value END,
                    updated_at = EXCLUDED.updated_at;
            ",
            values
        );

        sqlx::query(&sql)
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
