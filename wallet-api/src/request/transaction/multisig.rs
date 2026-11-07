use std::fmt;
use wallet_database::entities::asset_token_key::AssetTokenKey;

pub struct ServiceFeePayer {
    pub from: String,
    pub chain_code: String,
    pub symbol: String,
    pub fee_setting: Option<String>,
    pub request_resource_id: Option<String>,
    pub token_address: Option<String>,
}

impl ServiceFeePayer {
    pub fn token_key(&self) -> AssetTokenKey {
        AssetTokenKey::from_raw(self.token_address.as_deref())
    }
}

pub struct DeployFeePayer {
    pub account_id: String,
    pub fee_setting: String,
}

pub struct Executor {
    pub address: String,
    pub password: String,
}

impl fmt::Debug for Executor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Executor")
            .field("address", &self.address)
            .field("password", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::Executor;

    #[test]
    fn executor_debug_redacts_password() {
        let req = Executor { address: "addr".to_string(), password: "super-secret".to_string() };

        let debug = format!("{req:?}");
        assert!(!debug.contains("super-secret"));
        assert!(debug.contains("<redacted>"));
    }
}
