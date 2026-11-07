use sqlx::types::chrono::{DateTime, Utc};

pub mod config_key {
    pub const MIN_VALUE_SWITCH: &str = "min_value_switch";
    pub const BLOCK_BROWSER_URL_LIST: &str = "block_browser_url_list";
    pub const OFFICIAL_WEBSITE: &str = "official_website";
    pub const APP_DOWNLOAD_QR_CODE_URL: &str = "app_download_qr_code_url";
    pub const APP_DOWNLOAD_URL: &str = "app_download_url";
    pub const LANGUAGE: &str = "language";
    pub const MQTT_URL: &str = "mqtt_url";
    pub const CURRENCY: &str = "currency";
    pub const KEYSTORE_KDF_ALGORITHM: &str = "keystore_kdf_algorithm";
    pub const WALLET_TREE_STRATEGY: &str = "wallet_tree_strategy";
    pub const APP_VERSION: &str = "app_version";
    pub const INVITE_CODE: &str = "invite_code";
    pub const KEYS_RESET_STATUS: &str = "keys_reset_status";
    pub const KEYS_RESET_EPOCH: &str = "keys_reset_epoch";
}

pub(crate) const USD: &str = "USD";

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
pub struct InviteCode {
    pub code: Option<String>,
    pub status: Option<bool>,
}

impl InviteCode {
    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for InviteCode {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_str(&value)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Language {
    pub language: String,
}

impl Language {
    pub fn new(language: &str) -> Self {
        Self { language: language.to_string() }
    }

    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for Language {
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

    pub fn url_with_protocol(&self) -> String {
        if !self.url.starts_with("mqtt://") {
            format!("mqtt://{}", self.url)
        } else {
            self.url.clone()
        }
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
        Self { url: url.to_string() }
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
}
impl MinValueSwitchConfig {
    pub fn new(switch: bool, value: f64) -> Self {
        Self { switch, value }
    }

    pub fn get_key(symbol: &str, sn: &str) -> String {
        format!("{}_{}", sn, symbol)
    }

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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Currency {
    pub currency: String,
}

impl Default for Currency {
    fn default() -> Self {
        Self { currency: USD.to_string() }
    }
}

impl Currency {
    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for Currency {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_str(&value)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct KeystoreKdfAlgorithm {
    pub keystore_kdf_algorithm: wallet_tree::KdfAlgorithm,
}

impl KeystoreKdfAlgorithm {
    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for KeystoreKdfAlgorithm {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_str(&value)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WalletTreeStrategy {
    pub wallet_tree_strategy: wallet_tree::WalletTreeStrategy,
}

impl WalletTreeStrategy {
    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for WalletTreeStrategy {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_str(&value)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct AppVersion {
    pub app_version: String,
}

impl AppVersion {
    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for AppVersion {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_str(&value)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct KeysResetStatus {
    pub status: Option<bool>,
}

impl KeysResetStatus {
    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl TryFrom<String> for KeysResetStatus {
    type Error = crate::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_str(&value)?)
    }
}
