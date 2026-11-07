#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnconfirmMsgRes {
    pub list: Vec<String>,
}

// 根据msgid查询消息的返回
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnconfirmMsgResp {
    pub body: Option<String>,
}
