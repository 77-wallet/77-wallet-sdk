use std::{
    collections::HashSet,
    ops::{Deref, DerefMut},
};
use wallet_database::entities::chain::ChainEntity;
use wallet_transport_backend::response_vo::coin::TokenCurrency;
use wallet_types::{
    chain::address::{category::AddressCategory, r#type::AddressType},
    valueobject::TokenPriceChangeBody,
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
    pub chain_list: HashSet<ChainInfo>,
    pub is_multichain: bool,
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
impl CoinInfoList {
    // 标记多链资产的 is_multichain 属性
    pub(crate) fn mark_multichain_assets(&mut self) {
        // 使用 HashSet 来存储每个 symbol 对应的不同 chain_code，以避免重复
        let mut symbol_chain_map: std::collections::HashMap<String, HashSet<String>> =
            std::collections::HashMap::new();

        // 先填充 symbol_chain_map，每个 symbol 对应的 HashSet 包含不同的 chain_code
        for asset in self.iter() {
            for chain_info in asset.chain_list.iter() {
                symbol_chain_map
                    .entry(asset.symbol.clone())
                    .or_default()
                    .insert(chain_info.chain_code.clone());
            }
        }

        // 再次遍历 self，设置 is_multichain 标记
        for asset in self.iter_mut() {
            if let Some(chain_codes) = symbol_chain_map.get(&asset.symbol) {
                asset.is_multichain = chain_codes.len() > 1;
            }
        }
    }
}

#[derive(Debug, serde::Deserialize, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainInfo {
    pub chain_code: String,
    pub token_address: Option<String>,
    pub protocol: Option<String>,
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

// impl TryFrom<(TokenCurrency, TokenHistoryPrice)> for QueryHistoryPrice {
//     type Error = crate::ServiceError;

//     fn try_from(
//         (token_currency, assets): (TokenCurrency, TokenHistoryPrice),
//     ) -> Result<Self, Self::Error> {
//         let balance = assets.price;
//         let config = crate::config::CONFIG.read().await;
//         let currency = config.currency();

//         let price = wallet_types::Decimal::from_f64_retain(token_currency.get_price(currency))
//             .unwrap_or_default();
//         let fiat_balance = price * balance;

//         Ok(QueryHistoryPrice {
//             date: assets.date,
//             price: BalanceInfo {
//                 amount: wallet_utils::conversion::decimal_to_f64(&balance)?,
//                 currency: currency.to_string(),
//                 unit_price: Some(wallet_utils::conversion::decimal_to_f64(&price)?),
//                 fiat_value: Some(wallet_utils::conversion::decimal_to_f64(&fiat_balance)?),
//             },
//         })
//     }
// }

// impl From<Vec<ExchangeRateEntity>> for TokenCurrencies {
//     fn from(value: Vec<ExchangeRateEntity>) -> Self {
//         let mut map = std::collections::HashMap::new();
//         for entity in value {
//             let symbol = entity.symbol.to_ascii_lowercase();
//             let name = entity.name;
//             let token_currency = TokenCurrency {
//                 name,
//                 chain_code: entity.chain_code,
//                 code: entity.symbol,
//                 price: entity.price.parse::<f64>().unwrap_or_default(),
//                 currency_price: entity.currency.parse::<f64>().unwrap_or_default(),
//             };
//             map.insert(symbol, token_currency);
//         }
//         TokenCurrencies(map)
//     }
// }

#[derive(Debug, serde::Serialize, PartialEq, Eq, Hash)]
pub struct TokenCurrencyId {
    pub symbol: String,
    pub chain_code: String,
}

impl TokenCurrencyId {
    pub fn new(symbol: &str, chain_code: &str) -> Self {
        Self {
            symbol: symbol.to_ascii_lowercase(),
            chain_code: chain_code.to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
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

impl TokenCurrencies {
    pub async fn calculate_token_price_changes(
        data: TokenPriceChangeBody,
        exchange_rate: f64,
    ) -> Result<TokenPriceChangeRes, crate::ServiceError> {
        // let market_value = wallet_utils::conversion::decimal_from_f64(data.market_value)?;
        // let day_change_amount =
        //     wallet_utils::conversion::decimal_from_f64(data.day_change_amount.unwrap_or_default())?;
        let balance = Self::calculate(exchange_rate, data.price).await?;
        let market_value = Self::calculate(exchange_rate, data.market_value).await?;
        let day_change_amount =
            Self::calculate(exchange_rate, data.day_change_amount.unwrap_or_default()).await?;
        Ok((data, balance, market_value, day_change_amount).into())
    }

    pub async fn calculate(
        exchange_rate: f64,
        value: f64,
    ) -> Result<BalanceInfo, crate::ServiceError> {
        let config = crate::app_state::APP_STATE.read().await;
        let currency = config.currency();
        let unit_price = value * exchange_rate;

        Ok(BalanceInfo {
            amount: Default::default(),
            currency: currency.to_string(),
            unit_price: Some(unit_price),
            fiat_value: Default::default(),
        })
    }

    pub async fn calculate_chain_assets_list(
        &self,
        datas: Vec<wallet_database::entities::assets::AssetsEntityWithAddressType>,
        chains: Vec<ChainEntity>,
    ) -> Result<Vec<ChainAssets>, crate::ServiceError> {
        let mut res = Vec::new();

        for assets in datas {
            if let Some(chain) = chains
                .iter()
                .find(|chain| chain.chain_code == assets.chain_code)
            {
                let balance = self
                    .calculate_to_balance(&assets.balance, &assets.symbol, &assets.chain_code)
                    .await?;

                let btc_address_type_opt: AddressType = assets.address_type().try_into()?;
                let address_catogary = btc_address_type_opt.into();

                let name = if assets.chain_code == "btc"
                    && let AddressCategory::Btc(address_catogary) = address_catogary
                {
                    address_catogary.to_string()
                } else {
                    chain.name.clone()
                };

                res.push(crate::response_vo::chain::ChainAssets {
                    chain_code: assets.chain_code,
                    name,
                    address: assets.address,
                    token_address: assets.token_address,
                    // address_catogary,
                    balance,
                    symbol: assets.symbol,
                    is_multisig: assets.is_multisig,
                    // is_multichain: false,
                })
            }
        }
        Ok(res)
    }

    // pub async fn calculate_token_price_change(
    //     &self,
    //     data: TokenPriceChangeBody,
    // ) -> Result<TokenPriceChangeRes, crate::ServiceError> {
    //     let balance = self
    //         .calculate_to_balance(
    //             &wallet_types::Decimal::default().to_string(),
    //             &data.symbol,
    //             &data.chain_code,
    //         )
    //         .await?;
    //     Ok((data, balance).into())
    // }

    pub async fn calculate_to_balance(
        &self,
        balance: &str,
        symbol: &str,
        chain_code: &str,
    ) -> Result<BalanceInfo, crate::ServiceError> {
        let balance = wallet_utils::parse_func::decimal_from_str(balance)?;

        let config = crate::app_state::APP_STATE.read().await;
        let currency = config.currency();
        let token_currency_id = TokenCurrencyId::new(symbol, chain_code);

        let (price, fiat_balance) = if let Some(token_currency) = self.0.get(&token_currency_id) {
            let price = token_currency
                .get_price(currency)
                .and_then(wallet_types::Decimal::from_f64_retain);

            let fiat_balance = price.map(|p| p * balance);
            (price, fiat_balance)
        } else {
            (None, None)
        };

        Ok(BalanceInfo {
            amount: wallet_utils::conversion::decimal_to_f64(&balance)?,
            currency: currency.to_string(),
            unit_price: price
                .map(|p| wallet_utils::conversion::decimal_to_f64(&p))
                .transpose()?,
            fiat_value: fiat_balance
                .map(|p| wallet_utils::conversion::decimal_to_f64(&p))
                .transpose()?,
        })
    }

    pub async fn calculate_account_total_assets(
        &self,
        data: &mut [wallet_database::entities::assets::AssetsEntity],
    ) -> Result<BalanceInfo, crate::ServiceError> {
        let mut account_total_assets = Some(wallet_types::Decimal::default());
        let mut amount = wallet_types::Decimal::default();
        let config = crate::app_state::APP_STATE.read().await;
        let currency = config.currency();
        for assets in data.iter_mut() {
            let token_currency_id = TokenCurrencyId::new(&assets.symbol, &assets.chain_code);
            // let value = if let Some(token_currency) = self.0.get(&token_currency_id) {
            //     let balance = wallet_utils::parse_func::decimal_from_str(&assets.balance)?;

            //     let price =
            //         wallet_types::Decimal::from_f64_retain(token_currency.get_price(currency))
            //             .unwrap_or_default();
            //     price * balance
            // } else {
            //     wallet_types::Decimal::default()
            // };

            let value = if let Some(token_currency) = self.0.get(&token_currency_id) {
                let balance = wallet_utils::parse_func::decimal_from_str(&assets.balance)?;
                let price = token_currency.get_price(currency);
                if let Some(price) = price {
                    let price = wallet_types::Decimal::from_f64_retain(price).unwrap_or_default();
                    Some(price * balance)
                } else {
                    None
                }
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

    pub async fn calculate_assets(
        &self,
        data: wallet_database::entities::assets::AssetsEntity,
        existing_asset: &mut super::assets::AccountChainAsset,
    ) -> Result<(), crate::ServiceError> {
        let balance = wallet_utils::parse_func::decimal_from_str(&data.balance)?;
        if balance.is_zero() {
            return Ok(());
        }
        let balance_f = wallet_utils::parse_func::f64_from_str(&data.balance)?;

        let token_currency_id = TokenCurrencyId::new(&data.symbol, &data.chain_code);
        let (price, _fiat_balance) = if let Some(token_currency) = self.0.get(&token_currency_id) {
            let config = crate::app_state::APP_STATE.read().await;
            let currency = config.currency();

            let price = token_currency.get_price(currency);
            let fiat_balance = price.map(|p| p * balance_f);
            (price, fiat_balance)
        } else {
            (None, None)
        };

        let BalanceInfo {
            amount,
            currency: _,
            unit_price: _,
            fiat_value,
        } = &mut existing_asset.balance;

        let after_balance = *amount + balance_f;
        *amount = after_balance;
        let fiat_balance = price.map(|p| p * after_balance);
        *fiat_value = fiat_balance;

        // existing_asset.usdt_balance = (after_balance * unit_price).to_string();
        // FIXME: btc 的资产是 非multisig 的，需要特殊处理
        // existing_asset.is_multichain = true;

        Ok(())
    }

    pub async fn calculate_assets_entity(
        &self,
        assets: &wallet_database::entities::assets::AssetsEntity,
    ) -> Result<BalanceInfo, crate::ServiceError> {
        self.calculate_to_balance(&assets.balance, &assets.symbol, &assets.chain_code)
            .await
    }

    pub async fn calculate_account_infos(
        &self,
        data: Vec<wallet_database::entities::account::AccountEntity>,
        chains: &ChainCodeAndName,
    ) -> Result<AccountInfos, crate::ServiceError> {
        let mut account_list = Vec::<crate::response_vo::wallet::AccountInfo>::new();

        for account in data {
            let btc_address_type_opt: AddressType = account.address_type().try_into()?;
            let address_type = btc_address_type_opt.into();

            if let Some(info) = account_list
                .iter_mut()
                .find(|info| info.account_id == account.account_id)
            {
                let name = chains.get(&account.chain_code);
                info.chain.push(crate::response_vo::wallet::ChainInfo {
                    address: account.address,
                    wallet_address: account.wallet_address,
                    derivation_path: account.derivation_path,
                    chain_code: account.chain_code,
                    name: name.cloned(),
                    address_type,
                    created_at: account.created_at,
                    updated_at: account.updated_at,
                })
            } else {
                let name = chains.get(&account.chain_code);
                let account_index_map =
                    wallet_utils::address::AccountIndexMap::from_account_id(account.account_id)?;
                account_list.push(crate::response_vo::wallet::AccountInfo {
                    account_id: account.account_id,
                    account_index_map,
                    name: account.name,
                    balance: BalanceInfo::new_without_amount().await?,
                    chain: vec![crate::response_vo::wallet::ChainInfo {
                        address: account.address,
                        wallet_address: account.wallet_address,
                        derivation_path: account.derivation_path,
                        chain_code: account.chain_code,
                        name: name.cloned(),
                        address_type,
                        created_at: account.created_at,
                        updated_at: account.updated_at,
                    }],
                })
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
    #[serde(
        rename = "code",
        deserialize_with = "wallet_utils::serde_func::deserialize_uppercase"
    )]
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
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub decimals: u8,
}

impl From<(TokenPriceChangeBody, BalanceInfo, BalanceInfo, BalanceInfo)> for TokenPriceChangeRes {
    fn from(
        (body, balance, market_value, day_change_amount): (
            TokenPriceChangeBody,
            BalanceInfo,
            BalanceInfo,
            BalanceInfo,
        ),
    ) -> Self {
        Self {
            id: body.id,
            chain_code: body.chain_code,
            symbol: body.symbol,
            default_token: body.default_token,
            enable: body.enable,
            market_value,
            master: body.master,
            name: body.name,
            balance,
            price_percentage: body.price_percentage,
            status: body.status,
            token_address: body.token_address,
            unit: body.unit,
            // price: body.price,
            day_change_amount,
        }
    }
}
