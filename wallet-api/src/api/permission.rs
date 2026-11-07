use super::ReturnType;
use crate::{
    manager::WalletManager,
    request::permission::PermissionReq,
    response_vo::{
        EstimateFeeResp,
        standard_wallet::permission::{AccountPermission, ManagerPermissionResp, PermissionList},
    },
    service::permission::PermissionService,
};

impl WalletManager {
    // all permission list
    pub fn permission_list(&self) -> ReturnType<PermissionList> {
        PermissionService::permission_list()
    }

    // account permission
    pub async fn account_permission(
        &self,
        address: String,
    ) -> ReturnType<Option<AccountPermission>> {
        PermissionService::new(self.ctx).await?.account_permission(address).await
    }

    // 管理其账号的权限
    pub async fn manager_permission(
        &self,
        grantor_addr: String,
    ) -> ReturnType<Vec<ManagerPermissionResp>> {
        PermissionService::new(self.ctx).await?.manager_permission(grantor_addr).await
    }

    pub async fn modify_permission_fee(
        &self,
        req: PermissionReq,
        types: String,
    ) -> ReturnType<EstimateFeeResp> {
        PermissionService::new(self.ctx).await?.modify_permission_fee(req, types).await
    }

    pub async fn modify_permission(
        &self,
        req: PermissionReq,
        types: String,
        password: String,
    ) -> ReturnType<String> {
        PermissionService::new(self.ctx).await?.modify_permission(req, types, password).await
    }

    pub async fn build_multisig_queue(
        &self,
        req: PermissionReq,
        types: String,
        password: String,
        expiration: i64,
    ) -> ReturnType<String> {
        PermissionService::new(self.ctx)
            .await?
            .build_multisig_permission(req, types, expiration, password)
            .await
    }
}
