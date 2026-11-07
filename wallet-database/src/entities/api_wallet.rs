use std::fmt;

#[derive(serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ApiWalletEntity {
    pub id: i64,
    pub name: String,
    pub uid: String,
    pub address: String,
    #[serde(skip_serializing)]
    pub phrase: Vec<u8>,
    #[serde(skip_serializing)]
    pub seed: Vec<u8>,
    pub binding_address: Option<String>,
    pub api_wallet_type: ApiWalletType,
    pub merchant_id: Option<String>,
    pub app_id: Option<String>,
    pub sn: Option<String>,
    pub status: u8,
    pub is_init: u16,
    pub import_stage: u8,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

impl fmt::Debug for ApiWalletEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiWalletEntity")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("uid", &self.uid)
            .field("address", &self.address)
            .field("phrase", &"<redacted>")
            .field("seed", &"<redacted>")
            .field("binding_address", &self.binding_address)
            .field("api_wallet_type", &self.api_wallet_type)
            .field("merchant_id", &self.merchant_id)
            .field("app_id", &self.app_id)
            .field("sn", &self.sn)
            .field("status", &self.status)
            .field("is_init", &self.is_init)
            .field("import_stage", &self.import_stage)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    serde_repr::Deserialize_repr,
    serde_repr::Serialize_repr,
    sqlx::Type,
    PartialEq,
)]
#[repr(u8)]
pub enum ApiWalletType {
    SubAccount = 1,
    Withdrawal = 2,
}

impl TryFrom<u8> for ApiWalletType {
    type Error = crate::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ApiWalletType::SubAccount),
            2 => Ok(ApiWalletType::Withdrawal),
            _ => Err(crate::Error::InvalidValue(value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiWalletEntity, ApiWalletType};
    use sqlx::types::chrono::{TimeZone, Utc};

    #[test]
    fn api_wallet_type_try_from_accepts_only_1_and_2() {
        assert!(matches!(ApiWalletType::try_from(1), Ok(ApiWalletType::SubAccount)));
        assert!(matches!(ApiWalletType::try_from(2), Ok(ApiWalletType::Withdrawal)));
        assert!(ApiWalletType::try_from(0).is_err());
        assert!(ApiWalletType::try_from(3).is_err());
    }

    #[test]
    fn api_wallet_entity_debug_redacts_phrase_and_seed() {
        let req = ApiWalletEntity {
            id: 1,
            name: "wallet".to_string(),
            uid: "uid".to_string(),
            address: "addr".to_string(),
            phrase: b"phrase words".to_vec(),
            seed: b"seed bytes".to_vec(),
            binding_address: None,
            api_wallet_type: ApiWalletType::SubAccount,
            merchant_id: None,
            app_id: None,
            sn: Some("sn".to_string()),
            status: 1,
            is_init: 1,
            import_stage: 0,
            created_at: Utc.timestamp_opt(0, 0).single().unwrap(),
            updated_at: None,
        };

        let debug = format!("{req:?}");
        assert!(!debug.contains("phrase words"));
        assert!(!debug.contains("seed bytes"));
        assert!(debug.contains("<redacted>"));
    }
}
