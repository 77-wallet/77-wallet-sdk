use crate::{
    context::Context,
    response_vo::standard_wallet::account::{BalanceInfo, BalanceStr},
};
use wallet_database::{
    CoreDbPool,
    entities::asset_token_key::AssetTokenKey,
    repositories::{coin::CoinRepo, exchange_rate::ExchangeRateRepo},
};
use wallet_transport_backend::response_vo::coin::TokenCurrency;
use wallet_utils::unit;

/// 代币价格获取器
pub struct TokenCurrencyGetter;

impl TokenCurrencyGetter {
    pub async fn get_currency_by_token_key_with_pool(
        pool: &CoreDbPool,
        currency: &str,
        chain_code: &str,
        symbol: &str,
        token_key: AssetTokenKey,
    ) -> Result<TokenCurrency, crate::error::service::ServiceError> {
        // 获取数据库连接池
        let coin = CoinRepo::coin_by_chain_token_key(chain_code, token_key, pool).await?;

        // 获取价格信息
        let (price, currency_price, rate) =
            Self::calculate_price_info(pool, &coin.price, currency).await?;

        Ok(TokenCurrency {
            chain_code: chain_code.to_string(),
            code: symbol.to_string(),
            name: coin.name,
            price,
            currency_price,
            rate,
            decimals: coin.decimals,
        })
    }

    pub async fn get_currency_by_token_key(
        ctx: &Context,
        currency: &str,
        chain_code: &str,
        symbol: &str,
        token_key: AssetTokenKey,
    ) -> Result<TokenCurrency, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        Self::get_currency_by_token_key_with_pool(&pool, currency, chain_code, symbol, token_key)
            .await
    }

    #[deprecated(note = "use get_currency_by_token_key with explicit Context")]
    pub async fn get_currency_by_token_key_with_ctx(
        ctx: &Context,
        currency: &str,
        chain_code: &str,
        symbol: &str,
        token_key: AssetTokenKey,
    ) -> Result<TokenCurrency, crate::error::service::ServiceError> {
        Self::get_currency_by_token_key(ctx, currency, chain_code, symbol, token_key).await
    }

    /// 获取余额信息（token-key 强类型入口）
    pub async fn get_balance_info_by_token_key(
        ctx: &Context,
        chain_code: &str,
        symbol: &str,
        amount: f64,
        token_key: AssetTokenKey,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        // 获取当前应用的货币设置
        let currency = Self::get_app_currency().await;

        // 获取代币价格信息
        let token_price =
            Self::get_currency_by_token_key(ctx, &currency, chain_code, symbol, token_key).await?;

        Ok(BalanceInfo::new(amount, Some(token_price.get_price(&currency)), &currency))
    }

    #[deprecated(note = "use get_balance_info_by_token_key with explicit Context")]
    pub async fn get_balance_info_by_token_key_with_ctx(
        ctx: &Context,
        chain_code: &str,
        symbol: &str,
        amount: f64,
        token_key: AssetTokenKey,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        Self::get_balance_info_by_token_key(ctx, chain_code, symbol, amount, token_key).await
    }

    // 查询后端的币价，并转换为balance数据结构(修改为本地)
    pub async fn get_bal_by_backend(
        ctx: &Context,
        chain_code: &str,
        token_addr: &str,
        amount: &str,
        decimals: u8,
    ) -> Result<BalanceStr, crate::error::service::ServiceError> {
        // 获取当前应用的货币设置
        let currency = Self::get_app_currency().await;

        // let backend = crate::manager::Context::get_global_backend_api()?;
        // let token_price = backend.token_price(chain_code, token_addr).await?;
        let pool = ctx.core_pool()?;

        // 查询代币信息
        let token = CoinRepo::coin_by_chain_token_key(
            chain_code,
            AssetTokenKey::from_raw(Some(token_addr)),
            &pool,
        )
        .await?;

        // 计算价格信息
        let (price, currency_price, _) =
            Self::calculate_price_info(&pool, &token.price, &currency).await?;
        let unit_price = if currency.eq_ignore_ascii_case("usdt") { price } else { currency_price };

        // 如果价格为空，则返回错误
        let Some(unit_price) = unit_price else {
            return Err(crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(
                    "Token price not available".to_string(),
                ),
            ));
        };

        // 转换金额
        let amount = unit::convert_to_u256(amount, decimals)?;

        Ok(BalanceStr::new(amount, Some(unit_price), &currency, decimals)?)
    }

    /// 获取当前应用的货币设置
    async fn get_app_currency() -> String {
        let state = crate::app_state::APP_STATE.read().await;
        state.currency().to_string()
    }

    /// 计算价格信息
    /// - pool: 数据库连接池
    /// - price_str: 代币价格字符串
    /// - currency: 法币符号
    /// - 返回值: (USDT价格, 法币价格, 汇率)
    async fn calculate_price_info(
        pool: &wallet_database::CoreDbPool,
        price_str: &str,
        currency: &str,
    ) -> Result<(Option<f64>, Option<f64>, f64), crate::error::service::ServiceError> {
        // 获取汇率（如果不是USDT）
        let rate = if currency.eq_ignore_ascii_case("usdt") {
            1.0 // USDT的汇率为1
        } else {
            ExchangeRateRepo::exchange_rate(currency, pool.clone()).await?.rate
        };

        // 解析价格
        if !price_str.is_empty() {
            let price = unit::string_to_f64(price_str)?;
            Ok((Some(price), Some(price * rate), rate))
        } else {
            Ok((None, None, rate))
        }
    }
}
