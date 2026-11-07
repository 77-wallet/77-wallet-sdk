use std::fmt;

pub struct ResetRootReq {
    pub language_code: u8,
    pub phrase: String,
    pub salt: String,
    pub wallet_address: String,
    pub new_password: String,
    pub subkey_password: Option<String>,
}

impl fmt::Debug for ResetRootReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResetRootReq")
            .field("language_code", &self.language_code)
            .field("phrase", &"<redacted>")
            .field("salt", &"<redacted>")
            .field("wallet_address", &self.wallet_address)
            .field("new_password", &"<redacted>")
            .field("subkey_password", &self.subkey_password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(serde::Deserialize)]
pub struct CreateWalletReq {
    pub language_code: u8,
    pub phrase: String,
    pub salt: String,
    pub wallet_name: String,
    pub account_name: String,
    pub is_default_name: bool,
    pub wallet_password: String,
    pub derive_password: Option<String>,
    // 邀请码
    pub invite_code: Option<String>,
}

impl CreateWalletReq {
    pub fn new(
        language_code: u8,
        phrase: &str,
        salt: &str,
        wallet_name: &str,
        account_name: &str,
        is_default_name: bool,
        wallet_password: &str,
        derive_password: Option<String>,
        invite_code: Option<String>,
    ) -> Self {
        Self {
            language_code,
            phrase: phrase.to_string(),
            salt: salt.to_string(),
            wallet_name: wallet_name.to_string(),
            account_name: account_name.to_string(),
            is_default_name,
            wallet_password: wallet_password.to_string(),
            derive_password,
            invite_code,
        }
    }
}

impl Default for CreateWalletReq {
    fn default() -> Self {
        CreateWalletReq::new(
            1,
            "",
            "test-salt",
            "wallet",
            "account",
            true,
            "super-secret",
            Some("derive-secret".to_string()),
            Some("invite".to_string()),
        )
    }
}

impl fmt::Debug for CreateWalletReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateWalletReq")
            .field("language_code", &self.language_code)
            .field("phrase", &"<redacted>")
            .field("salt", &"<redacted>")
            .field("wallet_name", &self.wallet_name)
            .field("account_name", &self.account_name)
            .field("is_default_name", &self.is_default_name)
            .field("wallet_password", &"<redacted>")
            .field("derive_password", &self.derive_password.as_ref().map(|_| "<redacted>"))
            .field("invite_code", &self.invite_code)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{CreateWalletReq, ResetRootReq};

    #[test]
    fn create_wallet_debug_redacts_sensitive_fields() {
        let req = CreateWalletReq::default();

        let debug = format!("{req:?}");
        assert!(!debug.contains("phrase words"));
        assert!(!debug.contains("test-salt"));
        assert!(!debug.contains("super-secret"));
        assert!(!debug.contains("derive-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn reset_root_debug_redacts_sensitive_fields() {
        let req = ResetRootReq {
            language_code: 1,
            phrase: "phrase words".to_string(),
            salt: "test-salt".to_string(),
            wallet_address: "wallet".to_string(),
            new_password: "super-secret".to_string(),
            subkey_password: Some("sub-secret".to_string()),
        };

        let debug = format!("{req:?}");
        assert!(!debug.contains("phrase words"));
        assert!(!debug.contains("test-salt"));
        assert!(!debug.contains("super-secret"));
        assert!(!debug.contains("sub-secret"));
        assert!(debug.contains("<redacted>"));
    }
}
