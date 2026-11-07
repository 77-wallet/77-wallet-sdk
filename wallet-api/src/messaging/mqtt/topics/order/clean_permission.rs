// 清空权限事件
use crate::{context::Context, domain::permission::PermissionDomain};
use wallet_database::{CoreDbPool, repositories::permission::PermissionRepo};

// biz_type = CLEAN_PERMISSION
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CleanPermission {
    pub grantor_addr: String,
}

impl CleanPermission {
    fn name(&self) -> String {
        "CLEAN_PERMISSION".to_string()
    }

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
        let pool = ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());

        let event_name = self.name();
        tracing::info!(
            event_name = %event_name,
            ?self,
            "Clean Permission");

        // 删除权限
        PermissionRepo::delete_all(&core_pool, &self.grantor_addr).await?;

        // 更新队列数据
        PermissionDomain::queue_fail_and_upload(ctx, &pool, &self.grantor_addr).await?;
        Ok(())
    }
}
