pub mod inventory;
pub mod protected;
pub(crate) mod v1;
pub(crate) mod v2;

use std::{
    collections::BTreeMap,
    fmt,
    ops::{Deref, DerefMut},
    path::Path,
};

use secrecy::zeroize::Zeroize;
use wallet_crypto::{KdfAlgorithm, KeystoreBuilder, RecoverableData};

use crate::naming::v2::KeyMeta;

pub trait IoStrategy: Send + Sync {
    fn store(
        &self,
        name: &str,
        data: &dyn AsRef<[u8]>,
        file_path: &dyn AsRef<Path>,
        password: &str,
        algorithm: KdfAlgorithm,
    ) -> Result<(), crate::Error>;

    fn load_custom(
        &self,
        subs_dir: &dyn AsRef<Path>,
        name: &str,
        password: &str,
    ) -> Result<RecoverableData, crate::Error> {
        let data = KeystoreBuilder::new_decrypt(subs_dir.as_ref().join(name), password).load()?;

        Ok(data)
    }

    fn load_root(
        &self,
        wallet_address: &str,
        root_dir: &dyn AsRef<std::path::Path>,
        password: &str,
    ) -> Result<RootData, crate::Error>;

    fn load_subkey(
        &self,
        account_index_map: Option<&wallet_utils::address::AccountIndexMap>,
        address: &str,
        chain_code: &str,
        derivation_path: &str,
        subs_dir: &dyn AsRef<std::path::Path>,
        password: &str,
    ) -> Result<Vec<u8>, crate::Error>;

    fn load_account(
        &self,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        subs_dir: &dyn AsRef<std::path::Path>,
        password: &str,
    ) -> Result<AccountData, crate::Error>;

    fn delete_root(
        &self,
        address: &str,
        root_dir: &dyn AsRef<std::path::Path>,
    ) -> Result<(), crate::Error>;

    fn store_root(
        &self,
        address: &str,
        seed: &[u8],
        phrase: &str,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
        algorithm: KdfAlgorithm,
    ) -> Result<(), crate::Error>;

    fn store_subkey(
        &self,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        address: &str,
        chain_code: &str,
        derivation_path: &str,
        data: &dyn AsRef<[u8]>,
        file_path: &dyn AsRef<Path>,
        password: &str,
        algorithm: KdfAlgorithm,
    ) -> Result<(), crate::Error>;

    fn store_subkeys_bulk(
        &self,
        subkeys: Vec<BulkSubkey>,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
        algorithm: wallet_crypto::KdfAlgorithm,
    ) -> Result<(), crate::Error>;

    fn delete_account(
        &self,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        file_path: &dyn AsRef<std::path::Path>,
    ) -> Result<(), crate::Error>;
}

pub struct BulkSubkey {
    pub account_index_map: wallet_utils::address::AccountIndexMap,
    pub address: String,
    pub chain_code: String,
    pub derivation_path: String,
    pub data: Vec<u8>,
}

impl BulkSubkey {
    pub fn new(
        account_index_map: wallet_utils::address::AccountIndexMap,
        address: &str,
        chain_code: &str,
        derivation_path: &str,
        data: Vec<u8>,
    ) -> Self {
        Self {
            account_index_map: account_index_map.clone(),
            address: address.to_string(),
            chain_code: chain_code.to_string(),
            derivation_path: derivation_path.to_string(),
            data,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, Default)]
pub struct AccountData(BTreeMap<KeyMeta, Vec<u8>>);

impl AccountData {
    pub fn into_inner(self) -> BTreeMap<KeyMeta, Vec<u8>> {
        self.0
    }
}

impl Deref for AccountData {
    type Target = BTreeMap<KeyMeta, Vec<u8>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AccountData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct RootData {
    phrase: String,
    seed: Vec<u8>,
}

impl Zeroize for RootData {
    fn zeroize(&mut self) {
        self.phrase.zeroize();
        self.seed.zeroize();
    }
}

impl Drop for RootData {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl fmt::Debug for RootData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RootData")
            .field("phrase", &"<redacted>")
            .field("seed", &"<redacted>")
            .finish()
    }
}

impl RootData {
    pub fn new(phrase: &str, seed: &[u8]) -> Self {
        Self { phrase: phrase.to_string(), seed: seed.to_vec() }
    }

    pub fn phrase(&self) -> &str {
        &self.phrase
    }

    pub fn seed(&self) -> &[u8] {
        &self.seed
    }
}

impl TryFrom<RecoverableData> for RootData {
    type Error = crate::Error;

    fn try_from(value: RecoverableData) -> Result<Self, Self::Error> {
        Ok(wallet_utils::serde_func::serde_from_slice(&value.inner())?)
    }
}

#[cfg(test)]
mod tests {
    use super::RootData;
    use secrecy::zeroize::Zeroize;

    #[test]
    fn root_data_debug_redacts_phrase_and_seed() {
        let req = RootData::new("phrase words", &[1, 2, 3]);
        let debug = format!("{req:?}");
        assert!(!debug.contains("phrase words"));
        assert!(!debug.contains("1, 2, 3"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn root_data_zeroize_clears_phrase_and_seed() {
        let mut root = RootData::new("phrase words", &[1, 2, 3]);

        root.zeroize();

        assert!(root.phrase().is_empty());
        assert!(root.seed().is_empty());
    }
}
