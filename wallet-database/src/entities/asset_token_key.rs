use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum AssetTokenKey {
    #[default]
    Native,
    Contract(String),
}

impl AssetTokenKey {
    pub fn from_raw(token: Option<&str>) -> Self {
        match token.map(str::trim) {
            Some(token) if !token.is_empty() => Self::Contract(token.to_string()),
            _ => Self::Native,
        }
    }

    /// Canonical comparison/event key for EVM contracts. Other chains retain case.
    /// Keep DB decoding unchanged so writes can reuse legacy stored spellings.
    pub fn normalized_for_chain(&self, chain_code: &str) -> Self {
        match self {
            Self::Contract(token)
                if crate::entities::api_withdraw::is_signed_evm_chain(chain_code) =>
            {
                Self::Contract(token.to_ascii_lowercase())
            }
            _ => self.clone(),
        }
    }

    pub fn matches_for_chain(&self, other: &Self, chain_code: &str) -> bool {
        match (self, other) {
            (Self::Contract(left), Self::Contract(right))
                if crate::entities::api_withdraw::is_signed_evm_chain(chain_code) =>
            {
                left.eq_ignore_ascii_case(right)
            }
            _ => self == other,
        }
    }

    pub fn from_db_value(token: &str) -> Self {
        Self::from_raw(Some(token))
    }

    pub fn as_db_str(&self) -> &str {
        match self {
            Self::Native => "",
            Self::Contract(token) => token.as_str(),
        }
    }

    pub fn to_chain_token_option(&self) -> Option<String> {
        match self {
            Self::Native => None,
            Self::Contract(token) => {
                let normalized = token.trim();
                if normalized.is_empty() { None } else { Some(normalized.to_string()) }
            }
        }
    }

    pub fn into_chain_token_option(self) -> Option<String> {
        match self {
            Self::Native => None,
            Self::Contract(token) => {
                let normalized = token.trim();
                if normalized.is_empty() { None } else { Some(normalized.to_string()) }
            }
        }
    }

    pub fn to_api_token_option_legacy(&self) -> Option<String> {
        Some(self.as_db_str().to_string())
    }

    pub fn into_api_token_option_legacy(self) -> Option<String> {
        Some(match self {
            Self::Native => String::new(),
            Self::Contract(token) => token,
        })
    }

    pub fn to_option_string_for_api(&self) -> Option<String> {
        self.to_api_token_option_legacy()
    }

    pub fn into_option_string_for_api(self) -> Option<String> {
        self.into_api_token_option_legacy()
    }

    pub fn is_native(&self) -> bool {
        matches!(self, Self::Native)
    }

    pub fn is_contract(&self) -> bool {
        matches!(self, Self::Contract(_))
    }
}

impl std::fmt::Display for AssetTokenKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db_str())
    }
}

impl From<Option<String>> for AssetTokenKey {
    fn from(value: Option<String>) -> Self {
        Self::from_raw(value.as_deref())
    }
}

impl From<String> for AssetTokenKey {
    fn from(value: String) -> Self {
        Self::from_raw(Some(value.as_str()))
    }
}

impl From<&str> for AssetTokenKey {
    fn from(value: &str) -> Self {
        Self::from_raw(Some(value))
    }
}

impl PartialEq<Option<String>> for AssetTokenKey {
    fn eq(&self, other: &Option<String>) -> bool {
        self == &AssetTokenKey::from(other.clone())
    }
}

impl Serialize for AssetTokenKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_db_str())
    }
}

impl<'de> Deserialize<'de> for AssetTokenKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Option::<String>::deserialize(deserializer)?;
        Ok(Self::from_raw(raw.as_deref()))
    }
}

impl sqlx::Type<sqlx::Sqlite> for AssetTokenKey {
    fn type_info() -> <sqlx::Sqlite as sqlx::Database>::TypeInfo {
        <String as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Sqlite> for AssetTokenKey {
    fn encode_by_ref(
        &self,
        buf: &mut <sqlx::Sqlite as sqlx::Database>::ArgumentBuffer<'q>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        buf.push(sqlx::sqlite::SqliteArgumentValue::Text(self.as_db_str().to_string().into()));
        Ok(sqlx::encode::IsNull::No)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for AssetTokenKey {
    fn decode(
        value: <sqlx::Sqlite as sqlx::Database>::ValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let raw = <String as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;
        Ok(Self::from_db_value(&raw))
    }
}

#[cfg(test)]
mod tests {
    use super::AssetTokenKey;

    #[test]
    fn token_case_canonicalizes_only_supported_evm_contracts() {
        let mixed = AssetTokenKey::from(" 0xaBc ");
        let lower = AssetTokenKey::from("0xabc");
        for chain in ["eth", "bnb", "polygon", "avalanche", "fantom", "arbitrum", "base", "scroll"]
        {
            assert_eq!(mixed.normalized_for_chain(chain), lower);
            assert!(mixed.matches_for_chain(&lower, chain));
            assert_eq!(AssetTokenKey::Native.normalized_for_chain(chain), AssetTokenKey::Native);
            assert!(!mixed.matches_for_chain(&AssetTokenKey::Native, chain));
        }
        for chain in ["tron", "sol", "sui", "ton", "btc", "unknown"] {
            assert_eq!(mixed.normalized_for_chain(chain), mixed);
            assert!(!mixed.matches_for_chain(&lower, chain));
        }
    }

    #[test]
    fn asset_token_key_normalizes_native_inputs() {
        assert_eq!(AssetTokenKey::from_raw(None), AssetTokenKey::Native);
        assert_eq!(AssetTokenKey::from_raw(Some("")), AssetTokenKey::Native);
        assert_eq!(AssetTokenKey::from_raw(Some("   ")), AssetTokenKey::Native);
    }

    #[test]
    fn asset_token_key_normalizes_contract_inputs() {
        assert_eq!(
            AssetTokenKey::from_raw(Some(" 0xabc ")),
            AssetTokenKey::Contract("0xabc".to_string())
        );
    }

    #[test]
    fn asset_token_key_keeps_db_encoding_compatible() {
        assert_eq!(AssetTokenKey::Native.as_db_str(), "");
        assert_eq!(AssetTokenKey::Contract("0xabc".to_string()).as_db_str(), "0xabc");
    }

    #[test]
    fn asset_token_key_chain_option_normalizes_blank_contract() {
        assert_eq!(AssetTokenKey::Contract("".to_string()).to_chain_token_option(), None);
        assert_eq!(AssetTokenKey::Contract("   ".to_string()).to_chain_token_option(), None);
    }
}
