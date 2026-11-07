use sqlx::types::chrono;

#[derive(Debug, Clone, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UnFreezeEntity {
    pub id: String,
    pub tx_hash: String,
    pub owner_address: String,
    pub resource_type: String,
    pub amount: String,
    pub freeze_time: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub struct NewUnFreezeEntity {
    pub tx_hash: String,
    pub owner_address: String,
    pub resource_type: String,
    pub amount: String,
    pub freeze_time: i64,
}

#[derive(Debug, Clone, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DelegateEntity {
    pub id: String,
    pub tx_hash: String,
    pub owner_address: String,
    pub receiver_address: String,
    pub resource_type: String,
    pub amount: String,
    pub status: i8,
    pub lock: i64,
    pub lock_period: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct NewDelegateEntity {
    pub tx_hash: String,
    pub owner_address: String,
    pub receiver_address: String,
    pub resource_type: String,
    pub amount: String,
    pub lock: i64,
    pub lock_period: i64,
}
