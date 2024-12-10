use crate::{entities::address_book::AddressBookEntity, pagination::Pagination};
use sqlx::{Executor, Pool, Sqlite};
use std::sync::Arc;

pub struct AddressBookDao;

impl AddressBookDao {
    pub async fn insert<'a, E>(
        executor: E,
        name: &str,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let query = r#"
            INSERT INTO address_book (name, address, chain_code, created_at,updated_at)
            VALUES (?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            RETURNING id, name, address, chain_code, created_at, updated_at
        "#;

        let mut rec = sqlx::query_as::<_, AddressBookEntity>(query)
            .bind(name)
            .bind(address)
            .bind(chain_code)
            .fetch_all(executor)
            .await?;
        Ok(rec.pop())
    }

    pub async fn update<'a, E>(
        executor: E,
        id: u32,
        name: &str,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let query = r#"
            UPDATE address_book
            SET
                name = $1,
                address = $2,
                chain_code =$3,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE id = $4
            RETURNING *
        "#;
        let mut rec = sqlx::query_as::<_, AddressBookEntity>(query)
            .bind(name)
            .bind(address)
            .bind(chain_code)
            .bind(id)
            .fetch_all(executor)
            .await?;

        Ok(rec.pop())
    }

    pub async fn delete<'a, E>(executor: E, id: i32) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let query = r#"DELETE FROM address_book WHERE id = ?"#;
        sqlx::query(query).bind(id).execute(executor).await?;
        Ok(())
    }

    pub async fn find_condition<'a, E>(
        exec: E,
        conditions: Vec<(&str, &str)>,
    ) -> Result<Option<AddressBookEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut query = r#"SELECT * FROM address_book "#.to_string();
        let mut query_where = Vec::new();

        for (key, value) in conditions.iter() {
            query_where.push(format!("{} = '{}'", key, value));
        }
        if !query_where.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&query_where.join(" AND "));
        }

        query.push_str(" ORDER BY created_at DESC LIMIT 1");
        let res = sqlx::query_as::<_, AddressBookEntity>(&query)
            .fetch_optional(exec)
            .await?;
        Ok(res)
    }

    pub async fn check_not_self<'a, E>(
        exec: E,
        id: u32,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let query =
            r#"SELECT * FROM address_book where id <> ? and address = ? and chain_code = ?"#
                .to_string();

        let res = sqlx::query_as::<_, AddressBookEntity>(&query)
            .bind(id)
            .bind(address)
            .bind(chain_code)
            .fetch_optional(exec)
            .await?;
        Ok(res)
    }

    pub async fn list(
        pool: Arc<Pool<Sqlite>>,
        chain_code: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<AddressBookEntity>, crate::DatabaseError> {
        let mut query =
            r#"SELECT id, name, address, chain_code, created_at, updated_at FROM address_book"#
                .to_string();

        if let Some(chain) = chain_code {
            query.push_str(format!(" WHERE chain_code = '{}'", chain).as_str());
        }
        let sql = format!("{} ORDER BY created_at DESC", query);
        let res = Pagination::<AddressBookEntity>::init(page, page_size);
        res.page(&*pool, sql.as_str()).await
    }

    pub async fn find_by_address<'a, E>(
        exec: E,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let query = "select * from address_book where address = ? and chain_code = ?";

        let res = sqlx::query_as(query)
            .bind(address)
            .bind(chain_code)
            .fetch_optional(exec)
            .await?;
        Ok(res)
    }
}
