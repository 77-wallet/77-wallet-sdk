use std::{
    cmp::Ordering,
    collections::HashSet,
    ops::{Deref, DerefMut},
};

use alloy::primitives::map::HashMap;
use wallet_database::entities::assets::AssetsEntity;

use super::account::BalanceInfo;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountAssetsRes {
    pub account_total_assets: BalanceInfo,
}

// impl TryFrom<(TokenCurrency, &AssetsEntity)> for BalanceInfo {
//     type Error = crate::ServiceError;

//     fn try_from(
//         (token_currency, assets): (TokenCurrency, &AssetsEntity),
//     ) -> Result<Self, Self::Error> {
//         let balance = wallet_utils::parse_func::decimal_from_str(&assets.balance)?;
//         let config = crate::config::CONFIG.read().await;
//         let currency = config.currency();

//         let price = wallet_types::Decimal::from_f64_retain(token_currency.get_price(currency))
//             .unwrap_or_default();
//         let fiat_balance = price * balance;

//         Ok(BalanceInfo {
//             amount: wallet_utils::conversion::decimal_to_f64(&balance)?,
//             currency: currency.to_string(),
//             unit_price: Some(wallet_utils::conversion::decimal_to_f64(&price)?),
//             fiat_value: Some(wallet_utils::conversion::decimal_to_f64(&fiat_balance)?),
//         })
//     }
// }

// impl TryFrom<Vec<AssetsEntity>> for GetAccountAssetsRes {
//     type Error = crate::Error;

//     fn try_from(value: Vec<AssetsEntity>) -> Result<Self, Self::Error> {
//         let mut account_total_assets = wallet_types::Decimal::default();

//         for assets in value {
//             let AssetsEntity {
//                 name,
//                 symbol,
//                 decimals,
//                 address,
//                 chain_code,
//                 token_address,
//                 protocol,
//                 status,
//                 is_multisig,
//                 balance,
//                 created_at,
//                 updated_at,
//             } = assets;

//             let unit_price = crate::service_impl::asset::AssetService::get_current_coin_unit_price(
//                 &symbol,
//                 &chain_code,
//             );

//             let balance = wallet_utils::parse_func::decimal_from_str(&balance)?;

//             let value = unit_price * balance;
//             account_total_assets += value;
//         }

//         Ok(GetAccountAssetsRes {
//             account_total_assets,
//         })
//     }
// }

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChainAssetsRes {
    pub chain_assets: Vec<CoinAssets>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetCoinListRes {
    pub coin_list: Vec<CoinAssets>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinAssets {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub address: String,
    pub chain_code: String,
    pub token_address: String,
    pub status: u8,
    pub balance: BalanceInfo,
    pub is_multisig: i8,
    // pub balance: String,
    // pub _balance: String,
    // pub unit_price: String,
    // pub unit
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

// impl TryFrom<AssetsEntity> for CoinAssets {
//     type Error = crate::ServiceError;
//     fn try_from(value: AssetsEntity) -> Result<Self, Self::Error> {
//         let balance = wallet_utils::parse_func::decimal_from_str(&value.balance)?;
//         let config = crate::config::CONFIG.read().await;
//         let currency = config.currency();

//         Ok(Self {
//             name: value.name,
//             symbol: value.symbol,
//             decimals: value.decimals,
//             address: value.address,
//             chain_code: value.chain_code,
//             token_address: value.token_address,
//             status: value.status,
//             balance: BalanceInfo {
//                 amount: wallet_utils::conversion::decimal_to_f64(&balance)?,
//                 currency: currency.to_string(),
//                 unit_price: None,
//                 fiat_value: None,
//             },
//             created_at: value.created_at,
//             updated_at: value.updated_at,
//         })
//     }
// }

impl From<(BalanceInfo, AssetsEntity)> for CoinAssets {
    fn from((balance, value): (BalanceInfo, AssetsEntity)) -> Self {
        CoinAssets {
            name: value.name,
            symbol: value.symbol,
            decimals: value.decimals,
            address: value.address,
            chain_code: value.chain_code,
            token_address: value.token_address,
            status: value.status,
            balance,
            is_multisig: value.is_multisig,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountChainAsset {
    pub chain_code: String,
    pub symbol: String,
    pub name: String,
    pub balance: BalanceInfo,
    // pub usdt_balance: String,
    pub is_multichain: bool,
    pub is_multisig: i8,
    // pub chains: Vec<ChainAssets>,
}

#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountChainAssetList(pub Vec<AccountChainAsset>);

impl Deref for AccountChainAssetList {
    type Target = Vec<AccountChainAsset>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AccountChainAssetList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl AccountChainAssetList {
    // 标记多链资产的 is_multichain 属性
    pub(crate) fn mark_multichain_assets(&mut self) {
        // 使用 HashSet 来存储每个 symbol 对应的不同 chain_code，以避免重复
        let mut symbol_chain_map: HashMap<String, HashSet<String>> = HashMap::new();

        // 先填充 symbol_chain_map，每个 symbol 对应的 HashSet 包含不同的 chain_code
        for asset in self.iter() {
            symbol_chain_map
                .entry(asset.symbol.clone())
                .or_default()
                .insert(asset.chain_code.clone());
        }

        // 再次遍历 self，设置 is_multichain 标记
        for asset in self.iter_mut() {
            if let Some(chain_codes) = symbol_chain_map.get(&asset.symbol) {
                asset.is_multichain = chain_codes.len() > 1;
            }
        }
    }

    // 排序函数
    pub(crate) fn sort_account_chain_assets(&mut self) {
        self.sort_by(|a, b| {
            // 首先比较 fiat_value 从大到小
            match b.balance.fiat_value.partial_cmp(&a.balance.fiat_value) {
                Some(Ordering::Equal) => {
                    // 如果 fiat_value 相等，比较 unit_price 从大到小
                    match b.balance.unit_price.partial_cmp(&a.balance.unit_price) {
                        Some(Ordering::Equal) => {
                            // 如果 unit_price 也相等，按 chain_code 字母顺序
                            a.chain_code.cmp(&b.chain_code)
                        }
                        other_order => other_order.unwrap_or(Ordering::Equal),
                    }
                }
                other_order => other_order.unwrap_or(Ordering::Equal),
            }
        });
    }
}
