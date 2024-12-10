use wallet_database::entities::{
    coin::CoinEntity,
    exchange_rate::{ExchangeRateEntity, QueryReq},
};
use wallet_transport_backend::response_vo::coin::TokenCurrency;
use wallet_utils::unit;

pub struct TokenCurrencyGetter;

impl TokenCurrencyGetter {
    /// currency 法币符号,chain_code 链码  symbol 币符号
    pub async fn get_currency(
        currency: &str,
        chain_code: &str,
        symbol: &str,
    ) -> Result<TokenCurrency, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let coin = CoinEntity::get_coin(chain_code, symbol, pool.as_ref())
            .await?
            .ok_or(crate::BusinessError::Coin(crate::CoinError::NotFound(
                format!("chain_code = {},symbol = {}", chain_code, symbol),
            )))?;

        // get rate
        let req = QueryReq {
            target_currency: Some(currency.to_string()),
        };
        let exchange = ExchangeRateEntity::detail(pool.as_ref(), &req)
            .await?
            .ok_or(crate::BusinessError::ExchangeRate(
                crate::ExchangeRate::NotFound,
            ))?;

        let (price, currency_price) = if !coin.price.is_empty() {
            let price = unit::string_to_f64(&coin.price)?;
            (Some(price), Some(exchange.rate * price))
        } else {
            (None, None)
        };

        Ok(TokenCurrency {
            chain_code: chain_code.to_string(),
            code: symbol.to_string(),
            name: coin.name,
            price,
            currency_price,
            rate: exchange.rate,
        })
    }
}
