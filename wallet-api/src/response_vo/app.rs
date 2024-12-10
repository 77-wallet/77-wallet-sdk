use wallet_database::entities::{device::DeviceEntity, wallet::WalletEntity};

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConfigRes {
    pub fiat: String,
    pub unread_count: UnreadCount,
    pub wallet_list: Vec<WalletEntity>,
    pub device_info: Option<DeviceEntity>,
    pub url: crate::request::init::UrlParams,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadCount {
    pub system_notification: i64,
    pub announcement: i64,
}
