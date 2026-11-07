use crate::entities::asset_token_key::AssetTokenKey;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetsId {
    pub address: String,
    pub chain_code: String,
    pub token_address: AssetTokenKey,
}

impl AssetsId {
    pub fn new(address: &str, chain_code: &str, token_address: AssetTokenKey) -> Self {
        Self { address: address.to_string(), chain_code: chain_code.to_string(), token_address }
    }

    pub fn from_token_key(address: &str, chain_code: &str, token_address: AssetTokenKey) -> Self {
        Self { address: address.to_string(), chain_code: chain_code.to_string(), token_address }
    }
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct AssetsEntity {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub address: String,
    pub chain_code: String,
    pub token_address: AssetTokenKey,
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
impl AssetsEntity {
    pub fn get_assets_id(&self) -> AssetsId {
        AssetsId {
            address: self.address.clone(),
            chain_code: self.chain_code.clone(),
            token_address: self.token_key(),
        }
    }

    pub fn token_key(&self) -> AssetTokenKey {
        self.token_address.clone()
    }
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct AssetsEntityWithAddressType {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub address: String,
    pub chain_code: String,
    pub token_address: AssetTokenKey,
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

    pub fn token_key(&self) -> AssetTokenKey {
        self.token_address.clone()
    }
}
