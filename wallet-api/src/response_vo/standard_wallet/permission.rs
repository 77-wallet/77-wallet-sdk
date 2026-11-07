use wallet_chain_interact::tron::operations::permissions::{ContractType, PermissionTypes};
use wallet_database::entities::permission::PermissionWithUserEntity;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionList {
    // 资产权限
    pub assets: Vec<i8>,
    // 资源
    pub resource: Vec<i8>,
    // 账号权限
    pub accounts: Vec<i8>,
    // TRC1.0
    pub trx_assets: Vec<i8>,
    // 智能合约
    pub contract: Vec<i8>,
    // 超级代表
    pub super_present: Vec<i8>,
    // bancor
    pub bancor: Vec<i8>,
}

impl Default for PermissionList {
    fn default() -> Self {
        Self {
            assets: vec![
                ContractType::TransferContract.to_i8(),
                ContractType::FreezeBalanceContract.to_i8(),
                ContractType::UnfreezeBalanceContract.to_i8(),
                ContractType::FreezeBalanceV2Contract.to_i8(),
                ContractType::UnfreezeBalanceV2Contract.to_i8(),
                ContractType::WithdrawExpireUnfreezeContract.to_i8(),
                ContractType::CancelAllUnfreezeV2Contract.to_i8(),
            ],
            resource: vec![
                ContractType::DelegateResourceContract.to_i8(),
                ContractType::UnDelegateResourceContract.to_i8(),
            ],
            accounts: vec![
                ContractType::AccountCreateContract.to_i8(),
                ContractType::AccountUpdateContract.to_i8(),
            ],
            trx_assets: vec![
                ContractType::TransferAssetContract.to_i8(),
                ContractType::AssetIssueContract.to_i8(),
                ContractType::UpdateAssetContract.to_i8(),
                ContractType::ParticipateAssetIssueContract.to_i8(),
                ContractType::UnfreezeAssetContract.to_i8(),
            ],
            contract: vec![
                ContractType::CreateSmartContract.to_i8(),
                ContractType::TriggerSmartContract.to_i8(),
                ContractType::UpdateSettingContract.to_i8(),
                ContractType::UpdateEnergyLimitContract.to_i8(),
                ContractType::ClearABIContract.to_i8(),
            ],
            super_present: vec![
                ContractType::VoteWitnessContract.to_i8(),
                ContractType::WithdrawBalanceContract.to_i8(),
                ContractType::ProposalCreateContract.to_i8(),
                ContractType::ProposalApproveContract.to_i8(),
                ContractType::ProposalDeleteContract.to_i8(),
                ContractType::WitnessCreateContract.to_i8(),
                ContractType::WitnessUpdateContract.to_i8(),
                ContractType::UpdateBrokerageContract.to_i8(),
            ],
            bancor: vec![
                ContractType::ExchangeCreateContract.to_i8(),
                ContractType::ExchangeInjectContract.to_i8(),
                ContractType::ExchangeWithdrawContract.to_i8(),
                ContractType::ExchangeTransactionContract.to_i8(),
            ],
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPermission {
    pub owner: PermissionResp,
    pub actives: Vec<PermissionResp>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResp {
    pub name: String,
    pub active_id: Option<i8>,
    pub threshold: i8,
    pub operations: Option<Vec<i8>>,
    pub keys: Vec<Keys>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerPermissionResp {
    pub grantor_addr: String,
    pub name: String,
    pub permission: PermissionResp,
}

impl TryFrom<&wallet_chain_interact::tron::operations::multisig::Permission> for PermissionResp {
    type Error = crate::error::service::ServiceError;

    fn try_from(
        value: &wallet_chain_interact::tron::operations::multisig::Permission,
    ) -> Result<Self, Self::Error> {
        let (operations, id) = if value.permission_name == "owner" {
            (None, None)
        } else {
            let operations = PermissionTypes::from_hex(value.operations.as_ref().unwrap())?;
            (Some(operations), value.id)
        };

        Ok(PermissionResp {
            name: value.permission_name.clone(),
            threshold: value.threshold as i8,
            active_id: id,
            operations,
            keys: value.keys.iter().map(Keys::from).collect(),
        })
    }
}

impl TryFrom<&PermissionWithUserEntity> for PermissionResp {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: &PermissionWithUserEntity) -> Result<Self, Self::Error> {
        let operations = PermissionTypes::from_hex(&value.permission.operations)?;

        let keys = value
            .user
            .iter()
            .map(|u| Keys::new(u.address.clone(), u.weight as i32, u.is_self))
            .collect();

        Ok(PermissionResp {
            name: value.permission.name.clone(),
            threshold: value.permission.threshold as i8,
            active_id: Some(value.permission.active_id as i8),
            operations: Some(operations),
            keys,
        })
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Keys {
    pub name: String,
    pub address: String,
    pub is_self: i64,
    weight: i32,
}

impl Keys {
    pub fn new(address: String, weight: i32, is_self: i64) -> Self {
        Self { name: String::new(), address, weight, is_self }
    }
}

impl From<&wallet_chain_interact::tron::operations::multisig::Keys> for Keys {
    fn from(value: &wallet_chain_interact::tron::operations::multisig::Keys) -> Self {
        Keys {
            name: String::new(),
            address: value.address.clone(),
            weight: value.weight,
            is_self: 0,
        }
    }
}
