#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMsgQueryUnconfirmMsgRes {
    pub list: Vec<String>,
}
