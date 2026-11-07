#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Strategy {
    pub uid: String,
    pub threshold: u32,
    pub chain_configs: Vec<ChainConfig>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainConfig {
    pub chain_code: String,
    pub chain_address_type: Option<String>,
    pub normal_address: IndexAndAddress,
    pub risk_address: IndexAndAddress,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexAndAddress {
    pub index: Option<i32>,
    pub address: String,
}

impl Strategy {
    pub fn new(uid: &str, threshold: u32, chain_configs: Vec<ChainConfig>) -> Self {
        Self { uid: uid.to_string(), threshold, chain_configs }
    }
}

pub type SaveWithdrawStrategyReq = Strategy;
pub type SaveCollectStrategyReq = Strategy;
