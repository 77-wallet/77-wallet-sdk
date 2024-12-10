use super::multisig_signatures::{MultisigSignatureEntities, NewSignatureEntity};
use sqlx::types::chrono::{DateTime, Utc};

pub mod fail_reason {
    pub const SIGN_FAILED: &str = "sign_failed";
    pub const EXPIRED: &str = "expired";
    pub const CANCEL: &str = "cancel";
}

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow, Clone)]
pub struct MultisigQueueEntity {
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
    pub raw_data: String,
    /// 0待签名 1待执行 2已执行
    pub status: i8,
    pub notes: String,
    pub fail_reason: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub account_id: String,
}
impl MultisigQueueEntity {
    pub fn token_address(&self) -> Option<String> {
        self.token_addr
            .as_ref()
            .filter(|token| !token.is_empty())
            .cloned()
    }

    // default status : pending signature
    pub fn compute_status(sign_num: usize, threshold: usize) -> MultisigQueueStatus {
        if sign_num > 0 && sign_num < threshold {
            return MultisigQueueStatus::HasSignature;
        } else if sign_num >= threshold {
            return MultisigQueueStatus::PendingExecution;
        }

        MultisigQueueStatus::PendingSignature
    }

    pub fn can_cancel(&self) -> bool {
        self.status == MultisigQueueStatus::PendingSignature.to_i8()
            || self.status == MultisigQueueStatus::HasSignature.to_i8()
            || self.status == MultisigQueueStatus::PendingExecution.to_i8()
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MultisigQueueStatus {
    PendingSignature = 0, // 待签名
    HasSignature,         // 已签名
    PendingExecution,     // 待执行
    InConfirmation,       // 确认中(交易已经提交，等待链上确认)
    Success,              // 成功
    Fail,                 // 失败
}

impl MultisigQueueStatus {
    pub fn to_i8(&self) -> i8 {
        match self {
            MultisigQueueStatus::PendingSignature => 0,
            MultisigQueueStatus::HasSignature => 1,
            MultisigQueueStatus::PendingExecution => 2,
            MultisigQueueStatus::InConfirmation => 3,
            MultisigQueueStatus::Success => 4,
            MultisigQueueStatus::Fail => 5,
        }
    }

    pub fn from_i8(status: i8) -> MultisigQueueStatus {
        match status {
            0 => MultisigQueueStatus::PendingSignature,
            1 => MultisigQueueStatus::HasSignature,
            2 => MultisigQueueStatus::PendingExecution,
            3 => MultisigQueueStatus::InConfirmation,
            4 => MultisigQueueStatus::Success,
            5 => MultisigQueueStatus::Fail,
            _ => MultisigQueueStatus::PendingSignature,
        }
    }
}

#[derive(Debug, serde::Serialize, sqlx::FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MultisigQueueWithAccountEntity {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub threshold: i32,
    pub member_num: i32,
    pub initiator_addr: String,
    #[sqlx(default)]
    pub sign_num: Option<i64>,
    pub owner: i32,
    pub from_addr: String,
    pub to_addr: String,
    pub value: String,
    pub expiration: i64,
    pub symbol: String,
    pub chain_code: String,
    pub fail_reason: String,
    pub msg_hash: String,
    pub tx_hash: String,
    pub status: i8,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct NewMultisigQueueEntity {
    pub id: String,
    pub account_id: String,
    pub from_addr: String,
    pub to_addr: String,
    pub value: String,
    pub symbol: String,
    pub expiration: i64,
    pub chain_code: String,
    pub token_addr: Option<String>,
    pub msg_hash: String,
    pub tx_hash: String,
    pub raw_data: String,
    pub status: MultisigQueueStatus,
    pub notes: String,
    pub fail_reason: String,
    pub signatures: Vec<NewSignatureEntity>,
    pub create_at: DateTime<Utc>,
}
impl NewMultisigQueueEntity {
    pub fn with_msg_hash(mut self, msg_hash: &str) -> Self {
        self.msg_hash = msg_hash.to_string();
        self
    }

    pub fn with_raw_data(mut self, raw_data: &str) -> Self {
        self.raw_data = raw_data.to_owned();
        self
    }

    pub fn with_id(mut self, id: &str) -> Self {
        self.id = id.to_string();
        self
    }

    pub fn set_id(mut self) -> Self {
        let id = wallet_utils::snowflake::get_uid().unwrap();
        self.id = id.to_string();
        self
    }

    pub fn with_signatures(mut self, signatures: NewSignatureEntity) -> Self {
        self.signatures.push(signatures);
        self
    }

    pub fn with_token(mut self, token: Option<String>) -> Self {
        self.token_addr = token;
        self
    }

    // recover multisig queue need check status
    pub fn check_expiration(mut self) -> Self {
        if self.needs_expiration_check() && self.is_expired() {
            self.status = MultisigQueueStatus::Fail;
            self.fail_reason = fail_reason::EXPIRED.to_string();
        }
        self
    }

    fn needs_expiration_check(&self) -> bool {
        matches!(
            self.status,
            MultisigQueueStatus::InConfirmation
                | MultisigQueueStatus::HasSignature
                | MultisigQueueStatus::PendingSignature
        ) && self.tx_hash.is_empty()
    }

    fn is_expired(&self) -> bool {
        self.expiration < wallet_utils::time::now().timestamp()
    }
}

#[derive(serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MemberSignedResult {
    pub name: String,
    pub address: String,
    pub is_self: i8,
    pub singed: i8,
    pub signature: String,
}
impl MemberSignedResult {
    pub fn new(name: &str, address: &str, is_self: i8) -> Self {
        Self {
            name: name.to_string(),
            address: address.to_string(),
            is_self,
            singed: 0,
            signature: "".to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MultisigQueueData {
    pub queue: MultisigQueueEntity,
    pub signatures: MultisigSignatureEntities,
}
impl MultisigQueueData {
    pub fn new(
        queue: MultisigQueueEntity,
        signatures: MultisigSignatureEntities,
    ) -> MultisigQueueData {
        Self { queue, signatures }
    }

    pub fn to_string(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::hex_func::bincode_encode(self)?)
    }

    pub fn from_string(data: &str) -> Result<Self, crate::Error> {
        Ok(wallet_utils::hex_func::bincode_decode(data)?)
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct QueueTaskEntity {
    pub id: String,
    pub status: i8,
}

impl From<MultisigQueueEntity> for NewMultisigQueueEntity {
    fn from(value: MultisigQueueEntity) -> Self {
        NewMultisigQueueEntity {
            id: value.id,
            account_id: value.account_id,
            from_addr: value.from_addr,
            to_addr: value.to_addr,
            value: value.value,
            symbol: value.symbol,
            expiration: value.expiration,
            chain_code: value.chain_code,
            token_addr: value.token_addr,
            msg_hash: value.msg_hash,
            tx_hash: value.tx_hash,
            raw_data: value.raw_data,
            status: MultisigQueueStatus::from_i8(value.status),
            notes: value.notes,
            fail_reason: value.fail_reason.to_string(),
            signatures: vec![],
            create_at: value.created_at,
        }
    }
}

#[test]
fn test() {
    let str = "12000000000000003230333632353130373235383734303733362200000000000000544e50546a384462626136597857355a613674466836534a4d5a476255797563585122000000000000005458444b31716a65794b784454425565467945516951433742674470516d3634673101000000000000003114c65267000000000300000000000000545258040000000000000074726f6e004000000000000000666237366262333966303062343839386661653164653235336165343934656464643239323536666662373639666136306238636164353533383035663565310000000000000000d80300000000000051414141414141414141426d596a6332596d497a4f5759774d4749304f446b345a6d466c4d57526c4d6a557a595755304f54526c5a47526b4d6a6b794e545a6d5a6d49334e6a6c6d59545977596a686a595751314e544d344d44566d4e575578647745414141414141414237496d4e76626e527959574e30496a706265794a7759584a68625756305a5849694f6e7369646d4673645755694f6e73695957317664573530496a6f784d4441774d4441774c434a766432356c636c39685a4752795a584e7a496a6f694e4445344f444d33597a686b4e7a4a684e4455774e54526a4e6d566a5a444a6c5a4455354e6d5534597a4e694d44497a5a5463335a57557a49697769644739665957526b636d567a63794936496a51785a546b774e54646a4d5463794e54673259546b305a44417a4f574d794e47466d5a474d325a544d324e44466b5a6a4179593245324d434a394c434a306558426c5833567962434936496e5235634755755a3239765a32786c595842706379356a6232307663484a766447396a6232777556484a68626e4e6d5a584a4462323530636d466a64434a394c434a306558426c496a6f6956484a68626e4e6d5a584a4462323530636d466a64434a3958537769636d566d58324a7362324e7258324a356447567a496a6f695954677a59694973496e4a6c5a6c39696247396a6131396f59584e6f496a6f69593251775a446469597a686c596a64695a5756684f434973496d563463476c7959585270623234694f6a45334d7a4d304e7a63354e6a63774d444173496e52706257567a64474674634349364d54637a4d7a51334d4463774f4467314e33304b4151414141414141414442684d444a684f444e694d6a49774f474e6b4d475133596d4d345a574933596d566c595467304d446b34597a6c6b59575268596a6b7a4d6a56684e6a63774f4441784d5449324d7a42684d6d51334e4463354e7a41324e544a6c4e6a63325a6a5a6d4e6a6332597a59314e6a45334d4459354e7a4d795a54597a4e6d59325a444a6d4e7a41334d6a5a6d4e7a51325a6a597a4e6d5932597a4a6c4e5451334d6a59784e6d55334d7a59324e6a55334d6a517a4e6d59325a5463304e7a49324d54597a4e7a51784d6a4d794d4745784e5451784f44677a4e324d345a446379595451314d445530597a5a6c593251795a5751314f545a6c4f474d7a596a41794d3255334e32566c4d7a45794d5455304d5755354d445533597a45334d6a55344e6d45354e4751774d7a6c6a4d6a52685a6d526a4e6d557a4e6a51785a4759774d6d4e684e6a41784f474d774f44517a5a4463775a6a6c6a4f446c6d5a4464694f544d7941414141414141414141413d05060000000000000073616c617279060000000000000063616e63656c1400000000000000323032342d31322d30365430373a33383a32385a0012000000000000003139353333303539303935363938323237320000000000000000";

    let rs = MultisigQueueData::from_string(&str).unwrap();
    println!("{:#?}", rs);
}
