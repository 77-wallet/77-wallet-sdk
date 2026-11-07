#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChainInfo {
    pub id: String,
    pub rpc: String,
    pub http_url: Option<String>,
    pub name: String,
    #[serde(rename = "code")]
    pub chain_code: String,
    pub test: bool,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChainUrlInfo {
    /// 查看链上地址URL
    pub address_url: Option<String>,
    /// 查看链上hash URL
    pub hash_url: Option<String>,
    /// contract
    pub token_url: Option<String>,
    /// 链编码
    #[serde(rename = "code")]
    pub chain_code: String,
    /// 启用/禁用：true启用，false禁用
    pub enable: bool,
    /// 链名称
    pub name: String,
    /// 主币编码
    pub master_token_code: Option<String>,
    /// 版本
    pub app_version_code: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct ChainList {
    pub list: Vec<ChainUrlInfo>,
}

impl From<&Vec<ChainUrlInfo>> for ChainList {
    fn from(value: &Vec<ChainUrlInfo>) -> Self {
        Self { list: value.to_owned() }
    }
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
