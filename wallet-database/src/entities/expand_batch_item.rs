#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ExpandBatchItemEntity {
    pub batch_id: String,
    pub uid: String,
    pub chain_code: String,
    pub input_index: i32,
    pub status: ExpandItemStatus,
    pub retry_count: i32,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    pub last_init_dispatched_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

/// CreateDispatched: Create 已派发（或确认无需派发） - 事实态
///
/// InitDispatched: Init 已派发（或确认无需派发） - 事实态
///
/// Done: 强事实完成
///
/// Failed: 人工/策略性终止
#[derive(
    Debug, Clone, serde_repr::Serialize_repr, serde_repr::Deserialize_repr, sqlx::Type, PartialEq,
)]
#[repr(i32)]
pub enum ExpandItemStatus {
    CreateDispatched = 1,
    InitDispatched = 2,
    Done = 3,
    Failed = 4,
}

#[derive(Debug, Clone)]
pub struct CreateExpandBatchItemEntity {
    pub batch_id: String,
    pub uid: String,
    pub chain_code: String,
    pub input_index: i32,
}

impl CreateExpandBatchItemEntity {
    pub fn new(batch_id: &str, uid: &str, chain_code: &str, input_index: i32) -> Self {
        Self {
            batch_id: batch_id.to_string(),
            uid: uid.to_string(),
            chain_code: chain_code.to_string(),
            input_index,
        }
    }
}
