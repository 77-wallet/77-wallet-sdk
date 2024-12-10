#[derive(Debug, Default, serde::Serialize, sqlx::FromRow, wallet_macro::macros::Resource)]
#[resource(
    query_req = "crate::entities::system_notification::QueryReq",
    sqlite_table_name = "system_notification"
)]
#[serde(rename_all = "camelCase")]
pub struct SystemNotificationEntity {
    #[resource(detail = "QueryReq")]
    pub id: String,
    pub r#type: String,
    #[resource(detail = "QueryReq")]
    #[serde(skip_serializing)]
    pub key: Option<String>,
    #[resource(detail = "QueryReq")]
    #[serde(skip_serializing)]
    pub value: Option<String>,
    pub content: String,
    pub status: i8,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

#[derive(Debug, Clone)]
pub struct CreateSystemNotificationEntity {
    pub id: String,
    pub r#type: String,
    pub key: Option<String>,
    pub value: Option<String>,
    pub content: String,
    pub status: i8,
}

impl CreateSystemNotificationEntity {
    pub fn new(
        id: &str,
        r#type: &str,
        content: &str,
        status: i8,
        key: Option<String>,
        value: Option<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            r#type: r#type.to_string(),
            key,
            value,
            content: content.to_string(),
            status,
        }
    }
}

pub struct QueryReq {
    pub id: Option<String>,
    pub key: Option<String>,
    pub value: Option<String>,
}
