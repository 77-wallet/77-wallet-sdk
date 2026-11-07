use serde::Serialize;
use wallet_database::entities::{
    multisig_account::MultisigAccountEntity, multisig_member::MultisigMemberEntity,
    multisig_queue::MultisigQueueEntity,
};
use wallet_transport_backend::MultisigServiceFeeInfo;

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MultisigAccountInfo {
    #[serde(flatten)]
    pub account: MultisigAccountEntity,
    pub member: Vec<MultisigMemberEntity>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MultisigAccountList {
    #[serde(flatten)]
    pub account: MultisigAccountEntity,
    pub symbol: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MultisigFeeVo {
    pub symbol: String,
    pub token_address: String,
    pub fee: String,
    pub old_fee: String,
    pub remain_score: i32,
    pub score_trans_id: String,
}

impl From<MultisigServiceFeeInfo> for MultisigFeeVo {
    fn from(value: MultisigServiceFeeInfo) -> Self {
        Self {
            symbol: value.fee_token_code.to_uppercase(),
            token_address: value.token_address,
            fee: value.free.to_string(),
            old_fee: value.old_free.to_string(),
            score_trans_id: value.score_trans_id,
            remain_score: value.score,
        }
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AddressStatus {
    pub address_status: i32,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct QueueInfo {
    pub id: String,
    pub from_addr: String,
    pub to_addr: String,
    pub value: String,
    pub expiration: i64,
    pub symbol: String,
    pub chain_code: String,
    pub token_addr: Option<String>,
    pub msg_hash: String,
    pub tx_hash: String,
    pub status: i8,
    pub notes: String,
    pub fail_reason: String,
    pub account_id: String,
}

impl From<MultisigQueueEntity> for QueueInfo {
    fn from(value: MultisigQueueEntity) -> Self {
        Self {
            id: value.id,
            from_addr: value.from_addr,
            to_addr: value.to_addr,
            value: value.value,
            expiration: value.expiration,
            symbol: value.symbol,
            chain_code: value.chain_code,
            token_addr: value.token_addr.to_option_string_for_api(),
            msg_hash: value.msg_hash,
            tx_hash: value.tx_hash,
            status: value.status,
            notes: value.notes,
            fail_reason: value.fail_reason,
            account_id: value.account_id,
        }
    }
}
