use wallet_database::repositories::{
    account::AccountRepoTrait, chain::ChainRepoTrait, wallet::WalletRepoTrait, ResourcesRepo,
};

use crate::request::wallet::ResetRootReq;

const DEFAULT_SALT: &str = "salt";

pub struct WalletDomain {}
impl Default for WalletDomain {
    fn default() -> Self {
        Self::new()
    }
}
impl WalletDomain {
    pub fn new() -> Self {
        Self {}
    }

    pub(crate) fn encrypt_password(
        password: &str,
        salt: &str,
    ) -> Result<String, crate::ServiceError> {
        let encrypted_password = wallet_utils::pbkdf2_string(
            password,
            &format!("{}{}", salt, DEFAULT_SALT),
            100000,
            32,
        )?;
        Ok(encrypted_password)
    }

    pub(crate) fn validate_password(
        device: &wallet_database::entities::device::DeviceEntity,
        password: &str,
    ) -> Result<(), crate::ServiceError> {
        let encrypted_password = Self::encrypt_password(password, &device.sn)?;

        let Some(pw) = &device.password else {
            return Err(crate::BusinessError::Wallet(crate::WalletError::PasswordNotSet).into());
        };

        if pw != &encrypted_password {
            return Err(crate::BusinessError::Wallet(crate::WalletError::PasswordIncorrect).into());
        }
        Ok(())
    }

    pub(crate) fn get_seed_wallet(
        &self,
        dirs: &crate::manager::Dirs,
        wallet_address: &str,
        wallet_password: &str,
    ) -> Result<wallet_keystore::wallet::seed::SeedWallet, crate::ServiceError> {
        let root_dir = dirs.get_root_dir(wallet_address)?;
        Ok(wallet_keystore::Keystore::load_seed_keystore(
            wallet_address,
            &root_dir,
            wallet_password,
        )?)
    }

    pub(crate) async fn reset_root(
        &self,
        repo: &mut ResourcesRepo,
        root_dir: std::path::PathBuf,
        subs_dir: std::path::PathBuf,
        wallet_tree: wallet_tree::wallet_tree::WalletTree,
        private_key: Vec<u8>,
        seed: Vec<u8>,
        req: ResetRootReq,
    ) -> Result<(), crate::ServiceError> {
        // todo!()
        let ResetRootReq {
            language_code,
            phrase,
            salt,
            wallet_address,
            new_password,
            subkey_password,
        } = req;

        // Parse the provided address

        let alloy_wallet_address = wallet_address
            .parse::<alloy::primitives::Address>()
            .map_err(|e| crate::SystemError::Service(e.to_string()))?;

        // Verify that the provided mnemonic phrase and salt generate the expected address
        // TODO:
        wallet_keystore::api::KeystoreApi::check_wallet_address(
            language_code,
            &phrase,
            &salt,
            wallet_chain_instance::instance::Address::EthAddress(alloy_wallet_address),
            // None,
            // Some(ChainCode::Eth),
        )
        .map_err(|e| crate::SystemError::Service(e.to_string()))?;

        // Clear any existing keystore at the storage path
        wallet_utils::file_func::recreate_dir_all(&root_dir)?;

        // Create a new root keystore with the new password
        wallet_keystore::api::KeystoreApi::initialize_root_keystore(
            &wallet_address,
            &private_key,
            &seed,
            &phrase,
            &root_dir,
            &new_password,
            // name,
        )?;

        if subs_dir.exists() {
            if let Some(subkey_password) = subkey_password {
                // Retrieve the wallet branch for the specified wallet
                let wallet = wallet_tree
                    .get_wallet_branch(&wallet_address.to_string())
                    .map_err(|e| crate::SystemError::Service(e.to_string()))?;
                let root_info = &wallet.root_info;

                let seed_wallet = wallet_keystore::Keystore::load_seed_keystore(
                    &root_info.address.to_string(),
                    &root_dir,
                    &new_password,
                )?;

                let seed = seed_wallet.seed;
                // let keypair = instance.gen_keypair(&seed, index)?;

                for info in wallet.accounts.iter() {
                    // TODO:
                    let account = repo
                        .detail_by_address_and_chain_code(
                            &info.address,
                            &info.chain_code.to_string(),
                        )
                        .await?;
                    if let Some(account) = account
                        && let Some(chain) = repo.detail_with_node(&account.chain_code).await?
                    {
                        let instance = wallet_chain_instance::instance::ChainObject::new(
                            &account.chain_code,
                            account.address_type(),
                            chain.network.as_str().into(),
                        )?;
                        wallet_keystore::api::KeystoreApi::initialize_child_keystore(
                            &instance,
                            &seed,
                            &info.derivation_path,
                            subs_dir.to_string_lossy().to_string().as_str(),
                            &subkey_password,
                        )?;
                    }
                }
            } else {
                wallet_tree
                    .deprecate_subkeys(alloy_wallet_address, subs_dir)
                    .map_err(|e| crate::SystemError::Service(e.to_string()))?;
            }
        }

        Ok(())
    }

    pub(crate) async fn restart_existing_wallet(
        &self,
        repo: &mut ResourcesRepo,
        address: &str,
    ) -> Result<std::collections::HashSet<u32>, crate::ServiceError> {
        // 查询钱包状态并处理重启逻辑
        let mut account_ids = std::collections::HashSet::new();
        if let Some(wallet) = WalletRepoTrait::detail_all_status(repo, address).await? {
            if wallet.status == 2 {
                WalletRepoTrait::restart(repo, &[address]).await?;
                for account in AccountRepoTrait::restart(repo, address).await? {
                    account_ids.insert(account.account_id);
                }
            }
        }
        if account_ids.is_empty() {
            account_ids.insert(1);
        }
        Ok(account_ids)
    }
}
