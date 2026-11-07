pub(crate) mod delete_builder;
pub(crate) mod query_builder;
pub(crate) mod update_builder;

use async_trait::async_trait;
use sqlx::{Executor, Sqlite, sqlite::SqliteArguments};

pub trait SqlQueryBuilder<'q>: Sized {
    fn build_sql(self) -> Result<(String, SqliteArguments<'q>), crate::Error>;
}

#[async_trait]
pub trait SqlExecutableNoReturn<'a>: SqlQueryBuilder<'a> {
    async fn execute<'e, E>(self, executor: E) -> Result<(), crate::Error>
    where
        E: Executor<'e, Database = Sqlite> + Send,
    {
        let (sql, args) = self.build_sql()?;
        sqlx::query_with(&sql, args)
            .execute(executor)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }
}

#[async_trait]
pub trait SqlExecutableReturn<'a, T>: SqlQueryBuilder<'a>
where
    for<'r> T: sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin + 'a,
{
    async fn fetch_all<'e, E>(self, executor: E) -> Result<Vec<T>, crate::Error>
    where
        E: Executor<'e, Database = Sqlite> + Send,
    {
        let (sql, args) = self.build_sql()?;
        sqlx::query_as_with::<_, T, _>(&sql, args)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    async fn fetch_optional<'e, E>(self, executor: E) -> Result<Option<T>, crate::Error>
    where
        E: Executor<'e, Database = Sqlite> + Send,
    {
        let (sql, args) = self.build_sql()?;
        sqlx::query_as_with::<_, T, _>(&sql, args)
            .fetch_optional(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    async fn fetch_one<'e, E>(self, executor: E) -> Result<T, crate::Error>
    where
        E: Executor<'e, Database = Sqlite> + Send,
    {
        let (sql, args) = self.build_sql()?;
        sqlx::query_as_with::<_, T, _>(&sql, args)
            .fetch_one(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SqlExecutableNoReturn, SqlExecutableReturn, SqlQueryBuilder,
        delete_builder::DynamicDeleteBuilder, query_builder::DynamicQueryBuilder,
        update_builder::DynamicUpdateBuilder,
    };
    use sqlx::{
        Encode, Executor, Sqlite, Type,
        encode::IsNull,
        error::BoxDynError,
        sqlite::{SqliteArgumentValue, SqlitePoolOptions, SqliteTypeInfo},
    };

    struct FailingValue;

    impl<'q> Encode<'q, Sqlite> for FailingValue {
        fn encode_by_ref(
            &self,
            _buf: &mut Vec<SqliteArgumentValue<'q>>,
        ) -> Result<IsNull, BoxDynError> {
            Err("bind failed".into())
        }
    }

    impl Type<Sqlite> for FailingValue {
        fn type_info() -> SqliteTypeInfo {
            <String as Type<Sqlite>>::type_info()
        }
    }

    #[tokio::test]
    async fn query_builder_preserves_arg_order() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("in-memory sqlite");

        pool.execute("CREATE TABLE t (a TEXT, b TEXT)").await.expect("create table");
        pool.execute("INSERT INTO t (a, b) VALUES ('x', 'y')").await.expect("insert row");

        let row: (String, String) = DynamicQueryBuilder::new("SELECT a, b FROM t")
            .and_where_eq("a", "x")
            .and_where_eq("b", "y")
            .fetch_one(&pool)
            .await
            .expect("query row");

        assert_eq!(row, ("x".to_string(), "y".to_string()));
    }

    #[tokio::test]
    async fn builder_reports_bind_failure() {
        let err = DynamicQueryBuilder::new("SELECT 1")
            .and_where_eq("a", FailingValue)
            .build_sql()
            .expect_err("bind failure should surface");

        match err {
            crate::Error::Other(msg) if msg.contains("failed to bind sql arg") => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn update_builder_has_no_returning_by_default() {
        let (sql, _) = DynamicUpdateBuilder::new("demo")
            .set("name", "n")
            .and_where_eq("id", 1_i64)
            .build_sql()
            .expect("build sql");

        assert!(!sql.contains("RETURNING"));
    }

    #[test]
    fn delete_builder_has_no_returning_by_default() {
        let (sql, _) = DynamicDeleteBuilder::new("demo")
            .and_where_eq("id", 1_i64)
            .build_sql()
            .expect("build sql");

        assert!(!sql.contains("RETURNING"));
    }

    #[test]
    fn returning_is_explicit() {
        let (sql, _) = DynamicUpdateBuilder::new("demo")
            .set("name", "n")
            .and_where_eq("id", 1_i64)
            .returning("*")
            .build_sql()
            .expect("build sql");

        assert!(sql.ends_with(" RETURNING *"));
    }

    #[tokio::test]
    async fn update_execute_still_works_without_returning() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .expect("in-memory sqlite");

        pool.execute("CREATE TABLE demo (id INTEGER PRIMARY KEY, name TEXT)")
            .await
            .expect("create table");
        pool.execute("INSERT INTO demo (id, name) VALUES (1, 'old')").await.expect("insert row");

        DynamicUpdateBuilder::new("demo")
            .set("name", "new")
            .and_where_eq("id", 1_i64)
            .execute(&pool)
            .await
            .expect("update row");

        let row: (String,) = sqlx::query_as("SELECT name FROM demo WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("load row");
        assert_eq!(row.0, "new");
    }
}
