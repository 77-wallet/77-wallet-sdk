use serde::Serialize;
use wallet_database::entities::{
    multisig_queue::{MemberSignedResult, MultisigQueueWithAccountEntity},
    multisig_signatures::MultisigSignatureStatus,
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
    pub queue: MultisigQueueWithAccountEntity,
    pub signature: Vec<MemberSignedResult>,
}

// #[derive(Serialize, Debug)]
// pub struct MultisigSignatureResult {
//     pub name: String,
//     pub address: String,
//     pub is_self: i8,
//     pub singed: i8,
//     pub signature: String,
// }
// impl MultisigSignatureResult {
//     pub fn new(name: &str, address: &str, is_self: i8) -> Self {
//         Self {
//             name: name.to_string(),
//             address: address.to_string(),
//             is_self,
//             singed: 0,
//             signature: "".to_string(),
//         }
//     }
// }
