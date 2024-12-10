#[derive(Debug, Default, serde::Serialize, sqlx::FromRow, wallet_macro::macros ::Resource)]
#[serde(rename_all = "camelCase")]
#[resource(
    query_req = "crate::entities::account::QueryReq",
    sqlite_table_name = "account"
)]
pub struct AccountEntity {
    #[resource(detail = "QueryReq")]
    pub account_id: u32,
    #[resource(detail = "QueryReq")]
    pub address: String,
    pub pubkey: String,
    address_type: String,
    #[resource(detail = "QueryReq")]
    pub wallet_address: String,
    pub derivation_path: String,
    #[resource(detail = "QueryReq")]
    pub chain_code: String,
    pub name: String,
    #[resource(detail = "QueryReq")]
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
        pubkey: String,
        wallet_address: String,
        derivation_path: String,
        chain_code: String,
        name: &str,
    ) -> Self {
        Self {
            account_id,
            address: address.to_string(),
            pubkey,
            address_type: "".to_string(),
            wallet_address,
            derivation_path,
            chain_code,
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
