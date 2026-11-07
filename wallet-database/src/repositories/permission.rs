use crate::{
    CoreDbPool,
    dao::{permission::PermissionDao, permission_user::PermissionUserDao},
    entities::{
        permission::{PermissionEntity, PermissionWithUserEntity},
        permission_user::PermissionUserEntity,
    },
};

pub struct PermissionRepo;

impl PermissionRepo {
    pub fn get_id(grantor_addr: &str, active_id: i64) -> String {
        PermissionDao::get_id(grantor_addr, active_id)
    }

    // 新增权限以及成员
    pub async fn add_with_user(
        pool: &CoreDbPool,
        permission: &PermissionEntity,
        users: &[PermissionUserEntity],
    ) -> Result<(), crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        PermissionDao::add(permission, tx.as_mut()).await?;

        PermissionUserDao::batch_add(users, tx.as_mut()).await?;

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        Ok(())
    }

    pub async fn del_add(
        pool: &CoreDbPool,
        permissions: &[PermissionEntity],
        users: &[PermissionUserEntity],
        grantor_addr: &str,
    ) -> Result<(), crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        // 删除原来的权限
        PermissionDao::delete_by_grantor_addr(grantor_addr, tx.as_mut()).await?;
        // 删除成员
        PermissionUserDao::delete_by_grantor_addr(grantor_addr, tx.as_mut()).await?;

        // 新增权限
        for permission in permissions {
            PermissionDao::add(permission, tx.as_mut()).await?;
        }

        //  新增成员
        PermissionUserDao::batch_add(users, tx.as_mut()).await?;

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        Ok(())
    }

    // 新增权限以及成员
    pub async fn update_with_user(
        pool: &CoreDbPool,
        permission: &PermissionEntity,
        users: &[PermissionUserEntity],
    ) -> Result<(), crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        // 修改原permission
        PermissionDao::update(permission, tx.as_mut()).await?;

        // 删除原来的成员
        PermissionUserDao::delete_by_permission(&permission.id, tx.as_mut()).await?;

        // 批量新增
        PermissionUserDao::batch_add(users, tx.as_mut()).await?;

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        Ok(())
    }

    pub async fn update_permission(
        pool: &CoreDbPool,
        permission: &PermissionEntity,
    ) -> Result<(), crate::Error> {
        Ok(PermissionDao::update(permission, pool.write_ref()).await?)
    }

    pub async fn update_self_mark(
        pool: &CoreDbPool,
        grantor_addr: &str,
        address: &str,
    ) -> Result<(), crate::Error> {
        Ok(PermissionUserDao::update_self_mark(grantor_addr, address, pool.write_ref()).await?)
    }

    pub async fn permission_with_user(
        pool: &CoreDbPool,
        grantor_addr: &str,
        active_id: i64,
        include_del: bool,
    ) -> Result<Option<PermissionWithUserEntity>, crate::Error> {
        let permission = PermissionDao::find_by_grantor_active(
            grantor_addr,
            active_id,
            include_del,
            pool.read_ref(),
        )
        .await?;

        if let Some(permission) = permission {
            let user =
                PermissionUserDao::find_by_permission(&permission.id, pool.read_ref()).await?;

            Ok(Some(PermissionWithUserEntity { permission, user }))
        } else {
            Ok(None)
        }
    }

    // 所有的权限
    pub async fn all_permission_with_user(
        pool: &CoreDbPool,
        user_addr: &str,
    ) -> Result<Vec<PermissionWithUserEntity>, crate::Error> {
        let permissions = PermissionDao::all_permission(pool.read_ref(), user_addr).await?;

        let mut result = vec![];
        for permission in permissions {
            let users =
                PermissionUserDao::find_by_permission(&permission.id, pool.read_ref()).await?;

            result.push(PermissionWithUserEntity { permission, user: users });
        }
        Ok(result)
    }

    pub async fn find_by_grantor_and_active(
        pool: &CoreDbPool,
        grantor_addr: &str,
        active_id: i64,
        include_del: bool,
    ) -> Result<Option<PermissionEntity>, crate::Error> {
        let res = PermissionDao::find_by_grantor_active(
            grantor_addr,
            active_id,
            include_del,
            pool.read_ref(),
        )
        .await?;
        Ok(res)
    }

    // delete permission and user
    pub async fn delete_all(pool: &CoreDbPool, grantor_addr: &str) -> Result<(), crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        // delete permission
        PermissionDao::delete_by_grantor_addr(grantor_addr, tx.as_mut()).await?;

        // delete all users
        PermissionUserDao::delete_by_grantor_addr(grantor_addr, tx.as_mut()).await?;

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        Ok(())
    }

    pub async fn delete_all_by_id(pool: &CoreDbPool, id: &str) -> Result<(), crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        // delete permission
        PermissionDao::delete_by_id(id, tx.as_mut()).await?;

        // delete all users
        PermissionUserDao::delete_by_permission(id, tx.as_mut()).await?;

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        Ok(())
    }

    // 删除成员以及权限
    pub async fn delete_one(pool: &CoreDbPool, id: &str) -> Result<(), crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        // 删除原来的权限
        PermissionDao::delete_by_id(id, tx.as_mut()).await?;
        // 删除成员
        PermissionUserDao::delete_by_permission(id, tx.as_mut()).await?;

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        Ok(())
    }

    pub async fn find_by_id(pool: &CoreDbPool, id: &str) -> Result<PermissionEntity, crate::Error> {
        let rs = PermissionDao::find_by_id(id, false, pool.read_ref())
            .await?
            .ok_or(crate::DatabaseError::ReturningNone)?;
        Ok(rs)
    }

    pub async fn find_option(
        pool: &CoreDbPool,
        id: &str,
    ) -> Result<Option<PermissionEntity>, crate::Error> {
        Ok(PermissionDao::find_by_id(id, false, pool.read_ref()).await?)
    }

    pub async fn self_user(
        pool: &CoreDbPool,
        permission_id: &str,
    ) -> Result<Vec<PermissionUserEntity>, crate::Error> {
        Ok(PermissionUserDao::self_users(permission_id, pool.read_ref()).await?)
    }

    pub async fn permission_by_users(
        pool: &CoreDbPool,
        users: &Vec<String>,
    ) -> Result<Vec<PermissionEntity>, crate::Error> {
        Ok(PermissionDao::permission_by_uses(pool.read_ref(), users).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::PermissionRepo;
    use crate::{
        dao::{permission::PermissionDao, permission_user::PermissionUserDao},
        entities::{permission::PermissionEntity, permission_user::PermissionUserEntity},
        repositories::test_helper::setup_core_pool,
    };
    use chrono::Utc;

    fn build_permission(grantor_addr: &str, active_id: i64) -> PermissionEntity {
        PermissionEntity {
            id: PermissionRepo::get_id(grantor_addr, active_id),
            name: "perm_name".to_string(),
            grantor_addr: grantor_addr.to_string(),
            types: "active".to_string(),
            active_id,
            threshold: 1,
            member: 1,
            chain_code: "tron".to_string(),
            operations: "ops".to_string(),
            is_del: 0,
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    fn build_user(grantor_addr: &str, permission_id: &str, address: &str) -> PermissionUserEntity {
        PermissionUserEntity {
            id: None,
            address: address.to_string(),
            grantor_addr: grantor_addr.to_string(),
            permission_id: permission_id.to_string(),
            is_self: 1,
            weight: 1,
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    #[tokio::test]
    async fn permission_repo_add_with_user_and_query_success() {
        let pool = setup_core_pool("wallet_db_permission_repo_success").await;
        let permission = build_permission("T_grantor_p1", 1);
        let users = vec![build_user("T_grantor_p1", &permission.id, "T_user_p1")];

        PermissionRepo::add_with_user(&pool, &permission, &users).await.unwrap();

        let found =
            PermissionRepo::permission_with_user(&pool, "T_grantor_p1", 1, false).await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.permission.id, permission.id);
        assert_eq!(found.user.len(), 1);
        assert_eq!(found.user[0].address, "T_user_p1");
    }

    #[tokio::test]
    async fn permission_repo_missing_permission_returns_none() {
        let pool = setup_core_pool("wallet_db_permission_repo_edge").await;
        let found = PermissionRepo::find_by_grantor_and_active(&pool, "T_missing", 99, false)
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn permission_repo_tx_rollback_keeps_permission_absent() {
        let pool = setup_core_pool("wallet_db_permission_repo_rollback").await;
        let permission = build_permission("T_grantor_rb", 7);
        let users = vec![build_user("T_grantor_rb", &permission.id, "T_user_rb")];

        let mut tx = pool.write_ref().begin().await.unwrap();
        PermissionDao::add(&permission, tx.as_mut()).await.unwrap();
        PermissionUserDao::batch_add(&users, tx.as_mut()).await.unwrap();
        tx.rollback().await.unwrap();

        let found =
            PermissionRepo::permission_with_user(&pool, "T_grantor_rb", 7, false).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn permission_repo_treats_apostrophes_in_user_fields_as_data() {
        let pool = setup_core_pool("wallet_db_permission_repo_apostrophe").await;
        let permission = build_permission("T_grantor_quote", 8);
        let quoted_address = "T_user_'_quote";
        let users = vec![build_user("T_grantor_'_quote", &permission.id, quoted_address)];

        PermissionRepo::add_with_user(&pool, &permission, &users).await.unwrap();

        let found =
            PermissionUserDao::find_by_permission(&permission.id, pool.read_ref()).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].address, quoted_address);
        assert_eq!(found[0].grantor_addr, "T_grantor_'_quote");
    }
}
