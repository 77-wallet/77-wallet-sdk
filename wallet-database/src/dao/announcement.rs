use crate::{
    entities::announcement::{AnnouncementEntity, CreateAnnouncementVo},
    pagination::Pagination,
};

impl AnnouncementEntity {
    pub async fn update_existing<'a, E>(
        exec: E,
        reqs: Vec<CreateAnnouncementVo>,
    ) -> Result<(), crate::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
    {
        if reqs.is_empty() {
            return Ok(());
        }

        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new("UPDATE announcement SET ");

        // 使用辅助函数生成 CASE 语句
        Self::append_case(&mut query_builder, "title", &reqs, |req| req.title.clone());
        Self::append_case(&mut query_builder, "content", &reqs, |req| {
            req.content.clone()
        });
        Self::append_case(&mut query_builder, "language", &reqs, |req| {
            req.language.clone()
        });

        // 更新更新时间字段
        query_builder.push("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') ");

        // 限制只更新存在的行
        query_builder.push("WHERE id IN (");
        for (i, req) in reqs.iter().enumerate() {
            if i > 0 {
                query_builder.push(", ");
            }
            query_builder.push_bind(&req.id);
        }
        query_builder.push(")");

        // 执行查询
        let query = query_builder.build();
        query
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    /// 辅助函数：构造字段的 CASE 语句
    fn append_case<'a, 'b, F, T>(
        query_builder: &'a mut sqlx::QueryBuilder<'b, sqlx::Sqlite>,
        field_name: &str,
        reqs: &'b [CreateAnnouncementVo],
        value_extractor: F,
    ) where
        F: Fn(&CreateAnnouncementVo) -> T,
        T: ToString,
    {
        query_builder.push(format!("{field_name} = CASE id "));
        for req in reqs {
            query_builder
                .push("WHEN ")
                .push_bind(&req.id)
                .push(" THEN ")
                .push_bind(value_extractor(req).to_string())
                .push(" ");
        }
        query_builder.push("END, ");
    }

    pub async fn upsert<'a, E>(exec: E, reqs: Vec<CreateAnnouncementVo>) -> Result<(), crate::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
    {
        if reqs.is_empty() {
            return Ok(());
        }

        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "INSERT INTO announcement (id, title, content, language, status, created_at, updated_at) ",
        );

        query_builder.push_values(reqs, |mut b, req| {
            let send_time = req
                .send_time
                .as_deref()
                .and_then(|s| {
                    sqlx::types::chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
                })
                .map(|dt| dt.and_utc().timestamp());

            b.push_bind(req.id)
                .push_bind(req.title)
                .push_bind(req.content)
                .push_bind(req.language)
                .push_bind(req.status);
            if let Some(send_time) = send_time {
                b.push_bind(send_time);
            } else {
                b.push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')");
            };
            b.push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')");
        });

        query_builder.push(
            " ON CONFLICT (id) DO UPDATE SET 
                    title = excluded.title,
                    content = excluded.content,
                    language = excluded.language,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        );

        let query = query_builder.build();

        query
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list<'a, E>(exec: E) -> Result<Vec<Self>, crate::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
    {
        let sql = "SELECT * FROM announcement ORDER BY created_at DESC";
        sqlx::query_as::<sqlx::Sqlite, Self>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_announcement_list<'a, E>(
        exec: &E,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<Self>, crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        use crate::pagination::Pagination;

        let sql = "SELECT * FROM announcement ORDER BY created_at DESC";
        let paginate = Pagination::<Self>::init(page, page_size);
        Ok(paginate.page(exec, sql).await?)
    }

    pub async fn get_announcement_by_id<'a, E>(
        exec: &E,
        id: &str,
    ) -> Result<Option<Self>, crate::Error>
    where
        for<'c> &'c E: sqlx::Executor<'c, Database = sqlx::Sqlite>,
    {
        let sql = "SELECT * FROM announcement WHERE id = ?;";
        sqlx::query_as::<sqlx::Sqlite, AnnouncementEntity>(sql)
            .bind(id)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_status<'a, E>(
        exec: E,
        id: Option<&str>,
        status: i32,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
    {
        // 基础的 SQL 语句
        let mut sql = "UPDATE announcement SET status = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')".to_string();

        // 如果提供了 id，添加 WHERE 子句
        if id.is_some() {
            sql.push_str(" WHERE id = ?");
        }

        // 添加 RETURNING 子句
        sql.push_str(" RETURNING *;");

        // 创建查询
        let mut query = sqlx::query_as::<_, AnnouncementEntity>(&sql).bind(status); // 绑定 status

        // 如果提供了 id，绑定 id
        if let Some(id_val) = id {
            query = query.bind(id_val);
        }

        // 执行查询并返回结果
        query
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn count_status_zero<'a, E>(exec: E) -> Result<i64, crate::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
    {
        let sql = "SELECT COUNT(*) FROM announcement WHERE status = 0;";
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn physical_delete<'a, E>(exec: E, id: &str) -> Result<(), crate::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
    {
        let sql = r#"
            DELETE FROM announcement
            WHERE id = $1
            RETURNING *
            "#;

        sqlx::query(sql)
            .bind(id)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
