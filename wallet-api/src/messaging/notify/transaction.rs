use crate::messaging::mqtt::topics::{MultiSignTransAccept, MultiSignTransAcceptCompleteMsgBody};
use wallet_database::entities::{asset_token_key::AssetTokenKey, bill::BillKind};

// biz_type = CONFIRMATION
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmationFrontend {
    /// 队列id
    pub id: String,
    pub from_addr: String,
    pub to_addr: String,
    pub value: String,
    pub expiration: i64,
    pub symbol: String,
    pub chain_code: String,
    pub token_addr: AssetTokenKey,
    /// 签名哈希
    pub msg_hash: String,
    /// 交易哈希
    pub tx_hash: String,
    pub raw_data: String,
    /// 0待签名 1待执行 2已执行
    pub status: i8,
    pub notes: String,
    pub bill_kind: BillKind,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
}
impl TryFrom<&MultiSignTransAccept> for ConfirmationFrontend {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: &MultiSignTransAccept) -> Result<Self, crate::error::service::ServiceError> {
        let value = &value.queue;
        Ok(Self {
            id: value.id.to_string(),
            from_addr: value.from_addr.to_string(),
            to_addr: value.to_addr.to_string(),
            value: value.value.to_string(),
            expiration: value.expiration,
            symbol: value.symbol.to_string(),
            chain_code: value.chain_code.to_string(),
            token_addr: value.token_addr.clone(),
            msg_hash: value.msg_hash.to_string(),
            tx_hash: value.tx_hash.to_string(),
            raw_data: value.raw_data.to_string(),
            status: value.status,
            notes: value.notes.to_string(),
            bill_kind: BillKind::try_from(value.transfer_type)?,
            created_at: value.created_at,
        })
    }
}

// biz_type = MULTI_SIGN_TRANS_ACCEPT_COMPLETE_MSG
pub(crate) type MultiSignTransAcceptCompleteMsgFrontend = MultiSignTransAcceptCompleteMsgBody;

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
    pub token: AssetTokenKey,
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
