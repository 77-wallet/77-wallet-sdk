#[derive(Debug, Clone, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AnnouncementEntity {
    pub id: String,
    pub title: String,
    pub content: String,
    pub language: String,
    pub status: u8,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
pub struct CreateAnnouncementVo {
    pub id: String,
    pub title: String,
    pub content: String,
    pub language: String,
    pub status: u8,
    pub send_time: Option<String>,
}
