use crate::error::service::ServiceError;
use async_trait::async_trait;
use std::sync::Arc;
use wallet_transport_backend::{
    api::BackendApi,
    request::{
        DeviceDeleteReq, KeysInitReq,
        api_wallet::{
            address::ExpandAddressCompleteReq,
            swap::{ApiInitSwapReq, ApiInitSwapResponse},
            wallet::{
                AppIdImportRechargeWalletReq, AppIdImportReq, AppIdUidUsageReq, BindAppIdReq,
            },
        },
    },
    response_vo::api_wallet::wallet::{
        AppIdUidUsageRes, KeysUidCheckRes, QueryUidBindInfoRes, QueryWalletActivationInfoResp,
    },
};

#[async_trait]
pub trait ApiWalletBackend: Send + Sync {
    async fn wallet_bind_appid(&self, req: BindAppIdReq) -> Result<(), ServiceError>;
    async fn init_api_wallet(&self, req: AppIdImportReq) -> Result<(), ServiceError>;
    async fn old_keys_init(&self, req: KeysInitReq) -> Result<(), ServiceError>;
    async fn appid_import(&self, req: AppIdImportReq) -> Result<(), ServiceError>;
    async fn appid_import_recharge_wallet(
        &self,
        req: AppIdImportRechargeWalletReq,
    ) -> Result<(), ServiceError>;
    async fn keys_uid_check(&self, uid: &str) -> Result<KeysUidCheckRes, ServiceError>;
    async fn query_uid_bind_info(&self, uid: &str) -> Result<QueryUidBindInfoRes, ServiceError>;
    async fn query_wallet_activation_info(
        &self,
        uid: &str,
    ) -> Result<QueryWalletActivationInfoResp, ServiceError>;
    async fn appid_uid_usage(
        &self,
        req: AppIdUidUsageReq,
    ) -> Result<AppIdUidUsageRes, ServiceError>;
    async fn expand_address_complete(
        &self,
        req: ExpandAddressCompleteReq,
    ) -> Result<(), ServiceError>;
    async fn appid_withdrawal_wallet_change(
        &self,
        withdrawal_uid: &str,
        org_app_id: &str,
    ) -> Result<(), ServiceError>;
    async fn init_swap(&self, req: &ApiInitSwapReq) -> Result<ApiInitSwapResponse, ServiceError>;
    async fn device_delete(&self, req: &DeviceDeleteReq) -> Result<Option<()>, ServiceError>;
}

pub struct RealApiWalletBackend {
    inner: Arc<BackendApi>,
}

impl RealApiWalletBackend {
    pub fn new(inner: Arc<BackendApi>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl ApiWalletBackend for RealApiWalletBackend {
    async fn wallet_bind_appid(&self, req: BindAppIdReq) -> Result<(), ServiceError> {
        self.inner.wallet_bind_appid(&req).await.map_err(|e| e.into())
    }

    async fn init_api_wallet(&self, req: AppIdImportReq) -> Result<(), ServiceError> {
        self.inner.init_api_wallet(req).await.map_err(|e| e.into())
    }

    async fn old_keys_init(&self, req: KeysInitReq) -> Result<(), ServiceError> {
        self.inner.old_keys_init(&req).await.map_err(ServiceError::from)?;
        Ok(())
    }

    async fn appid_import(&self, req: AppIdImportReq) -> Result<(), ServiceError> {
        self.inner.appid_import(req).await.map_err(|e| e.into())
    }

    async fn appid_import_recharge_wallet(
        &self,
        req: AppIdImportRechargeWalletReq,
    ) -> Result<(), ServiceError> {
        self.inner.appid_import_recharge_wallet(req).await.map_err(|e| e.into())
    }

    async fn keys_uid_check(&self, uid: &str) -> Result<KeysUidCheckRes, ServiceError> {
        self.inner.keys_uid_check(uid).await.map_err(|e| e.into())
    }

    async fn query_uid_bind_info(&self, uid: &str) -> Result<QueryUidBindInfoRes, ServiceError> {
        self.inner.query_uid_bind_info(uid).await.map_err(|e| e.into())
    }

    async fn query_wallet_activation_info(
        &self,
        uid: &str,
    ) -> Result<QueryWalletActivationInfoResp, ServiceError> {
        self.inner.query_wallet_activation_info(uid).await.map_err(|e| e.into())
    }

    async fn appid_uid_usage(
        &self,
        req: AppIdUidUsageReq,
    ) -> Result<AppIdUidUsageRes, ServiceError> {
        self.inner.appid_uid_usage(req).await.map_err(|e| e.into())
    }

    async fn expand_address_complete(
        &self,
        req: ExpandAddressCompleteReq,
    ) -> Result<(), ServiceError> {
        self.inner.expand_address_complete(req).await.map_err(|e| e.into())
    }

    async fn appid_withdrawal_wallet_change(
        &self,
        withdrawal_uid: &str,
        org_app_id: &str,
    ) -> Result<(), ServiceError> {
        self.inner
            .appid_withdrawal_wallet_change(withdrawal_uid, org_app_id)
            .await
            .map_err(|e| e.into())
    }

    async fn init_swap(&self, req: &ApiInitSwapReq) -> Result<ApiInitSwapResponse, ServiceError> {
        self.inner.init_swap(req).await.map_err(|e| e.into())
    }

    async fn device_delete(&self, req: &DeviceDeleteReq) -> Result<Option<()>, ServiceError> {
        self.inner.device_delete(req).await.map_err(|e| e.into())
    }
}
