use crate::domain::account::AccountDomain;
use wallet_database::entities::{api_account::ApiAccountEntity, api_wallet::ApiWalletType};
use wallet_types::chain::address::category::AddressCategory;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiAccountInfo {
    pub account_id: u32,
    pub account_index_map: wallet_utils::address::AccountIndexMap,
    pub name: String,
    pub balance: crate::response_vo::standard_wallet::account::BalanceInfo,
    pub chain: Vec<ApiAccountChainInfo>,
    pub api_wallet_type: ApiWalletType,
}

/// API 钱包账户的链展示信息。
///
/// API 钱包账户列表请求已经由 `wallet_address` 锁定钱包范围，所以这里不再返回
/// 普通钱包 `ChainInfo` 里的所属钱包地址，避免 App 展示一个对用户无意义的字段。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiAccountChainInfo {
    pub address: String,
    pub derivation_path: String,
    pub chain_code: String,
    pub name: Option<String>,
    pub address_type: AddressCategory,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiAccountInfos(pub Vec<ApiAccountInfo>);

impl std::ops::Deref for ApiAccountInfos {
    type Target = Vec<ApiAccountInfo>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for ApiAccountInfos {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryApiAccountDerivationPath {
    pub address: String,
    pub derivation_path: String,
    pub chain_code: String,
    pub address_type: AddressCategory,
}

impl QueryApiAccountDerivationPath {
    pub fn new(
        address: &str,
        derivation_path: &str,
        chain_code: &str,
        address_type: AddressCategory,
    ) -> Self {
        Self {
            address: address.to_string(),
            derivation_path: derivation_path.to_string(),
            chain_code: chain_code.to_string(),
            address_type,
        }
    }
}

impl TryFrom<ApiAccountEntity> for QueryApiAccountDerivationPath {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: ApiAccountEntity) -> Result<Self, Self::Error> {
        let address_type =
            AccountDomain::get_show_address_type(&value.chain_code, value.address_type())?;

        Ok(QueryApiAccountDerivationPath {
            address: value.address,
            derivation_path: value.derivation_path,
            chain_code: value.chain_code,
            address_type,
        })
    }
}

/// 地址搜索结果项
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWalletAddressSearchItem {
    pub account_id: u32,
    pub account_name: Option<String>,
    pub address: String,
    pub chain_code: String,
}

impl From<ApiAccountEntity> for ApiWalletAddressSearchItem {
    fn from(entity: ApiAccountEntity) -> Self {
        Self {
            account_id: entity.account_id,
            account_name: Some(entity.name),
            address: entity.address,
            chain_code: entity.chain_code,
        }
    }
}

/// 地址搜索响应
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWalletAddressSearchResp {
    pub items: Vec<ApiWalletAddressSearchItem>,
}

#[cfg(test)]
mod tests {
    use super::{ApiAccountChainInfo, ApiAccountInfo};
    use crate::response_vo::standard_wallet::account::BalanceInfo;
    use wallet_database::entities::api_wallet::ApiWalletType;
    use wallet_types::chain::address::category::AddressCategory;

    #[test]
    fn api_wallet_account_chain_info_omits_owner_wallet_address() {
        let account = ApiAccountInfo {
            account_id: 1,
            account_index_map: wallet_utils::address::AccountIndexMap::from_account_id(1)
                .expect("valid account id"),
            name: "account-1".to_string(),
            balance: BalanceInfo {
                amount: 0.0,
                currency: "USD".to_string(),
                unit_price: Some(0.0),
                fiat_value: Some(0.0),
            },
            chain: vec![ApiAccountChainInfo {
                address: "TQn9Y2khEsLJW1ChVWFMSMeRDow5KcbLSE".to_string(),
                derivation_path: "m/44'/195'/0'/0/0".to_string(),
                chain_code: "tron".to_string(),
                name: Some("TRON".to_string()),
                address_type: AddressCategory::Other,
            }],
            api_wallet_type: ApiWalletType::SubAccount,
        };

        let value = serde_json::to_value(&account).expect("serialize account info");

        assert_eq!(value["chain"][0]["address"], "TQn9Y2khEsLJW1ChVWFMSMeRDow5KcbLSE");
        assert!(value["chain"][0].get("walletAddress").is_none());
    }
}
