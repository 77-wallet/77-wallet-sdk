use crate::response_vo::api_wallet::wallet::UidStatus;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindAppIdReq {
    recharge_uid: String,
    withdrawal_uid: String,
    org_app_id: String,
    sn: String,
}

impl BindAppIdReq {
    pub fn new(recharge_uid: &str, withdrawal_uid: &str, org_app_id: &str, sn: &str) -> Self {
        Self {
            recharge_uid: recharge_uid.to_string(),
            withdrawal_uid: withdrawal_uid.to_string(),
            org_app_id: org_app_id.to_string(),
            sn: sn.to_string(),
        }
    }
}

pub type UnBindAppIdReq = BindAppIdReq;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWalletActivationConfigReq {
    chain_code: String,
    uid: String,
    address_list: Vec<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIdImportReq {
    sn: String,
    recharge_uid: Option<String>,
    withdrawal_uid: Option<String>,
}

impl AppIdImportReq {
    pub fn new(sn: &str) -> Self {
        Self { sn: sn.to_string(), recharge_uid: None, withdrawal_uid: None }
    }

    pub fn set_recharge_uid(&mut self, recharge_uid: &str) {
        self.recharge_uid = Some(recharge_uid.to_string());
    }

    pub fn set_withdrawal_uid(&mut self, withdrawal_uid: &str) {
        self.withdrawal_uid = Some(withdrawal_uid.to_string());
    }
}

pub type InitApiWalletReq = AppIdImportReq;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIdUidUsageReq {
    org_app_id: String,
    uid: String,
    wallet_type: UidStatus,
}

impl AppIdUidUsageReq {
    pub fn new(org_app_id: &str, uid: &str, wallet_type: UidStatus) -> Self {
        Self { org_app_id: org_app_id.to_string(), uid: uid.to_string(), wallet_type }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIdImportRechargeWalletReq {
    sn: String,
    recharge_uid: String,
}

impl AppIdImportRechargeWalletReq {
    pub fn new(sn: &str, recharge_uid: &str) -> Self {
        Self { sn: sn.to_string(), recharge_uid: recharge_uid.to_string() }
    }
}

/// Scope of a merchant application's latest configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfigReq {
    pub app_id: String,
    pub org_id: String,
}
