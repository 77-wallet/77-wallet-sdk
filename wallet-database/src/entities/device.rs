use std::fmt;

#[derive(Default, serde::Serialize, sqlx::FromRow, Clone)]
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
    // pub currency: String,
    pub is_init: u16,
    pub language_init: u16,
    pub password: Option<String>,
    #[serde(skip_serializing)]
    pub password_proof: Option<String>,
    #[serde(skip_serializing)]
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    #[serde(skip_serializing)]
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

impl fmt::Debug for DeviceEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceEntity")
            .field("sn", &self.sn)
            .field("device_type", &self.device_type)
            .field("code", &self.code)
            .field("system_ver", &self.system_ver)
            .field("iemi", &self.iemi)
            .field("meid", &self.meid)
            .field("iccid", &self.iccid)
            .field("mem", &self.mem)
            .field("app_id", &self.app_id)
            .field("uid", &self.uid)
            .field("is_init", &self.is_init)
            .field("language_init", &self.language_init)
            .field("password", &"<redacted>")
            .field("password_proof", &self.password_proof.as_ref().map(|_| "<redacted>"))
            .finish()
    }
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

#[cfg(test)]
mod tests {
    use super::DeviceEntity;
    use sqlx::types::chrono::{TimeZone, Utc};

    #[test]
    fn device_entity_debug_redacts_passwords() {
        let req = DeviceEntity {
            sn: "sn".to_string(),
            device_type: "type".to_string(),
            code: "code".to_string(),
            system_ver: Some("ver".to_string()),
            iemi: Some("iemi".to_string()),
            meid: Some("meid".to_string()),
            iccid: Some("iccid".to_string()),
            mem: Some("mem".to_string()),
            app_id: Some("app".to_string()),
            uid: Some("uid".to_string()),
            is_init: 1,
            language_init: 1,
            password: Some("super-secret".to_string()),
            password_proof: Some("proof-secret".to_string()),
            created_at: Utc.timestamp_opt(0, 0).single().unwrap(),
            updated_at: None,
        };

        let debug = format!("{req:?}");
        assert!(!debug.contains("super-secret"));
        assert!(!debug.contains("proof-secret"));
        assert!(debug.contains("<redacted>"));
    }
}
