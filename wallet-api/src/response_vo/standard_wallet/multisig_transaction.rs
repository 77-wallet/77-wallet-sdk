use serde::Serialize;
use wallet_database::entities::{
    multisig_account::{MultiAccountOwner, MultisigAccountEntity},
    multisig_queue::{MemberSignedResult, MultisigQueueSimpleEntity},
    multisig_signatures::MultisigSignatureStatus,
    permission::PermissionEntity,
};

// 签名的结果
pub struct TransactionSignatureResult {
    pub tx_id: String,
    pub signer: String,
    pub signature: String,
    pub status: MultisigSignatureStatus,
}
impl TransactionSignatureResult {
    pub fn new(
        tx_id: &str,
        signer: &str,
        signature: &str,
        status: MultisigSignatureStatus,
    ) -> Self {
        Self {
            tx_id: tx_id.to_string(),
            signer: signer.to_string(),
            signature: signature.to_string(),
            status,
        }
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MultisigQueueInfoVo {
    #[serde(flatten)]
    pub queue: MultisigQueueSimpleEntity,
    #[serde(flatten)]
    pub extra: ExtraData,
    pub sign_num: i64,
    pub signature: Vec<MemberSignedResult>,
}

// 多签账号或者权限里面的数据
#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExtraData {
    pub name: String,
    pub threshold: i64,
    pub member_num: i64,
    pub initiator_addr: String,
    pub owner: i64,
}

impl From<PermissionEntity> for ExtraData {
    fn from(value: PermissionEntity) -> Self {
        Self {
            name: value.name,
            threshold: value.threshold,
            member_num: value.member,
            initiator_addr: value.grantor_addr,
            owner: MultiAccountOwner::Owner.to_i8() as i64,
        }
    }
}

impl From<MultisigAccountEntity> for ExtraData {
    fn from(value: MultisigAccountEntity) -> Self {
        Self {
            name: value.name,
            threshold: value.threshold as i64,
            member_num: value.member_num as i64,
            initiator_addr: value.initiator_addr,
            owner: value.owner as i64,
        }
    }
}
