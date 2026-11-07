use std::fmt;

use crate::entities::api_wallet::ApiWalletType;

#[derive(serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApiAccountEntity {
    pub id: i64,
    pub account_id: u32,
    pub name: String,
    pub address: String,
    pub pubkey: Option<String>,
    pub address_type: String,
    pub wallet_address: String,
    pub uid: String,
    pub derivation_path: String,
    pub derivation_path_index: i32,
    pub chain_code: String,
    pub api_wallet_type: ApiWalletType,
    pub status: i32,
    pub is_init: i32,
    pub is_expand: i32,
    pub is_used: bool,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

/// Minimal persisted identity needed for device unbinding and guarded deletion.
#[derive(Debug, sqlx::FromRow)]
pub struct ApiAccountDeletionRow {
    pub id: i64,
    pub chain_code: String,
    pub address: String,
}

/// Lightweight row used to traverse a wallet's active addresses without
/// materializing every account column in memory.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ApiAccountAddressPageRow {
    pub id: i64,
    pub address: String,
}

impl fmt::Debug for ApiAccountEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiAccountEntity")
            .field("id", &self.id)
            .field("account_id", &self.account_id)
            .field("name", &self.name)
            .field("address", &self.address)
            .field("pubkey", &self.pubkey)
            .field("address_type", &self.address_type)
            .field("wallet_address", &self.wallet_address)
            .field("uid", &self.uid)
            .field("derivation_path", &self.derivation_path)
            .field("derivation_path_index", &self.derivation_path_index)
            .field("chain_code", &self.chain_code)
            .field("api_wallet_type", &self.api_wallet_type)
            .field("status", &self.status)
            .field("is_init", &self.is_init)
            .field("is_expand", &self.is_expand)
            .field("is_used", &self.is_used)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

impl ApiAccountEntity {
    pub fn address_type(&self) -> Option<String> {
        (!self.address_type.is_empty()).then(|| self.address_type.clone())
    }
}

#[derive(Debug, serde::Serialize, sqlx::FromRow, Clone)]
pub struct CreateApiAccountVo {
    pub account_id: u32,
    pub address: String,
    pub pubkey: String,
    pub address_type: String,
    pub wallet_address: String,
    pub uid: String,
    pub derivation_path: String,
    pub derivation_path_index: i32,
    pub chain_code: String,
    pub name: String,
    pub api_wallet_type: ApiWalletType,
    pub is_init: i32,
}

impl CreateApiAccountVo {
    pub fn new(
        account_id: u32,
        address: &str,
        pubkey: &str,
        wallet_address: &str,
        uid: &str,
        derivation_path: &str,
        derivation_path_index: i32,
        chain_code: &str,
        name: &str,
        api_wallet_type: ApiWalletType,
    ) -> Self {
        Self {
            account_id,
            address: address.to_string(),
            pubkey: pubkey.to_string(),
            address_type: "".to_string(),
            wallet_address: wallet_address.to_string(),
            uid: uid.to_string(),
            derivation_path: derivation_path.to_string(),
            derivation_path_index,
            chain_code: chain_code.to_string(),
            name: name.to_string(),
            api_wallet_type,
            is_init: 0,
        }
    }

    pub fn with_address_type(mut self, address_type: &str) -> Self {
        self.address_type = address_type.to_string();
        self
    }

    pub fn with_is_init(mut self, is_init: bool) -> Self {
        self.is_init = is_init as i32;
        self
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountToWalletAddress {
    pub address: String,
    pub wallet_address: String,
}

#[derive(serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApiAccountWalletMapping {
    pub account_id: u32,
    #[sqlx(rename = "name")]
    pub account_name: String,
    pub address: String,
    pub wallet_address: String,
    pub seed: String,
    pub uid: String,
    pub api_wallet_type: ApiWalletType,
}

impl fmt::Debug for ApiAccountWalletMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiAccountWalletMapping")
            .field("account_id", &self.account_id)
            .field("account_name", &self.account_name)
            .field("address", &self.address)
            .field("wallet_address", &self.wallet_address)
            .field("seed", &"<redacted>")
            .field("uid", &self.uid)
            .field("api_wallet_type", &self.api_wallet_type)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiAccountEntity, ApiAccountWalletMapping, ApiWalletType};
    use sqlx::types::chrono::{TimeZone, Utc};

    #[test]
    fn api_account_entity_debug_redacts_seed_like_fields() {
        let req = ApiAccountEntity {
            id: 1,
            account_id: 2,
            name: "name".to_string(),
            address: "addr".to_string(),
            pubkey: Some("pubkey".to_string()),
            address_type: "type".to_string(),
            wallet_address: "wallet".to_string(),
            uid: "uid".to_string(),
            derivation_path: "m/44'/60'/0'/0/0".to_string(),
            derivation_path_index: 0,
            chain_code: "eth".to_string(),
            api_wallet_type: ApiWalletType::SubAccount,
            status: 1,
            is_init: 1,
            is_expand: 0,
            is_used: false,
            created_at: Utc.timestamp_opt(0, 0).single().unwrap(),
            updated_at: None,
        };

        let debug = format!("{req:?}");
        assert!(!debug.contains("seed bytes"));
        assert!(!debug.contains("phrase words"));
        assert!(debug.contains("pubkey"));
    }

    #[test]
    fn api_account_wallet_mapping_debug_redacts_seed() {
        let req = ApiAccountWalletMapping {
            account_id: 1,
            account_name: "name".to_string(),
            address: "addr".to_string(),
            wallet_address: "wallet".to_string(),
            seed: "seed bytes".to_string(),
            uid: "uid".to_string(),
            api_wallet_type: ApiWalletType::Withdrawal,
        };

        let debug = format!("{req:?}");
        assert!(!debug.contains("seed bytes"));
        assert!(debug.contains("<redacted>"));
    }
}
