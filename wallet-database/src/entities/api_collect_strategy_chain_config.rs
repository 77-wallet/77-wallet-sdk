#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ApiCollectStrategyChainConfigEntity {
    pub id: i64,
    pub strategy_id: i64,
    pub chain_code: String,
    pub chain_address_type: Option<String>,
    pub normal_idx: Option<i32>,
    pub normal_address: String,
    pub risk_idx: Option<i32>,
    pub risk_address: String,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}
