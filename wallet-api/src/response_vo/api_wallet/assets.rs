use std::{
    cmp::Ordering,
    ops::{Deref, DerefMut},
};

use crate::response_vo::standard_wallet::{account::BalanceInfo, chain::ChainList};

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiAccountChainAsset {
    pub chain_code: String,
    pub symbol: String,
    pub name: String,
    /// key: chainCode, value: tokenAddress
    pub chain_list: ChainList,
    pub balance: BalanceInfo,
    // pub is_multichain: bool,
    pub is_multisig: i8,
    pub is_default: bool,
}

#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApiAccountChainAssetList(pub Vec<ApiAccountChainAsset>);

impl Deref for ApiAccountChainAssetList {
    type Target = Vec<ApiAccountChainAsset>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ApiAccountChainAssetList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ApiAccountChainAssetList {
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
