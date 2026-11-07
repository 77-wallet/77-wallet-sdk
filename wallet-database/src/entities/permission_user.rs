use chrono::{DateTime, Utc};

#[derive(
    Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow, Clone, Hash, PartialEq, Eq,
)]
#[serde(rename_all = "camelCase")]
pub struct PermissionUserEntity {
    pub id: Option<i64>,
    pub address: String,
    pub grantor_addr: String,
    pub permission_id: String,
    pub is_self: i64,
    pub weight: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}
