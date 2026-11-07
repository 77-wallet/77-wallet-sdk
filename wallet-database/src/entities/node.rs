#[derive(Debug, Default, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]

pub struct NodeEntity {
    pub node_id: String,
    pub name: String,
    pub chain_code: String,
    pub rpc_url: String,
    pub ws_url: String,
    pub http_url: String,
    pub network: String,
    pub status: u8,
    pub is_local: u8,
    #[serde(skip_serializing, skip_deserializing)]
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    #[serde(skip_serializing, skip_deserializing)]
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct NodeCreateVo {
    pub node_id: String,
    pub name: String,
    pub chain_code: String,
    pub rpc_url: String,
    pub ws_url: String,
    pub http_url: String,
    pub network: String,
    pub status: u8,
    pub is_local: u8,
}
