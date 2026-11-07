use crate::messaging::mqtt::topics::{TronSignFreezeDelegateVoteChange, Vote};
use wallet_database::entities::{asset_token_key::AssetTokenKey, bill::BillKind};

// biz_type = RESOURCE_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceChangeFrontend {
    // 交易hash
    pub tx_hash: String,
    // 链编码
    pub chain_code: String,
    // 币种符号
    pub symbol: String,
    // 交易方式 0转入 1转出 2初始化
    pub transfer_type: i8,
    // 交易类型 1:普通交易，2:部署多签账号 3:服务费
    pub tx_kind: BillKind,
    // from地址
    pub from_addr: String,
    // to地址
    pub to_addr: String,
    // 合约地址
    pub token: AssetTokenKey,
    // 交易额
    pub value: f64,
    // 交易额-usdt
    pub value_usdt: Option<f64>,
    // 手续费
    pub transaction_fee: f64,
    // 交易时间
    pub transaction_time: String,
    // 交易状态
    pub status: bool,
    // 是否多签 1-是，0-否
    pub is_multisig: i32,
    // 块高
    pub block_height: i64,
    // 备注
    pub notes: String,
    // 业务id
    pub queue_id: String,
    // 带宽消耗
    pub net_used: u64,
    // 能量消耗
    pub energy_used: Option<u64>,
    // BANDWIDTH  / ENERGY
    pub resource: Option<String>,
    // 是否锁定
    pub lock: Option<bool>,
    // 锁定周期
    pub lock_period: Option<String>,
    // 投票的节点信息
    pub votes: Vec<Vote>,
}

impl From<TronSignFreezeDelegateVoteChange> for ResourceChangeFrontend {
    fn from(value: TronSignFreezeDelegateVoteChange) -> Self {
        Self {
            tx_hash: value.tx_hash,
            chain_code: value.chain_code,
            symbol: value.symbol,
            transfer_type: value.transfer_type,
            tx_kind: value.tx_kind,
            from_addr: value.from_addr,
            to_addr: value.to_addr,
            token: value.token.into(),
            value: value.value,
            value_usdt: value.value_usdt,
            transaction_fee: value.transaction_fee,
            transaction_time: value.transaction_time,
            status: value.status,
            is_multisig: value.is_multisig,
            block_height: value.block_height,
            notes: value.notes,
            queue_id: value.queue_id,
            net_used: value.net_used,
            energy_used: value.energy_used,
            resource: value.resource,
            lock: value.lock,
            lock_period: value.lock_period,
            votes: value.votes,
        }
    }
}
