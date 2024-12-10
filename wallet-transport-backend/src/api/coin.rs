use std::collections::HashMap;

use crate::{
    request::{TokenQueryByContractAddressReq, TokenQueryByPageReq},
    response_vo::coin::{CoinInfos, TokenPriceInfos, TokenQueryByContractAddressRes, TokenRates},
};

use super::BackendApi;

impl BackendApi {
    pub async fn custom_token_init(
        &self,
        req: crate::request::CustomTokenInitReq,
    ) -> Result<bool, crate::Error> {
        let res = self
            .client
            .post("token/custom/token/init")
            .json(serde_json::json!(req))
            .send::<crate::response::BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn token_subscribe(
        &self,
        req: crate::request::TokenSubscribeReq,
    ) -> Result<HashMap<String, serde_json::Value>, crate::Error> {
        let res = self
            .client
            .post("token/subscribe")
            .json(serde_json::json!(req))
            .send::<serde_json::Value>()
            .await?;
        let res: crate::response::BackendResponse =
            wallet_utils::serde_func::serde_from_value(res)?;
        res.process()
    }

    pub async fn token_query_price(
        &self,
        req: crate::request::TokenQueryPriceReq,
    ) -> Result<TokenPriceInfos, crate::Error> {
        let res = self
            .client
            .post("token/queryPrice")
            .json(serde_json::json!(req))
            .send::<crate::response::BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn _token_query_price(
        &self,
        req: crate::request::TokenQueryPriceReq,
    ) -> Result<wallet_types::valueobject::TokenPopularByPages, crate::Error> {
        let res = self
            .client
            .post("token/queryPrice")
            .json(serde_json::json!(req))
            .send::<crate::response::BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn token_rates(&self) -> Result<TokenRates, crate::Error> {
        let res = self
            .client
            .post("token/queryRates")
            .send::<crate::response::BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn token_query_by_page(
        &self,
        req: &TokenQueryByPageReq,
    ) -> Result<CoinInfos, crate::Error> {
        let req = serde_json::json!(req);

        let res = self
            .client
            .post("token/queryByPage")
            .json(req)
            .send::<crate::response::BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn token_query_by_contract_address(
        &self,
        req: &TokenQueryByContractAddressReq,
    ) -> Result<TokenQueryByContractAddressRes, crate::Error> {
        let req = serde_json::json!(req);

        let res = self
            .client
            .post("token/queryByContractAddress")
            .json(req)
            .send::<crate::response::BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn token_cancel_subscribe(
        &self,
        req: crate::request::TokenCancelSubscribeReq,
    ) -> Result<HashMap<String, serde_json::Value>, crate::Error> {
        let res = self
            .client
            .post("token/cancelSubscribe")
            .json(serde_json::json!(req))
            .send::<crate::response::BackendResponse>()
            .await?;
        res.process()
    }

    /// query token fee_rate
    pub async fn token_query_by_currency(
        &self,
        chain_code: &str,
        currency: &str,
        symbol: &str,
    ) -> Result<crate::response_vo::coin::TokenCurrency, crate::Error> {
        let mut params = HashMap::new();

        let symbol = symbol.to_lowercase();
        params.insert("chainCode", chain_code);
        params.insert("code", &symbol);
        params.insert("currency", currency);

        let res = self
            .client
            .post("token/queryByCurrency")
            .json(params)
            .send::<crate::response::BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn query_history_price(
        &self,
        req: &crate::request::TokenQueryHistoryPrice,
    ) -> Result<crate::response_vo::coin::TokenHistoryPrices, crate::Error> {
        let res = self
            .client
            .post("token/queryHisPrice")
            .json(req)
            .send::<crate::response::BackendResponse>()
            .await?;
        res.process()
    }

    pub async fn query_popular_by_page(
        &self,
        req: &crate::request::TokenQueryPopularByPageReq,
    ) -> Result<wallet_types::valueobject::TokenPopularByPages, crate::Error> {
        let res = self
            .client
            .post("token/queryPopularByPage")
            .json(req)
            .send::<crate::response::BackendResponse>()
            .await?;
        res.process()
    }
}

#[cfg(test)]
mod test {
    use wallet_utils::init_test_log;

    use crate::{
        api::BackendApi,
        request::{
            CustomTokenInitReq, TokenCancelSubscribeReq, TokenQueryByContractAddressReq,
            TokenQueryByPageReq, TokenQueryHistoryPrice, TokenQueryPopularByPageReq,
            TokenQueryPrice, TokenQueryPriceReq, TokenSubscribeReq,
        },
    };

    #[tokio::test]
    async fn test_custom_token_init() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = CustomTokenInitReq {
            chain_code: "eth".to_string(),
            symbol: "XRP".to_string(),
            token_name: "XRP(IBC)".to_string(),
            contract_address: Some("0x628F76eAB0C1298F7a24d337bBbF1ef8A1Ea6A24".to_string()),
            master: false,
            unit: 6,
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .custom_token_init(req)
            .await
            .unwrap();

        println!("[test_token_subscribe] res: {res:?}");
    }

    #[tokio::test]
    async fn test_token_subscribe() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = TokenSubscribeReq {
            chain_code: "eth".to_string(),
            address: "1".to_string(),
            index: Some(0),
            contract_account_address: None,
            uid: "1".to_string(),
            sn: "1".to_string(),
            app_id: "1".to_string(),
            device_type: Some("ANDROID".to_string()),
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .token_subscribe(req)
            .await
            .unwrap();

        println!("[test_token_subscribe] res: {res:?}");
    }

    #[tokio::test]
    async fn test_token_query_currency() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .token_query_by_currency("tron", "USDT", "trx")
            .await
            .unwrap();

        tracing::info!("[test_token_query_price] res: {res:?}");
    }

    #[tokio::test]
    async fn test_token_cancel_subscribe() {
        let base_url = crate::consts::BASE_URL;

        let req = TokenCancelSubscribeReq {
            address: "".to_string(),
            contract_address: "".to_string(),
            sn: "".to_string(),
        };
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .token_cancel_subscribe(req)
            .await
            .unwrap();

        println!("[test_token_subscribe] res: {res:?}");
    }

    #[tokio::test]
    async fn test_token_query_price() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = TokenQueryPriceReq(vec![TokenQueryPrice {
            chain_code: "ltc".to_string(),
            contract_address_list: vec!["".to_string()],
        }]);
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .token_query_price(req)
            .await
            .unwrap();

        tracing::info!("[test_token_list] res: {res:?}");
    }

    #[tokio::test]
    async fn test_token_list() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = TokenQueryByPageReq::new_default_token(Vec::new(), 0, 100);

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .token_query_by_page(&req)
            .await
            .unwrap();

        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        println!("[test_token_list] res: {:?}", res);
    }

    #[tokio::test]
    async fn test_popular_token_list() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = TokenQueryByPageReq::new_popular_token(0, 1000);

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .token_query_by_page(&req)
            .await
            .unwrap();

        println!("[test_token_list] res: {:?}", res);
    }

    #[tokio::test]
    async fn test_token_query_by_contract_address() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = TokenQueryByContractAddressReq {
            chain_code: "tron".to_string(),
            contract_address: "".to_string(),
        };

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .token_query_by_contract_address(&req)
            .await
            .unwrap();

        println!("[test_token_default_list] res: {:?}", res);
    }

    #[tokio::test]
    async fn test_token_query_his_price() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = TokenQueryHistoryPrice {
            chain_code: "tron".to_string(),
            symbol: "USDT".to_string(),
            date_type: "DAY".to_string(),
            currency: "usd".to_string(),
            contract_address: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string(),
        };

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .query_history_price(&req)
            .await
            .unwrap();

        println!("[test_token_default_list] res: {:?}", res);
    }

    #[tokio::test]
    async fn test_token_rates() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .token_rates()
            .await
            .unwrap();

        println!("[test_token_default_list] res: {:?}", res);
        let res = serde_json::to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
    }

    #[tokio::test]
    async fn test_query_popular_by_page() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let code = None;
        let chain_code = None;
        let order_column = Some("marketValue".to_string());
        let order_type = Some("DESC".to_string());
        let req = TokenQueryPopularByPageReq::new(
            code.clone(),
            chain_code.clone(),
            order_column.clone(),
            order_type.clone(),
            0,
            300,
        );

        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            .query_popular_by_page(&req)
            .await
            .unwrap();

        let res = serde_json::to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
    }

    #[tokio::test]
    async fn _test_token_query_price() {
        init_test_log();
        let base_url = crate::consts::BASE_URL;

        let req = TokenQueryPriceReq(vec![TokenQueryPrice {
            chain_code: "tron".to_string(),
            contract_address_list: vec!["".to_string()],
        }]);
        let res = BackendApi::new(Some(base_url.to_string()), None)
            .unwrap()
            ._token_query_price(req)
            .await
            .unwrap();

        println!("[test_query_popular_by_page] res: {:?}", res);
        let res = serde_json::to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
    }
}
