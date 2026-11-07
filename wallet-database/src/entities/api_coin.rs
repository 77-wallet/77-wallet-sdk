use crate::entities::asset_token_key::AssetTokenKey;
use chrono::{DateTime, Utc};

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct ApiCoinData {
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
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl ApiCoinData {
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
        created_at: DateTime<Utc>,
        updated_at: Option<DateTime<Utc>>,
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

#[derive(Debug, Clone, Default, serde::Serialize, sqlx::FromRow)]
pub struct ApiCoinEntity {
    pub id: i64,
    pub name: String,
    pub symbol: String,
    pub chain_code: String,
    pub token_address: AssetTokenKey,
    pub price: String,
    pub protocol: Option<String>,
    pub decimals: u8,
    pub is_default: u8,
    pub is_popular: u8,
    pub is_custom: u8,
    pub status: u8,
    // // 是否支持兑换
    // pub swappable: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl ApiCoinEntity {
    pub fn token_key(&self) -> AssetTokenKey {
        self.token_address.clone()
    }
}
