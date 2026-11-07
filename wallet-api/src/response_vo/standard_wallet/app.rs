use wallet_database::entities::{
    api_wallet::ApiWalletEntity, device::DeviceEntity, multisig_account::MultisigAccountEntity,
    multisig_queue::MultisigQueueEntity, wallet::WalletEntity,
};

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetConfigRes {
    pub fiat: String,
    pub language: String,
    pub unread_count: UnreadCount,
    pub standard_wallet_list: Vec<ConfigWalletInfo>,
    pub api_wallet_list: ConfigApiWalletList,
    pub device_info: Option<DeviceEntity>,
    pub url: crate::request::init::UrlParams,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigWalletInfo {
    pub address: String,
    pub uid: String,
    pub name: String,
    pub app_id: Option<String>,
}

impl From<WalletEntity> for ConfigWalletInfo {
    fn from(value: WalletEntity) -> Self {
        ConfigWalletInfo { address: value.address, uid: value.uid, name: value.name, app_id: None }
    }
}

impl From<&ApiWalletEntity> for ConfigWalletInfo {
    fn from(value: &ApiWalletEntity) -> Self {
        ConfigWalletInfo {
            address: value.address.clone(),
            uid: value.uid.clone(),
            name: value.name.clone(),
            app_id: value.app_id.clone(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigApiWalletList(pub Vec<ConfigApiWalletItem>);

impl std::ops::Deref for ConfigApiWalletList {
    type Target = Vec<ConfigApiWalletItem>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for ConfigApiWalletList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ConfigApiWalletList {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, item: ConfigApiWalletItem) {
        self.0.push(item);
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigApiWalletItem {
    pub recharge_wallet: Option<ConfigWalletInfo>,
    pub withdraw_wallet: Option<ConfigWalletInfo>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadCount {
    pub system_notification: i64,
    pub announcement: i64,
}

#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GlobalMsg {
    // 待处理的交易(通用了 account base 结构)
    pub pending_multisig_trans: Vec<MultisigAccountBase>,
    // 待部署的多签
    pub pending_deploy_multisig: Vec<MultisigAccountBase>,
    // 同意的多签数量
    pub pending_agree_multisig: Vec<MultisigAccountBase>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultisigAccountBase {
    pub id: String,
    pub address: String,
    pub status: Option<i32>,
}

impl From<MultisigAccountEntity> for MultisigAccountBase {
    fn from(entity: MultisigAccountEntity) -> Self {
        MultisigAccountBase { id: entity.id, address: entity.address, status: None }
    }
}

impl From<&MultisigQueueEntity> for MultisigAccountBase {
    fn from(entity: &MultisigQueueEntity) -> Self {
        MultisigAccountBase {
            id: entity.id.clone(),
            address: entity.from_addr.clone(),
            status: Some(entity.status as i32),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfigApiWalletItem, ConfigApiWalletList, ConfigWalletInfo};

    #[test]
    fn get_config_wallet_payload_omits_runtime_fields() {
        let list = ConfigApiWalletList(vec![ConfigApiWalletItem {
            recharge_wallet: Some(ConfigWalletInfo {
                address: "address".to_string(),
                uid: "uid".to_string(),
                name: "name".to_string(),
                app_id: Some("app".to_string()),
            }),
            withdraw_wallet: None,
        }]);

        let value = serde_json::to_value(list).expect("serialize config api wallet list");
        let wallet = &value[0]["rechargeWallet"];

        assert_eq!(wallet["address"], "address");
        assert_eq!(wallet["uid"], "uid");
        assert_eq!(wallet["name"], "name");
        assert_eq!(wallet["appId"], "app");
        assert!(wallet.get("balance").is_none());
        assert!(wallet.get("accounts").is_none());
    }
}
