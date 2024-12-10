use wallet_database::entities::system_notification::SystemNotificationEntity;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemNotification {
    pub id: String,
    pub r#type: String,
    #[serde(skip_serializing)]
    pub key: Option<String>,
    #[serde(skip_serializing)]
    pub value: Option<String>,
    pub content: String,
    pub is_exist: bool,
    pub status: i8,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

impl From<(SystemNotificationEntity, bool)> for SystemNotification {
    fn from((entity, is_exist): (SystemNotificationEntity, bool)) -> Self {
        Self {
            id: entity.id,
            r#type: entity.r#type,
            key: entity.key,
            value: entity.value,
            content: entity.content,
            is_exist,
            status: entity.status,
            created_at: entity.created_at,
            updated_at: entity.updated_at,
        }
    }
}
