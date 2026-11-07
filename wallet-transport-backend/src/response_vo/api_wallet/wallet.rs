#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeysUidCheckRes {
    pub uid: String,
    pub status: UidStatus,
}

impl KeysUidCheckRes {
    pub fn is_normal_wallet(&self) -> bool {
        self.status == UidStatus::NormalWallet
    }

    pub fn is_api_wallet(&self) -> bool {
        self.status == UidStatus::ApiRaw || self.status == UidStatus::ApiWaw
    }

    pub fn is_sub_account_wallet(&self) -> bool {
        self.status == UidStatus::ApiRaw
    }

    pub fn is_withdrawal_wallet(&self) -> bool {
        self.status == UidStatus::ApiWaw
    }

    pub fn is_not_found(&self) -> bool {
        self.status == UidStatus::NotFound
    }
}

// #[derive(Debug, serde_repr::Deserialize_repr, serde_repr::Serialize_repr, PartialEq, Eq)]
#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
// #[repr(u8)]
pub enum UidStatus {
    /// 普通钱包
    NormalWallet,
    /// API钱包-收款钱包
    ApiRaw,
    /// API钱包-出款钱包
    ApiWaw,
    /// 4 UID不存在
    NotFound,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryWalletActivationInfoResp(pub Vec<QueryWalletActivationInfoRespItem>);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryWalletActivationInfoRespItem {
    pub chain: String,
    pub active: ActiveStatus,
}

#[derive(Debug, serde_repr::Deserialize_repr, serde_repr::Serialize_repr, Clone)]
#[repr(u8)]
pub enum ActiveStatus {
    Active = 1,
    Inactive = 0,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryUidBindInfoRes {
    // pub sn: String,
    pub app_id: String,
    /// 商户id
    pub org_id: String,
    pub bind_status: bool,
    pub sn: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIdUidUsageRes {
    pub used: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfigResp {
    pub configs: AppWarningConfigs,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppWarningConfigs {
    // Deliberately required: missing/malformed data must not become an empty snapshot.
    pub balance_notify_cfg_list: Vec<AppBalanceNotifyConfig>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBalanceNotifyConfig {
    pub amount: rust_decimal::Decimal,
    pub chain_code: String,
    pub contract_address: String,
    pub status: bool,
    pub token_code: String,
}
