#[derive(Debug)]
pub struct SetConfigReq {
    pub fiat: String,
    pub url: crate::request::init::UrlParams,
    pub device_type: String,
    pub sn: String,
    pub code: String,
    pub system_ver: String,
    pub iemi: Option<String>,
    pub meid: Option<String>,
    pub iccid: Option<String>,
    pub mem: Option<String>,
    pub app_id: Option<String>,
}

// #[derive(Debug, serde::Serialize)]
// #[serde(rename_all = "camelCase")]
// pub struct GetConfigRes {
//     pub fiat: String,
//     pub wallet_list: Vec<WalletEntity>,
//     pub device_info: Option<DeviceEntity>,
//     pub url: crate::request::init::UrlParams,
// }
