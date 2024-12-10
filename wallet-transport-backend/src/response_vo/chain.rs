#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainInfo {
    pub rpc: String,
    pub name: String,
    #[serde(rename = "code")]
    pub chain_code: String,
    pub test: bool,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainUrlInfo {
    /// 查看链上地址URL
    pub address_url: Option<String>,
    /// 查看链上hash URL
    pub hash_url: Option<String>,
    #[serde(rename = "code")]
    pub chain_code: String,
    pub enable: Option<bool>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ChainList {
    pub list: Vec<ChainUrlInfo>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct DefaultChainList {
    pub list: Vec<String>,
}
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ChainInfos {
    pub list: Vec<ChainInfo>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GasOracle {
    pub safe_gas_price: Option<String>,
    pub propose_gas_price: Option<String>,
    pub fast_gas_price: Option<String>,
    #[serde(default)]
    pub suggest_base_fee: Option<String>,
    pub gas_used_ratio: Option<String>,
}
