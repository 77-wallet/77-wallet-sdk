#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ApiNonceEntity {
    pub id: i64,
    pub uid: String,
    pub name: String,
    pub from_addr: String,
    pub chain_code: String,
    pub nonce: u64,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

/// Identity for shared nonce ownership and in-memory gates. Other chains keep
/// their case-sensitive address semantics; order/audit strings are not rewritten.
pub fn nonce_address_key(address: &str, chain: &str) -> String {
    if super::api_withdraw::is_signed_evm_chain(chain) {
        // Match SQLite trim()/lower() exactly. Tabs and other invalid address
        // characters are not silently treated as alternate valid spellings.
        address.trim_matches(' ').to_ascii_lowercase()
    } else {
        address.to_owned()
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn nonce_identity_matches_sql_ascii_normalization() {
        assert_eq!(nonce_address_key(" AbC ", "eth"), "abc");
        assert_eq!(nonce_address_key("\tAbC\t", "eth"), "\tabc\t");
        assert_eq!(nonce_address_key(" AbC ", "sol"), " AbC ");
    }
}
