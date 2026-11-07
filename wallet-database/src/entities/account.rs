use wallet_utils::address::AccountIndexMap;

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountEntity {
    pub account_id: u32,
    pub address: String,
    pub pubkey: String,
    address_type: String,
    pub wallet_address: String,
    pub derivation_path: String,
    pub chain_code: String,
    pub name: String,
    // 1 启用
    pub status: u8,
    pub is_init: u16,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

impl AccountEntity {
    pub fn address_type(&self) -> Option<String> {
        (!self.address_type.is_empty()).then(|| self.address_type.clone())
    }
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct CreateAccountVo {
    pub account_id: u32,
    pub address: String,
    pub pubkey: String,
    pub address_type: String,
    pub wallet_address: String,
    pub derivation_path: String,
    pub chain_code: String,
    pub name: String,
}

impl CreateAccountVo {
    pub fn new(
        account_id: u32,
        address: &str,
        pubkey: &str,
        wallet_address: &str,
        derivation_path: &str,
        chain_code: &str,
        name: &str,
    ) -> Self {
        Self {
            account_id,
            address: address.to_string(),
            pubkey: pubkey.to_string(),
            address_type: "".to_string(),
            wallet_address: wallet_address.to_string(),
            derivation_path: derivation_path.to_string(),
            chain_code: chain_code.to_string(),
            name: name.to_string(),
        }
    }

    pub fn with_address_type(mut self, address_type: &str) -> Self {
        self.address_type = address_type.to_string();
        self
    }
}

pub struct QueryReq {
    pub wallet_address: Option<String>,
    pub address: Option<String>,
    pub chain_code: Option<String>,
    pub account_id: Option<u32>,
    pub status: Option<u8>,
}

impl QueryReq {
    pub fn new_address_chain(address: &str, chain: &str) -> Self {
        Self {
            wallet_address: None,
            address: Some(address.to_string()),
            chain_code: Some(chain.to_string()),
            account_id: None,
            status: Some(1),
        }
    }
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountWalletMapping {
    pub account_id: u32,
    #[sqlx(rename = "name")]
    pub account_name: String,
    pub wallet_address: String,
    pub uid: String,
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountWithWalletEntity {
    pub account_id: u32,
    // 账户名称
    pub name: String,
    pub address: String,
    pub address_type: String,
    pub wallet_address: String,
    pub wallet_name: String,
    pub uid: String,
}

impl AccountWithWalletEntity {
    pub fn get_index(&self) -> Result<i32, crate::Error> {
        let account_index_map = AccountIndexMap::from_account_id(self.account_id)?;

        Ok(account_index_map.input_index)
    }
}
