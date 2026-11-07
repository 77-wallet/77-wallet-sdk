use crate::{
    api::ReturnType, manager::WalletManager,
    response_vo::standard_wallet::coin::TokenPriceChangeRes, service::coin::CoinService,
};
use wallet_transport_backend::response_vo::coin::{CoinMarketValue, TokenHistoryPrices};

impl WalletManager {
    pub async fn get_hot_coin_list(
        &self,
        wallet_address: &str,
        account_id: u32,
        chain_code: Option<String>,
        keyword: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> ReturnType<
        wallet_database::pagination::Pagination<
            crate::response_vo::standard_wallet::coin::CoinInfo,
        >,
    > {
        CoinService::new(self.ctx)
            .get_hot_coin_list(
                wallet_address,
                Some(account_id),
                chain_code,
                keyword,
                Some(false),
                page,
                page_size,
            )
            .await
    }

    pub async fn get_multisig_hot_coin_list(
        &self,
        address: &str,
        chain_code: Option<String>,
        keyword: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> ReturnType<
        wallet_database::pagination::Pagination<
            crate::response_vo::standard_wallet::coin::CoinInfo,
        >,
    > {
        CoinService::new(self.ctx)
            .get_hot_coin_list(address, None, chain_code, keyword, Some(true), page, page_size)
            .await
    }

    pub async fn pull_hot_coins(&self) -> ReturnType<()> {
        CoinService::new(self.ctx).pull_hot_coins().await
    }

    pub async fn get_token_price(
        &self,
        symbols: Vec<String>,
    ) -> ReturnType<Vec<TokenPriceChangeRes>> {
        CoinService::new(self.ctx).get_token_price(symbols).await
    }

    pub async fn query_token_info(
        &self,
        chain_code: &str,
        token_address: &str,
    ) -> ReturnType<crate::response_vo::standard_wallet::coin::TokenInfo> {
        CoinService::new(self.ctx).query_token_info(chain_code, token_address.to_string()).await
    }
    pub async fn customize_coin(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_code: &str,
        token_address: &str,
        protocol: Option<String>,
    ) -> ReturnType<()> {
        CoinService::new(self.ctx)
            .customize_coin(
                address,
                account_id,
                chain_code,
                token_address.to_string(),
                protocol,
                false,
            )
            .await
    }

    pub async fn customize_multisig_coin(
        &self,
        address: &str,
        chain_code: &str,
        token_address: &str,
        protocol: Option<String>,
    ) -> ReturnType<()> {
        CoinService::new(self.ctx)
            .customize_coin(address, None, chain_code, token_address.to_string(), protocol, true)
            .await
    }

    pub async fn query_history_price(
        &self,
        req: wallet_transport_backend::request::TokenQueryHistoryPrice,
    ) -> ReturnType<TokenHistoryPrices> {
        CoinService::new(self.ctx).query_history_price(req).await
    }

    pub async fn coin_market_value(
        &self,
        req: std::collections::HashMap<String, String>,
    ) -> ReturnType<CoinMarketValue> {
        CoinService::new(self.ctx).market_value(req).await
    }

    pub async fn query_popular_by_page(
        &self,
        keyword: Option<String>,
        chain_code: Option<String>,
        order_column: Option<String>,
        order_type: Option<String>,
        page_num: i64,
        page_size: i64,
    ) -> ReturnType<wallet_database::pagination::Pagination<TokenPriceChangeRes>> {
        let order_column = order_column.and_then(|s| if s.is_empty() { None } else { Some(s) });
        let order_type = order_type.and_then(|s| if s.is_empty() { None } else { Some(s) });

        let req = wallet_transport_backend::request::TokenQueryPopularByPageReq {
            code: keyword,
            chain_code,
            order_column,
            order_type,
            page_num,
            page_size,
        };

        CoinService::new(self.ctx).query_popular_by_page(req).await
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use crate::testkit::env::get_manager;
    use anyhow::Result;
    use wallet_transport_backend::request::TokenQueryHistoryPrice;

    #[tokio::test]
    async fn test_get_hot_coin_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // let keyword = Some("StR");
        // let keyword = None;
        // let keyword = Some("USDT");
        let keyword = None;
        // let chain_code = Some("btc");
        let chain_code = Some("sol".to_string());
        // let chain_code = None;
        let wallet_address = "0x868Bd024461e572555c26Ed196FfabAA475BFcCd";
        let res = wallet_manager
            .get_hot_coin_list(wallet_address, 1, chain_code, keyword, 0, 1000)
            .await?;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {}", res);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_multisig_hot_coin_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // let keyword = Some("StR");
        let keyword = None;
        let chain_code = Some("tron".to_string());
        let res = wallet_manager
            .get_multisig_hot_coin_list(
                "TT4QgNx2rVD35tYU1LJ6tH5Ya1bxmannBK",
                chain_code,
                keyword,
                0,
                1000,
            )
            .await?;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_pull_hot_coins() -> Result<()> {
        wallet_utils::init_test_log();
        let (wallet_manager, _test_params) = get_manager().await?;

        let res = wallet_manager.pull_hot_coins().await?;

        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_query_token_info() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // let chain_code = "tron";
        // let chain_code = "btc";
        // let chain_code = "sol";
        let chain_code = "sui";
        // let token_address = "0x628F76eAB0C1298F7a24d337bBbF1ef8A1Ea6A24";
        // let token_address = "0xB8c77482e45F1F44dE1745F52C74426C631bDD52";
        // let token_address = "TFzMRRzQFhY9XFS37veoswLRuWLNtbyhiB";
        // let token_address = "bc1qgw4dunlmtvdy4vc8zauma4qjqtmrktjf8mw6le";
        // let token_address = "SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf";
        // let token_address = "So11111111111111111111111111111111111111112";
        let token_address =
        // "0x55d398326f99059fF775485246999027B3197955";
        // "0x288710173f12f677ac38b0c2b764a0fea8108cb5e32059c3dd8f650d65e2cb25::pepe::PEPE";
        // "EQACLXDwit01stiqK9FvYiJo15luVzfD5zU8uwDSq6JXxbP8";
        "0xdeeb7a4662eec9f2f3def03fb937a663dddaa2e215b8078a284d026b7946c270::deep::DEEP";

        // let token_address = "0x7a19f93b1ACF9FF8d33d21702298f2F0CdC93654";

        // let chain_code = "tron";
        // let token_address = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";

        // let chain_code = "sol";
        // let token_address = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN";

        // let chain_code = "bnb";
        // let token_address = "0x8965349fb649A33a30cbFDa057D8eC2C48AbE2A2";

        // let chain_code = "tron";
        // let token_address = "TTFreuJ4pYDaCeEMEtiR1GQDwPPrS4jKFk";

        let res = wallet_manager.query_token_info(chain_code, token_address).await?;
        tracing::info!("res: {res:?}");
        let res = serde_json::to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_customize_coin() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let wallet_address = "0x868Bd024461e572555c26Ed196FfabAA475BFcCd";
        let chain_code = "sol";
        let protocol = None;
        let token_address = "5goWRao6a3yNC4d6UjMdQxonkCMvKBwdpubU3qhfcdf1";

        let res = wallet_manager
            .customize_coin(wallet_address, Some(1), chain_code, token_address, protocol)
            .await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_customize_multisig_coin() -> Result<()> {
        wallet_utils::init_test_log();
        let (wallet_manager, _test_params) = get_manager().await?;
        let address = "0xa01e0ee36c61B7C667F741094603290E5a2182C1";
        let chain_code = "bnb";
        let protocol = None;
        let token_address = "0x55d398326f99059fF775485246999027B3197955";

        let res = wallet_manager
            .customize_multisig_coin(address, chain_code, token_address, protocol)
            .await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_token_price() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // let symbols = vec![
        //     "TRX".to_string(),
        //     "USDT".to_string(),
        //     "ETH".to_string(),
        //     "CAKE".to_string(),
        //     "USDC".to_string(),
        //     "BNB".to_string(),
        //     "SOL".to_string(),
        //     "BTC".to_string(),
        // ];
        let symbols = vec!["TRX".to_string()];

        let res = wallet_manager.get_token_price(symbols).await?;
        let res = serde_json::to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_query_history_price() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // [24小时：DAY，7天：WEEK，月：MONTH，年：YEAR，2年：YEAR2]
        let req = TokenQueryHistoryPrice {
            // symbol: "USDT".to_string(),
            chain_code: "tron".to_string(),
            // date_type: "DAY".to_string(),
            // date_type: "WEEK".to_string(),
            date_type: "MONTH".to_string(),
            // date_type: "YEAR".to_string(),
            // date_type: "YEAR2".to_string(),
            currency: "USD".to_string(),
            contract_address: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string(),
        };

        let res = wallet_manager.query_history_price(req).await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_query_popular_by_page() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        // let keyword = Some("b".to_string());
        let keyword = None;
        // let chain_code = Some("tron".to_string());
        let chain_code = None;
        let order_column = Some("marketValue".to_string());
        // let order_column = None;
        let order_type = Some("DESC".to_string());
        // let order_type = None;
        let page_num = 0;
        let page_size = 20;

        let res = wallet_manager
            .query_popular_by_page(
                keyword,
                chain_code,
                order_column,
                order_type,
                page_num,
                page_size,
            )
            .await?;
        tracing::info!("res: {res:?}");
        let res = serde_json::to_string(&res).unwrap();
        tracing::info!("res: {res:?}");

        Ok(())
    }
}
