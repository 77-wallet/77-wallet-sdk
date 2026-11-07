use wallet_database::{
    entities::account::AccountEntity,
    repositories::{
        account::AccountRepo, chain::ChainRepo, coin::CoinRepo, device::DeviceRepo,
        multisig_account::MultisigAccountRepo, wallet::WalletRepo,
    },
};
use wallet_transport_backend::request::{
    AddressBatchInitReq, AddressUpdateAccountNameReq, TokenQueryPriceReq,
};
use wallet_types::{chain::chain::ChainCode, constant::chain_code};
use wallet_utils::address::AccountIndexMap;

use crate::{
    context::Context,
    domain::{
        self, account::AccountDomain, api_wallet::wallet::ApiWalletDomain,
        app::config::ConfigDomain, chain::ChainDomain, permission::PermissionDomain,
        wallet::WalletDomain,
    },
    error::service::ResultExt,
    infrastructure::task_queue::{
        CommonTask, RecoverDataBody,
        backend::{BackendApiTask, BackendApiTaskData},
        task::Tasks,
    },
    response_vo::standard_wallet::account::{
        CurrentAccountInfo, DerivedAddressesList, QueryAccountDerivationPath,
    },
};

pub struct AccountService {
    ctx: &'static Context,
    pub wallet_domain: WalletDomain,
    // keystore: wallet_crypto::Keystore
}

pub(crate) async fn change_sub_password(
    ctx: &Context,
    wallet_address: &str,
    account_id: u32,
    old_password: &str,
    new_password: &str,
) -> Result<(), crate::error::service::ServiceError> {
    let account_index_map = AccountIndexMap::from_account_id(account_id)?;
    AccountDomain::set_account_password(
        ctx,
        wallet_address,
        &account_index_map,
        old_password,
        new_password,
    )
    .await
}

impl AccountService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx, wallet_domain: WalletDomain::new() }
    }

    pub(crate) async fn switch_account(
        self,
        _wallet_address: &str,
        _account_id: u32,
    ) -> Result<(), crate::error::service::ServiceError> {
        // let pool = crate::manager::Context::get_global_sqlite_pool()?;
        // let mut tx = self.repo;
        // let accounts = tx
        //     .get_account_list_by_wallet_address_and_account_id(
        //         Some(wallet_address),
        //         Some(account_id),
        //     )
        //     .await?;

        // let accounts = accounts
        //     .into_iter()
        //     .map(|account| account.address)
        //     .collect();
        // let regular_assets_list =
        //     AssetsRepoTrait::get_coin_assets_in_address_all_status(&mut tx, accounts).await?;

        // let multisig_accounts = domain::multisig::MultisigDomain::list(&pool).await?;

        // let multisig_accounts = multisig_accounts
        //     .into_iter()
        //     .map(|account| account.address)
        //     .collect();
        // let multisig_assets_list =
        //     AssetsRepoTrait::get_coin_assets_in_address_all_status(&mut tx, multisig_accounts)
        //         .await?;

        // Sync multisig assets status with regular assets
        // for multisig_asset in multisig_assets_list.iter() {
        //     if let Some(regular_asset) = regular_assets_list
        //         .iter()
        //         .find(|&ra| ra.symbol == multisig_asset.symbol)
        //     {
        //         AssetsRepoTrait::update_status(
        //             &mut tx,
        //             &multisig_asset.chain_code,
        //             &multisig_asset.symbol,
        //             multisig_asset.token_address(),
        //             regular_asset.status,
        //         )
        //         .await?;
        //     }
        // }

        Ok(())
    }

    pub async fn create_account(
        self,
        wallet_address: &str,
        wallet_password: &str,
        derivation_path: Option<String>,
        index: Option<i32>,
        name: &str,
        is_default_name: bool,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;

        WalletDomain::validate_password_with_context(self.ctx, wallet_password).await?;
        // 根据钱包地址查询是否有钱包
        let wallet = WalletRepo::wallet_detail_by_address(core_pool.clone(), wallet_address)
            .await?
            .ok_or(crate::error::business::BusinessError::Wallet(
                crate::error::business::wallet::WalletError::NotFound,
            ))
            .with_context(&format!("WalletRepo::wallet_detail_by_address: {}", wallet_address))?;

        // 获取种子
        let seed = WalletDomain::get_seed(self.ctx, &wallet.address, wallet_password).await?;
        // 获取默认链和币
        let default_chain_list = ChainRepo::get_chain_list(&core_pool).await?;
        let default_coins_list = CoinRepo::default_coin_list(&core_pool).await?;

        // 根据派生路径
        let hd_path = if let Some(derivation_path) = &derivation_path {
            let hd_path = wallet_chain_instance::derivation_path::get_account_hd_path_from_path(
                derivation_path,
            )?;
            Some(hd_path)
        } else {
            None
        };

        // 如果有指定派生路径，就获取该链的所有chain_code
        let chains: Vec<String> = if let Some(hd_path) = &hd_path {
            hd_path.get_chain_codes()?.0.into_iter().map(|path| path.to_string()).collect()
        }
        // 或者使用默认链的链码
        else {
            default_chain_list.iter().map(|chain| chain.chain_code.clone()).collect()
        };

        // 获取该账户的最大索引，并加一
        let account_index_map = if let Some(index) = index {
            let index = wallet_utils::address::AccountIndexMap::from_input_index(index)?;
            if AccountRepo::has_account_id(core_pool.clone(), &wallet.address, index.account_id)
                .await?
            {
                return Err(crate::error::service::ServiceError::Business(
                    crate::error::business::BusinessError::Account(
                        crate::error::business::account::AccountError::AlreadyExist,
                    ),
                ));
            };
            index
        } else if let Some(hd_path) = hd_path {
            wallet_utils::address::AccountIndexMap::from_index(hd_path.get_account_id()?)?
        } else if let Some(max_account) = AccountRepo::account_detail_by_max_id_and_wallet_address(
            core_pool.clone(),
            &wallet.address,
        )
        .await?
        {
            wallet_utils::address::AccountIndexMap::from_account_id(max_account.account_id + 1)?
        } else {
            wallet_utils::address::AccountIndexMap::from_account_id(1)?
        };

        let mut req: TokenQueryPriceReq = TokenQueryPriceReq(Vec::new());
        let mut subkeys = Vec::<wallet_tree::file_ops::BulkSubkey>::new();

        let mut address_batch_init_task_data = AddressBatchInitReq(Vec::new());

        ChainDomain::init_chains_assets(
            self.ctx,
            &default_coins_list,
            &mut req,
            &mut address_batch_init_task_data,
            &mut subkeys,
            &chains,
            &seed,
            &account_index_map,
            derivation_path.as_deref(),
            &wallet.uid,
            &wallet.address,
            name,
            is_default_name,
        )
        .await?;

        let tron_address =
            subkeys.iter().find(|s| s.chain_code == chain_code::TRON).map(|s| s.address.clone());

        crate::application::wallet::WalletApplication::store_subkeys(
            self.ctx,
            wallet_address,
            subkeys,
            wallet_password,
        )
        .await?;

        // let device_bind_address_task_data =
        //     domain::app::DeviceDomain::gen_device_bind_address_task_data().await?;

        // 恢复多签账号、多签队列
        let mut body = RecoverDataBody::new(&wallet.uid);
        if let Some(tron_address) = tron_address {
            body.tron_address = Some(tron_address);
        };

        let address_batch_init_task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::ADDRESS_BATCH_INIT,
            &address_batch_init_task_data,
        )?;
        Tasks::new()
            .push(CommonTask::QueryCoinPrice(req))
            // .push(Task::BackendApi(BackendApiTask::BackendApi(
            //     device_bind_address_task_data,
            // )))
            .push(CommonTask::RecoverMultisigAccountData(body))
            .push(BackendApiTask::BackendApi(address_batch_init_task_data))
            .send_with_ctx(self.ctx)
            .await?;
        // for task in address_init_task_data {
        //     Tasks::new()
        //         .push(Task::BackendApi(BackendApiTask::BackendApi(task)))
        //         .send()
        //         .await?;
        // }

        Ok(())
    }

    pub async fn get_account_derivation_path(
        self,
        wallet_address: &str,
        index: u32,
    ) -> Result<Vec<QueryAccountDerivationPath>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let list = AccountRepo::get_account_list_by_wallet_address_and_account_id(
            core_pool,
            Some(wallet_address),
            Some(index),
        )
        .await?;
        let mut res = Vec::new();
        for data in list {
            let address_type =
                AccountDomain::get_show_address_type(&data.chain_code, data.address_type())?;
            res.push(QueryAccountDerivationPath::new(
                &data.address,
                &data.derivation_path,
                &data.chain_code,
                address_type,
            ));
        }

        Ok(res)
    }

    pub async fn list_derived_addresses(
        self,
        wallet_address: &str,
        index: i32,
        password: &str,
        all: bool,
    ) -> Result<Vec<DerivedAddressesList>, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;

        WalletDomain::validate_password_with_context(self.ctx, password).await?;

        let account_index_map = wallet_utils::address::AccountIndexMap::from_input_index(index)?;
        let seed = WalletDomain::get_seed(self.ctx, wallet_address, password).await?;

        // 获取默认链和币
        let chains = if !all {
            vec!["btc".to_string(), "eth".to_string(), "tron".to_string(), "sol".to_string()]
        } else {
            let default_chain_list = ChainRepo::get_chain_list(&pool).await?;
            // 如果有指定派生路径，就获取该链的所有chain_code
            default_chain_list.iter().map(|chain| chain.chain_code.clone()).collect()
        };

        let mut res = Vec::new();
        for chain in chains.iter() {
            let code: ChainCode = chain.as_str().try_into()?;
            let address_types = WalletDomain::address_type_by_chain(code);

            let Ok(node) = ChainDomain::get_node_with_ctx(self.ctx, chain).await else {
                continue;
            };
            for address_type in address_types {
                let instance: wallet_chain_instance::instance::ChainObject = (
                    &code,
                    &address_type,
                    crate::domain::chain::ChainDomain::network_kind_from_node_network(
                        &node.network,
                    ),
                )
                    .try_into()?;

                let keypair = instance
                    .gen_keypair_with_index_address_type(&seed, account_index_map.input_index)?;

                let address_type = instance.address_type().into();
                let derivation_path = keypair.derivation_path();
                let address = keypair.address();

                let mut derived_address = DerivedAddressesList::new(
                    &address,
                    &derivation_path,
                    &node.chain_code,
                    address_type,
                );

                match code {
                    ChainCode::Solana | ChainCode::Sui | ChainCode::Ton => {
                        let account = AccountRepo::detail_by_address_and_chain_code(
                            pool.clone(),
                            &address,
                            &node.chain_code,
                        )
                        .await?;
                        if let Some(account) = account {
                            derived_address.with_mapping_account(account.account_id, account.name);
                        };

                        if account_index_map.input_index < 0 {
                            derived_address
                                .with_mapping_positive_index(account_index_map.unhardend_index);
                        }
                    }
                    _ => {}
                }

                res.push(derived_address);
            }
        }

        Ok(res)
    }

    pub async fn account_details(
        self,
        address: &str,
    ) -> Result<Option<AccountEntity>, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;
        let res = AccountRepo::account(pool.clone(), address).await?;
        Ok(res)
    }

    pub async fn edit_account_name(
        self,
        account_id: u32,
        wallet_address: &str,
        name: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        AccountRepo::edit_account_name(core_pool.clone(), account_id, wallet_address, name).await?;
        let Some(wallet) =
            WalletRepo::wallet_detail_by_address(core_pool.clone(), wallet_address).await?
        else {
            return Err(crate::error::business::BusinessError::Wallet(
                crate::error::business::wallet::WalletError::NotFound,
            )
            .into());
        };
        let account_index_map =
            wallet_utils::address::AccountIndexMap::from_account_id(account_id)?;

        let req =
            AddressUpdateAccountNameReq::new(&wallet.uid, account_index_map.input_index, name);
        let req = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::ADDRESS_UPDATE_ACCOUNT_NAME,
            &req,
        )?;
        Tasks::new().push(BackendApiTask::BackendApi(req)).send_with_ctx(self.ctx).await?;

        Ok(())
    }

    pub async fn physical_delete_account(
        self,
        wallet_address: &str,
        account_id: u32,
        password: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;
        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(pool.clone(), sn).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };
        WalletDomain::validate_password_with_context(self.ctx, password).await?;
        // Check if this is the last account
        let account_count =
            AccountRepo::count_unique_account_ids(pool.clone(), wallet_address).await?;
        if account_count <= 1 {
            return Err(crate::error::business::BusinessError::Account(
                crate::error::business::account::AccountError::CannotDeleteLastAccount,
            )
            .into());
        }

        crate::application::wallet::WalletApplication::delete_account_files(
            self.ctx,
            wallet_address,
            account_id,
        )
        .await?;

        let deleted =
            AccountRepo::physical_delete(pool.clone(), wallet_address, account_id).await?;

        let device_unbind_address_task =
            domain::app::DeviceDomain::gen_device_unbind_all_address_task_data(
                &deleted,
                Vec::new(),
                &device.sn,
            )
            .await?;

        let pool = self.ctx.get_global_sqlite_pool()?;
        // delete permission
        for account in deleted.iter() {
            if account.chain_code == chain_code::TRON {
                PermissionDomain::delete_by_address(&pool, &account.address).await?;
            }
        }

        Tasks::new()
            .push(BackendApiTask::BackendApi(device_unbind_address_task))
            .send_with_ctx(self.ctx)
            .await?;

        Ok(())
    }

    pub async fn set_all_password(
        &mut self,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let ctx = self.ctx;
        let old_password = zeroize::Zeroizing::new(old_password.to_owned());
        let new_password = zeroize::Zeroizing::new(new_password.to_owned());
        // Keep rotation and its guard alive if the caller cancels midway through
        // re-encryption. Creation must not see a half-finished credential revision.
        tokio::spawn(async move {
            let _credential_guard = ctx.api_wallet_credential_gate.lock().await;
            Self::new(ctx).set_all_password_inner(&old_password, &new_password).await
        })
        .await
        .map_err(|_| {
            crate::error::system::SystemError::Internal(
                "password rotation worker failed".to_owned(),
            )
        })?
    }

    async fn set_all_password_inner(
        &mut self,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        WalletDomain::validate_password_with_context(self.ctx, old_password).await?;
        let api_wallet_domain = ApiWalletDomain::new(self.ctx);
        api_wallet_domain.clear_wallet_unlock_session().await?;
        let pool = self.ctx.core_pool()?;
        let indices = AccountRepo::get_all_account_indices(pool.clone()).await?;
        let wallet_list = WalletRepo::wallet_list(pool.clone()).await?;

        for wallet in wallet_list {
            AccountDomain::resume_root_password_change(
                self.ctx,
                &wallet.address,
                old_password,
                new_password,
            )
            .await?;
            for index in &indices {
                let account_index_map = AccountIndexMap::from_account_id(*index)?;
                AccountDomain::resume_account_password_change(
                    self.ctx,
                    &wallet.address,
                    &account_index_map,
                    old_password,
                    new_password,
                )
                .await?;
            }
        }
        tracing::info!("set_all_password done");
        api_wallet_domain.reset_api_wallet_seed(old_password, new_password).await?;
        tracing::info!("reset_api_wallet_seed done");
        api_wallet_domain.initialize_wallet_unlock_session(new_password).await?;
        tracing::info!("wallet unlock session reinitialized after password change");

        // 生成并存储 password_proof
        let sn = self.ctx.get_sn();
        let proof =
            crate::domain::wallet::WalletDomain::generate_password_proof(self.ctx, new_password)
                .await?;
        DeviceRepo::update_password_proof(pool.clone(), sn, Some(&proof)).await?;
        tracing::info!("password_proof updated");

        Ok(())
    }

    pub async fn set_sub_password(
        &mut self,
        address: &str,
        chain_code: &str,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        // let req = wallet_database::entities::account::QueryReq {
        //     wallet_address: None,
        //     address: Some(address.to_string()),
        //     chain_code: Some(chain_code.to_string()),
        //     account_id: None,
        //     status: Some(1),
        // };
        let account =
            AccountRepo::detail_by_address_and_chain_code(core_pool.clone(), address, chain_code)
                .await?
                .ok_or(crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::NotFound(address.to_string()),
                ))?;

        change_sub_password(
            self.ctx,
            &account.wallet_address,
            account.account_id,
            old_password,
            new_password,
        )
        .await
    }

    pub async fn get_account_private_key(
        &mut self,
        password: &str,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<
        crate::response_vo::standard_wallet::account::GetAccountPrivateKeyRes,
        crate::error::service::ServiceError,
    > {
        WalletDomain::validate_password_with_context(self.ctx, password).await?;
        let pool = self.ctx.core_pool()?;
        let account_list =
            AccountRepo::account_list_by_wallet_address_and_account_id_and_chain_codes(
                pool.clone(),
                Some(wallet_address),
                Some(account_id),
                Vec::new(),
            )
            .await?;
        let chains = ChainRepo::get_chain_list(&pool).await?;

        let mut res = Vec::new();

        let account_index_map = AccountIndexMap::from_account_id(account_id)?;

        let data = domain::account::open_accounts_pk_with_password(
            self.ctx,
            &account_index_map,
            wallet_address,
            password,
        )
        .await?;
        for account in account_list {
            if let Some((_, pk)) = data.iter().find(|(meta, _)| {
                meta.chain_code == account.chain_code
                    && meta.address == account.address
                    && meta.derivation_path == account.derivation_path
            }) {
                let address_type = AccountDomain::get_show_address_type(
                    &account.chain_code,
                    account.address_type(),
                )?;
                if let Some(chain) =
                    chains.iter().find(|chain| chain.chain_code == account.chain_code)
                {
                    let data = crate::response_vo::standard_wallet::account::GetAccountPrivateKey {
                        chain_code: account.chain_code,
                        name: chain.name.clone(),
                        address: account.address,
                        address_type,
                        private_key: pk.to_string(),
                    };
                    res.push(data);
                }
            }
        }

        Ok(crate::response_vo::standard_wallet::account::GetAccountPrivateKeyRes(res))
    }

    pub async fn get_account_list(
        &mut self,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
    ) -> Result<Vec<AccountEntity>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        Ok(AccountRepo::get_account_list_by_wallet_address_and_account_id(
            core_pool,
            wallet_address,
            account_id,
        )
        .await?)
    }

    pub async fn current_chain_address(
        &self,
        address: String,
        account_id: u32,
        chain_code: &str,
    ) -> Result<Vec<QueryAccountDerivationPath>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;

        let res =
            AccountRepo::current_chain_address(address, account_id, chain_code, core_pool).await?;

        let result = res
            .into_iter()
            .map(QueryAccountDerivationPath::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(result)
    }

    pub async fn current_accounts(
        &mut self,
        wallet_address: &str,
        account_id: i32,
    ) -> Result<Vec<CurrentAccountInfo>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let accounts = AccountRepo::get_account_list_by_wallet_address_and_account_id(
            core_pool,
            Some(wallet_address),
            Some(account_id as u32),
        )
        .await?;

        let pool = self.ctx.core_pool()?;

        let mut result = vec![];
        for account in accounts.into_iter() {
            let is_multisig = if account.chain_code == chain_code::TRON {
                MultisigAccountRepo::find_by_condition(&pool, "address", &account.address)
                    .await?
                    .is_some()
            } else {
                false
            };
            let address_type =
                AccountDomain::get_show_address_type(&account.chain_code, account.address_type())?;

            result.push(CurrentAccountInfo {
                chain_code: account.chain_code,
                address: account.address,
                address_type,
                is_multisig,
            });
        }

        Ok(result)
    }

    // pub fn recover_subkey(
    //     &self,
    //     wallet_name: &str,
    //     address: &str,
    // ) -> Result<(), crate::ServiceError> {
    //     let dirs = crate::manager::Context::get_global_dirs()?;
    //     // Get the path to the subkeys directory for the given wallet name.
    //     let subs_path = dirs.get_subs_dir(wallet_name)?;

    //     // Traverse the directory structure to obtain the wallet tree.
    //     let mut wallet_tree =
    //         wallet_tree::wallet_tree::WalletTree::traverse_directory_structure(&dirs.wallet_dir)?;

    //     // Call the recover_subkey function to recover the subkey,
    //     // passing in the wallet tree, address, subkeys path, and wallet name.
    //     let wallet = wallet_tree.get_mut_wallet_branch(wallet_name)?;
    //     Ok(wallet.recover_subkey(address, subs_path)?)
    // }
}
