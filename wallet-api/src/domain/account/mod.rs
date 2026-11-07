use wallet_database::repositories::{
    account::AccountRepo, chain::ChainRepo, device::DeviceRepo, wallet::WalletRepo,
};
use wallet_transport_backend::request::AddressInitReq;
use wallet_types::chain::{
    address::{
        category::AddressCategory,
        r#type::{AddressType, BtcAddressType, TonAddressType},
    },
    chain::ChainCode,
};

use super::app::config::ConfigDomain;
use crate::{
    application::wallet::WalletApplication,
    context::Context,
    error::{business::BusinessError, service::ServiceError, system::SystemError},
    response_vo::standard_wallet::account::CreateAccountRes,
    service::asset::AddressChainCode,
};

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

    // 获取地址对应的展示类型 比如: p2pkh --> legacy
    pub fn get_show_address_type(
        chain_code: &str,
        address_type: Option<String>,
    ) -> Result<AddressCategory, ServiceError> {
        let chain_code = ChainCode::try_from(chain_code)?;

        if let Some(types_str) = address_type {
            match chain_code {
                ChainCode::Bitcoin | ChainCode::Litecoin | ChainCode::Dogcoin => {
                    let address_type = types_str.as_str();

                    let res = BtcAddressType::try_from(address_type)?;
                    Ok(AddressCategory::Btc(res.into()))
                }
                ChainCode::Ton => {
                    let address_type = types_str.as_str();
                    let res = TonAddressType::try_from(address_type)?;
                    Ok(AddressCategory::Ton(res))
                }

                _ => Ok(AddressCategory::Other),
            }
        } else {
            Ok(AddressCategory::Other)
        }
    }

    pub async fn get_addresses(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_codes: Vec<String>,
        is_multisig: Option<bool>,
        core_pool: &wallet_database::CoreDbPool,
    ) -> Result<Vec<AddressChainCode>, ServiceError> {
        let mut account_addresses = Vec::new();

        if let Some(is_multisig) = is_multisig {
            if is_multisig {
                tracing::debug!("开始查询多签账户 is_multisig: {is_multisig}");
                tracing::debug!("多签账户地址 address: {address}");

                // 查询多签账户下的资产
                let account = super::multisig::MultisigDomain::account_by_address(
                    address,
                    true,
                    &core_pool.into_inner(),
                )
                .await?;
                tracing::debug!("查询成功 account: {account:?}");
                account_addresses.push(AddressChainCode {
                    address: account.address,
                    chain_code: account.chain_code,
                });
            } else {
                // 获取钱包下的这个账户的所有地址
                let accounts =
                    AccountRepo::account_list_by_wallet_address_and_account_id_and_chain_codes(
                        core_pool.clone(),
                        Some(address),
                        account_id,
                        chain_codes,
                    )
                    .await?;

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
            let accounts =
                AccountRepo::account_list_by_wallet_address_and_account_id_and_chain_codes(
                    core_pool.clone(),
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
        tracing::debug!("[get addresses] account_addresses: {account_addresses:?}");
        Ok(account_addresses)
    }

    pub(crate) async fn create_account_v2(
        ctx: &Context,
        seed: &[u8],
        instance: &wallet_chain_instance::instance::ChainObject,
        derivation_path: Option<&str>,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        uid: &str,
        wallet_address: &str,
        name: &str,
        is_default_name: bool,
    ) -> Result<(CreateAccountRes, String, Option<AddressInitReq>), ServiceError> {
        let (address, derivation_path, address_init_req) = Self::derive_subkey(
            ctx,
            uid,
            seed,
            account_index_map,
            instance,
            derivation_path,
            wallet_address,
            name,
            is_default_name,
        )
        .await?;
        let res = CreateAccountRes { address: address.to_string() };

        Ok((res, derivation_path, address_init_req))
    }

    pub(crate) async fn derive_subkey(
        ctx: &Context,
        uid: &str,
        seed: &[u8],
        account_index_map: &wallet_utils::address::AccountIndexMap,
        instance: &wallet_chain_instance::instance::ChainObject,
        derivation_path: Option<&str>,
        wallet_address: &str,
        name: &str,
        is_default_name: bool,
    ) -> Result<(String, String, Option<AddressInitReq>), ServiceError> {
        let account_name = if is_default_name {
            format!("{name}{}", account_index_map.account_id)
        } else {
            name.to_string()
        };

        let keypair = if let Some(derivation_path) = derivation_path {
            instance
                .gen_keypair_with_derivation_path(seed, derivation_path)
                .map_err(|e| SystemError::Service(e.to_string()))?
        } else {
            instance
                .gen_keypair_with_index_address_type(seed, account_index_map.input_index)
                .map_err(|e| SystemError::Service(e.to_string()))?
        };

        let derivation_path = keypair.derivation_path();
        let chain_code = keypair.chain_code().to_string();

        let address_type = instance.address_type();
        let address = keypair.address();
        let pubkey = keypair.pubkey();

        let mut req = wallet_database::entities::account::CreateAccountVo::new(
            account_index_map.account_id,
            &address,
            &pubkey,
            wallet_address,
            &derivation_path,
            &chain_code,
            &account_name,
        );

        let core_pool = ctx.core_pool()?;
        let sn = ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(core_pool.clone(), &sn).await? else {
            return Err(ServiceError::Business(BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )));
        };

        let account = AccountRepo::detail_by_address_and_chain_code(
            core_pool.clone(),
            &address,
            &instance.chain_code().to_string(),
        )
        .await?;

        let address_init_req = if let Some(account) = account {
            tracing::info!("已存在: {}", account.address);
            None
        } else {
            Some(wallet_transport_backend::request::AddressInitReq::new(
                uid,
                &address,
                account_index_map.input_index,
                &instance.chain_code().to_string(),
                &device.sn,
                vec!["".to_string()],
                &account_name,
            ))
        };

        match address_type {
            AddressType::Btc(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            AddressType::Ltc(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            AddressType::Dog(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            AddressType::Ton(address_type) => {
                req = req.with_address_type(address_type.as_ref());
            }
            _ => {}
        }
        AccountRepo::upsert_multi_account(core_pool.clone(), vec![req]).await?;
        Ok((address, derivation_path, address_init_req))
    }

    pub async fn set_root_password(
        ctx: &Context,
        wallet_address: &str,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), ServiceError> {
        let wallet = WalletRepo::detail(ctx.core_pool()?, wallet_address)
            .await?
            .ok_or(BusinessError::Wallet(crate::error::business::wallet::WalletError::NotFound))?;
        WalletApplication::change_root_password(ctx, &wallet.address, old_password, new_password)
            .await
    }

    pub(crate) async fn resume_root_password_change(
        ctx: &Context,
        wallet_address: &str,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), ServiceError> {
        WalletApplication::resume_root_password_change(
            ctx,
            wallet_address,
            old_password,
            new_password,
        )
        .await
    }

    pub async fn set_account_password(
        ctx: &Context,
        wallet_address: &str,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), ServiceError> {
        WalletApplication::change_account_password(
            ctx,
            wallet_address,
            account_index_map,
            old_password,
            new_password,
        )
        .await
    }

    pub(crate) async fn resume_account_password_change(
        ctx: &Context,
        wallet_address: &str,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), ServiceError> {
        WalletApplication::resume_account_password_change(
            ctx,
            wallet_address,
            account_index_map,
            old_password,
            new_password,
        )
        .await
    }

    /// 废弃方法，不再使用
    #[deprecated(since = "0.1.0", note = "此方法已被废弃，不再使用")]
    pub async fn set_verify_password(ctx: &Context, password: &str) -> Result<(), ServiceError> {
        let dirs = ctx.get_global_dirs();
        wallet_tree::api::KeystoreApi::remove_verify_file(&dirs.root_dir)?;
        let wallet_tree_strategy = ConfigDomain::get_wallet_tree_strategy(ctx).await?;
        let wallet_tree = wallet_tree_strategy.get_wallet_tree(&dirs.wallet_dir)?;

        wallet_tree::api::KeystoreApi::store_verify_file(&*wallet_tree, &dirs.root_dir, password)?;

        Ok(())
    }

    pub(crate) async fn generate_subkey(
        instance: &wallet_chain_instance::instance::ChainObject,
        seed: &[u8],
        address: &str,
        chain_code: &str,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        derivation_path: &str,
    ) -> Result<wallet_tree::file_ops::BulkSubkey, crate::error::service::ServiceError> {
        let keypair =
            instance.gen_keypair_with_index_address_type(seed, account_index_map.input_index)?;
        let private_key = keypair.private_key_bytes()?;

        Ok(wallet_tree::file_ops::BulkSubkey::new(
            account_index_map.clone(),
            address,
            chain_code,
            derivation_path,
            private_key,
        ))
    }
}

pub async fn open_accounts_pk_with_password(
    ctx: &Context,
    account_index_map: &wallet_utils::address::AccountIndexMap,
    address: &str,
    password: &str,
) -> Result<
    std::collections::HashMap<wallet_tree::KeyMeta, wallet_chain_interact::types::ChainPrivateKey>,
    ServiceError,
> {
    let core_pool = ctx.core_pool()?;
    let account_data =
        WalletApplication::load_account_data(ctx, address, account_index_map, password).await?;
    let mut res = std::collections::HashMap::default();
    for (meta, key) in account_data.into_inner() {
        let chain_code = &meta.chain_code;
        let Some(chain) = ChainRepo::detail_with_node(&core_pool, chain_code).await? else {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::NotFound(chain_code.to_string()),
                ),
            ));
        };
        let chain_code = chain_code.as_str().try_into()?;

        let private_key = match chain_code {
            ChainCode::Solana => {
                wallet_utils::parse_func::sol_keypair_from_bytes(&key)?.to_base58_string()
            }
            ChainCode::Bitcoin => wallet_chain_interact::btc::wif_private_key(
                &key,
                crate::domain::chain::ChainDomain::network_kind_from_node_network(&chain.network),
            )?,
            _ => hex::encode(key),
        };

        res.insert(meta, private_key.into());
    }
    Ok(res)
}

pub async fn open_subpk_with_password(
    ctx: &Context,
    chain_code: &str,
    address: &str,
    password: &str,
) -> Result<wallet_chain_interact::types::ChainPrivateKey, ServiceError> {
    let core_pool = ctx.core_pool()?;

    let account =
        AccountRepo::detail_by_address_and_chain_code(core_pool.clone(), address, chain_code)
            .await?
            .ok_or(crate::error::business::BusinessError::Account(
                crate::error::business::account::AccountError::NotFound(address.to_string()),
            ))?;

    let wallet = WalletRepo::detail(core_pool.clone(), &account.wallet_address).await?.ok_or(
        crate::error::business::BusinessError::Wallet(
            crate::error::business::wallet::WalletError::NotFound,
        ),
    )?;
    let Some(chain) = ChainRepo::detail_with_node(&core_pool, chain_code).await? else {
        return Err(crate::error::service::ServiceError::Business(
            crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::NotFound(chain_code.to_string()),
            ),
        ));
    };

    let chain_code: ChainCode = chain_code.try_into()?;

    let account_index_map =
        wallet_utils::address::AccountIndexMap::from_account_id(account.account_id)?;
    let expected = wallet_tree::KeyMeta {
        chain_code: chain_code.to_string(),
        address: address.to_owned(),
        derivation_path: account.derivation_path.clone(),
    };
    let key = WalletApplication::load_sub_key(
        ctx,
        &wallet.address,
        &account_index_map,
        &expected,
        password,
    )
    .await?;

    // TODO: 优化
    let private_key = match chain_code {
        ChainCode::Solana => {
            wallet_utils::parse_func::sol_keypair_from_bytes(&key)?.to_base58_string()
        }
        ChainCode::Bitcoin => wallet_chain_interact::btc::wif_private_key(
            &key,
            crate::domain::chain::ChainDomain::network_kind_from_node_network(&chain.network),
        )?,
        ChainCode::Dogcoin => wallet_chain_interact::dog::wif_private_key(
            &key,
            crate::domain::chain::ChainDomain::network_kind_from_node_network(&chain.network),
        )?,
        ChainCode::Litecoin => wallet_chain_interact::ltc::wif_private_key(
            &key,
            crate::domain::chain::ChainDomain::network_kind_from_node_network(&chain.network),
        )?,
        _ => hex::encode(key),
    };
    Ok(private_key.into())
}
