#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiInitSwapReq {
    pub sn: String,
    pub client_pub_key: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiInitSwapResponseData {
    pub pub_key: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiInitSwapResponse {
    pub success: bool,
    pub code: Option<String>,
    pub msg: Option<String>,
    pub data: Option<ApiInitSwapResponseData>,
}
