use wallet_chain_interact::tron::params as tron_params;

#[derive(serde::Serialize, Debug)]
pub struct FreezeBalanceReq {
    pub owner_address: String,
    pub resource: String,
    pub frozen_balance: String,
}
impl TryFrom<FreezeBalanceReq> for tron_params::FreezeBalanceArgs {
    type Error = crate::error::ServiceError;
    fn try_from(value: FreezeBalanceReq) -> Result<Self, Self::Error> {
        let args = tron_params::FreezeBalanceArgs::new(
            &value.owner_address,
            &value.resource,
            &value.frozen_balance,
        )?;
        Ok(args)
    }
}

#[derive(serde::Serialize, Debug)]
pub struct UnFreezeBalanceReq {
    pub owner_address: String,
    pub resource: String,
    pub unfreeze_balance: String,
}

impl From<&UnFreezeBalanceReq> for wallet_database::entities::stake::NewUnFreezeEntity {
    fn from(value: &UnFreezeBalanceReq) -> Self {
        Self {
            tx_hash: "".to_string(),
            owner_address: value.owner_address.clone(),
            resource_type: value.resource.clone(),
            amount: value.unfreeze_balance.clone(),
            freeze_time: 0,
        }
    }
}

impl TryFrom<UnFreezeBalanceReq> for tron_params::UnFreezeBalanceArgs {
    type Error = crate::error::ServiceError;
    fn try_from(value: UnFreezeBalanceReq) -> Result<Self, Self::Error> {
        let args = tron_params::UnFreezeBalanceArgs::new(
            &value.owner_address,
            &value.resource,
            &value.unfreeze_balance,
        )?;
        Ok(args)
    }
}

#[derive(serde::Serialize, Debug)]
pub struct DelegateReq {
    pub owner_address: String,
    pub receiver_address: String,
    pub balance: String,
    pub resource: String,
    pub lock: bool,
    pub lock_period: i64,
}

impl From<&DelegateReq> for wallet_database::entities::stake::NewDelegateEntity {
    fn from(value: &DelegateReq) -> Self {
        Self {
            tx_hash: "".to_string(),
            owner_address: value.owner_address.clone(),
            receiver_address: value.receiver_address.clone(),
            amount: value.balance.clone(),
            resource_type: value.resource.clone(),
            lock: value.lock.into(),
            lock_period: value.lock_period,
        }
    }
}

impl TryFrom<DelegateReq> for tron_params::DelegateArgs {
    type Error = crate::error::ServiceError;
    fn try_from(value: DelegateReq) -> Result<Self, Self::Error> {
        let balance = wallet_utils::unit::convert_to_u256(&value.balance, 6)?;
        let args = Self {
            owner_address: wallet_utils::address::bs58_addr_to_hex(&value.owner_address)?,
            receiver_address: wallet_utils::address::bs58_addr_to_hex(&value.receiver_address)?,
            balance: balance.to::<u64>(),
            resource: tron_params::ResourceType::try_from(value.resource.as_str())?,
            lock: value.lock,
            lock_period: value.lock_period,
        };
        Ok(args)
    }
}
