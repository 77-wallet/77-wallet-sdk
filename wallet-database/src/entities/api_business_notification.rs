#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApiBusinessNotification {
    pub id: String,
    pub uid: String,
    pub kind: String,
    pub business_key: String,
    pub payload: String,
    pub is_read: bool,
    pub resolved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub struct NewApiBusinessNotification<'a> {
    pub id: &'a str,
    pub uid: &'a str,
    pub kind: &'a str,
    pub business_key: &'a str,
    pub payload: &'a str,
}
