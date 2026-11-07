use crate::{entities::address_book::AddressBookEntity, pagination::Pagination};
use sqlx::{Executor, Pool, QueryBuilder, Sqlite};
use std::sync::Arc;

pub struct AddressBookDao;

fn address_book_column(key: &str) -> Result<&'static str, crate::DatabaseError> {
    match key {
        "id" => Ok("id"),
        "name" => Ok("name"),
        "address" => Ok("address"),
        "chain_code" => Ok("chain_code"),
        "created_at" => Ok("created_at"),
        "updated_at" => Ok("updated_at"),
        _ => Err(crate::DatabaseError::QueryFailed),
    }
}

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
        let mut query = QueryBuilder::<Sqlite>::new("SELECT * FROM address_book");
        if !conditions.is_empty() {
            query.push(" WHERE ");
            for (index, (key, value)) in conditions.iter().enumerate() {
                if index != 0 {
                    query.push(" AND ");
                }
                query.push(address_book_column(key)?).push(" = ").push_bind(value);
            }
        }

        query.push(" ORDER BY created_at DESC LIMIT 1");
        let res = query.build_query_as::<AddressBookEntity>().fetch_optional(exec).await?;
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
        let mut count_query = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM address_book");
        if let Some(chain) = chain_code {
            count_query.push(" WHERE chain_code = ").push_bind(chain);
        }
        let total_count = count_query.build_query_scalar::<i64>().fetch_one(&*pool).await?;

        let mut data_query = QueryBuilder::<Sqlite>::new(
            "SELECT id, name, address, chain_code, created_at, updated_at FROM address_book",
        );
        if let Some(chain) = chain_code {
            data_query.push(" WHERE chain_code = ").push_bind(chain);
        }
        data_query
            .push(" ORDER BY created_at DESC LIMIT ")
            .push_bind(page_size)
            .push(" OFFSET ")
            .push_bind(page * page_size);
        let data = data_query.build_query_as::<AddressBookEntity>().fetch_all(&*pool).await?;

        Ok(Pagination { page, page_size, total_count, data })
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

        let res = sqlx::query_as(query).bind(address).bind(chain_code).fetch_optional(exec).await?;
        Ok(res)
    }
}
