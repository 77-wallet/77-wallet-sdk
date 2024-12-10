#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppVersionRes {
    /// 设别系统 ANDROID, IOS, PC, WEB
    pub device_type: Option<String>,
    /// 版本号
    pub version: Option<String>,
    /// 最低兼容版本号
    pub min_version: Option<String>,
    /// 更新类型 {force:强制更新}, {remind:提示更新}, {new_version:发现新版本}
    pub update_type: Option<String>,
    /// 频率
    pub frequency: Option<i32>,
    /// 提示语
    pub remark: Option<String>,
    /// 开始时间
    pub start_time: Option<String>,
    /// 状态 {not_start:未开始}, {in_progress:进行中}, {expire:失效}
    pub status: Option<String>,
    /// 操作人员
    pub operator: Option<String>,
    /// 下载链接类型
    pub r#type: Option<String>,
    /// 下载链接
    pub download_url: Option<String>,
    /// 服务器上app路径
    pub app_url: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateType {
    Force,
    Remind,
    NewVersion,
    Current,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetFiatRes {
    pub fiat: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetOfficialWebsiteRes {
    pub official_website: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindConfigByKeyRes {
    pub create_time: Option<String>,
    pub update_time: Option<String>,
    #[serde(skip)]
    pub id: Option<String>,
    pub name: Option<String>,
    pub key: Option<String>,
    pub value: Option<String>,
    pub remark: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRpcToken {
    pub token: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSendMsgAccount {
    pub amount: f64,
    pub sn: String,
    pub is_open: bool,
}
