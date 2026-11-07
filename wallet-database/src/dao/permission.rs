use crate::entities::permission::PermissionEntity;
use chrono::SecondsFormat;
use sqlx::{Executor, Sqlite};

pub(crate) struct PermissionDao;

impl PermissionDao {
    pub fn get_id(grantor_addr: &str, active_id: i64) -> String {
        let id = active_id.to_string();
        wallet_utils::snowflake::gen_hash_uid(vec![grantor_addr, &id])
    }

    pub async fn add<'a, E>(
        permission: &PermissionEntity,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            INSERT INTO permission
            (id, name,grantor_addr, types, active_id, threshold, member, chain_code, operations, is_del, created_at)
                VALUES
            (?,?, ?, ?, ?, ?, ?, ?, ?, ?,?)"#;

        sqlx::query(sql)
            .bind(&permission.id)
            .bind(&permission.name)
            .bind(&permission.grantor_addr)
            .bind(&permission.types)
            .bind(permission.active_id)
            .bind(permission.threshold)
            .bind(permission.member)
            .bind(&permission.chain_code)
            .bind(&permission.operations)
            .bind(permission.is_del)
            .bind(permission.created_at.to_rfc3339_opts(SecondsFormat::Secs, true))
            .execute(exec)
            .await?;

        Ok(())
    }

    pub async fn update<'a, E>(
        permission: &PermissionEntity,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        UPDATE permission
        SET 
            name = ?,
            threshold = ?, 
            member = ?, 
            chain_code = ?, 
            operations = ?, 
            is_del = ?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE grantor_addr = ? and active_id = ?
    "#;
        tracing::warn!("{:#?}", permission);

        sqlx::query(sql)
            .bind(&permission.name)
            .bind(permission.threshold)
            .bind(permission.member)
            .bind(&permission.chain_code)
            .bind(&permission.operations)
            .bind(permission.is_del)
            .bind(&permission.grantor_addr)
            .bind(permission.active_id)
            .execute(exec)
            .await?;

        Ok(())
    }

    pub async fn all_permission<'a, E>(
        exec: E,
        user_addr: &str,
    ) -> Result<Vec<PermissionEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"SELECT * FROM permission p WHERE EXISTS 
        (
            SELECT 1 FROM permission_user u WHERE u.permission_id = p.id AND u.address = ? 
        ) and p.is_del = 0;"#;

        let result =
            sqlx::query_as::<_, PermissionEntity>(sql).bind(user_addr).fetch_all(exec).await?;

        Ok(result)
    }

    pub async fn permission_by_uses<'a, E>(
        exec: E,
        users: &Vec<String>,
    ) -> Result<Vec<PermissionEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        // 如果没有用户，直接返回空向量
        if users.is_empty() {
            return Ok(Vec::new());
        }

        // 根据用户数量构造占位符,
        let placeholders = users.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT * FROM permission p WHERE EXISTS (
            SELECT 1 FROM permission_user u 
            WHERE u.permission_id = p.id 
              AND u.address in ({})
        ) AND p.is_del = 0;",
            placeholders
        );

        // 创建查询并依次绑定每个用户地址
        let mut query = sqlx::query_as::<_, PermissionEntity>(&sql);
        for user in users {
            query = query.bind(user.clone());
        }

        let result = query.fetch_all(exec).await?;
        Ok(result)
    }

    // 1 包含删除 0包含
    pub async fn find_by_grantor_active<'a, E>(
        grantor_addr: &str,
        active_id: i64,
        include_del: bool,
        exec: E,
    ) -> Result<Option<PermissionEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = if include_del {
            r#"select * from permission where grantor_addr = ? and active_id = ?"#
        } else {
            r#"select * from permission where grantor_addr = ? and active_id = ? and is_del = 0"#
        };

        let result = sqlx::query_as::<_, PermissionEntity>(sql)
            .bind(grantor_addr)
            .bind(active_id)
            .bind(include_del)
            .fetch_optional(exec)
            .await?;

        Ok(result)
    }

    pub async fn find_by_id<'a, E>(
        id: &str,
        include_del: bool,
        exec: E,
    ) -> Result<Option<PermissionEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = if include_del {
            r#"select * from permission where id = ?"#
        } else {
            r#"select * from permission where id = ? and is_del = 0"#
        };

        let result = sqlx::query_as::<_, PermissionEntity>(sql)
            .bind(id)
            .bind(include_del)
            .fetch_optional(exec)
            .await?;

        Ok(result)
    }

    pub async fn delete_by_grantor_addr<'a, E>(
        grantor_addr: &str,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"delete from permission where grantor_addr = ?"#;

        sqlx::query(sql).bind(grantor_addr).execute(exec).await?;

        Ok(())
    }

    pub async fn delete_by_id<'a, E>(id: &str, exec: E) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"delete from permission where id = ?"#;

        sqlx::query(sql).bind(id).execute(exec).await?;

        Ok(())
    }
}
