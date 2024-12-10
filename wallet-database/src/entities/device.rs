#[derive(Debug, Default, serde::Serialize, sqlx::FromRow, Clone)]
pub struct DeviceEntity {
    pub sn: String,
    pub device_type: String,
    pub code: String,
    pub system_ver: Option<String>,
    pub iemi: Option<String>,
    pub meid: Option<String>,
    pub iccid: Option<String>,
    pub mem: Option<String>,
    pub app_id: Option<String>,
    pub uid: Option<String>,
    pub currency: String,
    pub is_init: u16,
    pub language_init: u16,
    pub password: Option<String>,
    #[serde(skip_serializing)]
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    #[serde(skip_serializing)]
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

#[derive(Debug)]
pub struct CreateDeviceEntity {
    pub device_type: String,
    pub sn: String,
    pub code: String,
    pub system_ver: String,
    pub iemi: Option<String>,
    pub meid: Option<String>,
    pub iccid: Option<String>,
    pub mem: Option<String>,
    pub app_id: Option<String>,
    pub is_init: u16,
    pub language_init: u16,
}
