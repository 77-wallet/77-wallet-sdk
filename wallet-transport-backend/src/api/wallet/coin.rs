use crate::{
    CoinInfo,
    api::BackendApi,
    consts::endpoint::TOKEN_CUSTOM_TOKEN_INIT,
    request::{AllTokenQueryByPageReq, SwapTokenQueryReq},
    response::response::BackendResponse,
    response_vo::coin::{
        CoinInfos, CoinMarketValue, CoinSwappable, CoinSwappableList, TokenPopularByPages,
        TokenPrice, TokenPriceInfos,
    },
};
use serde_json::json;

impl BackendApi {
    pub async fn custom_token_init(
        &self,
        req: crate::request::CustomTokenInitReq,
    ) -> Result<bool, crate::Error> {
        let res = self
            .client
            .post(TOKEN_CUSTOM_TOKEN_INIT)
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn token_query_price(
        &self,
        req: &crate::request::TokenQueryPriceReq,
    ) -> Result<TokenPriceInfos, crate::Error> {
        let res = self
            .client
            .post("token/queryPrice")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn query_history_price(
        &self,
        req: &crate::request::TokenQueryHistoryPrice,
    ) -> Result<crate::response_vo::coin::TokenHistoryPrices, crate::Error> {
        let res =
            self.client.post("token/queryHisPrice").json(req).send::<BackendResponse>().await?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn query_popular_by_page(
        &self,
        req: &crate::request::TokenQueryPopularByPageReq,
    ) -> Result<TokenPopularByPages, crate::Error> {
        let res = self
            .client
            .post("token/queryPopularByPage")
            .json(req)
            .send::<BackendResponse>()
            .await?;
        res.process(&self.aes_cbc_cryptor)
    }

    pub async fn token_balance_refresh(
        &self,
        req: crate::request::TokenBalanceRefreshReq,
    ) -> Result<(), crate::Error> {
        let res = self
            .client
            .post("token/balance/refresh")
            .json(serde_json::json!(req))
            .send::<BackendResponse>()
            .await?;
        res.process(&self.aes_cbc_cryptor)
    }
    // 单独查询token价格
    pub async fn token_price(
        &self,
        chain_code: &str,
        token_addr: &str,
    ) -> Result<TokenPrice, crate::Error> {
        let endpoint = "token/collect/token";

        let req = serde_json::json!({
            "chainCode": chain_code,
            "tokenAddress": token_addr
        });

        Ok(self.post_request::<_, TokenPrice>(endpoint, &req).await?)
    }

    // 查询后端所有的token(排除自定义token)
    async fn query_all_tokens(
        &self,
        req: AllTokenQueryByPageReq,
    ) -> Result<CoinInfos, crate::Error> {
        let endpoint = "token/queryAllExcludeCustomByPage";

        Ok(self.post_request::<_, CoinInfos>(endpoint, &req).await?)
    }

    pub async fn fetch_all_tokens(
        &self,
        create_at: Option<String>,
        update_at: Option<String>,
    ) -> Result<Vec<CoinInfo>, crate::Error> {
        let mut page = 0;
        let page_size = 500;

        let mut result = Vec::new();

        loop {
            let req =
                AllTokenQueryByPageReq::new(create_at.clone(), update_at.clone(), page, page_size);

            let mut resp = self.query_all_tokens(req).await?;
            result.append(&mut resp.list);
            page += 1;
            if page >= resp.total_page {
                break;
            }

            // match self.query_all_tokens(req).await {
            //     Ok(mut resp) => {
            //         result.append(&mut resp.list);
            //         page += 1;
            //         if page >= resp.total_page {
            //             break;
            //         }
            //     }
            //     Err(e) => {
            //         tracing::error!("query_all_tokens error: {e:?}");
            //         break;
            //     }
            // }
        }

        Ok(result)
    }

    pub async fn swap_token_list(
        &self,
        req: SwapTokenQueryReq,
    ) -> Result<TokenPopularByPages, crate::Error> {
        let endpoint = "token/swapTokenList";

        let token = self.post_request::<_, TokenPopularByPages>(endpoint, &req).await?;

        Ok(token)
    }

    // 查询代币的市值
    pub async fn coin_market_value(
        &self,
        coin: std::collections::HashMap<String, String>,
    ) -> Result<CoinMarketValue, crate::Error> {
        let endpoint = "token/queryTokenSummaryDetail";
        let req = json!({
            "chainTokenAddrMap":coin
        });
        let market_value = self.post_request::<_, CoinMarketValue>(endpoint, &req).await?;
        Ok(market_value)
    }

    // 查询支持swap的代币
    pub async fn swappable_coin(&self) -> Result<Vec<CoinSwappable>, crate::Error> {
        let req = json!({});
        let endpoint = "token/swappable/list";
        let res = self.post_request::<_, CoinSwappableList>(endpoint, &req).await?;

        Ok(res.list)
    }
}
