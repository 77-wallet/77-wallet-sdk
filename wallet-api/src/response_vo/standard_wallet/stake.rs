use crate::request::stake::{DelegateReq, UnDelegateReq};
use sqlx::types::chrono::{DateTime, Utc};
use wallet_chain_interact::tron::{
    consts,
    operations::stake::{DelegateResouce, ResourceType},
};
use wallet_database::entities::bill::BillKind;
use wallet_transport_backend::response_vo::stake::NodeRespList;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceResp {
    // trx
    pub amount: i64,
    // energy bandwidth
    pub resource_type: ResourceType,
    // 对应资源的数量
    pub resource_value: f64,
}
impl ResourceResp {
    pub fn new(amount: i64, resource_type: ResourceType, resource_value: f64) -> Self {
        Self { amount, resource_type, resource_value }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegateRemaingTime {
    pub days: f64,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreezeListResp {
    pub resource: ResourceResp,
    pub opration_time: Option<DateTime<Utc>>,
}

impl FreezeListResp {
    pub fn new(resource: ResourceResp) -> Self {
        Self { resource, opration_time: None }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreezeResp {
    pub owner_address: String,
    pub resource: ResourceResp,
    pub votes: i64,
    pub bill_kind: BillKind,
    pub tx_hash: String,
    pub expiration_at: Option<DateTime<Utc>>,
    // 可能存在提取的金额
    pub withdraw_amount: i64,
}
impl FreezeResp {
    pub fn new(
        owner_address: String,
        resource: ResourceResp,
        tx_hash: String,
        bill_kind: BillKind,
    ) -> Self {
        Self {
            owner_address,
            votes: resource.amount,
            resource,
            tx_hash,
            bill_kind,
            expiration_at: None,
            withdraw_amount: 0,
        }
    }

    pub fn expiration_at(mut self, date: DateTime<Utc>) -> Self {
        self.expiration_at = Some(date);
        self
    }

    pub fn withdraw_amount(mut self, amount: i64) -> Self {
        self.withdraw_amount = amount;
        self
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawUnfreezeResp {
    pub amount: i64,
    pub owner_address: String,
    pub tx_hash: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelAllUnFreezeResp {
    pub owner_address: String,
    pub votes: i64,
    pub energy: f64,
    pub bandwidth: f64,
    pub amount: i64,
    pub tx_hash: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnfreezeListResp {
    pub amount: i64,
    pub resource_type: ResourceType,
    pub available_at: DateTime<Utc>,
}
impl UnfreezeListResp {
    pub fn new(amount: i64, resource_type: ResourceType, available_at: i64) -> Self {
        let time = DateTime::from_timestamp_millis(available_at).unwrap_or_default();
        Self { amount: amount / consts::TRX_TO_SUN as i64, resource_type, available_at: time }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrxToResourceResp {
    pub resource: ResourceResp,
    // 能够得到的投票数量
    pub votes: i64,
    // 预计转账次数
    pub transfer_times: f64,
}
impl TrxToResourceResp {
    pub fn new(resource: ResourceResp, consumer: f64) -> Self {
        Self {
            transfer_times: (resource.amount as f64 / consumer).floor(),
            votes: resource.amount,
            resource,
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceToTrxResp {
    pub amount: i64,
    pub votes: i64,
    pub transfer_times: f64,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegateResp {
    pub owner_address: String,
    pub receiver_address: String,
    pub resource: ResourceResp,
    pub bill_kind: BillKind,
    pub tx_hash: String,
}
impl DelegateResp {
    pub fn new_delegate(
        req: DelegateReq,
        resource: ResourceResp,
        bill_kind: BillKind,
        tx_hash: String,
    ) -> Self {
        Self {
            owner_address: req.owner_address.to_string(),
            receiver_address: req.receiver_address.to_string(),
            resource,
            bill_kind,
            tx_hash,
        }
    }

    pub fn new_undelegate(
        req: UnDelegateReq,
        resource: ResourceResp,
        bill_kind: BillKind,
        tx_hash: String,
    ) -> Self {
        Self {
            owner_address: req.owner_address.to_string(),
            receiver_address: req.receiver_address.to_string(),
            resource,
            bill_kind,
            tx_hash,
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegateListResp {
    pub from: String,
    pub to: String,
    pub resource: ResourceResp,
    pub expire_time: Option<DateTime<Utc>>,
}

impl DelegateListResp {
    // acount unit is sun
    pub fn new(
        delegate: &DelegateResouce,
        resource: ResourceResp,
        expire_time: i64,
    ) -> Result<DelegateListResp, crate::error::service::ServiceError> {
        let expire_time =
            if expire_time > 0 { DateTime::from_timestamp_millis(expire_time) } else { None };

        Ok(DelegateListResp {
            from: delegate.from.to_string(),
            to: delegate.to.to_string(),
            resource,
            expire_time,
        })
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchDelegateResp {
    pub owner_address: String,
    pub result: Vec<BatchRes>,
    pub resource: ResourceResp,
    pub bill_kind: BillKind,
    pub hashs: Vec<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRes {
    pub address: String,
    pub status: bool,
}

impl BatchDelegateResp {
    pub fn new(
        owner_address: String,
        res: (Vec<BatchRes>, Vec<String>),
        resource: ResourceResp,
        bill_kind: BillKind,
    ) -> Self {
        Self { owner_address, result: res.0, resource, bill_kind, hashs: res.1 }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoteListResp {
    pub total: u16,
    pub total_votes: i64,
    pub data: Vec<Witness>,
}

impl VoteListResp {
    pub fn sort_data(&mut self) {
        self.data.sort_by(|a, b| {
            // 先按 brokerage 倒序排序
            b.brokerage
                .partial_cmp(&a.brokerage)
                .unwrap()
                .then_with(|| b.apr.partial_cmp(&a.apr).unwrap()) // 再按 apr 倒序排序
        });
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Witness {
    pub name: Option<String>,
    pub address: String,
    pub vote_count: i64,
    pub vote_count_by_owner: Option<i64>,
    pub url: String,
    pub brokerage: f64,
    pub apr: f64,
}

impl From<NodeRespList> for Witness {
    fn from(value: NodeRespList) -> Self {
        Witness::new(
            value.name,
            &value.address,
            value.vote_count,
            &value.url,
            value.brokerage,
            value.apr,
        )
    }
}

impl Witness {
    pub fn new(
        name: Option<String>,
        address: &str,
        vote_count: i64,
        url: &str,
        brokerage: f64,
        apr: f64,
    ) -> Self {
        Self {
            name,
            address: address.to_string(),
            vote_count,
            url: url.to_string(),
            brokerage,
            apr,
            vote_count_by_owner: None,
        }
    }

    pub fn with_vote_count_by_owner(mut self, vote_count_by_owner: i64) -> Self {
        self.vote_count_by_owner = Some(vote_count_by_owner);
        self
    }
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VoterInfoResp {
    pub balance: f64,
    pub reward: f64,
    pub tron_power_limit: i64,
    pub tron_power_used: i64,
    // pub votes: Votes,
    pub comprehensive_apr: f64,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct Votes(Vec<Vote>);

#[derive(serde::Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Vote {
    pub vote_address: String,
    pub vote_count: i64,
}

impl From<wallet_chain_interact::tron::protocol::account::Vote> for Vote {
    fn from(vote: wallet_chain_interact::tron::protocol::account::Vote) -> Self {
        Self { vote_address: vote.vote_address, vote_count: vote.vote_count }
    }
}

impl From<Vec<wallet_chain_interact::tron::protocol::account::Vote>> for Votes {
    fn from(votes: Vec<wallet_chain_interact::tron::protocol::account::Vote>) -> Self {
        Self(votes.into_iter().map(Vote::from).collect())
    }
}

impl VoterInfoResp {
    pub fn new(
        balance: f64,
        reward: f64,
        tron_power_limit: i64,
        tron_power_used: i64,
        // votes: Votes,
        comprehensive_apr: f64,
    ) -> Self {
        Self {
            balance,
            reward,
            tron_power_limit,
            tron_power_used,
            // votes,
            comprehensive_apr,
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AddressExists {
    pub address: String,
    pub exists: bool,
}
