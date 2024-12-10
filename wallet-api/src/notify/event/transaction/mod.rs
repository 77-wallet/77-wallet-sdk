// biz_type = MULTI_SIGN_TRANS_ACCEPT
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiSignTransAcceptFrontend {
    /// 队列id
    pub id: String,
    pub from_addr: String,
    pub to_addr: String,
    pub value: String,
    pub expiration: i64,
    pub symbol: String,
    pub chain_code: String,
    pub token_addr: Option<String>,
    /// 签名哈希
    pub msg_hash: String,
    /// 交易哈希
    pub tx_hash: String,
    pub raw_data: String,
    /// 0待签名 1待执行 2已执行
    pub status: i8,
    pub notes: String,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
}

// biz_type = MULTI_SIGN_TRANS_ACCEPT_COMPLETE_MSG
pub(crate) type MultiSignTransAcceptCompleteMsgFrontend =
    crate::mqtt::payload::incoming::transaction::MultiSignTransAcceptCompleteMsgBody;

// biz_type = ACCT_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcctChangeFrontend {
    // 交易hash
    pub tx_hash: String,
    // 链码
    pub chain_code: String,
    // 币种符号
    pub symbol: String,
    // 交易方式 0转入 1转出 2初始化
    pub transfer_type: i8,
    // 交易类型 1:普通交易，2:部署多签账号 3:服务费
    pub tx_kind: i8,
    // 发起方
    pub from_addr: String,
    // 接收方
    pub to_addr: String,
    // 合约地址
    pub token: Option<String>,
    // 交易额
    pub value: f64,
    // 手续费
    pub transaction_fee: f64,
    // 交易时间
    pub transaction_time: String,
    // 交易状态 true-成功 false-失败
    pub status: bool,
    // 是否多签 1-是，0-否
    pub is_multisig: i32,
    // 队列id
    pub queue_id: String,
    // 块高
    pub block_height: i64,
    // 备注
    pub notes: String,
}
