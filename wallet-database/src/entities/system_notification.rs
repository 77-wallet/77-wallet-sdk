#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SystemNotificationEntity {
    pub id: String,
    pub r#type: String,
    #[serde(skip_serializing)]
    pub key: Option<String>,
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
