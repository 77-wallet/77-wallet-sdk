use wallet_database::{
    entities::{
        account::AccountEntity, chain::ChainEntity, device::DeviceEntity, wallet::WalletEntity,
    },
    repositories::{account::AccountRepoTrait, device::DeviceRepoTrait, ResourcesRepo},
};
use wallet_types::chain::{address::r#type::AddressType, chain::ChainCode};

use crate::{response_vo::account::CreateAccountRes, service::asset::AddressChainCode};

use super::task_queue::{BackendApiTask, Task};

pub struct AccountDomain {}

impl Default for AccountDomain {
    fn default() -> Self {
        Self::new()
    }
}

impl AccountDomain {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn get_addresses(
        &self,
        repo: &mut ResourcesRepo,
        address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<AddressChainCode>, crate::ServiceError> {
        let pool = crate::Context::get_global_sqlite_pool()?;
        let mut account_addresses = Vec::new();

        let chain_codes = if let Some(chain_code) = &chain_code {
            vec![chain_code.to_string()]
        } else {
            vec![]
        };

        if let Some(is_multisig) = is_multisig {
            if is_multisig {
                // 查询多签账户下的资产
                let account =
                    super::multisig::MultisigDomain::account_by_address(address, true, &pool)
                        .await?;
                account_addresses.push(AddressChainCode {
                    address: account.address,
                    chain_code: account.chain_code,
                });
            } else {
                tracing::warn!("chain_codes: {chain_codes:?}");
                // 获取钱包下的这个账户的所有地址
                let accounts = repo
                    .account_list_by_wallet_address_and_account_id_and_chain_codes(
                        Some(address),
                        account_id,
                        chain_codes,
                    )
                    .await?;
                let mut condition = Vec::new();
                if let Some(chain_code) = &chain_code {
                    condition.push(("chain_code", chain_code.as_str()));
                }
                for account in accounts {
                    if !account_addresses.iter().any(|address| {
                        address.address == account.address
                            && address.chain_code == account.chain_code
                    }) {
                        account_addresses.push(AddressChainCode {
                            address: account.address,
                            chain_code: account.chain_code,
                        });
                    }
                }
            }
        } else {
            // 获取钱包下的这个账户的所有地址
            let accounts = repo
                .account_list_by_wallet_address_and_account_id_and_chain_codes(
                    Some(address),
                    account_id,
                    chain_codes,
                )
                .await?;
            for account in accounts {
                if !account_addresses.iter().any(|address| {
                    address.address == account.address && address.chain_code == account.chain_code
                }) {
                    account_addresses.push(AddressChainCode {
                        address: account.address,
                        chain_code: account.chain_code,
                    });
                }
            }
        }

        Ok(account_addresses)
    }

    pub async fn create_account_with_derivation_path(
        &self,
        repo: &mut ResourcesRepo,
        dirs: &crate::manager::Dirs,
        seed: &[u8],
        instance: wallet_chain_instance::instance::ChainObject,
        derivation_path: &Option<String>,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        uid: &str,
        wallet_address: &str,
        root_password: &str,
        derive_password: Option<String>,
        name: &str,
        is_default_name: bool,
    ) -> Result<CreateAccountRes, crate::ServiceError> {
        let (address, name) = self
            .derive_subkey(
                repo,
                dirs,
                seed,
                account_index_map,
                &instance,
                derivation_path,
                wallet_address,
                root_password,
                derive_password,
                name,
                is_default_name,
            )
            .await?;

        let res = CreateAccountRes {
            address: address.to_string(),
        };
        Self::address_init(
            repo,
            uid,
            &address,
            account_index_map.input_index,
            &instance.chain_code().to_string(),
            &name,
        )
        .await?;

        Ok(res)
    }

    pub async fn address_init(
        repo: &mut ResourcesRepo,
        uid: &str,
        address: &str,
        index: i32,
        chain_code: &str,
        name: &str,
    ) -> Result<(), crate::ServiceError> {
        let Some(device) = DeviceRepoTrait::get_device_info(repo).await? else {
            return Err(crate::BusinessError::Device(crate::DeviceError::Uninitialized).into());
        };
        let address_init_req = wallet_transport_backend::request::AddressInitReq::new(
            uid,
            address,
            index,
            chain_code,
            &device.sn,
            vec!["".to_string()],
            name,
        );
        let address_init_task_data = crate::domain::task_queue::BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::ADDRESS_INIT,
            &address_init_req,
        )?;

        super::task_queue::Tasks::new()
            .push(Task::BackendApi(BackendApiTask::BackendApi(
                address_init_task_data,
            )))
            .send()
            .await?;
        Ok(())
    }

    pub async fn create_account_with_account_id(
        &self,
        repo: &mut ResourcesRepo,
        dirs: &crate::manager::Dirs,
        seed: &[u8],
        instance: wallet_chain_instance::instance::ChainObject,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        uid: &str,
        wallet_address: &str,
        root_password: &str,
        derive_password: Option<String>,
        name: &str,
        is_default_name: bool,
    ) -> Result<CreateAccountRes, crate::ServiceError> {
        let (address, name) = self
            .derive_subkey(
                repo,
                dirs,
                seed,
                account_index_map,
                &instance,
                &None,
                wallet_address,
                root_password,
                derive_password,
                name,
                is_default_name,
            )
            .await?;

        let res = CreateAccountRes {
            address: address.to_string(),
        };
        Self::address_init(
            repo,
            uid,
            &address,
            account_index_map.input_index,
            &instance.chain_code().to_string(),
            &name,
        )
        .await?;

        Ok(res)
    }

    pub(crate) async fn derive_subkey(
        &self,
        repo: &mut ResourcesRepo,
        dirs: &crate::manager::Dirs,
        seed: &[u8],
        account_index_map: &wallet_utils::address::AccountIndexMap,
        instance: &wallet_chain_instance::instance::ChainObject,
        derivation_path: &Option<String>,
        wallet_address: &str,
        root_password: &str,
        derive_password: Option<String>,
        name: &str,
        is_default_name: bool,
    ) -> Result<(String, String), crate::ServiceError> {
        let account_name = if is_default_name {
            format!("{name}{}", account_index_map.account_id)
        } else {
            name.to_string()
        };
        // Get the path to the subkeys directory for the given wallet name.
        let subs_dir = dirs.get_subs_dir(wallet_address)?;

        let keypair = if let Some(derivation_path) = derivation_path {
            instance
                .gen_keypair_with_derivation_path(seed, derivation_path)
                .map_err(|e| crate::SystemError::Service(e.to_string()))?
        } else {
            instance
                .gen_keypair_with_index_address_type(seed, account_index_map.input_index)
                .map_err(|e| crate::SystemError::Service(e.to_string()))?
        };

        let derivation_path = keypair.derivation_path();
        let chain_code = keypair.chain_code().to_string();

        let address_type = instance.address_type();
        // Get the root keystore using the root password

        // Call the derive_subkey function from the wallet manager handler,
        // passing in the root directory, subkeys path, wallet tree, derivation path,
        // wallet name, root password, and derive password.

        let derive_wallet = wallet_keystore::api::KeystoreApi::initialize_child_keystore(
            instance,
            seed,
            &derivation_path,
            subs_dir.to_string_lossy().to_string().as_str(),
            &derive_password.unwrap_or(root_password.to_owned()),
        )
        .map_err(|e| crate::SystemError::Service(e.to_string()))?;

        let address = derive_wallet.address().to_string();
        let pubkey = keypair.pubkey();

        let mut req = wallet_database::entities::account::CreateAccountVo::new(
            account_index_map.account_id,
            &address,
            pubkey,
            wallet_address.to_string(),
            derivation_path,
            chain_code,
            &account_name,
        );
        if let AddressType::Btc(address_type) = address_type {
            req = req.with_address_type(address_type.as_ref());
        };
        repo.upsert_multi_account(vec![req]).await?;
        Ok((address, account_name))
    }
}

pub async fn open_account_pk_with_password(
    chain_code: &str,
    address: &str,
    password: &str,
) -> Result<wallet_chain_interact::types::ChainPrivateKey, crate::ServiceError> {
    let pool = crate::manager::Context::get_global_sqlite_pool()?;
    let Some(device) = DeviceEntity::get_device_info(&*pool).await? else {
        return Err(crate::BusinessError::Device(crate::DeviceError::Uninitialized).into());
    };
    super::wallet::WalletDomain::validate_password(&device, password)?;

    let db = crate::manager::Context::get_global_sqlite_pool()?;
    let dirs = crate::manager::Context::get_global_dirs()?;

    let req = wallet_database::entities::account::QueryReq::new_address_chain(address, chain_code);

    let account = AccountEntity::detail(db.as_ref(), &req)
        .await?
        .ok_or(crate::BusinessError::Account(crate::AccountError::NotFound))?;

    let wallet = WalletEntity::detail(db.as_ref(), &account.wallet_address)
        .await?
        .ok_or(crate::BusinessError::Wallet(crate::WalletError::NotFound))?;
    let Some(chain) = ChainEntity::chain_node_info(db.as_ref(), chain_code).await? else {
        return Err(crate::ServiceError::Business(crate::BusinessError::Chain(
            crate::ChainError::NotFound(chain_code.to_string()),
        )));
    };
    let instance = wallet_chain_instance::instance::ChainObject::new(
        chain_code,
        account.address_type(),
        chain.network.as_str().into(),
    )?;

    let chain_code = chain_code.try_into()?;
    let name = wallet_tree::wallet_tree::WalletBranch::get_sub_pk_filename(
        &account.address,
        &chain_code,
        &account.derivation_path,
    )?;

    let subs_path = dirs.get_subs_dir(&wallet.address)?;
    let storage_path = subs_path.join(name);

    let key = wallet_keystore::api::KeystoreApi::get_private_key(
        password,
        &storage_path,
        instance.gen_gen_address()?,
    )?;

    // TODO: 优化
    let private_key = match chain_code {
        ChainCode::Solana => {
            wallet_utils::parse_func::sol_keypair_from_bytes(&key)?.to_base58_string()
        }
        ChainCode::Bitcoin => {
            wallet_chain_interact::btc::wif_private_key(&key, chain.network.as_str().into())?
        }
        _ => hex::encode(key),
    };
    Ok(private_key.into())
}
