use serde::{Deserialize, Serialize};
use sqlx::types::chrono::Utc;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AssetQueryStateEntity {
    pub uid: String,
    pub chain_code: String,
    pub page: i64,
    pub status: AssetQueryStatus,
    pub index_list_json: String,
    pub attempt_count: i64,
    pub last_error: Option<String>,
    #[serde(skip_serializing)]
    pub created_at: chrono::DateTime<Utc>,
    #[serde(skip_serializing)]
    pub updated_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct CreateAssetQueryStateEntity {
    pub uid: String,
    pub chain_code: String,
    pub page: i64,
    pub status: AssetQueryStatus,
    pub index_list_json: String,
}

impl CreateAssetQueryStateEntity {
    pub fn new(uid: &str, chain_code: &str, page: i64, index_list_json: &str) -> Self {
        Self {
            uid: uid.to_string(),
            chain_code: chain_code.to_string(),
            page,
            status: AssetQueryStatus::Pending,
            index_list_json: index_list_json.to_string(),
        }
    }
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
pub enum AssetQueryStatus {
    Pending = 0,
    Running = 1,
    Done = 2,
    Failed = 3,
}
