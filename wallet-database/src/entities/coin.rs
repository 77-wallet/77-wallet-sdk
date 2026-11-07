use crate::entities::asset_token_key::AssetTokenKey;
use chrono::{DateTime, Utc};

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct CoinData {
    pub name: Option<String>,
    pub symbol: String,
    pub chain_code: String,
    pub token_address: AssetTokenKey,
    pub price: Option<String>,
    pub protocol: Option<String>,
    pub decimals: u8,
    pub is_default: u8,
    pub is_popular: u8,
    pub is_custom: u8,
    pub status: u8,
    // 是否支持兑换
    pub swappable: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl CoinData {
    pub fn new(
        name: Option<String>,
        symbol: &str,
        chain_code: &str,
        token_address: AssetTokenKey,
        price: Option<String>,
        protocol: Option<String>,
        decimals: u8,
        is_default: u8,
        is_popular: u8,
        status: u8,
        swappable: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            name,
            symbol: symbol.to_string(),
            chain_code: chain_code.to_string(),
            token_address,
            price,
            protocol,
            decimals,
            is_default,
            is_popular,
            is_custom: 0,
            status,
            swappable,
            created_at,
            updated_at,
        }
    }

    pub fn with_custom(mut self, is_custom: u8) -> Self {
        self.is_custom = is_custom;
        self
    }

    pub fn with_status(mut self, status: u8) -> Self {
        self.status = status;
        self
    }

    pub fn token_key(&self) -> AssetTokenKey {
        self.token_address.clone()
    }
}

#[derive(Debug)]
pub struct CoinId {
    pub chain_code: String,
    pub symbol: String,
    pub token_address: AssetTokenKey,
}

impl CoinId {
    pub fn new(chain_code: &str, symbol: &str, token_address: AssetTokenKey) -> Self {
        Self { chain_code: chain_code.to_string(), symbol: symbol.to_string(), token_address }
    }

    pub fn from_token_key(chain_code: &str, symbol: &str, token_address: AssetTokenKey) -> Self {
        Self { chain_code: chain_code.to_string(), symbol: symbol.to_string(), token_address }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolId {
    pub chain_code: String,
    pub symbol: String,
}

impl SymbolId {
    pub fn new(chain_code: &str, symbol: &str) -> Self {
        Self { chain_code: chain_code.to_string(), symbol: symbol.to_string() }
    }
}

pub struct MainCoin {
    pub symbol: String,
    pub decimal: u8,
}
#[derive(Debug, Default, serde::Serialize, sqlx::FromRow, Eq, PartialEq, Hash)]
pub struct CoinEntity {
    pub name: String,
    pub symbol: String,
    pub chain_code: String,
    pub token_address: AssetTokenKey,
    pub price: String,
    pub protocol: Option<String>,
    pub decimals: u8,
    pub is_default: u8,
    pub is_popular: u8,
    pub status: u8,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

impl CoinEntity {
    pub fn token_key(&self) -> AssetTokenKey {
        self.token_address.clone()
    }
}

#[derive(Debug, PartialEq)]
pub enum CoinMultisigStatus {
    /// The account is not a multisig account.
    NotMultisig,
    /// The account is a multisig account.
    IsMultisig,
    /// The account is in the process of being deployed.
    Deploying,
}
impl CoinMultisigStatus {
    pub fn to_i8(&self) -> i8 {
        match self {
            CoinMultisigStatus::NotMultisig => 0,
            CoinMultisigStatus::IsMultisig => 1,
            CoinMultisigStatus::Deploying => 2,
        }
    }
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct CoinWithAssets {
    pub symbol: String,
    pub chain_code: String,
    pub token_address: String,
    pub balance: String,
    pub decimals: u32,
    pub name: String,
    pub price: String,
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct BatchCoinSwappable {
    pub symbol: String,
    pub chain_code: String,
    pub token_address: String,
}
