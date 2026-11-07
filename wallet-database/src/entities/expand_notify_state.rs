use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExpandNotifyStateEntity {
    pub uid: String,
    pub chain_code: String,
    pub last_notified_page: i64,
    pub updated_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct CreateExpandNotifyStateEntity {
    pub uid: String,
    pub chain_code: String,
    pub last_notified_page: i64,
}

impl CreateExpandNotifyStateEntity {
    pub fn new(uid: &str, chain_code: &str, last_notified_page: i64) -> Self {
        Self { uid: uid.to_string(), chain_code: chain_code.to_string(), last_notified_page }
    }
}
