use secrecy::{ExposeSecret, zeroize::Zeroize as _};
use serde::Serialize;
use wallet_crypto::{KeystoreBuilder, RecoverableData};

use crate::naming::{FileType, v1::LegacyNaming};

use super::IoStrategy;

use crate::naming::NamingStrategy as _;

#[derive(Debug, Default, PartialEq, Clone, Serialize)]
pub struct LegacyIo {
    naming: std::marker::PhantomData<crate::naming::v1::LegacyNaming>,
}

impl IoStrategy for LegacyIo {
    fn store(
        &self,
        name: &str,
        data: &dyn AsRef<[u8]>,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
        algorithm: wallet_crypto::KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        let rng = rand::thread_rng();
        KeystoreBuilder::new_encrypt(file_path, password, data, rng, algorithm, name).save()?;
        Ok(())
    }

    fn load_account(
        &self,
        _account_index_map: &wallet_utils::address::AccountIndexMap,
        _subs_dir: &dyn AsRef<std::path::Path>,
        _password: &str,
    ) -> Result<super::AccountData, crate::Error> {
        todo!()
    }

    fn load_root(
        &self,
        wallet_address: &str,
        root_dir: &dyn AsRef<std::path::Path>,
        password: &str,
    ) -> Result<super::RootData, crate::Error> {
        let seed_meta = LegacyNaming::generate_filemeta(
            FileType::Seed,
            Some(wallet_address.to_string()),
            None,
            None,
            None,
        )?;
        let seed_filename = LegacyNaming::encode(seed_meta)?;

        let seed: SeedWallet =
            KeystoreBuilder::new_decrypt(root_dir.as_ref().join(seed_filename), password)
                .load()?
                .try_into()?;

        let phrase_meta = LegacyNaming::generate_filemeta(
            FileType::Phrase,
            Some(wallet_address.to_string()),
            None,
            None,
            None,
        )?;
        let phrase_filename = LegacyNaming::encode(phrase_meta)?;

        let phrase_wallet: PhraseWallet =
            KeystoreBuilder::new_decrypt(root_dir.as_ref().join(phrase_filename), password)
                .load()?
                .try_into()?;
        let seed: &[u8] = seed.into_seed().expose_secret();

        Ok(super::RootData::new(phrase_wallet.phrase.expose_secret(), seed))
    }

    fn load_subkey(
        &self,
        account_index_map: Option<&wallet_utils::address::AccountIndexMap>,
        address: &str,
        chain_code: &str,
        derivation_path: &str,
        subs_dir: &dyn AsRef<std::path::Path>,
        password: &str,
    ) -> Result<Vec<u8>, crate::Error> {
        let pk_meta = LegacyNaming::generate_filemeta(
            FileType::DerivedData,
            Some(address.to_string()),
            account_index_map,
            Some(chain_code.to_string()),
            Some(derivation_path.to_string()),
        )?;
        let pk_filename = LegacyNaming::encode(pk_meta)?;

        let data =
            KeystoreBuilder::new_decrypt(subs_dir.as_ref().join(pk_filename), password).load()?;
        let pk: PkWallet = data.try_into()?;

        let res: &[u8] = pk.pkey().expose_secret().as_ref();
        Ok(res.to_vec())
    }

    fn store_root(
        &self,
        address: &str,
        seed: &[u8],
        phrase: &str,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
        algorithm: wallet_crypto::KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        let phrase_meta = LegacyNaming::generate_filemeta(
            FileType::Phrase,
            Some(address.to_string()),
            None,
            None,
            None,
        )?;
        let seed_meta = LegacyNaming::generate_filemeta(
            FileType::Seed,
            Some(address.to_string()),
            None,
            None,
            None,
        )?;
        let seed_filename = LegacyNaming::encode(seed_meta)?;
        let phrase_filename = LegacyNaming::encode(phrase_meta)?;

        KeystoreBuilder::new_encrypt(
            file_path,
            password,
            seed,
            rand::thread_rng(),
            algorithm.clone(),
            &seed_filename,
        )
        .save()?;

        KeystoreBuilder::new_encrypt(
            file_path,
            password,
            phrase,
            rand::thread_rng(),
            algorithm,
            &phrase_filename,
        )
        .save()?;
        Ok(())
    }

    fn store_subkey(
        &self,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        address: &str,
        chain_code: &str,
        derivation_path: &str,
        data: &dyn AsRef<[u8]>,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
        algorithm: wallet_crypto::KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        let file_meta = LegacyNaming::generate_filemeta(
            FileType::DerivedData,
            Some(address.to_string()),
            Some(account_index_map),
            Some(chain_code.to_string()),
            Some(derivation_path.to_string()),
        )?;

        let name = LegacyNaming::encode(file_meta)?;

        let rng = rand::thread_rng();
        KeystoreBuilder::new_encrypt(file_path, password, data, rng, algorithm, &name).save()?;
        Ok(())
    }

    fn store_subkeys_bulk(
        &self,
        subkeys: Vec<super::BulkSubkey>,
        file_path: &dyn AsRef<std::path::Path>,
        password: &str,
        algorithm: wallet_crypto::KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        for subkey in subkeys {
            let file_meta = LegacyNaming::generate_filemeta(
                FileType::DerivedData,
                Some(subkey.address),
                Some(&subkey.account_index_map),
                Some(subkey.chain_code.to_string()),
                Some(subkey.derivation_path.to_string()),
            )?;

            let name = LegacyNaming::encode(file_meta)?;
            let rng = rand::thread_rng();
            KeystoreBuilder::new_encrypt(
                file_path,
                password,
                &subkey.data,
                rng,
                algorithm.clone(),
                &name,
            )
            .save()?;
        }
        Ok(())
    }

    fn delete_root(
        &self,
        address: &str,
        root_dir: &dyn AsRef<std::path::Path>,
    ) -> Result<(), crate::Error> {
        let path = root_dir.as_ref().join(LegacyNaming::encode(LegacyNaming::generate_filemeta(
            FileType::PrivateKey,
            Some(address.to_string()),
            None,
            None,
            None,
        )?)?);
        wallet_utils::file_func::remove_file(path)?;
        let path = root_dir.as_ref().join(LegacyNaming::encode(LegacyNaming::generate_filemeta(
            FileType::Phrase,
            Some(address.to_string()),
            None,
            None,
            None,
        )?)?);
        wallet_utils::file_func::remove_file(path)?;
        let path = root_dir.as_ref().join(LegacyNaming::encode(LegacyNaming::generate_filemeta(
            FileType::Seed,
            Some(address.to_string()),
            None,
            None,
            None,
        )?)?);
        wallet_utils::file_func::remove_file(path)?;

        Ok(())
    }

    #[allow(dead_code)]
    fn delete_account(
        &self,
        _account_index_map: &wallet_utils::address::AccountIndexMap,
        _file_path: &dyn AsRef<std::path::Path>,
    ) -> Result<(), crate::Error> {
        unimplemented!("Never call this method")
    }
}

#[derive(Clone)]
pub struct PhraseWallet {
    /// The wallet's private key.
    pub phrase: secrecy::SecretString,
}

impl Drop for PhraseWallet {
    fn drop(&mut self) {
        self.phrase.zeroize();
    }
}

impl PhraseWallet {
    /// Construct a new wallet with an external [`PrehashSigner`].
    pub fn new(phrase: &str) -> Self {
        PhraseWallet { phrase: phrase.into() }
    }
}

impl TryFrom<RecoverableData> for PhraseWallet {
    type Error = crate::Error;

    fn try_from(value: RecoverableData) -> Result<Self, Self::Error> {
        Ok(Self::new(&value.into_string()?))
    }
}

#[derive(Clone)]
pub struct PkWallet {
    pub pkey: secrecy::SecretSlice<u8>,
}

impl Drop for PkWallet {
    fn drop(&mut self) {
        self.pkey.zeroize();
    }
}

impl PkWallet {
    /// Construct a new wallet with an external [`PrehashSigner`].
    pub fn new(pkey: &[u8]) -> Self {
        PkWallet { pkey: pkey.to_vec().into() }
    }

    /// Returns this wallet's signer.
    pub fn pkey(&self) -> &secrecy::SecretSlice<u8> {
        &self.pkey
    }
}

impl TryFrom<RecoverableData> for PkWallet {
    type Error = crate::Error;

    fn try_from(value: RecoverableData) -> Result<Self, Self::Error> {
        Ok(Self::new(&value.inner()))
    }
}

#[derive(Clone)]
pub struct SeedWallet {
    /// The wallet's private key.
    pub seed: secrecy::SecretSlice<u8>,
}

impl Drop for SeedWallet {
    fn drop(&mut self) {
        self.seed.zeroize();
    }
}

impl SeedWallet {
    /// Construct a new wallet with an external [`PrehashSigner`].
    pub fn new(seed: &[u8]) -> Self {
        SeedWallet { seed: seed.to_vec().into() }
    }

    /// Consumes this wallet and returns its signer.
    pub fn into_seed(&self) -> &secrecy::SecretSlice<u8> {
        &self.seed
    }
}

impl TryFrom<RecoverableData> for SeedWallet {
    type Error = crate::Error;

    fn try_from(value: RecoverableData) -> Result<Self, Self::Error> {
        Ok(Self::new(&value.inner()))
    }
}
