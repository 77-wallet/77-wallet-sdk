use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use super::account::BalanceInfo;
use wallet_database::entities::{api_chain::ApiChainEntity, chain::ChainEntity};

type ChainCode = String;
type ChainName = String;
#[derive(Debug, serde::Serialize)]
pub struct ChainCodeAndName(std::collections::HashMap<ChainCode, ChainName>);

impl std::ops::Deref for ChainCodeAndName {
    type Target = std::collections::HashMap<ChainCode, ChainName>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for ChainCodeAndName {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec<ChainEntity>> for ChainCodeAndName {
    fn from(chain: Vec<ChainEntity>) -> Self {
        let mut res = std::collections::HashMap::new();
        for chain in chain {
            res.insert(chain.chain_code, chain.name);
        }
        Self(res)
    }
}

impl From<Vec<ApiChainEntity>> for ChainCodeAndName {
    fn from(chain: Vec<ApiChainEntity>) -> Self {
        let mut res = std::collections::HashMap::new();
        for chain in chain {
            res.insert(chain.chain_code, chain.name);
        }
        Self(res)
    }
}

#[derive(Debug, serde::Serialize)]
pub struct TokenInfo {
    pub token_name: String,
    pub total_assets: wallet_types::Decimal,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainAssets {
    pub chain_code: String,
    pub name: String,
    pub symbol: String,
    pub address: String,
    pub token_address: String,
    // pub address_catogary: BtcAddressCategoryOpt,
    pub balance: BalanceInfo,
    /// 0/普通资产 1/多签资产 2/待部署多签账户的普通资产
    pub is_multisig: i8,
    // pub is_multichain: bool,
    pub asset_quantity_ratio: f64,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDynData {
    pub chain_code: String,
    pub node_id: String,
    pub name: String,
    pub delay: u64,
    pub block_height: i64,
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NodeListRes {
    pub node_id: String,
    pub name: String,
    pub chain_code: String,
    pub rpc_url: String,
    pub ws_url: String,
    pub status: u8,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct ChainList(pub HashMap<String, String>);

impl ChainList {}

impl Deref for ChainList {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ChainList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
