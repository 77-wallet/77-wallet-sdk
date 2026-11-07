use std::ops::{Deref, DerefMut};
use wallet_database::entities::{
    api_chain::ApiChainEntity, asset_token_key::AssetTokenKey, chain::ChainEntity,
};
use wallet_transport_backend::response_vo::coin::{TokenCurrency, TokenPriceChangeBody};

use crate::{
    domain::{account::AccountDomain, app::config::ConfigDomain},
    response_vo::standard_wallet::chain::ChainList,
};

use super::{
    account::BalanceInfo,
    chain::{ChainAssets, ChainCodeAndName},
    wallet::AccountInfos,
};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinInfo {
    pub symbol: String,
    pub name: Option<String>,
    // pub chain_list: HashSet<ChainInfo>,
    pub chain_list: ChainList,
    // pub is_multichain: bool,
    pub is_default: bool,
    // 热门代币
    pub hot_coin: bool,
    // 展示合约地址
    pub show_contract: bool,
}

#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CoinInfoList(pub Vec<CoinInfo>);

impl Deref for CoinInfoList {
    type Target = Vec<CoinInfo>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CoinInfoList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHistoryPrice {
    pub date: String,
    pub price: BalanceInfo,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHistoryPriceRes(pub Vec<QueryHistoryPrice>);

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq, Hash)]
pub struct TokenCurrencyId {
    pub symbol: String,
    pub chain_code: String,
    pub token_address: AssetTokenKey,
}

impl TokenCurrencyId {
    pub fn new<T: Into<AssetTokenKey>>(symbol: &str, chain_code: &str, token_address: T) -> Self {
        Self {
            symbol: symbol.to_ascii_lowercase(),
            chain_code: chain_code.to_string(),
            token_address: token_address.into(),
        }
    }

    // pub(crate) fn gen_key(&self) -> String {
    //     Self::make_key(
    //         &self.symbol.to_ascii_uppercase(),
    //         &self.chain_code,
    //         self.token_address.as_db_str(),
    //     )
    // }

    // pub(crate) fn make_key(symbol: &str, chain_code: &str, token_address: &str) -> String {
    //     format!("{}:{}:{}", symbol, chain_code, token_address)
    // }
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct TokenCurrencies(pub std::collections::HashMap<TokenCurrencyId, TokenCurrency>);

impl Deref for TokenCurrencies {
    type Target = std::collections::HashMap<TokenCurrencyId, TokenCurrency>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for TokenCurrencies {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/**
 * AssetsWithAddressType trait定义了具有地址类型信息的资产实体需要实现的方法
 * 用于统一处理AssetsEntityWithAddressType和ApiAssetsEntityWithAddressType两种相似的实体类型
 */
trait AssetsWithAddressType {
    fn balance(&self) -> &str;
    fn symbol(&self) -> &str;
    fn chain_code(&self) -> &str;
    fn token_address(&self) -> Option<String>;
    fn address(&self) -> &str;
    fn is_multisig(&self) -> i8;
    fn address_type(&self) -> Option<String>;
}

/**
 * 为AssetsEntityWithAddressType实现AssetsWithAddressType trait
 */
impl AssetsWithAddressType for wallet_database::entities::assets::AssetsEntityWithAddressType {
    fn balance(&self) -> &str {
        &self.balance
    }
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn chain_code(&self) -> &str {
        &self.chain_code
    }
    fn token_address(&self) -> Option<String> {
        self.token_key().to_option_string_for_api()
    }
    fn address(&self) -> &str {
        &self.address
    }
    fn is_multisig(&self) -> i8 {
        self.is_multisig
    }
    fn address_type(&self) -> Option<String> {
        self.address_type()
    }
}

/**
 * 为ApiAssetsEntityWithAddressType实现AssetsWithAddressType trait
 */
impl AssetsWithAddressType
    for wallet_database::entities::api_assets::ApiAssetsEntityWithAddressType
{
    fn balance(&self) -> &str {
        &self.balance
    }
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn chain_code(&self) -> &str {
        &self.chain_code
    }
    fn token_address(&self) -> Option<String> {
        self.token_key().to_option_string_for_api()
    }
    fn address(&self) -> &str {
        &self.address
    }
    fn is_multisig(&self) -> i8 {
        self.is_multisig
    }
    fn address_type(&self) -> Option<String> {
        self.address_type()
    }
}

// 定义一个 trait 来抽象不同类型的 ChainEntity
trait ChainEntityTrait {
    fn chain_code(&self) -> &str;
    fn name(&self) -> &str;
}

// 为 ChainEntity 实现 trait
impl ChainEntityTrait for ChainEntity {
    fn chain_code(&self) -> &str {
        &self.chain_code
    }
    fn name(&self) -> &str {
        &self.name
    }
}

// 为 ApiChainEntity 实现 trait
impl ChainEntityTrait for ApiChainEntity {
    fn chain_code(&self) -> &str {
        &self.chain_code
    }
    fn name(&self) -> &str {
        &self.name
    }
}

/**
 * AssetsEntityTrait trait定义了资产实体需要实现的方法
 * 用于统一处理AssetsEntity和ApiAssetsEntity两种相似的实体类型
 */
trait AssetsEntityTrait {
    fn balance(&self) -> &str;
    fn symbol(&self) -> &str;
    fn chain_code(&self) -> &str;
    fn token_address(&self) -> Option<String>;
}

/**
 * 为AssetsEntity实现AssetsEntityTrait trait
 */
impl AssetsEntityTrait for wallet_database::entities::assets::AssetsEntity {
    fn balance(&self) -> &str {
        &self.balance
    }
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn chain_code(&self) -> &str {
        &self.chain_code
    }
    fn token_address(&self) -> Option<String> {
        self.token_key().to_option_string_for_api()
    }
}

/**
 * 为ApiAssetsEntity实现AssetsEntityTrait trait
 */
impl AssetsEntityTrait for wallet_database::entities::api_assets::ApiAssetsEntity {
    fn balance(&self) -> &str {
        &self.balance
    }
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn chain_code(&self) -> &str {
        &self.chain_code
    }
    fn token_address(&self) -> Option<String> {
        self.token_key().to_option_string_for_api()
    }
}

/**
 * AccountChainAssetTrait trait定义了账户链资产需要实现的方法
 * 用于统一处理AccountChainAsset和ApiAccountChainAsset两种相似的实体类型
 */
trait AccountChainAssetTrait {
    fn balance_mut(&mut self) -> &mut BalanceInfo;
}

/**
 * 为AccountChainAsset实现AccountChainAssetTrait trait
 */
impl AccountChainAssetTrait for super::assets::AccountChainAsset {
    fn balance_mut(&mut self) -> &mut BalanceInfo {
        &mut self.balance
    }
}

/**
 * 为ApiAccountChainAsset实现AccountChainAssetTrait trait
 */
impl AccountChainAssetTrait for crate::response_vo::api_wallet::assets::ApiAccountChainAsset {
    fn balance_mut(&mut self) -> &mut BalanceInfo {
        &mut self.balance
    }
}

impl TokenCurrencies {
    pub async fn calculate_token_price_changes(
        ctx: &'static crate::context::Context,
        data: &TokenPriceChangeBody,
        exchange_rate: f64,
    ) -> Result<TokenPriceChangeRes, crate::error::service::ServiceError> {
        // let market_value = wallet_utils::conversion::decimal_from_f64(data.market_value)?;
        // let day_change_amount =
        //     wallet_utils::conversion::decimal_from_f64(data.day_change_amount.unwrap_or_default())?;
        let balance = Self::calculate(ctx, exchange_rate, data.price).await?;
        let market_value = Self::calculate(ctx, exchange_rate, data.market_value).await?;
        let day_change_amount =
            Self::calculate(ctx, exchange_rate, data.day_change_amount.unwrap_or_default()).await?;
        Ok((data, balance, market_value, day_change_amount).into())
    }

    pub async fn calculate(
        ctx: &'static crate::context::Context,
        exchange_rate: f64,
        value: f64,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        // let config = crate::app_state::APP_STATE.read().await;
        // let currency = config.currency();
        // let currency = "USD";
        let currency = ConfigDomain::get_currency(ctx).await?;
        let unit_price = value * exchange_rate;

        Ok(BalanceInfo {
            amount: Default::default(),
            currency,
            unit_price: Some(unit_price),
            fiat_value: Default::default(),
        })
    }

    // 泛型方法处理不同类型的资产列表
    /**
     * 泛型方法：计算链资产列表
     *
     * 该方法用于统一处理AssetsEntityWithAddressType和ApiAssetsEntityWithAddressType两种资产实体类型
     * 以及ChainEntity和ApiChainEntity两种链实体类型，实现了代码复用
     *
     * @param data 资产数据列表，需要实现AssetsWithAddressType trait
     * @param chains 链数据列表，需要实现ChainEntityTrait trait
     * @return 链资产列表
     */
    async fn calculate_chain_assets_list_generic<T, U>(
        &self,
        ctx: &'static crate::context::Context,
        data: Vec<T>,
        chains: Vec<U>,
    ) -> Result<Vec<ChainAssets>, crate::error::service::ServiceError>
    where
        T: AssetsWithAddressType,
        U: ChainEntityTrait,
    {
        // 计算所有币种的总数
        let mut sum = f64::default();
        for assets in &data {
            let balance = wallet_utils::parse_func::f64_from_str(assets.balance())?;
            sum += balance;
        }

        let mut res = Vec::new();
        for assets in data {
            if let Some(chain) =
                chains.iter().find(|chain| chain.chain_code() == assets.chain_code())
            {
                let balance = self
                    .async_calculate_to_balance(
                        ctx,
                        assets.balance(),
                        assets.symbol(),
                        assets.chain_code(),
                        assets.token_address(),
                    )
                    .await?;

                let name = if assets.chain_code() == "btc" || assets.chain_code() == "ltc" {
                    let address_category = AccountDomain::get_show_address_type(
                        assets.chain_code(),
                        assets.address_type(),
                    )?;
                    address_category.show_name().to_uppercase()
                } else {
                    chain.name().to_string()
                };

                let asset_quantity_ratio = balance.amount / sum;
                res.push(crate::response_vo::standard_wallet::chain::ChainAssets {
                    chain_code: assets.chain_code().to_string(),
                    name,
                    address: assets.address().to_string(),
                    token_address: assets.token_address().unwrap_or_default(),
                    balance,
                    symbol: assets.symbol().to_string(),
                    is_multisig: assets.is_multisig(),
                    asset_quantity_ratio,
                })
            }
        }
        Ok(res)
    }

    pub async fn calculate_chain_assets_list(
        &self,
        data: Vec<wallet_database::entities::assets::AssetsEntityWithAddressType>,
        chains: Vec<ChainEntity>,
        ctx: &'static crate::context::Context,
    ) -> Result<Vec<ChainAssets>, crate::error::service::ServiceError> {
        self.calculate_chain_assets_list_generic(ctx, data, chains).await
    }

    pub async fn calculate_api_chain_assets_list(
        &self,
        data: Vec<wallet_database::entities::api_assets::ApiAssetsEntityWithAddressType>,
        chains: Vec<ApiChainEntity>,
        ctx: &'static crate::context::Context,
    ) -> Result<Vec<ChainAssets>, crate::error::service::ServiceError> {
        self.calculate_chain_assets_list_generic(ctx, data, chains).await
    }

    pub async fn async_calculate_to_balance(
        &self,
        ctx: &'static crate::context::Context,
        balance: &str,
        symbol: &str,
        chain_code: &str,
        token_address: Option<String>,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        let balance = wallet_utils::parse_func::decimal_from_str(&balance)?;

        let currency = ConfigDomain::get_currency(ctx).await?;
        let token_currency_id = TokenCurrencyId::new(symbol, chain_code, token_address);

        let (price, fiat_balance) = if let Some(token_currency) = self.0.get(&token_currency_id) {
            // 获取价格，如果为None则使用默认值0.0
            let price_f64 = token_currency.get_price(&currency);
            let price = wallet_types::Decimal::from_f64_retain(price_f64);

            let fiat_balance = price.map(|p| p * balance);
            (price, fiat_balance)
        } else {
            // 如果没有找到对应的token_currency，使用默认价格0.0
            let price = wallet_types::Decimal::from_f64_retain(0.0);
            let fiat_balance = price.map(|p| p * balance);
            (price, fiat_balance)
        };

        Ok(BalanceInfo {
            amount: wallet_utils::conversion::decimal_to_f64(&balance)?,
            currency: currency.to_string(),
            unit_price: price.map(|p| wallet_utils::conversion::decimal_to_f64(&p)).transpose()?,
            fiat_value: fiat_balance
                .map(|p| wallet_utils::conversion::decimal_to_f64(&p))
                .transpose()?,
        })
    }

    pub fn calculate_to_balance(
        &self,
        currency: &str,
        balance: &str,
        symbol: &str,
        chain_code: &str,
        token_address: Option<String>,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        let balance = wallet_utils::parse_func::decimal_from_str(&balance)?;

        let token_currency_id = TokenCurrencyId::new(symbol, chain_code, token_address.clone());

        let (price, fiat_balance) = if let Some(token_currency) = self.0.get(&token_currency_id) {
            // 获取价格，内部已经处理了None的情况
            let price_f64 = token_currency.get_price(&currency);
            let price = wallet_types::Decimal::from_f64_retain(price_f64);

            let fiat_balance = price.map(|p| p * balance);
            (price, fiat_balance)
        } else {
            // 如果没有找到对应的token_currency，使用默认价格0.0
            let price = wallet_types::Decimal::from_f64_retain(0.0);
            let fiat_balance = price.map(|p| p * balance);
            (price, fiat_balance)
        };
        Ok(BalanceInfo {
            amount: wallet_utils::conversion::decimal_to_f64(&balance)?,
            currency: currency.to_string(),
            unit_price: price.map(|p| wallet_utils::conversion::decimal_to_f64(&p)).transpose()?,
            fiat_value: fiat_balance
                .map(|p| wallet_utils::conversion::decimal_to_f64(&p))
                .transpose()?,
        })
    }

    pub async fn calculate_account_total_assets(
        &self,
        ctx: &'static crate::context::Context,
        data: &mut [wallet_database::entities::assets::AssetsEntity],
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        let mut account_total_assets = Some(wallet_types::Decimal::default());
        let mut amount = wallet_types::Decimal::default();

        let currency = ConfigDomain::get_currency(ctx).await?;

        for assets in data.iter_mut() {
            let token_currency_id =
                TokenCurrencyId::new(&assets.symbol, &assets.chain_code, assets.token_address());

            let value = if let Some(token_currency) = self.0.get(&token_currency_id) {
                let balance = wallet_utils::parse_func::decimal_from_str(&assets.balance)?;
                let price = token_currency.get_price(&currency);
                let price = wallet_types::Decimal::from_f64_retain(price).unwrap_or_default();
                Some(price * balance)
            } else {
                None
            };

            amount += wallet_utils::parse_func::decimal_from_str(&assets.balance)?;
            account_total_assets =
                account_total_assets.map(|total| total + value.unwrap_or_default());
        }

        Ok(BalanceInfo {
            amount: wallet_utils::conversion::decimal_to_f64(&amount)?,
            currency: currency.to_string(),
            unit_price: Default::default(),
            fiat_value: account_total_assets
                .map(|total| wallet_utils::conversion::decimal_to_f64(&total))
                .transpose()?,
        })
    }

    // 泛型方法处理不同类型的资产计算
    async fn calculate_assets_generic<T, U>(
        &self,
        ctx: &'static crate::context::Context,
        data: T,
        existing_asset: &mut U,
    ) -> Result<(), crate::error::service::ServiceError>
    where
        T: AssetsEntityTrait,
        U: AccountChainAssetTrait,
    {
        let balance = wallet_utils::parse_func::decimal_from_str(data.balance())?;
        if balance.is_zero() {
            return Ok(());
        }
        let balance_f = wallet_utils::parse_func::f64_from_str(data.balance())?;

        let token_currency_id =
            TokenCurrencyId::new(data.symbol(), data.chain_code(), data.token_address());
        let (price, _fiat_balance) = if let Some(token_currency) = self.0.get(&token_currency_id) {
            let currency = ConfigDomain::get_currency(ctx).await?;
            let price = token_currency.get_price(&currency);
            let fiat_balance = Some(price * balance_f);
            (Some(price), fiat_balance)
        } else {
            (None, None)
        };

        let BalanceInfo { amount, currency: _, unit_price: _, fiat_value } =
            existing_asset.balance_mut();

        let after_balance = *amount + balance_f;
        *amount = after_balance;
        let fiat_balance = price.map(|p| p * after_balance);
        *fiat_value = fiat_balance;

        Ok(())
    }

    pub async fn calculate_assets(
        &self,
        data: wallet_database::entities::assets::AssetsEntity,
        existing_asset: &mut super::assets::AccountChainAsset,
        ctx: &'static crate::context::Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.calculate_assets_generic(ctx, data, existing_asset).await
    }

    pub async fn calculate_api_assets(
        &self,
        data: wallet_database::entities::api_assets::ApiAssetsEntity,
        existing_asset: &mut crate::response_vo::api_wallet::assets::ApiAccountChainAsset,
        ctx: &'static crate::context::Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.calculate_assets_generic(ctx, data, existing_asset).await
    }

    // 泛型方法处理不同类型的资产实体计算
    async fn calculate_any_assets_entity<T>(
        &self,
        ctx: &'static crate::context::Context,
        assets: &T,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError>
    where
        T: AssetsEntityTrait,
    {
        self.async_calculate_to_balance(
            ctx,
            assets.balance(),
            assets.symbol(),
            assets.chain_code(),
            assets.token_address(),
        )
        .await
    }

    pub async fn calculate_assets_entity(
        &self,
        assets: &wallet_database::entities::assets::AssetsEntity,
        ctx: &'static crate::context::Context,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        self.calculate_any_assets_entity(ctx, assets).await
    }

    pub async fn calculate_api_assets_entity(
        &self,
        assets: &wallet_database::entities::api_assets::ApiAssetsEntity,
        ctx: &'static crate::context::Context,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        self.calculate_any_assets_entity(ctx, assets).await
    }

    pub async fn calculate_account_infos(
        &self,
        ctx: &'static crate::context::Context,
        data: Vec<wallet_database::entities::account::AccountEntity>,
        chains: &ChainCodeAndName,
    ) -> Result<AccountInfos, crate::error::service::ServiceError> {
        let mut account_list =
            Vec::<crate::response_vo::standard_wallet::wallet::AccountInfo>::new();
        for account in data {
            // let btc_address_type_opt: AddressType = account.address_type().try_into()?;
            // let address_type = btc_address_type_opt.into();

            let address_type =
                AccountDomain::get_show_address_type(&account.chain_code, account.address_type())?;

            if let Some(info) =
                account_list.iter_mut().find(|info| info.account_id == account.account_id)
            {
                let name = chains.get(&account.chain_code);
                info.chain.push(crate::response_vo::standard_wallet::wallet::ChainInfo {
                    address: account.address,
                    wallet_address: account.wallet_address,
                    derivation_path: account.derivation_path,
                    chain_code: account.chain_code,
                    name: name.cloned(),
                    address_type,
                    created_at: account.created_at,
                    updated_at: account.updated_at,
                });
            } else {
                let name = chains.get(&account.chain_code);
                let account_index_map =
                    wallet_utils::address::AccountIndexMap::from_account_id(account.account_id)?;
                let balance = BalanceInfo::new_without_amount(ctx).await?;
                account_list.push(crate::response_vo::standard_wallet::wallet::AccountInfo {
                    account_id: account.account_id,
                    account_index_map,
                    name: account.name,
                    balance,
                    chain: vec![crate::response_vo::standard_wallet::wallet::ChainInfo {
                        address: account.address,
                        wallet_address: account.wallet_address,
                        derivation_path: account.derivation_path,
                        chain_code: account.chain_code,
                        name: name.cloned(),
                        address_type,
                        created_at: account.created_at,
                        updated_at: account.updated_at,
                    }],
                });
            }
        }
        Ok(AccountInfos(account_list))
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenPriceChangeRes {
    pub id: Option<String>,
    // 链码
    pub chain_code: String,
    // 代币编码
    #[serde(rename = "code", deserialize_with = "wallet_utils::serde_func::deserialize_uppercase")]
    pub symbol: String,
    // 默认代币
    pub default_token: Option<bool>,
    // 启用状态
    pub enable: bool,
    // 市值
    pub market_value: BalanceInfo,
    // 主币
    pub master: bool,
    // 代币名称
    pub name: Option<String>,
    // 单价(usdt)
    // pub price: f64,
    // 单价
    pub balance: BalanceInfo,
    // 波动
    pub price_percentage: Option<f64>,
    // 可以状态
    pub status: bool,
    // 代币合约地址
    pub token_address: Option<String>,
    // 24小时交易量
    pub day_change_amount: BalanceInfo,
    // 精度
    pub unit: Option<u8>,
    // 代币别名
    pub aname: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub decimals: u8,
}

impl From<(&TokenPriceChangeBody, BalanceInfo, BalanceInfo, BalanceInfo)> for TokenPriceChangeRes {
    fn from(
        (body, balance, market_value, day_change_amount): (
            &TokenPriceChangeBody,
            BalanceInfo,
            BalanceInfo,
            BalanceInfo,
        ),
    ) -> Self {
        Self {
            id: body.id.clone(),
            chain_code: body.chain_code.clone(),
            symbol: body.symbol.clone(),
            default_token: body.default_token,
            enable: body.enable,
            market_value,
            master: body.master,
            name: body.name.clone(),
            balance,
            price_percentage: body.price_percentage,
            status: body.status,
            token_address: body.token_address.clone(),
            unit: Some(body.unit),
            // price: body.price,
            day_change_amount,
            aname: body.aname.clone(),
        }
    }
}
