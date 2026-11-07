use crate::entities::asset_token_key::AssetTokenKey;
use sqlx::types::chrono::{DateTime, Utc};

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiAssetSyncStateEntity {
    pub address: String,
    pub chain_code: String,
    pub token_address: AssetTokenKey,
    pub status: ApiAssetSyncStatus,
    pub attempt_count: i64,
    pub last_error: Option<String>,
    #[serde(skip_serializing)]
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
    sqlx::Type,
)]
#[repr(u8)]
pub enum ApiAssetSyncStatus {
    Pending = 0,
    Running = 1,
    Done = 2,
    Failed = 3,
}
