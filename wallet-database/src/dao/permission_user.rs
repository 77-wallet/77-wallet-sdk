use crate::entities::permission_user::PermissionUserEntity;
use sqlx::{Executor, QueryBuilder, Sqlite};

pub(crate) struct PermissionUserDao;

impl PermissionUserDao {
    // 批量新增成员
    pub async fn batch_add<'a, E>(
        users: &[PermissionUserEntity],
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO permission_user (address,grantor_addr, permission_id, is_self, weight,created_at) ",
        );

        query.push_values(users, |mut row, param| {
            row.push_bind(&param.address)
                .push_bind(&param.grantor_addr)
                .push_bind(&param.permission_id)
                .push_bind(param.is_self)
                .push_bind(param.weight)
                .push_bind(param.created_at);
        });

        query.build().execute(exec).await?;

        Ok(())
    }

    pub async fn find_by_permission<'a, E>(
        permission_id: &str,
        exec: E,
    ) -> Result<Vec<PermissionUserEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"select * from permission_user where permission_id = ?"#;

        let result = sqlx::query_as::<_, PermissionUserEntity>(sql)
            .bind(permission_id)
            .fetch_all(exec)
            .await?;

        Ok(result)
    }

    pub async fn self_users<'a, E>(
        permission_id: &str,
        exec: E,
    ) -> Result<Vec<PermissionUserEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"select * from permission_user where permission_id = ? and is_self = 1"#;

        let result = sqlx::query_as::<_, PermissionUserEntity>(sql)
            .bind(permission_id)
            .fetch_all(exec)
            .await?;

        Ok(result)
    }

    pub async fn delete_by_permission<'a, E>(
        permission_id: &str,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"delete from permission_user where permission_id = ?"#;

        sqlx::query(sql).bind(permission_id).execute(exec).await?;

        Ok(())
    }

    pub async fn delete_by_grantor_addr<'a, E>(
        grantor_addr: &str,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"delete from permission_user where grantor_addr = ?"#;

        sqlx::query(sql).bind(grantor_addr).execute(exec).await?;

        Ok(())
    }

    pub async fn update_self_mark<'a, E>(
        grantor_addr: &str,
        address: &str,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql =
            r#"update permission_user set is_self = 0 where grantor_addr = ? and address = ?"#;

        sqlx::query(sql).bind(grantor_addr).bind(address).execute(exec).await?;

        Ok(())
    }
}
