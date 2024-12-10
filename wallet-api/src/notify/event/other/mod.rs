// biz_type = ERR
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrFront {
    pub event: String,
    pub message: String,
}

// biz_type = DEBUG
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugFront {
    pub message: serde_json::Value,
}

// biz_type = ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionErrorFrontend {
    pub message: String,
}
