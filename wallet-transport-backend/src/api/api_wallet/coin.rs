use crate::{
    consts::endpoint::api_wallet::API_WALLET_COIN_LIST,
    request::api_wallet::coin::ApiTokenQueryByPageReq,
    response_vo::api_wallet::{Pages, coin::ApiCoinInfo},
};
use wallet_ecdh::GLOBAL_KEY;

use crate::{Error::ApiBackend, api::BackendApi, api_request::ApiBackendRequest};

impl BackendApi {
    // api钱包查询币列表
    pub async fn api_wallet_coin_list(
        &self,
        req: ApiTokenQueryByPageReq,
    ) -> Result<Pages<ApiCoinInfo>, crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;

        let res =
            self.post_api_backend::<_, Pages<ApiCoinInfo>>(API_WALLET_COIN_LIST, api_req).await?;
        res.ok_or(ApiBackend(999, Some("no address list".to_string())))
    }

    pub async fn fetch_all_api_tokens(
        &self,
        create_at: Option<String>,
        update_at: Option<String>,
    ) -> Result<Vec<ApiCoinInfo>, crate::Error> {
        let mut page = 0;
        let page_size = 500;

        let mut result = Vec::new();

        loop {
            let req =
                ApiTokenQueryByPageReq::new(create_at.clone(), update_at.clone(), page, page_size);

            let mut resp = self.api_wallet_coin_list(req).await?;
            result.append(&mut resp.content);
            page += 1;
            if page >= resp.total_pages as i32 {
                break;
            }
        }

        Ok(result)
    }
}
