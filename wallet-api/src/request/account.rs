use std::fmt;

#[derive(serde::Deserialize, Clone)]
pub struct CreateAccountReq {
    pub wallet_address: String,
    pub root_password: String,
    pub derive_password: Option<String>,
    pub derivation_path: Option<String>,
    pub index: Option<i32>,
    pub name: String,
    pub is_default_name: bool,
}

impl CreateAccountReq {
    pub fn new(
        wallet_address: &str,
        root_password: &str,
        derive_password: Option<String>,
        derivation_path: Option<String>,
        index: Option<i32>,
        name: &str,
        is_default_name: bool,
    ) -> Self {
        Self {
            wallet_address: wallet_address.to_string(),
            root_password: root_password.to_string(),
            derive_password,
            derivation_path,
            index,
            name: name.to_string(),
            is_default_name,
        }
    }
}

impl Default for CreateAccountReq {
    fn default() -> Self {
        CreateAccountReq::new(
            "0xD0141DfbF3cAd251CC0665A85374E5B5aDa4BAfd",
            "super-secret",
            Some("derive-secret".to_string()),
            Some("m/44'/60'/0'/0/0".to_string()),
            Some(1),
            "name",
            false,
        )
    }
}

impl fmt::Debug for CreateAccountReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateAccountReq")
            .field("wallet_address", &self.wallet_address)
            .field("root_password", &"<redacted>")
            .field("derive_password", &self.derive_password.as_ref().map(|_| "<redacted>"))
            .field("derivation_path", &self.derivation_path)
            .field("index", &self.index)
            .field("name", &self.name)
            .field("is_default_name", &self.is_default_name)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::CreateAccountReq;

    #[test]
    fn create_account_debug_redacts_passwords() {
        let req = CreateAccountReq::new(
            "wallet",
            "super-secret",
            Some("derive-secret".to_string()),
            Some("m/44'/60'/0'/0/0".to_string()),
            Some(1),
            "name",
            false,
        );

        let debug = format!("{req:?}");
        assert!(!debug.contains("super-secret"));
        assert!(!debug.contains("derive-secret"));
        assert!(debug.contains("<redacted>"));
    }
}
