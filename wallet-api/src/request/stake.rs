use super::transaction::Signer;
use wallet_chain_interact::tron::{
    consts,
    operations::stake::{
        self, DelegateArgs, UnDelegateArgs, UnFreezeBalanceArgs, VoteWitnessArgs,
        WithdrawBalanceArgs,
    },
};

#[derive(serde::Serialize, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreezeBalanceReq {
    pub owner_address: String,
    pub resource: String,
    pub frozen_balance: i64,
    pub signer: Option<Signer>,
}
impl TryFrom<&FreezeBalanceReq> for stake::FreezeBalanceArgs {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: &FreezeBalanceReq) -> Result<Self, Self::Error> {
        let args = stake::FreezeBalanceArgs::new(
            &value.owner_address,
            &value.resource,
            value.frozen_balance,
            value.signer.clone().map(|s| s.permission_id),
        )?;
        Ok(args)
    }
}

#[derive(serde::Serialize, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnFreezeBalanceReq {
    pub owner_address: String,
    pub resource: String,
    pub unfreeze_balance: i64,
    pub signer: Option<Signer>,
}

impl TryFrom<&UnFreezeBalanceReq> for UnFreezeBalanceArgs {
    type Error = crate::error::service::ServiceError;
    fn try_from(value: &UnFreezeBalanceReq) -> Result<Self, Self::Error> {
        let args = stake::UnFreezeBalanceArgs::new(
            &value.owner_address,
            &value.resource,
            value.unfreeze_balance,
            value.signer.clone().map(|s| s.permission_id),
        )?;
        Ok(args)
    }
}

#[derive(serde::Serialize, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelAllUnFreezeReq {
    pub owner_address: String,
    pub signer: Option<Signer>,
}

#[derive(serde::Serialize, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegateReq {
    pub owner_address: String,
    pub receiver_address: String,
    pub balance: i64,
    pub resource: String,
    pub lock: bool,
    pub lock_period: f64,
    pub signer: Option<Signer>,
}

impl TryFrom<&DelegateReq> for DelegateArgs {
    type Error = crate::error::service::ServiceError;
    fn try_from(value: &DelegateReq) -> Result<Self, Self::Error> {
        let lock_period = expiration_time(value.lock_period);
        let args = Self {
            owner_address: wallet_utils::address::bs58_addr_to_hex(&value.owner_address)?,
            receiver_address: wallet_utils::address::bs58_addr_to_hex(&value.receiver_address)?,
            balance: value.balance * consts::TRX_VALUE,
            resource: stake::ResourceType::try_from(value.resource.as_str())?,
            lock: value.lock,
            lock_period,
            permission_id: value.signer.clone().map(|s| s.permission_id),
        };
        Ok(args)
    }
}

fn expiration_time(period: f64) -> i64 {
    if period <= 0.0 {
        return 0;
    }

    (period * 28800.0) as i64
}

#[derive(serde::Serialize, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnDelegateReq {
    pub owner_address: String,
    pub resource: String,
    pub receiver_address: String,
    pub balance: i64,
    pub signer: Option<Signer>,
}

impl TryFrom<&UnDelegateReq> for UnDelegateArgs {
    type Error = crate::error::service::ServiceError;
    fn try_from(value: &UnDelegateReq) -> Result<Self, Self::Error> {
        let args = Self {
            owner_address: wallet_utils::address::bs58_addr_to_hex(&value.owner_address)?,
            receiver_address: wallet_utils::address::bs58_addr_to_hex(&value.receiver_address)?,
            balance: value.balance * consts::TRX_VALUE,
            resource: stake::ResourceType::try_from(value.resource.as_str())?,
            permission_id: value.signer.clone().map(|s| s.permission_id),
        };
        Ok(args)
    }
}

#[derive(serde::Serialize, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoteWitnessReq {
    pub owner_address: String,
    pub votes: Vec<VotesReq>,
    pub signer: Option<Signer>,
}

impl VoteWitnessReq {
    pub fn new(owner_address: &str, votes: Vec<VotesReq>, signer: Option<Signer>) -> Self {
        Self { owner_address: owner_address.to_string(), votes, signer }
    }

    pub fn get_votes(&self) -> i64 {
        self.votes.iter().map(|item| item.vote_count).sum()
    }
}

#[derive(serde::Serialize, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VotesReq {
    pub vote_address: String,
    pub vote_count: i64,
    pub name: String,
}

impl VotesReq {
    pub fn new(vote_address: &str, vote_count: i64, name: &str) -> Self {
        Self { vote_address: vote_address.to_string(), vote_count, name: name.to_string() }
    }
}

impl TryFrom<&VoteWitnessReq> for VoteWitnessArgs {
    type Error = crate::error::service::ServiceError;
    fn try_from(value: &VoteWitnessReq) -> Result<Self, Self::Error> {
        let mut votes = Vec::new();
        for v in &value.votes {
            let vote = stake::Votes::new(&v.vote_address, v.vote_count)?;
            votes.push(vote);
        }

        Ok(VoteWitnessArgs::new(
            &value.owner_address,
            votes,
            value.signer.clone().map(|s| s.permission_id),
        )?)
    }
}

#[derive(serde::Serialize, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawBalanceReq {
    pub owner_address: String,
    pub signer: Option<Signer>,
}

impl WithdrawBalanceReq {
    pub fn new(owner_address: &str, signer: Option<Signer>) -> Self {
        Self { owner_address: owner_address.to_string(), signer }
    }
}

impl TryFrom<&WithdrawBalanceReq> for WithdrawBalanceArgs {
    type Error = crate::error::service::ServiceError;
    fn try_from(value: &WithdrawBalanceReq) -> Result<Self, Self::Error> {
        let args = Self {
            owner_address: value.owner_address.clone(),
            value: None,
            permission_id: value.signer.clone().map(|s| s.permission_id),
        };

        Ok(args)
    }
}

#[derive(serde::Serialize, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
// 批量取消代理
pub struct BatchDelegate {
    pub owner_address: String,
    pub resource_type: String,
    pub lock: bool,
    pub lock_period: f64,
    pub list: Vec<BatchList>,
    pub signer: Option<Signer>,
}

impl BatchDelegate {
    pub fn total(&self) -> i64 {
        self.list.iter().map(|t| t.value).sum()
    }
}

impl TryFrom<&BatchDelegate> for Vec<DelegateArgs> {
    type Error = crate::error::service::ServiceError;
    fn try_from(value: &BatchDelegate) -> Result<Self, Self::Error> {
        let owner_address = wallet_utils::address::bs58_addr_to_hex(&value.owner_address)?;
        let resource_type = stake::ResourceType::try_from(value.resource_type.as_str())?;

        let permission_id = value.signer.clone().map(|s| s.permission_id);

        let lock_period = expiration_time(value.lock_period);
        value
            .list
            .iter()
            .map(|item| {
                Ok(DelegateArgs {
                    owner_address: owner_address.clone(),
                    receiver_address: wallet_utils::address::bs58_addr_to_hex(
                        &item.receive_address,
                    )?,
                    balance: item.value * consts::TRX_VALUE,
                    resource: resource_type,
                    lock: value.lock,
                    lock_period,
                    permission_id,
                })
            })
            .collect()
    }
}

#[derive(serde::Serialize, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
// 批量取消代理
pub struct BatchUnDelegate {
    pub owner_address: String,
    pub resource_type: String,
    pub list: Vec<BatchList>,
    pub signer: Option<Signer>,
}
impl BatchUnDelegate {
    pub fn total(&self) -> i64 {
        self.list.iter().map(|t| t.value).sum()
    }
}

impl TryFrom<&BatchUnDelegate> for Vec<UnDelegateArgs> {
    type Error = crate::error::service::ServiceError;
    fn try_from(value: &BatchUnDelegate) -> Result<Self, Self::Error> {
        let owner_address = wallet_utils::address::bs58_addr_to_hex(&value.owner_address)?;
        let resource_type = stake::ResourceType::try_from(value.resource_type.as_str())?;

        let permission_id = value.signer.clone().map(|s| s.permission_id);

        value
            .list
            .iter()
            .map(|item| {
                Ok(UnDelegateArgs {
                    owner_address: owner_address.clone(),
                    receiver_address: wallet_utils::address::bs58_addr_to_hex(
                        &item.receive_address,
                    )?,
                    balance: item.value * consts::TRX_VALUE,
                    resource: resource_type,
                    permission_id,
                })
            })
            .collect()
    }
}

#[derive(serde::Serialize, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchList {
    pub receive_address: String,
    pub value: i64,
}

#[cfg(test)]
mod batch_tests {
    use super::{BatchDelegate, BatchList, BatchUnDelegate};
    use crate::request::transaction::Signer;
    use wallet_chain_interact::tron::{
        consts,
        operations::stake::{DelegateArgs, UnDelegateArgs},
    };

    const OWNER: &str = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1";
    const RECEIVER: &str = "TNPTj8Dbba6YxW5Za6tFh6SJMZGbUyucXQ";

    #[test]
    fn batch_delegate_contract_preserves_total_addresses_amount_lock_and_permission() {
        let req = BatchDelegate {
            owner_address: OWNER.to_string(),
            resource_type: "energy".to_string(),
            lock: true,
            lock_period: 2.0,
            list: vec![BatchList { receive_address: RECEIVER.to_string(), value: 7 }],
            signer: Some(Signer { address: OWNER.to_string(), permission_id: 3 }),
        };

        let args: Vec<DelegateArgs> = (&req).try_into().unwrap();

        assert_eq!(req.total(), 7);
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].owner_address, wallet_utils::address::bs58_addr_to_hex(OWNER).unwrap());
        assert_eq!(
            args[0].receiver_address,
            wallet_utils::address::bs58_addr_to_hex(RECEIVER).unwrap()
        );
        assert_eq!(args[0].balance, 7 * consts::TRX_VALUE);
        assert!(args[0].lock);
        assert_eq!(args[0].lock_period, 2 * 28_800);
        assert_eq!(args[0].permission_id, Some(3));
    }

    #[test]
    fn batch_reclaim_contract_preserves_total_addresses_amount_and_permission() {
        let req = BatchUnDelegate {
            owner_address: OWNER.to_string(),
            resource_type: "bandwidth".to_string(),
            list: vec![BatchList { receive_address: RECEIVER.to_string(), value: 9 }],
            signer: Some(Signer { address: OWNER.to_string(), permission_id: 4 }),
        };

        let args: Vec<UnDelegateArgs> = (&req).try_into().unwrap();

        assert_eq!(req.total(), 9);
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].owner_address, wallet_utils::address::bs58_addr_to_hex(OWNER).unwrap());
        assert_eq!(
            args[0].receiver_address,
            wallet_utils::address::bs58_addr_to_hex(RECEIVER).unwrap()
        );
        assert_eq!(args[0].balance, 9 * consts::TRX_VALUE);
        assert_eq!(args[0].permission_id, Some(4));
    }
}
