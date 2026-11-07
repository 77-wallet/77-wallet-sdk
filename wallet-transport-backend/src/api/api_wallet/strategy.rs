use crate::{
    Error::{ApiBackend, Backend},
    api::BackendApi,
    api_request::ApiBackendRequest,
    consts::endpoint::api_wallet::{
        API_WALLET_CONFIG, TRANS_STRATEGY_COLLECT_SAVE, TRANS_STRATEGY_GET_COLLECT_CONFIG,
        TRANS_STRATEGY_GET_WITHDRAWAL_CONFIG, TRANS_STRATEGY_WITHDRAWAL_SAVE,
    },
    request::api_wallet::strategy::*,
    response_vo::api_wallet::strategy::{CollectionStrategyResp, WithdrawStrategyResp},
};
use std::collections::HashMap;
use wallet_ecdh::GLOBAL_KEY;

impl BackendApi {
    // 保存&更新归集策略配置
    pub async fn save_collect_strategy(
        &self,
        req: &SaveCollectStrategyReq,
    ) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;

        self.post_api_backend::<_, ()>(TRANS_STRATEGY_COLLECT_SAVE, api_req).await?;
        Ok(())
    }

    // 保存&更新出款策略配置
    pub async fn save_withdrawal_strategy(
        &self,
        req: &SaveWithdrawStrategyReq,
    ) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;
        self.post_api_backend::<_, ()>(TRANS_STRATEGY_WITHDRAWAL_SAVE, api_req).await?;
        Ok(())
    }

    // 查询归集策略配置
    pub async fn query_collect_strategy(
        &self,
        uid: &str,
    ) -> Result<CollectionStrategyResp, crate::Error> {
        let mut req = HashMap::new();
        req.insert("uid", uid);
        let api_req = ApiBackendRequest::new(req)?;

        let res = self
            .post_api_backend::<_, CollectionStrategyResp>(
                TRANS_STRATEGY_GET_COLLECT_CONFIG,
                api_req,
            )
            .await?;
        res.ok_or(Backend(Some("no found list".to_string())))
    }

    // 查询出款策略配置
    pub async fn query_withdrawal_strategy(
        &self,
        uid: &str,
    ) -> Result<WithdrawStrategyResp, crate::Error> {
        let mut req = HashMap::new();
        req.insert("uid", uid);
        let api_req = ApiBackendRequest::new(req)?;
        let res = self
            .post_api_backend::<_, WithdrawStrategyResp>(
                TRANS_STRATEGY_GET_WITHDRAWAL_CONFIG,
                api_req,
            )
            .await?;

        res.ok_or(ApiBackend(999, Some("no fond list".to_string())))
    }

    // 查询策略默认值
    pub async fn query_api_wallet_configs(&self) -> Result<serde_json::Value, crate::Error> {
        let res = self.post_api_backend::<_, serde_json::Value>(API_WALLET_CONFIG, ()).await?;

        res.ok_or(ApiBackend(999, Some("no fond list".to_string())))
    }
}
