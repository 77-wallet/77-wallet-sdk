#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct CoinData {
    pub name: Option<String>,
    pub symbol: String,
    pub chain_code: String,
    pub token_address: Option<String>,
    pub price: Option<String>,
    pub protocol: Option<String>,
    pub decimals: u8,
    pub is_default: u8,
    pub is_popular: u8,
    pub is_custom: u8,
}

impl CoinData {
    pub fn new(
        name: Option<String>,
        symbol: &str,
        chain_code: &str,
        token_address: Option<String>,
        price: Option<String>,
        protocol: Option<String>,
        decimals: u8,
        is_default: u8,
        is_popular: u8,
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
        }
    }

    pub fn with_custom(mut self, is_custom: u8) -> Self {
        self.is_custom = is_custom;
        self
    }

    pub fn token_address(&self) -> Option<String> {
        match &self.token_address {
            Some(token_address) => {
                if token_address.is_empty() {
                    None
                } else {
                    Some(token_address.clone())
                }
            }
            None => None,
        }
    }
}

#[derive(Debug)]
pub struct CoinId {
    pub chain_code: String,
    pub symbol: String,
    pub token_address: Option<String>,
}

impl CoinId {
    pub fn new(chain_code: &str, symbol: &str, token_address: Option<String>) -> Self {
        Self {
            chain_code: chain_code.to_string(),
            symbol: symbol.to_string(),
            token_address,
        }
    }
}

#[derive(Debug)]
pub struct SymbolId {
    pub chain_code: String,
    pub symbol: String,
}

impl SymbolId {
    pub fn new(chain_code: &str, symbol: &str) -> Self {
        Self {
            chain_code: chain_code.to_string(),
            symbol: symbol.to_string(),
        }
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
    pub token_address: Option<String>,
    pub price: String,
    pub protocol: Option<String>,
    pub decimals: u8,
    pub is_default: u8,
    pub is_popular: u8,
    pub status: u8,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
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
