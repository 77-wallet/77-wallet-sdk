use crate::{
    consts::endpoint::{
        api_wallet::{
            APP_ID_BIND, APP_ID_UNBIND, APPID_IMPORT_RECHARGE_WALLET, APPID_IMPORT_WALLET,
            APPID_UID_USAGE, APPID_WITHDRAWAL_WALLET_CHANGE, INIT_API_WALLET, QUERY_UID_BIND_INFO,
            QUERY_WALLET_ACTIVATION_CONFIG, SAVE_WALLET_ACTIVATION_CONFIG,
        },
        old_wallet::OLD_KEYS_UID_CHECK,
    },
    request::api_wallet::wallet::{
        AppIdImportRechargeWalletReq, AppIdImportReq, AppIdUidUsageReq, BindAppIdReq,
        InitApiWalletReq, SaveWalletActivationConfigReq, UnBindAppIdReq,
    },
    response::response::BackendResponse,
    response_vo::api_wallet::wallet::{
        AppIdUidUsageRes, KeysUidCheckRes, QueryUidBindInfoRes, QueryWalletActivationInfoResp,
    },
};
use std::collections::HashMap;
use wallet_ecdh::GLOBAL_KEY;

use crate::{
    Error::{ApiBackend, Backend},
    api::BackendApi,
    api_request::ApiBackendRequest,
};

impl BackendApi {
    /// Fetch the latest configuration; absent data is an error, not an empty list.
    pub async fn query_app_config(
        &self,
        req: &crate::request::api_wallet::wallet::AppConfigReq,
    ) -> Result<crate::response_vo::api_wallet::wallet::AppConfigResp, crate::Error> {
        if req.app_id.trim().is_empty() || req.org_id.trim().is_empty() {
            return Err(Backend(Some("appId and orgId are required".into())));
        }
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let request = ApiBackendRequest::new(req)?;
        self.post_api_backend(crate::consts::endpoint::api_wallet::APP_CONFIG, request)
            .await?
            .ok_or(ApiBackend(999, Some("appConfig returned no data".into())))
    }

    // uid类型检查
    pub async fn keys_uid_check(&self, uid: &str) -> Result<KeysUidCheckRes, crate::Error> {
        let res = self
            .client
            .post(OLD_KEYS_UID_CHECK)
            .json(serde_json::json!({
                "uid": uid
            }))
            .send::<BackendResponse>()
            .await?;

        res.process(&self.aes_cbc_cryptor)
    }

    /// 钱包与 appId 绑定
    pub async fn wallet_bind_appid(&self, req: &BindAppIdReq) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        self.post_api_backend::<_, ()>(APP_ID_BIND, api_req).await?;
        Ok(())
    }

    // 钱包与 appId 解绑
    pub async fn wallet_unbind_appid(&self, req: &UnBindAppIdReq) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        self.post_api_backend::<_, ()>(APP_ID_UNBIND, api_req).await?;
        Ok(())
    }

    /// 设置UID为API钱包
    pub async fn init_api_wallet(&self, req: InitApiWalletReq) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        self.post_api_backend::<_, ()>(INIT_API_WALLET, api_req).await?;
        Ok(())
    }

    /// 保存钱包激活配置
    pub async fn save_wallet_activation_config(
        &self,
        req: SaveWalletActivationConfigReq,
    ) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        self.post_api_backend::<_, ()>(SAVE_WALLET_ACTIVATION_CONFIG, api_req).await?;
        Ok(())
    }

    /// 查询钱包激活信息
    pub async fn query_wallet_activation_info(
        &self,
        uid: &str,
    ) -> Result<QueryWalletActivationInfoResp, crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let mut req = HashMap::new();
        req.insert("uid", uid);
        let api_req = ApiBackendRequest::new(req)?;
        let res = self
            .post_api_backend::<_, QueryWalletActivationInfoResp>(
                QUERY_WALLET_ACTIVATION_CONFIG,
                api_req,
            )
            .await?;
        res.ok_or(ApiBackend(999, Some("no found list".to_string())))
    }

    /// 查询uid 绑定信息
    pub async fn query_uid_bind_info(
        &self,
        uid: &str,
    ) -> Result<QueryUidBindInfoRes, crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let mut req = HashMap::new();
        req.insert("uid", uid);
        let api_req = ApiBackendRequest::new(req)?;
        let res =
            self.post_api_backend::<_, QueryUidBindInfoRes>(QUERY_UID_BIND_INFO, api_req).await?;
        res.ok_or(ApiBackend(999, Some("no found list".to_string())))
    }

    /// uid与appid的绑定
    pub async fn appid_withdrawal_wallet_change(
        &self,
        withdrawal_uid: &str,
        org_app_id: &str,
    ) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let mut req = HashMap::new();
        req.insert("withdrawalUid", withdrawal_uid);
        req.insert("orgAppId", org_app_id);
        let api_req = ApiBackendRequest::new(req)?;
        self.post_api_backend::<_, ()>(APPID_WITHDRAWAL_WALLET_CHANGE, api_req).await?;
        Ok(())
    }

    pub async fn appid_import(&self, req: AppIdImportReq) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        self.post_api_backend::<_, ()>(APPID_IMPORT_WALLET, api_req).await?;
        Ok(())
    }

    pub async fn appid_import_recharge_wallet(
        &self,
        req: AppIdImportRechargeWalletReq,
    ) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        self.post_api_backend::<_, ()>(APPID_IMPORT_RECHARGE_WALLET, api_req).await?;
        Ok(())
    }

    /// uid是否在appId下使用过
    pub async fn appid_uid_usage(
        &self,
        req: AppIdUidUsageReq,
    ) -> Result<AppIdUidUsageRes, crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        let res = self.post_api_backend::<_, AppIdUidUsageRes>(APPID_UID_USAGE, api_req).await?;
        res.ok_or(Backend(Some("no found list".to_string())))
    }

    // // 绑定子账户钱包
    // pub async fn appid_sub_account_bind(
    //     &self,
    //     sn: &str,
    //     recharge_uid: &str,
    //     org_app_id: &str,
    // ) -> Result<(), crate::Error> {
    //     let res = self
    //         .client
    //         .post(APPID_SUB_ACCOUNT_BIND)
    //         .json(serde_json::json!({
    //             "sn": sn,
    //             "rechargeUid": recharge_uid,
    //             "orgAppId": org_app_id
    //         }))
    //         .send::<BackendResponse>()
    //         .await?;

    //     res.process(&self.aes_cbc_cryptor)
    // }
}
