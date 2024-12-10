use sqlx::types::chrono::{DateTime, Utc};

pub mod config_key {
    pub const MIN_VALUE_SWITCH: &str = "min_value_switch";
    pub const BLOCK_BROWSER_URL_LIST: &str = "block_browser_url_list";
    pub const OFFICIAL_WEBSITE: &str = "official_website";
    pub const APP_DOWNLOAD_QR_CODE_URL: &str = "app_download_qr_code_url";
    pub const APP_DOWNLOAD_URL: &str = "app_download_url";
    pub const LANGUAGE: &str = "language";
    pub const MQTT_URL: &str = "mqtt_url";
}

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntity {
    pub id: u32,
    pub key: String,
    pub value: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct OfficialWebsite {
    pub url: String,
}

impl OfficialWebsite {
    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for OfficialWebsite {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_str(&value)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MqttUrl {
    pub url: String,
}

impl MqttUrl {
    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for MqttUrl {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_str(&value)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct AppInstallDownload {
    pub url: String,
}

impl AppInstallDownload {
    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for AppInstallDownload {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_str(&value)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct VersionDownloadUrl {
    pub url: String,
}

impl VersionDownloadUrl {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for VersionDownloadUrl {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_str(&value)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MinValueSwitchConfig {
    // true 开启状态 false 关闭状态
    pub switch: bool,
    // 配置的金额
    pub value: f64,
    // 金额对应的法币符号
    pub currency: String,
}
impl MinValueSwitchConfig {
    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for MinValueSwitchConfig {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_str(&value)?)
    }
}
