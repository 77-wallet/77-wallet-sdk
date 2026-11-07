use crate::{
    consts::endpoint::api_wallet::API_WALLET_CHAIN_LIST,
    response_vo::api_wallet::chain::ApiChainListResp,
};
use std::collections::HashMap;
use wallet_ecdh::GLOBAL_KEY;

use crate::{Error::ApiBackend, api::BackendApi, api_request::ApiBackendRequest};

impl BackendApi {
    // api钱包查询链列表
    pub async fn api_wallet_chain_list(
        &self,
        app_version_code: &str,
    ) -> Result<ApiChainListResp, crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let mut req = HashMap::new();
        req.insert("appVersionCode", app_version_code);
        let api_req = ApiBackendRequest::new(req)?;

        // let res = self
        //     .client
        //     .post(API_WALLET_CHAIN_LIST)
        //     .json(api_req)
        //     .send::<ApiBackendResponse>()
        //     .await?;

        let opt =
            self.post_api_backend::<_, ApiChainListResp>(API_WALLET_CHAIN_LIST, api_req).await?;

        // let opt = res.process(API_WALLET_CHAIN_LIST)?;
        opt.ok_or(ApiBackend(999, Some("no address list".to_string())))
    }
}
