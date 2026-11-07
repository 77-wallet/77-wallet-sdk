use wallet_crypto::{KdfAlgorithm, KeystoreJson};
use wallet_utils::address::AccountIndexMap;

use crate::{file_ops::RootData, wallet_hierarchy::WalletTreeOps};

pub struct KeystoreApi;

impl KeystoreApi {
    // 传入助记词、盐，生成密钥，创建根Keystore，并且保存到文件
    pub fn initialize_root_keystore(
        wallet_tree: Box<dyn WalletTreeOps>,
        address: &str,
        root_data: RootData,
        path: &std::path::Path,
        password: &str,
        algorithm: KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        wallet_tree.io().store_root(
            address,
            root_data.seed(),
            root_data.phrase(),
            &path,
            password,
            algorithm,
        )?;
        Ok(())
    }

    // 传入derivation_path，由根私钥派生出子私钥，创建子Keystore，并生成keystore文件
    pub fn initialize_child_keystores<P: AsRef<std::path::Path>>(
        wallet_tree: Box<dyn WalletTreeOps>,
        subkeys: Vec<crate::file_ops::BulkSubkey>,
        subs_path: P,
        password: &str,
        algorithm: KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        wallet_tree.io().store_subkeys_bulk(subkeys, &subs_path, password, algorithm)?;

        Ok(())
    }

    pub fn check_wallet_address(
        language_code: u8,
        phrase: &str,
        salt: &str,
        address: wallet_chain_instance::instance::Address,
    ) -> Result<(), crate::Error> {
        use wallet_core::address::GenAddress as _;
        let (master_key, _) = wallet_core::xpriv::generate_master_key(language_code, phrase, salt)?;
        let signingkey: &coins_bip32::ecdsa::SigningKey = master_key.as_ref();
        let pkey = signingkey.to_bytes();

        let data = Box::new(wallet_chain_instance::instance::eth::address::EthGenAddress::new(
            wallet_types::chain::chain::ChainCode::Ethereum,
        ));
        let generated_address = data.generate(&pkey)?;

        if generated_address.ne(&address) {
            return Err(crate::Error::Parase);
        }
        Ok(())
    }

    pub fn remove_verify_file(root_dir: &std::path::Path) -> Result<(), crate::Error> {
        let file_name = "verify";
        let file_path = root_dir.join(file_name);
        if wallet_utils::file_func::exists(&file_path)? {
            wallet_utils::file_func::remove_file(file_path)?;
        }

        Ok(())
    }

    pub fn load_verify_file(
        wallet_tree: &dyn WalletTreeOps,
        root_dir: &std::path::PathBuf,
        password: &str,
    ) -> Result<(), crate::Error> {
        let file_name = "verify";
        wallet_tree.io().load_custom(&root_dir, file_name, password)?;
        Ok(())
    }

    pub fn store_verify_file(
        wallet_tree: &dyn WalletTreeOps,
        root_dir: &std::path::PathBuf,
        password: &str,
    ) -> Result<(), crate::Error> {
        let file_name = "verify";
        wallet_tree.io().store(
            file_name,
            &"data",
            &root_dir,
            password,
            crate::KdfAlgorithm::Argon2id,
        )?;
        Ok(())
    }

    pub fn load_sub_pk(
        wallet_tree: &dyn WalletTreeOps,
        account_index_map: Option<&wallet_utils::address::AccountIndexMap>,
        subs_dir: &std::path::Path,
        address: &str,
        chain_code: &str,
        derivation_path: &str,
        password: &str,
    ) -> Result<Vec<u8>, crate::Error> {
        let pk = wallet_tree.io().load_subkey(
            account_index_map,
            address,
            chain_code,
            derivation_path,
            &subs_dir,
            password,
        )?;
        Ok(pk)
    }

    pub fn load_account_pk(
        wallet_tree: &dyn WalletTreeOps,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        subs_dir: &std::path::Path,
        password: &str,
    ) -> Result<crate::file_ops::AccountData, crate::Error> {
        let account_data = wallet_tree.io().load_account(account_index_map, &subs_dir, password)?;
        Ok(account_data)
    }

    pub fn load_seed(
        wallet_tree: &dyn WalletTreeOps,
        root_dir: &std::path::Path,
        wallet_address: &str,
        password: &str,
    ) -> Result<Vec<u8>, crate::Error> {
        let root = wallet_tree.io().load_root(wallet_address, &root_dir, password)?;
        Ok(root.seed().to_vec())
    }

    pub fn load_root(
        wallet_tree: &dyn WalletTreeOps,
        root_dir: &std::path::Path,
        wallet_address: &str,
        password: &str,
    ) -> Result<RootData, crate::Error> {
        let root = wallet_tree.io().load_root(wallet_address, &root_dir, password)?;
        Ok(root)
    }

    pub fn load_phrase(
        wallet_tree: &dyn WalletTreeOps,
        root_dir: &std::path::Path,
        wallet_address: &str,
        password: &str,
    ) -> Result<String, crate::Error> {
        let root = wallet_tree.io().load_root(wallet_address, &root_dir.to_path_buf(), password)?;
        Ok(root.phrase().to_string())
    }

    pub fn update_root_password(
        root_dir: std::path::PathBuf,
        wallet_tree: Box<dyn WalletTreeOps>,
        wallet_address: &str,
        old_password: &str,
        new_password: &str,
        algorithm: KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        let root_data = Self::load_root(&*wallet_tree, &root_dir, wallet_address, old_password)?;
        Self::initialize_root_keystore(
            wallet_tree,
            wallet_address,
            root_data,
            &root_dir,
            new_password,
            algorithm,
        )
    }

    pub fn update_account_password(
        wallet_tree: Box<dyn WalletTreeOps>,
        subs_dir: &std::path::PathBuf,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        old_password: &str,
        new_password: &str,
        algorithm: KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        let account_data =
            Self::load_account_pk(&*wallet_tree, account_index_map, subs_dir, old_password)?;

        let mut subkeys = Vec::<crate::file_ops::BulkSubkey>::new();
        for (meta, pk) in account_data.iter() {
            let subkey = crate::file_ops::BulkSubkey::new(
                account_index_map.clone(),
                &meta.address,
                &meta.chain_code,
                &meta.derivation_path,
                pk.to_vec(),
            );
            subkeys.push(subkey);
        }
        wallet_tree.io().delete_account(account_index_map, subs_dir)?;

        Self::initialize_child_keystores(wallet_tree, subkeys, subs_dir, new_password, algorithm)?;

        Ok(())
    }

    pub fn update_child_password(
        subs_dir: std::path::PathBuf,
        instance: wallet_chain_instance::instance::ChainObject,
        wallet_tree: Box<dyn WalletTreeOps>,
        wallet_address: &str,
        address: &str,
        old_password: &str,
        new_password: &str,
        algorithm: KdfAlgorithm,
    ) -> Result<(), crate::Error> {
        if let Some(account) = wallet_tree
            .get_wallet_branch(wallet_address)?
            .get_account(address, instance.chain_code())
        {
            let meta = account.get_filemeta();

            let account_index_map = if let Some(meta) = meta.account_index() {
                Some(AccountIndexMap::from_account_id(meta)?)
            } else {
                None
            };

            let chain_code = account.get_chain_code().to_string();
            let derivation_path = account.get_derivation_path();
            let pk = wallet_tree.io().load_subkey(
                account_index_map.as_ref(),
                account.get_address(),
                &chain_code,
                derivation_path,
                &subs_dir,
                old_password,
            )?;

            wallet_tree.io().store_subkey(
                &account_index_map.ok_or(crate::Error::MissingIndex)?,
                address,
                &chain_code,
                derivation_path,
                &pk,
                &subs_dir,
                new_password,
                algorithm,
            )?;
        } else {
            tracing::error!("account not found");
        }

        Ok(())
    }

    pub fn generate_master_key_info(
        language_code: u8,
        phrase: &str,
        salt: &str,
    ) -> Result<RootInfo, crate::Error> {
        let (master_key, seed) =
            wallet_core::xpriv::generate_master_key(language_code, phrase, salt)?;
        let signingkey: &coins_bip32::ecdsa::SigningKey = master_key.as_ref();
        let private_key = signingkey.to_bytes();
        let address = alloy::signers::utils::secret_key_to_address(signingkey);
        Ok(RootInfo {
            private_key: private_key.to_vec(),
            phrase: phrase.to_string(),
            seed,
            address,
        })
    }

    pub fn reset_and_store_root_keys(
        wallet_tree: Box<dyn WalletTreeOps>,
        storage_path: &std::path::PathBuf,
        root_info: RootInfo,
        password: &str,
        algorithm: KdfAlgorithm,
    ) -> Result<String, crate::Error> {
        // 清理并重新创建目录
        wallet_utils::file_func::recreate_dir_all(storage_path)?;

        Self::initialize_root_keystore(
            wallet_tree,
            &root_info.address.to_string(),
            RootData::new(&root_info.phrase, &root_info.seed),
            storage_path,
            password,
            algorithm,
        )?;

        Ok(root_info.address.to_string())
    }

    pub fn read_keystore<P: AsRef<std::path::Path> + std::fmt::Debug>(
        path: P,
    ) -> Result<KeystoreJson, crate::Error> {
        let mut contents = String::new();
        wallet_utils::file_func::read(&mut contents, path)?;
        Ok(wallet_utils::serde_func::serde_from_str(&contents)?)
    }
}

pub struct RootInfo {
    pub private_key: Vec<u8>,
    pub phrase: String,
    pub seed: Vec<u8>,
    pub address: alloy::primitives::Address,
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::KeystoreApi;
    use crate::{
        file_ops::{BulkSubkey, RootData},
        wallet_hierarchy::v2::ModernWalletTree,
    };
    use wallet_chain_instance::instance::ChainObject;
    use wallet_types::chain::{
        address::r#type::{AddressType, TonAddressType},
        chain::ChainCode,
        network::NetworkKind,
    };

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}-{}", std::process::id()))
    }

    fn build_chain_object() -> ChainObject {
        let chain_code = ChainCode::Solana;
        let address_type = AddressType::Ton(TonAddressType::V4R2);
        let network = NetworkKind::Mainnet;

        (&chain_code, &address_type, network)
            .try_into()
            .expect("chain object should build for the test chain")
    }

    fn sol_private_key_string(private_key: &[u8]) -> Result<String, crate::Error> {
        Ok(wallet_utils::parse_func::sol_keypair_from_bytes(private_key)?.to_base58_string())
    }

    #[test]
    fn mnemonic_salt_child_subkey_roundtrip_returns_private_key() -> Result<(), crate::Error> {
        let phrase = "";
        let salt = "r0000007";
        let password = "q1111111";

        let root_info = KeystoreApi::generate_master_key_info(1, phrase, salt)?;
        let instance = build_chain_object();
        let account_index_map = wallet_utils::address::AccountIndexMap::from_account_id(1)?;
        let keypair = instance
            .gen_keypair_with_index_address_type(&root_info.seed, account_index_map.input_index)?;
        let private_key = keypair.private_key_bytes()?;
        let address = keypair.address().to_string();
        println!("address: {}", address);
        let private_key_string = sol_private_key_string(&private_key)?;
        println!("private_key_string: {}", private_key_string);
        let derivation_path = keypair.derivation_path();
        let chain_code = instance.chain_code().to_string();
        let address = keypair.address().to_string();

        let base_dir = unique_temp_dir("wallet-tree-roundtrip");
        let root_dir = base_dir.join("root");
        let subs_dir = base_dir.join("subs");
        wallet_utils::file_func::create_dir_all(&root_dir)?;
        wallet_utils::file_func::create_dir_all(&subs_dir)?;

        let root_tree = Box::new(ModernWalletTree::default());
        KeystoreApi::initialize_root_keystore(
            root_tree,
            &root_info.address.to_string(),
            RootData::new(&root_info.phrase, &root_info.seed),
            &root_dir,
            password,
            wallet_crypto::KdfAlgorithm::Argon2id,
        )?;

        let subkey = BulkSubkey::new(
            account_index_map.clone(),
            &address,
            &chain_code,
            &derivation_path,
            private_key.clone(),
        );
        let child_tree = Box::new(ModernWalletTree::default());
        KeystoreApi::initialize_child_keystores(
            child_tree,
            vec![subkey],
            &subs_dir,
            password,
            wallet_crypto::KdfAlgorithm::Argon2id,
        )?;

        let load_tree = Box::new(ModernWalletTree::default());
        let loaded_root = KeystoreApi::load_root(
            &*load_tree,
            &root_dir,
            &root_info.address.to_string(),
            password,
        )?;
        assert_eq!(loaded_root.phrase(), phrase);
        assert_eq!(loaded_root.seed(), root_info.seed.as_slice());

        let loaded_private_key = KeystoreApi::load_sub_pk(
            &*load_tree,
            Some(&account_index_map),
            &subs_dir,
            &address,
            &chain_code,
            &derivation_path,
            password,
        )?;

        assert_eq!(loaded_private_key, private_key);
        let loaded_private_key_string = sol_private_key_string(&loaded_private_key)?;
        assert_eq!(loaded_private_key_string, private_key_string);
        assert!(!private_key_string.is_empty());

        wallet_utils::file_func::remove_dir_all(&base_dir)?;
        Ok(())
    }

    #[test]
    fn load_subkey_with_wrong_password_fails_without_touching_files() -> Result<(), crate::Error> {
        let phrase = "";
        let salt = "wallet-tree-test-salt";
        let password = "wallet-tree-test-password";
        let wrong_password = "wallet-tree-wrong-password";

        let root_info = KeystoreApi::generate_master_key_info(1, phrase, salt)?;
        let instance = build_chain_object();
        let account_index_map = wallet_utils::address::AccountIndexMap::from_account_id(2)?;
        let keypair = instance
            .gen_keypair_with_index_address_type(&root_info.seed, account_index_map.input_index)?;
        let private_key = keypair.private_key_bytes()?;
        let private_key_string = sol_private_key_string(&private_key)?;
        let derivation_path = keypair.derivation_path();
        let chain_code = instance.chain_code().to_string();
        let address = keypair.address().to_string();

        let base_dir = unique_temp_dir("wallet-tree-wrong-password");
        let root_dir = base_dir.join("root");
        let subs_dir = base_dir.join("subs");
        wallet_utils::file_func::create_dir_all(&root_dir)?;
        wallet_utils::file_func::create_dir_all(&subs_dir)?;

        let root_tree = Box::new(ModernWalletTree::default());
        KeystoreApi::initialize_root_keystore(
            root_tree,
            &root_info.address.to_string(),
            RootData::new(&root_info.phrase, &root_info.seed),
            &root_dir,
            password,
            wallet_crypto::KdfAlgorithm::Argon2id,
        )?;

        let subkey = BulkSubkey::new(
            account_index_map.clone(),
            &address,
            &chain_code,
            &derivation_path,
            private_key,
        );
        let child_tree = Box::new(ModernWalletTree::default());
        KeystoreApi::initialize_child_keystores(
            child_tree,
            vec![subkey],
            &subs_dir,
            password,
            wallet_crypto::KdfAlgorithm::Argon2id,
        )?;

        let load_tree = Box::new(ModernWalletTree::default());
        let result = KeystoreApi::load_sub_pk(
            &*load_tree,
            Some(&account_index_map),
            &subs_dir,
            &address,
            &chain_code,
            &derivation_path,
            wrong_password,
        );

        assert!(result.is_err());
        assert!(!private_key_string.is_empty());
        assert!(root_dir.exists());
        assert!(subs_dir.exists());
        assert!(subs_dir.join("derived_meta.json").exists());

        wallet_utils::file_func::remove_dir_all(&base_dir)?;
        Ok(())
    }
}
