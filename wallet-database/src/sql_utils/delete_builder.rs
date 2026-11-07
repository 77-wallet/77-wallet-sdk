use sqlx::{Arguments as _, Sqlite, sqlite::SqliteArguments};

pub struct DynamicDeleteBuilder<'a> {
    table: String,
    where_clauses: Vec<String>,
    returning: Option<String>,
    args: SqliteArguments<'a>,
    bind_error: Option<crate::Error>,
}

impl<'a> DynamicDeleteBuilder<'a> {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            where_clauses: Vec::new(),
            returning: None,
            args: SqliteArguments::default(),
            bind_error: None,
        }
    }

    pub fn and_where_eq<T>(mut self, field: &str, val: T) -> Self
    where
        T: 'a + Send + sqlx::Encode<'a, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
        self.where_clauses.push(format!("{} = ?", field));
        self.push_arg(val);
        self
    }

    pub fn and_where_like(mut self, field: &str, keyword: &str) -> Self {
        self.where_clauses.push(format!("{} LIKE ?", field));
        self.push_arg(format!("%{}%", keyword));
        self
    }

    pub fn and_where_in<T>(mut self, field: &str, values: &[T]) -> Self
    where
        T: ToString + Send,
    {
        if values.is_empty() {
            return self;
        }

        let placeholders = std::iter::repeat("?").take(values.len()).collect::<Vec<_>>().join(", ");
        self.where_clauses.push(format!("{} IN ({})", field, placeholders));

        for value in values {
            self.push_arg(value.to_string());
        }
        self
    }

    pub fn returning(mut self, clause: &str) -> Self {
        self.returning = Some(clause.to_string());
        self
    }

    fn push_arg<T>(&mut self, value: T)
    where
        T: 'a + Send + sqlx::Encode<'a, Sqlite> + sqlx::Type<Sqlite>,
    {
        if self.bind_error.is_some() {
            return;
        }

        if let Err(err) = self.args.add(value) {
            self.bind_error = Some(crate::Error::Other(format!("failed to bind sql arg: {err}")));
        }
    }
}

#[async_trait::async_trait]
impl<'a> super::SqlExecutableNoReturn<'a> for DynamicDeleteBuilder<'a> {}

#[async_trait::async_trait]
impl<'a, T> super::SqlExecutableReturn<'a, T> for DynamicDeleteBuilder<'a> where
    T: for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin + 'static
{
}

impl<'q> super::SqlQueryBuilder<'q> for DynamicDeleteBuilder<'q> {
    fn build_sql(self) -> Result<(String, SqliteArguments<'q>), crate::Error> {
        if let Some(err) = self.bind_error {
            return Err(err);
        }

        let mut sql = format!("DELETE FROM {}", self.table);
        if !self.where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.where_clauses.join(" AND "));
        }

        if let Some(returning) = self.returning {
            sql.push_str(" RETURNING ");
            sql.push_str(&returning);
        }

        Ok((sql, self.args))
    }
}
