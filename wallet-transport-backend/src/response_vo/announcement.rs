#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulletinInfo {
    pub id: String,
    /// 公告类型
    pub r#type: Option<String>,
    /// 公告标题
    pub title: String,
    /// 公告内容
    pub content: String,
    /// 公告语言
    pub language: String,
    /// i18n
    pub i18n: I18n,
    /// 操作人
    pub operator: Option<String>,
    /// 发送状态
    pub bulletin_send_status_enum: Option<String>,
    /// 备注
    pub remark: Option<String>,
    /// 发送时间
    pub send_time: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct I18n {
    pub title: String,
    pub content: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulletinInfoList {
    pub list: Vec<BulletinInfo>,
}
