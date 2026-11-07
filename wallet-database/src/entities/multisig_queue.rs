use super::{
    asset_token_key::AssetTokenKey,
    bill::BillKind,
    multisig_signatures::{MultisigSignatureEntities, NewSignatureEntity},
};
use sqlx::types::chrono::{DateTime, Utc};
use wallet_types::constant::chain_code;

pub mod fail_reason {
    pub const SIGN_FAILED: &str = "sign_failed";
    pub const EXPIRED: &str = "expired";
    pub const CANCEL: &str = "cancel";
    pub const PERMISSION_CHANGE: &str = "permission_change";
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
    pub token_addr: AssetTokenKey,
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
    pub transfer_type: i8,
    pub permission_id: String,
}
impl MultisigQueueEntity {
    pub fn token_key(&self) -> AssetTokenKey {
        self.token_addr.clone()
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

    pub fn need_sync_status(&self) -> bool {
        !matches!(
            self,
            MultisigQueueStatus::Fail
                | MultisigQueueStatus::Success
                | MultisigQueueStatus::InConfirmation
        )
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
pub struct MultisigQueueSimpleEntity {
    pub id: String,
    pub account_id: String,
    pub permission_id: String,
    // #[sqlx(default)]
    // pub extra_data: Option<ExtraData>,
    // #[sqlx(default)]
    // pub sign_num: Option<i64>,
    // account 表
    #[sqlx(default)]
    pub owner: i32,
    pub from_addr: String,
    pub to_addr: String,
    pub value: String,
    pub expiration: i64,
    pub symbol: String,
    pub chain_code: String,
    #[serde(rename = "tokenAddress")]
    pub token_addr: String,
    pub fail_reason: String,
    pub transfer_type: i8,
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
    pub token_addr: AssetTokenKey,
    pub msg_hash: String,
    pub tx_hash: String,
    pub raw_data: String,
    pub status: MultisigQueueStatus,
    pub notes: String,
    pub fail_reason: String,
    pub signatures: Vec<NewSignatureEntity>,
    pub create_at: DateTime<Utc>,
    pub transfer_type: BillKind,
    pub permission_id: String,
}
impl NewMultisigQueueEntity {
    // expiration 小时对应的秒
    pub fn new(
        account_id: String,
        from_addr: String,
        to: String,
        expiration: i64,
        msg_hash: &str,
        raw_data: &str,
        bill_kind: BillKind,
        value: String,
    ) -> Self {
        let id = wallet_utils::snowflake::get_uid().unwrap();

        let expiration = wallet_utils::time::now().timestamp() + expiration;

        Self {
            id: id.to_string(),
            account_id,
            from_addr,
            to_addr: to,
            value,
            symbol: "TRX".to_string(),
            expiration,
            chain_code: chain_code::TRON.to_string(),
            token_addr: AssetTokenKey::Native,
            msg_hash: msg_hash.to_string(),
            tx_hash: String::new(),
            raw_data: raw_data.to_string(),
            status: MultisigQueueStatus::PendingSignature,
            notes: String::new(),
            fail_reason: String::new(),
            signatures: vec![],
            create_at: wallet_utils::time::now(),
            transfer_type: bill_kind,
            permission_id: String::new(),
        }
    }
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

    pub fn with_token(mut self, token: AssetTokenKey) -> Self {
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
            MultisigQueueStatus::PendingExecution
                | MultisigQueueStatus::HasSignature
                | MultisigQueueStatus::PendingSignature
        ) && self.tx_hash.is_empty()
    }

    fn is_expired(&self) -> bool {
        self.expiration < wallet_utils::time::now().timestamp()
    }

    pub fn compute_status(&mut self, threshold: i32) {
        let sign_num = self
            .signatures
            .iter()
            .filter(|s| !s.signature.is_empty())
            .map(|s| s.weight.unwrap_or(1))
            .sum::<i32>();

        if sign_num > 0 && sign_num < threshold {
            self.status = MultisigQueueStatus::HasSignature;
        } else if sign_num >= threshold {
            self.status = MultisigQueueStatus::PendingExecution;
        }
    }
}

#[derive(serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MemberSignedResult {
    pub name: String,
    pub address: String,
    pub is_self: i8,
    pub singed: i8,
    pub weight: i64,
    pub signature: String,
}
impl MemberSignedResult {
    pub fn new(name: &str, address: &str, is_self: i8, weight: i64) -> Self {
        Self {
            name: name.to_string(),
            address: address.to_string(),
            is_self,
            singed: 0,
            weight,
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
        Ok(wallet_utils::serde_func::serde_to_string(&self)?)
    }

    // 优先从json字符串中解析、没有在从bincode解析、版本兼容
    pub fn from_string(data: &str) -> Result<Self, crate::Error> {
        match wallet_utils::serde_func::serde_from_str::<MultisigQueueData>(data) {
            Ok(res) => Ok(res),
            Err(_) => Ok(wallet_utils::hex_func::bincode_decode(data)?),
        }
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
            transfer_type: BillKind::try_from(value.transfer_type).unwrap_or(BillKind::Transfer),
            permission_id: value.permission_id,
        }
    }
}

#[test]
fn test() {
    let str = "12000000000000003231343837393733393839373331393432342a000000000000003078613031653065653336633631423743363637463734313039343630333239304535613231383243312a000000000000003078304262356338323135383736304631433163373337323265346144353145453234663135353963330600000000000000302e3030323166447c67000000000300000000000000424e420300000000000000626e6200420000000000000030783532393163393839663634626135346534323434633365663830383832613734303336346561386262313465643165636331616138626665313037626636353442000000000000003078313431633534343463626662646237313431666236393565396264346134383230343337333264646535303738643065396230373539663662353661626631663806000000000000636130323030303030303030303030303330373836343338363433313331363633373338333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333036323632333536333338333233313335333833373336333036363331363333313633333733333337333233323635333436313634333533313635363533323334363633313335333533393633333333303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033373337333536363330333536313330333733343330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333133343330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303331333433303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303432303030303030303030303030303033303738333533323339333136333339333833393636333633343632363133353334363533343332333433343633333336353636333833303338333833323631333733343330333333363334363536313338363236323331333436353634333136353633363333313631363133383632363636353331333033373632363633363335333404000000000000000000000000000000001400000000000000323032352d30312d30365430393a30303a32325a00120000000000000032313133303435383634363234393838313602000000000000002e0000000000000012000000000000003231343837393733393839373331393432342a00000000000000307831343838303542343938313933373145454639413832326637463838306234324366363738333444820000000000000031636138383233643038333738653731373766663563333561386166633936646138323435373361383765663566333937643562303730356336623764343239343037636663626565346539373334616462636330366635346662646539613834643739663935643562353035366132386136326338656335313538326630383163011400000000000000323032352d30312d30375430343a30333a33335a011400000000000000323032352d30312d30375430343a30333a33355a2f0000000000000012000000000000003231343837393733393839373331393432342a00000000000000307833384662353937386531433044324134313941636433616533653939434435376266333331666333820000000000000039623035373136386261613137613932626633666262373062613635333537666432626262643366653139336438393536323261393533626534646665363866303561386231613761376439316361623633666533346434326162623131643539353834643334373566643739313865646233613762346130363733613964613162011400000000000000323032352d30312d30375430343a30333a33335a011400000000000000323032352d30312d30375430343a30333a33365a";

    let rs = MultisigQueueData::from_string(&str).unwrap();
    println!("{:#?}", rs);
}
