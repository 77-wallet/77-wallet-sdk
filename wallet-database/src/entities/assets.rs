#[derive(Debug)]
pub struct AssetsId {
    pub address: String,
    pub chain_code: String,
    pub symbol: String,
}

impl AssetsId {
    pub fn new(address: &str, chain_code: &str, symbol: &str) -> Self {
        Self {
            address: address.to_string(),
            chain_code: chain_code.to_string(),
            symbol: symbol.to_string(),
        }
    }
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct AssetsEntity {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub address: String,
    pub chain_code: String,
    pub token_address: String,
    pub protocol: Option<String>,
    pub status: u8,
    /// 0/普通资产 1/多签资产 2/待部署多签账户的普通资产
    pub is_multisig: i8,
    pub balance: String,
    #[serde(skip_serializing)]
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    #[serde(skip_serializing)]
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct AssetsEntityWithAddressType {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub address: String,
    pub chain_code: String,
    pub token_address: String,
    pub protocol: Option<String>,
    address_type: String,
    pub status: u8,
    pub is_multisig: i8,
    pub balance: String,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

impl AssetsEntityWithAddressType {
    pub fn address_type(&self) -> Option<String> {
        (!self.address_type.is_empty()).then(|| self.address_type.clone())
    }
}
