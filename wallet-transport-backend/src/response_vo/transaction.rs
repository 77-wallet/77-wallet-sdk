#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct RecordResp {
    pub list: Vec<SyncBillResp>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncBillResp {
    // 交易hash
    pub tx_hash: String,
    // 链码
    pub chain_code: String,
    // 币种符号
    #[serde(deserialize_with = "wallet_utils::serde_func::deserialize_uppercase")]
    pub symbol: String,
    // 交易方式 0转入 1转出 2初始化
    pub transfer_type: i8,
    // 交易类型 1:普通交易，2:部署多签账号 3:服务费
    pub tx_kind: i8,
    // 发起方
    pub from_addr: String,
    // 接收方
    #[serde(default)]
    pub to_addr: String,
    // 合约地址
    #[serde(default)]
    pub token: Option<String>,
    // 交易额
    #[serde(default)]
    pub value: f64,
    // 手续费
    pub transaction_fee: Option<f64>,
    // 交易时间
    #[serde(default)]
    pub transaction_time: String,
    // 交易状态 true-成功 false-失败
    pub status: bool,
    // 是否多签 1-是，0-否
    pub is_multisig: i32,
    pub net_used: Option<u64>,
    pub energy_used: Option<u64>,
    // 队列id
    #[serde(default)]
    pub queue_id: Option<String>,
    // 块高
    pub block_height: i64,
    // 备注
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub signer: Vec<String>,
    #[serde(default)]
    pub extra: Option<serde_json::Value>,
}
impl SyncBillResp {
    pub fn transaction_fee(&self) -> String {
        self.transaction_fee.map_or_else(|| "0".to_string(), |fee| fee.to_string())
    }
}
