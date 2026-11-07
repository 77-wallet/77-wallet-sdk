use crate::{
    api::ReturnType,
    context::Context,
    domain::{
        self,
        account::AccountDomain,
        api_wallet::{
            account::ApiAccountDomain, chain::ApiChainDomain, strategy::StrategyDomain,
            wallet::ApiWalletDomain,
        },
        permission::PermissionDomain,
        wallet::WalletDomain,
    },
    error::service::ServiceError,
    infrastructure::task_queue::{
        backend::{BackendApiTask, BackendApiTaskData},
        task::Tasks,
    },
    messaging::mqtt::topics::api_wallet::cmd::address_allock::AddressAllockType,
    response_vo::{
        api_wallet::account::{
            ApiAccountInfo, ApiWalletAddressSearchResp, QueryApiAccountDerivationPath,
        },
        standard_wallet::account::DerivedAddressesList,
    },
};
use std::collections::HashSet;
use wallet_chain_interact::types::ChainPrivateKey;
use wallet_database::{
    entities::api_wallet::ApiWalletType,
    pagination::Pagination,
    repositories::{
        api_wallet::{account::ApiAccountRepo, chain::ApiChainRepo, wallet::ApiWalletRepo},
        device::DeviceRepo,
    },
};
use wallet_transport_backend::request::AddressUpdateAccountNameReq;
use wallet_types::{
    chain::{address::category::AddressCategory, chain::ChainCode},
    constant::chain_code,
};
use wallet_utils::address::AccountIndexMap;

pub(crate) struct ApiAccountApplication {
    ctx: &'static Context,
}

impl ApiAccountApplication {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn search_api_wallet_address(
        &self,
        uid: &str,
        keyword: &str,
    ) -> ReturnType<ApiWalletAddressSearchResp> {
        ApiAccountDomain::search_address(self.ctx, uid, keyword).await
    }

    pub async fn list_api_accounts_v2(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiAccountInfo>, ServiceError> {
        tracing::info!(
            wallet_address = %wallet_address,
            account_id = ?account_id,
            chain_code = ?chain_code,
            page,
            page_size,
            "api_wallet.account.list_api_accounts_v2 requested"
        );
        ApiAccountDomain::list_api_accounts_v2_with_ctx(
            self.ctx,
            wallet_address,
            account_id,
            chain_code,
            page,
            page_size,
        )
        .await
    }

    pub async fn expand_address(
        &self,
        address_allock_type: AddressAllockType,
        chain_code: &str,
        index: Option<i32>,
        uid: &str,
        number: u32,
        serial_no: &str,
        batch_id: &str,
    ) -> Result<(), ServiceError> {
        ApiWalletDomain::new(self.ctx)
            .expand_address(
                "1",
                &address_allock_type,
                index,
                uid,
                chain_code,
                number,
                serial_no,
                batch_id,
            )
            .await?;

        Ok(())
    }

    pub async fn create_account(
        &self,
        wallet_address: &str,
        wallet_password: &str,
        indices: Vec<i32>,
        name: &str,
        is_default_name: bool,
        api_wallet_type: ApiWalletType,
    ) -> Result<(), ServiceError> {
        WalletDomain::validate_password_with_context(self.ctx, wallet_password).await?;
        let pool = self.ctx.api_wallet_pool()?;
        let default_chain_list = ApiChainRepo::get_chain_list(&pool).await?;
        let chains: Vec<String> =
            default_chain_list.iter().map(|chain| chain.chain_code.clone()).collect();
        ApiAccountDomain::create_api_account_with_ctx(
            self.ctx,
            wallet_address,
            chains,
            &indices,
            name,
            is_default_name,
            api_wallet_type,
            None,
            false,
            false,
            0,
        )
        .await?;

        Ok(())
    }

    pub async fn create_withdrawal_account(
        &self,
        wallet_address: &str,
        wallet_password: &str,
        derivation_path: Option<String>,
        index: Option<i32>,
        name: &str,
        is_default_name: bool,
    ) -> Result<(), ServiceError> {
        WalletDomain::validate_password_with_context(self.ctx, wallet_password).await?;
        let pool = self.ctx.api_wallet_pool()?;
        let default_chain_list = ApiChainRepo::get_chain_list(&pool).await?;

        let api_wallet = ApiWalletRepo::find_by_address(&pool, wallet_address).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;
        let hd_path = if let Some(derivation_path) = &derivation_path {
            let hd_path = wallet_chain_instance::derivation_path::get_account_hd_path_from_path(
                derivation_path,
            )?;
            Some(hd_path)
        } else {
            None
        };

        let chains: Vec<String> = if let Some(hd_path) = &hd_path {
            hd_path.get_chain_codes()?.0.into_iter().map(|path| path.to_string()).collect()
        } else {
            default_chain_list.iter().map(|chain| chain.chain_code.clone()).collect()
        };

        let account_index_map = if let Some(index) = index {
            let index = wallet_utils::address::AccountIndexMap::from_input_index(index)?;
            if ApiAccountRepo::has_account_id(
                &pool,
                &api_wallet.address,
                index.account_id,
                ApiWalletType::Withdrawal,
            )
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
        } else if let Some(max_account) =
            ApiAccountRepo::account_detail_by_max_id_and_wallet_address(
                &pool,
                &api_wallet.address,
                ApiWalletType::Withdrawal,
            )
            .await?
        {
            wallet_utils::address::AccountIndexMap::from_account_id(max_account.account_id + 1)?
        } else {
            wallet_utils::address::AccountIndexMap::from_account_id(1)?
        };

        ApiAccountDomain::create_api_account_with_ctx(
            self.ctx,
            wallet_address,
            chains,
            &[account_index_map.input_index],
            name,
            is_default_name,
            ApiWalletType::Withdrawal,
            None,
            false,
            false,
            0,
        )
        .await?;

        Ok(())
    }

    pub async fn get_account_private_key(
        &self,
        address: &str,
        chain_code: &str,
        password: &str,
    ) -> Result<ChainPrivateKey, ServiceError> {
        WalletDomain::validate_password_with_context(self.ctx, password).await?;
        Ok(ApiAccountDomain::get_private_key_with_ctx(self.ctx, address, chain_code).await?)
    }

    pub async fn address_used(
        &self,
        chain_code: &str,
        index: i32,
        uid: &str,
    ) -> Result<(), ServiceError> {
        Ok(ApiAccountDomain::address_used_with_ctx(self.ctx, chain_code, index, uid).await?)
    }

    pub async fn edit_account_name(
        &self,
        account_id: u32,
        wallet_address: &str,
        name: &str,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        ApiAccountRepo::edit_account_name(&pool, wallet_address, account_id, name).await?;

        let wallet = ApiWalletRepo::find_by_address(&pool, wallet_address).await?;
        if wallet.is_none() {
            return Err(crate::error::business::BusinessError::Wallet(
                crate::error::business::wallet::WalletError::NotFound,
            )
            .into());
        }
        let wallet = wallet.unwrap();

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
        &self,
        wallet_address: &str,
        account_id: u32,
        password: &str,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let core_pool = self.ctx.core_pool()?;
        let api_wallet = ApiWalletRepo::find_by_address(&pool, wallet_address).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;

        let index = AccountIndexMap::from_account_id(account_id)?;

        let strategy =
            StrategyDomain::query_withdraw_strategy_with_ctx(self.ctx, &api_wallet.uid).await?;

        if strategy.chain_configs.iter().any(|config| {
            config.normal_address.index == Some(index.input_index)
                || config.risk_address.index == Some(index.input_index)
        }) {
            return Err(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::ApiWalletError::Account(
                    crate::error::business::api_wallet::account::AccountError::ConfiguredWithdrawalStrategyAccountCantBeRemoved,
                ),
            )
            .into());
        }

        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(core_pool.clone(), sn).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };
        WalletDomain::validate_password_with_context(self.ctx, password).await?;
        let account_count = ApiAccountRepo::count_unique_account_ids(&pool, wallet_address).await?;
        if account_count <= 1 {
            return Err(crate::error::business::BusinessError::Account(
                crate::error::business::account::AccountError::CannotDeleteLastAccount,
            )
            .into());
        }

        let deleted = ApiAccountRepo::delete(&pool, wallet_address, account_id).await?;

        let device_unbind_address_task =
            domain::app::DeviceDomain::gen_device_unbind_all_api_address_task_data(
                &deleted, &device.sn,
            )
            .await?;

        for account in deleted.iter() {
            if account.chain_code == chain_code::TRON {
                PermissionDomain::delete_by_address(&core_pool.into_inner(), &account.address)
                    .await?;
            }
        }

        Tasks::new()
            .push(BackendApiTask::BackendApi(device_unbind_address_task))
            .send_with_ctx(self.ctx)
            .await?;

        Ok(())
    }

    pub async fn list_derived_addresses(
        &self,
        wallet_address: &str,
        index: i32,
        password: &str,
        all: bool,
    ) -> Result<Vec<DerivedAddressesList>, ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;

        WalletDomain::validate_password_with_context(self.ctx, password).await?;

        let account_index_map = wallet_utils::address::AccountIndexMap::from_input_index(index)?;

        let api_wallet = ApiWalletRepo::find_by_address(&pool, wallet_address).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;

        let seed = ApiWalletDomain::decrypt_seed(password, &api_wallet.seed).await?;

        let chains: Vec<String> = if !all {
            let all =
                vec!["btc".to_string(), "eth".to_string(), "tron".to_string(), "sol".to_string()];
            let default_chain_list = ApiChainRepo::get_chain_list(&pool).await?;
            let enabled_chain_codes: HashSet<_> =
                default_chain_list.into_iter().map(|chain| chain.chain_code).collect();
            all.into_iter().filter(|chain| enabled_chain_codes.contains(chain)).collect()
        } else {
            let default_chain_list = ApiChainRepo::get_chain_list(&pool).await?;
            default_chain_list.iter().map(|chain| chain.chain_code.clone()).collect()
        };

        let mut res = Vec::new();
        for chain in chains.iter() {
            let code: ChainCode = chain.as_str().try_into()?;
            let address_types = WalletDomain::address_type_by_chain(code);

            let Ok(node) = ApiChainDomain::get_node(chain, self.ctx).await else {
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
                        let account = ApiAccountRepo::find_one_by_address_chain_code(
                            &address,
                            &node.chain_code,
                            &pool,
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

    pub async fn get_account_derivation_path(
        &self,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<Vec<QueryApiAccountDerivationPath>, ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let results = ApiAccountRepo::list_by_wallet_address_account_id(
            &pool,
            Some(wallet_address),
            Some(account_id),
        )
        .await?;
        let r = results
            .into_iter()
            .map(|o| {
                let address_type =
                    AccountDomain::get_show_address_type(&o.chain_code, o.address_type())
                        .unwrap_or(AddressCategory::Other);

                QueryApiAccountDerivationPath::new(
                    o.wallet_address.as_str(),
                    o.derivation_path.as_str(),
                    o.chain_code.as_str(),
                    address_type,
                )
            })
            .collect();
        Ok(r)
    }
}
