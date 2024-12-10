#[derive(Debug, Default, serde::Serialize, sqlx::FromRow, wallet_macro::macros ::Resource)]
#[serde(rename_all = "camelCase")]
#[resource(
    // schema_name = "wallet",
    query_req = "crate::entities::node::QueryReq",
    sqlite_table_name = "node",
    // primary_key = "address:String, chain_code: String",
    // constraint = "account_address_chain_code_idx"
)]
pub struct NodeEntity {
    #[resource(detail = "QueryReq")]
    pub node_id: String,
    pub name: String,
    pub chain_code: String,
    pub rpc_url: String,
    pub ws_url: String,
    pub http_url: String,
    pub network: String,
    pub status: u8,
    #[serde(skip_serializing)]
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    #[serde(skip_serializing)]
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct NodeCreateVo {
    pub name: String,
    pub chain_code: String,
    pub rpc_url: String,
    pub ws_url: String,
    pub http_url: String,
    pub network: String,
    pub status: u8,
}

pub struct QueryReq {
    pub node_id: Option<String>,
}

impl QueryReq {
    pub fn new(node_id: &str) -> Self {
        Self {
            node_id: Some(node_id.to_string()),
        }
    }
}
