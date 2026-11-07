use sqlx::{Arguments as _, Sqlite, sqlite::SqliteArguments};

pub struct DynamicUpdateBuilder<'a> {
    table: String,
    set_clauses: Vec<String>,
    where_clauses: Vec<String>,
    returning: Option<String>,
    args: SqliteArguments<'a>,
    bind_error: Option<crate::Error>,
}

impl<'a> DynamicUpdateBuilder<'a> {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            set_clauses: vec![],
            where_clauses: vec![],
            returning: None,
            args: SqliteArguments::default(),
            bind_error: None,
        }
    }

    pub fn set<T>(mut self, field: &str, val: T) -> Self
    where
        T: 'a + Send + sqlx::Encode<'a, Sqlite> + sqlx::Type<Sqlite>,
    {
        self.set_clauses.push(format!("{} = ?", field));
        self.push_arg(val);
        self
    }

    // Restricted escape hatch: only pass static SQL fragments, never business input.
    pub fn set_raw(mut self, expr: &str) -> Self {
        self.set_clauses.push(expr.to_string());
        self
    }

    pub fn and_where_eq<T>(mut self, field: &str, val: T) -> Self
    where
        T: 'a + Send + sqlx::Encode<'a, Sqlite> + sqlx::Type<Sqlite>,
    {
        self.where_clauses.push(format!("{} = ?", field));
        self.push_arg(val);
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
impl<'a, T> super::SqlExecutableReturn<'a, T> for DynamicUpdateBuilder<'a> where
    T: for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> + Send + Unpin + 'static
{
}

#[async_trait::async_trait]
impl<'a> super::SqlExecutableNoReturn<'a> for DynamicUpdateBuilder<'a> {}

impl<'q> super::SqlQueryBuilder<'q> for DynamicUpdateBuilder<'q> {
    fn build_sql(self) -> Result<(String, SqliteArguments<'q>), crate::Error> {
        if let Some(err) = self.bind_error {
            return Err(err);
        }

        let mut sql = format!("UPDATE {} SET ", self.table);
        sql.push_str(&self.set_clauses.join(", "));

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
