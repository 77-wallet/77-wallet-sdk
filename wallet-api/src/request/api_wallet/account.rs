use std::fmt;

use wallet_database::entities::api_wallet::ApiWalletType;

#[derive(serde::Deserialize, Clone)]
pub struct CreateApiAccountReq {
    pub wallet_address: String,
    pub wallet_password: String,
    pub indices: Vec<i32>,
    pub name: String,
    pub is_default_name: bool,
    pub api_wallet_type: ApiWalletType,
}

impl CreateApiAccountReq {
    pub fn new(
        wallet_address: &str,
        wallet_password: &str,
        indices: Vec<i32>,
        name: &str,
        is_default_name: bool,
        api_wallet_type: ApiWalletType,
    ) -> Self {
        Self {
            wallet_address: wallet_address.to_string(),
            wallet_password: wallet_password.to_string(),
            indices,
            name: name.to_string(),
            is_default_name,
            api_wallet_type,
        }
    }
}

impl fmt::Debug for CreateApiAccountReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateApiAccountReq")
            .field("wallet_address", &self.wallet_address)
            .field("wallet_password", &"<redacted>")
            .field("indices", &self.indices)
            .field("name", &self.name)
            .field("is_default_name", &self.is_default_name)
            .field("api_wallet_type", &self.api_wallet_type)
            .finish()
    }
}

#[derive(serde::Deserialize, Clone)]
pub struct CreateWithdrawalAccountReq {
    pub wallet_address: String,
    pub wallet_password: String,
    pub derivation_path: Option<String>,
    pub index: Option<i32>,
    pub name: String,
    pub is_default_name: bool,
}

impl CreateWithdrawalAccountReq {
    pub fn new(
        wallet_address: &str,
        wallet_password: &str,
        derivation_path: Option<String>,
        index: Option<i32>,
        name: &str,
        is_default_name: bool,
    ) -> Self {
        Self {
            wallet_address: wallet_address.to_string(),
            wallet_password: wallet_password.to_string(),
            derivation_path,
            index,
            name: name.to_string(),
            is_default_name,
        }
    }
}

impl fmt::Debug for CreateWithdrawalAccountReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateWithdrawalAccountReq")
            .field("wallet_address", &self.wallet_address)
            .field("wallet_password", &"<redacted>")
            .field("derivation_path", &self.derivation_path)
            .field("index", &self.index)
            .field("name", &self.name)
            .field("is_default_name", &self.is_default_name)
            .finish()
    }
}

/// 地址搜索请求
#[derive(serde::Deserialize, Clone)]
pub struct ApiWalletAddressSearchReq {
    pub uid: String,
    pub keyword: String,
}

impl ApiWalletAddressSearchReq {
    pub fn new(uid: &str, keyword: &str) -> Self {
        Self { uid: uid.to_string(), keyword: keyword.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::{CreateApiAccountReq, CreateWithdrawalAccountReq};
    use wallet_database::entities::api_wallet::ApiWalletType;

    #[test]
    fn api_account_debug_redacts_password() {
        let req = CreateApiAccountReq::new(
            "wallet",
            "super-secret",
            vec![1, 2],
            "name",
            true,
            ApiWalletType::SubAccount,
        );

        let debug = format!("{req:?}");
        assert!(!debug.contains("super-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn withdrawal_account_debug_redacts_password() {
        let req = CreateWithdrawalAccountReq::new(
            "wallet",
            "super-secret",
            Some("m/44'/60'/0'/0/0".to_string()),
            Some(1),
            "name",
            true,
        );

        let debug = format!("{req:?}");
        assert!(!debug.contains("super-secret"));
        assert!(debug.contains("<redacted>"));
    }
}
