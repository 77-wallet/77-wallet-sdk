use crate::request::PermissionData;

use crate::api::BackendApi;

// 权限变更请求参数
#[derive(serde::Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionAcceptReq {
    pub hash: String,
    pub grantor_addr: String,
    pub sender_user: Vec<String>,
    pub back_user: Vec<String>,
    pub current: CurrentPermission,
    pub multi_sign_id: String,
}

#[derive(serde::Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CurrentPermission {
    // 上次的成员
    pub original_user: Vec<String>,
    // 修改后的成员
    pub new_user: Vec<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub types: String,
    pub active_id: i64,
    pub operations: String,
}

#[derive(serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetPermissionBackReq {
    pub address: Option<String>,
    pub uid: Option<String>,
}

#[derive(serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PermissionCleanReq {
    pub owner: String,
    pub users: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PermissionBackupResp {
    pub list: Vec<String>,
}

// 单签交易执行完成后需要同步到服务端
#[derive(serde::Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TransPermission {
    pub address: String,
    pub chain_code: String,
    pub tx_kind: i8,
    pub hash: String,
    pub permission_data: PermissionData,
}

impl BackendApi {
    pub async fn permission_accept(&self, req: PermissionAcceptReq) -> Result<(), crate::Error> {
        let endpoint = "permission/change";

        let _result = self.post_request::<_, bool>(endpoint, &req).await;

        Ok(())
    }

    pub async fn get_permission_backup(
        &self,
        req: GetPermissionBackReq,
    ) -> Result<PermissionBackupResp, crate::Error> {
        let endpoint = "permission/getBackUpData";

        let result = self.post_request::<_, PermissionBackupResp>(endpoint, &req).await?;

        Ok(result)
    }

    pub async fn permission_clean(
        &self,
        owner: &str,
        users: Vec<String>,
    ) -> Result<(), crate::Error> {
        let req = PermissionCleanReq { owner: owner.to_string(), users };

        let endpoint = "permission/activePermission/clean";

        let _result = self.post_request::<_, Option<bool>>(endpoint, &req).await;

        Ok(())
    }

    pub async fn trans_upload(&self, req: TransPermission) -> Result<(), crate::Error> {
        let endpoint = "permission/uploadTrans";

        let _result = self.post_request::<_, bool>(endpoint, &req).await;

        Ok(())
    }
}
