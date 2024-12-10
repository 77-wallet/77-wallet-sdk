pub mod token_price;
use crate::response_vo::coin::{TokenCurrencies, TokenCurrencyId, TokenPriceChangeRes};
pub use token_price::TokenCurrencyGetter;
use wallet_database::{
    entities::coin::CoinId,
    repositories::{coin::CoinRepoTrait, exchange_rate::ExchangeRateRepoTrait, ResourcesRepo},
};
use wallet_transport_backend::{request::TokenQueryPriceReq, response_vo::coin::TokenCurrency};

pub struct CoinDomain {}
impl Default for CoinDomain {
    fn default() -> Self {
        Self::new()
    }
}
impl CoinDomain {
    pub fn new() -> Self {
        Self {}
    }

    /// 查询代币汇率
    pub async fn get_token_currencies_v2(
        &mut self,
        repo: &mut ResourcesRepo,
    ) -> Result<TokenCurrencies, crate::ServiceError> {
        let config = crate::app_state::APP_STATE.read().await;
        let currency = config.currency();

        let coins = repo.coin_list(None, None).await?;

        let exchange_rate_list = repo.list().await?;
        // 查询本地的所有币符号
        let mut map = std::collections::HashMap::new();
        for coin in coins {
            let price = coin.price.parse::<f64>().unwrap_or_default();
            let (currency_price, rate) = if let Some(rate) = exchange_rate_list
                .iter()
                .find(|rate| rate.target_currency == currency)
            {
                (price * rate.rate, rate.rate)
            } else {
                (f64::default(), f64::default())
            };

            let symbol = coin.symbol.to_ascii_lowercase();
            let name = coin.name;

            let token_currency_id = TokenCurrencyId::new(&symbol, &coin.chain_code);

            let token_currency = TokenCurrency {
                name,
                chain_code: coin.chain_code,
                code: symbol.clone(),
                price: Some(price),
                currency_price: Some(currency_price),
                rate,
            };
            map.insert(token_currency_id, token_currency);
        }

        Ok(TokenCurrencies(map))
    }

    pub async fn get_token_price(
        &mut self,
        repo: &mut ResourcesRepo,
        symbols: Vec<String>,
    ) -> Result<Vec<TokenPriceChangeRes>, crate::ServiceError> {
        let tx = repo;
        let backend_api = crate::Context::get_global_backend_api()?;
        let coins = tx.coin_list_with_symbols(symbols, None).await?;

        let mut req: TokenQueryPriceReq = TokenQueryPriceReq(Vec::new());

        coins.into_iter().for_each(|coin| {
            let contract_address = coin.token_address.clone().unwrap_or_default();
            req.insert(&coin.chain_code, &contract_address);
        });

        let tokens = backend_api.token_query_price(req).await?.list;

        let config = crate::app_state::APP_STATE.read().await;
        let currency = config.currency();

        let exchange_rate = ExchangeRateRepoTrait::detail(tx, Some(currency.to_string())).await?;

        let mut res = Vec::new();
        if let Some(exchange_rate) = exchange_rate {
            for token in tokens {
                let coin_id = CoinId {
                    chain_code: token.chain_code.clone(),
                    symbol: token.symbol.clone(),
                    token_address: token.token_address.clone(),
                };
                tx.update_price_unit(&coin_id, &token.price.to_string(), token.unit)
                    .await?;
                let data =
                    TokenCurrencies::calculate_token_price_changes(token, exchange_rate.rate)
                        .await?;
                res.push(data);
            }
        }

        Ok(res)
    }
}
