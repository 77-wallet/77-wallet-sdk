use crate::{
    context::Context,
    domain::{chain::adapter::ChainAdapterFactory, permission::PermissionDomain},
    messaging::notify::{
        FrontendNotifyEvent, event::NotifyEvent, permission::PermissionChangeFrontend,
    },
    request::permission::PermissionReq,
};
use wallet_chain_interact::tron::{
    operations::{multisig::Permission, permissions::PermissionTypes},
    protocol::account::TronAccount,
};
use wallet_database::{
    CoreDbPool, DbPool,
    entities::{
        permission::{PermissionEntity, PermissionWithUserEntity},
        permission_user::PermissionUserEntity,
    },
    repositories::permission::PermissionRepo,
};
use wallet_types::constant::chain_code;

// biz_type = PERMISSION_ACCEPT
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PermissionAccept {
    pub grantor_addr: String,
    // 当前操作权限
    pub current: CurrentOp,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CurrentOp {
    // 上次的成员
    original_user: Vec<String>,
    // 修改后的成员
    new_user: Vec<String>,
    name: String,
    #[serde(rename = "type")]
    types: String,
    active_id: i64,
    operations: String,
}

// impl PermissionAccept {
//     pub fn to_json_val(&self) -> Result<serde_json::Value, crate::ServiceError> {
//         Ok(serde_func::serde_to_value(self)?)
//     }
// }

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct NewPermissionUser {
    pub permission: PermissionEntity,
    pub users: Vec<PermissionUserEntity>,
}

// 权限转换为数据库对应的实体
impl TryFrom<(&Permission, &str)> for NewPermissionUser {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: (&Permission, &str)) -> Result<Self, Self::Error> {
        let permission = value.0;

        let id = PermissionRepo::get_id(value.1, permission.id.unwrap_or_default() as i64);
        let time = wallet_utils::time::now();

        let p = PermissionEntity {
            id,
            name: permission.permission_name.clone(),
            grantor_addr: value.1.to_string(),
            types: "active".to_string(),
            active_id: permission.id.unwrap_or_default() as i64,
            threshold: permission.threshold as i64,
            member: permission.keys.len() as i64,
            chain_code: chain_code::TRON.to_string(),
            operations: permission.operations.clone().unwrap_or_default().clone(),
            is_del: 0,
            created_at: time,
            updated_at: None,
        };

        let mut users = vec![];

        for key in permission.keys.iter() {
            let user = PermissionUserEntity {
                id: None,
                address: key.address.clone(),
                grantor_addr: value.1.to_string(),
                permission_id: p.id.clone(),
                is_self: 0,
                weight: key.weight as i64,
                created_at: time,
                updated_at: None,
            };
            users.push(user);
        }

        Ok(NewPermissionUser { permission: p, users })
    }
}

impl PermissionAccept {
    pub async fn exec(
        &self,
        _msg_id: &str,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.exec_with_ctx(_msg_id, ctx).await
    }

    pub(crate) async fn exec_with_ctx(
        &self,
        _msg_id: &str,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(ctx).await?;
        let _ = _msg_id;
        let pool = ctx.get_global_sqlite_pool()?;
        let account = chain.account_info(&self.grantor_addr).await?;

        // 判断当前的事件是否是删除(删除需要同步所有的权限数据)
        if self.current.types == PermissionReq::DELETE {
            self.recover_all_old_permission(pool.clone(), &account, ctx).await?;

            PermissionDomain::queue_fail_and_upload(ctx, &pool, &self.grantor_addr).await?;
        } else {
            let address = &self.grantor_addr;
            let permissions =
                PermissionDomain::find_permission(&account, self.current.active_id, address)
                    .await?;

            self.upsert(pool.clone(), permissions, ctx).await?
        }

        Ok(())
    }

    // 删除事件、全部更新本地的权限
    async fn recover_all_old_permission(
        &self,
        pool: DbPool,
        account: &TronAccount,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = CoreDbPool::new(pool.clone());
        // 查询原来的数据并发送一个通知
        let old_permission = PermissionRepo::find_by_grantor_and_active(
            &core_pool,
            &self.grantor_addr,
            self.current.active_id,
            true,
        )
        .await?;

        // 权限是否包含自己
        let new_permission =
            PermissionDomain::self_contain_permission(&pool, account, &self.grantor_addr).await?;

        if !new_permission.is_empty() {
            // 删除原来的,新增
            PermissionDomain::del_add_update(&pool, new_permission, &self.grantor_addr).await?;
        } else {
            // 删除原来所有的权限;
            PermissionRepo::delete_all(&core_pool, &self.grantor_addr).await?;
        }

        // 系统通知发送
        if let Some(permission) = old_permission {
            Self::frontend_event(&permission, PermissionReq::DELETE, ctx).await?;
        };

        Ok(())
    }

    // 新增或者更新权限
    async fn upsert(
        &self,
        pool: DbPool,
        permissions: NewPermissionUser,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = CoreDbPool::new(pool.clone());
        // 查询出原来的权限
        let old_permission = PermissionRepo::permission_with_user(
            &core_pool,
            &permissions.permission.grantor_addr,
            permissions.permission.active_id,
            true,
        )
        .await?;

        // 存在走更新流程
        if let Some(old_permission) = old_permission {
            self.update(ctx, &pool, &permissions, old_permission).await?;

            // 消息重复通知,避免新增判断为
            if self.current.types == PermissionReq::UPDATE {
                Self::frontend_event(&permissions.permission, PermissionReq::UPDATE, ctx).await?;
            }
            Ok(())
        } else {
            let mut users = permissions.users.clone();

            PermissionDomain::mark_user_is_self(&pool, &mut users).await?;
            if users.iter().any(|u| u.is_self == 1) {
                PermissionRepo::add_with_user(&core_pool, &permissions.permission, &users).await?;

                Self::frontend_event(&permissions.permission, PermissionReq::NEW, ctx).await?;

                return Ok(());
            }

            Ok(())
        }
    }

    async fn update(
        &self,
        ctx: &'static Context,
        pool: &DbPool,
        permissions: &NewPermissionUser,
        old_permission: PermissionWithUserEntity,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = CoreDbPool::new(pool.clone());
        // 是否成员发生了变化
        if old_permission.user_has_changed(&permissions.users) {
            self.update_user_change(pool.clone(), permissions, &old_permission.permission.id)
                .await?;
        } else {
            PermissionRepo::update_permission(&core_pool, &permissions.permission).await?;
        }

        PermissionDomain::queue_fail_and_upload(ctx, pool, &self.grantor_addr).await?;
        Ok(())
    }

    async fn update_user_change(
        &self,
        pool: DbPool,
        permissions: &NewPermissionUser,
        id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = CoreDbPool::new(pool.clone());
        let mut users = permissions.users.clone();
        PermissionDomain::mark_user_is_self(&pool, &mut users).await?;

        // 成员变化是否把自己移除了(没有is_self = 1的数据)
        if users.iter().any(|u| u.is_self == 1) {
            tracing::warn!("change user ");
            PermissionRepo::update_with_user(&core_pool, &permissions.permission, &users).await?;
        } else {
            tracing::warn!(" not self delete ");
            PermissionRepo::delete_one(&core_pool, id).await?;
        }

        Ok(())
    }

    // system notify and fronted event
    async fn frontend_event(
        permission: &PermissionEntity,
        types: &str,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        // 1. system notify
        // let repo = RepositoryFactory::repo(pool.clone());
        // let system_notification_service = SystemNotificationService::new(repo);

        // if types != PermissionReq::DELETE {
        //     let notification =
        //         Notification::PermissionChange(PermissionChange::try_from((permission, types))?);
        //     let id = wallet_utils::snowflake::get_uid()?.to_string();
        //     system_notification_service
        //         .add_system_notification(&id, notification, 0)
        //         .await?;
        // }

        // 2. event
        let operations = PermissionTypes::from_hex(&permission.operations)?;
        let event = NotifyEvent::PermissionChanger(PermissionChangeFrontend::new(
            &permission.grantor_addr,
            types,
            operations,
        ));
        FrontendNotifyEvent::new(event).send_with_ctx(ctx).await?;

        Ok(())
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use crate::{messaging::mqtt::topics::PermissionAccept, testkit::env::get_manager};

    #[tokio::test]
    async fn new_permission() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        let (manager, _) = get_manager().await?;

        let str = r#"{"grantorAddr":"TUe3T6ErJvnoHMQwVrqK246MWeuCEBbyuR","current":{"originalUser":["TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1"],"newUser":[],"name":"修改权限","type":"delete","activeId":0,"operations":""}}"#;

        let change = serde_json::from_str::<PermissionAccept>(&str).unwrap();

        let res = change.exec("1", manager.ctx).await;
        println!("{:?}", res);
        Ok(())
    }
}
