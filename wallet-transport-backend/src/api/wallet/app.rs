use crate::{consts::endpoint::CLIENT_TASK_LOG_UPLOAD, request::LanguageInitReq};

use crate::api::BackendApi;

impl BackendApi {
    pub async fn app_install_save(
        &self,
        req: crate::request::AppInstallSaveReq,
    ) -> Result<serde_json::Value, crate::Error> {
        self.post_backend_json("/app/install/save", serde_json::json!(req)).await
    }

    pub async fn app_install_download(&self) -> Result<String, crate::Error> {
        self.post_backend_empty("/app/install/download").await
    }

    pub async fn mqtt_init(&self) -> Result<String, crate::Error> {
        self.post_backend_empty("mqtt/init").await
    }

    pub async fn rpc_token(&self, client_id: &str) -> Result<String, crate::Error> {
        self.post_backend_json("app/rpc/token", serde_json::json!({"clientId":client_id})).await
    }

    pub async fn version_view(
        &self,

        req: crate::request::VersionViewReq,
    ) -> Result<crate::response_vo::app::AppVersionRes, crate::Error> {
        self.post_backend_json("version/view", serde_json::json!(req)).await
    }

    pub async fn version_download_url(
        &self,

        url: &str,
    ) -> Result<crate::response_vo::app::AppVersionRes, crate::Error> {
        self.get_backend(&format!("version/download/{url}")).await
    }

    pub async fn language_init(&self, req: LanguageInitReq) -> Result<(), crate::Error> {
        self.post_backend_json("/language/init", serde_json::json!(req)).await
    }

    pub async fn set_invite_code(
        &self,

        req: crate::request::SetInviteeStatusReq,
    ) -> Result<(), crate::Error> {
        self.post_backend_json("/device/editDeviceInviteeStatus", serde_json::json!(req)).await
    }

    pub async fn client_task_log_upload(
        &self,

        req: crate::request::ClientTaskLogUploadReq,
    ) -> Result<(), crate::Error> {
        self.post_backend_json(CLIENT_TASK_LOG_UPLOAD, serde_json::json!(req)).await
    }
}
