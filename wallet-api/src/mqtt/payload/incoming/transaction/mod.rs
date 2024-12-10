pub mod acct_change;
pub mod multisign_trans_accept;
pub mod multisign_trans_accept_complete_msg;
pub mod multisign_trans_cancel;
use sqlx::types::chrono::{DateTime, Utc};
use wallet_database::entities::{
    multisig_queue::MultisigQueueEntity, multisig_signatures::NewSignatureEntity,
};

// biz_type = MULTI_SIGN_TRANS_ACCEPT
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MultiSignTransAccept {
    /// queue_id
    pub id: String,
    pub from_addr: String,
    pub to_addr: String,
    pub value: String,
    pub expiration: i64,
    #[serde(deserialize_with = "wallet_utils::serde_func::deserialize_uppercase")]
    pub symbol: String,
    pub chain_code: String,
    pub token_addr: Option<String>,
    pub msg_hash: String,
    pub tx_hash: String,
    pub raw_data: String,
    /// 0待签名 1待执行 2已执行
    pub status: i8,
    pub notes: String,
    pub created_at: DateTime<Utc>,
    pub signatures: Vec<NewSignatureEntity>,
    pub account_id: String,
}

impl From<MultisigQueueEntity> for MultiSignTransAccept {
    fn from(value: MultisigQueueEntity) -> Self {
        Self {
            id: value.id,
            from_addr: value.from_addr,
            to_addr: value.to_addr,
            value: value.value,
            expiration: value.expiration,
            symbol: value.symbol,
            chain_code: value.chain_code,
            token_addr: value.token_addr,
            msg_hash: value.msg_hash,
            tx_hash: value.tx_hash,
            raw_data: value.raw_data,
            status: value.status,
            notes: value.notes,
            created_at: value.created_at,
            signatures: vec![],
            account_id: value.account_id,
        }
    }
}

// MULTI_SIGN_TRANS_CANCEL
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MultiSignTransCancel {
    pub withdraw_id: String,
}

// biz_type = MULTI_SIGN_TRANS_ACCEPT_COMPLETE_MSG
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MultiSignTransAcceptCompleteMsg(Vec<MultiSignTransAcceptCompleteMsgBody>);

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MultiSignTransAcceptCompleteMsgBody {
    pub queue_id: String,
    pub address: String,
    pub signature: String,
    /// 0未签 1签名  2拒绝
    pub status: i8,
}

impl From<&NewSignatureEntity> for MultiSignTransAcceptCompleteMsgBody {
    fn from(value: &NewSignatureEntity) -> Self {
        Self {
            queue_id: value.queue_id.clone(),
            address: value.address.clone(),
            signature: value.signature.clone(),
            status: value.status.to_i8(),
        }
    }
}

// biz_type = ACCT_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AcctChange {
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
    pub transaction_fee: f64,
    // 交易时间
    #[serde(default)]
    pub transaction_time: String,
    // 交易状态 true-成功 false-失败
    pub status: bool,
    // 是否多签 1-是，0-否
    #[serde(default)]
    pub is_multisig: i32,
    // 队列id
    #[serde(default)]
    pub queue_id: String,
    // 块高
    pub block_height: i64,
    // 备注
    #[serde(default)]
    pub notes: String,
}
