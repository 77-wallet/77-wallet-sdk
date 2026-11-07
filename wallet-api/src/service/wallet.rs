use wallet_database::repositories::{
    account::AccountRepo,
    api_wallet::{
        account::ApiAccountRepo, address_query_state::AddressQueryStateRepo, wallet::ApiWalletRepo,
    },
    chain::ChainRepo,
    coin::CoinRepo,
    device::DeviceRepo,
    multisig_member::MultisigMemberRepo,
    wallet::WalletRepo,
};
use wallet_transport_backend::{
    consts::endpoint,
    request::{AddressBatchInitReq, DeviceDeleteReq, LanguageInitReq, TokenQueryPriceReq},
};
use wallet_tree::{api::KeystoreApi, file_ops::RootData};
use wallet_types::constant::chain_code;

use crate::{
    application::wallet::WalletApplication,
    context::Context,
    domain::{
        self,
        account::AccountDomain,
        api_wallet::wallet::ApiWalletDomain,
        app::{DeviceDomain, config::ConfigDomain},
        assets::AssetsDomain,
        chain::ChainDomain,
        coin::CoinDomain,
        multisig::MultisigDomain,
        permission::PermissionDomain,
        wallet::WalletDomain,
    },
    infrastructure::task_queue::{
        CommonTask, RecoverDataBody,
        backend::{BackendApiTask, BackendApiTaskData},
        task::Tasks,
    },
    response_vo::standard_wallet::{
        account::BalanceInfo,
        chain::ChainCodeAndName,
        wallet::{CreateWalletRes, GeneratePhraseRes, QueryPhraseRes},
    },
};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Export {
    chain_code: String,
    derivation_path: String,
    address_type: Option<String>,
}

pub struct WalletService {
    ctx: &'static Context,
    assets_domain: AssetsDomain,
}

impl WalletService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx, assets_domain: AssetsDomain::new() }
    }
    pub(crate) async fn encrypt_password(
        self,
        password: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(core_pool.clone(), sn).await? else {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Device(
                    crate::error::business::device::DeviceError::Uninitialized,
                ),
            ));
        };

        let encrypted_password = WalletDomain::encrypt_password(password, &device.sn)?;
        Ok(encrypted_password)
    }

    pub(crate) async fn validate_password(
        self,
        password: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        tracing::info!("validate_password");
        WalletApplication::validate_password(self.ctx, password).await?;
        tracing::info!("validate_password end");
        Ok(())
    }

    pub(crate) async fn switch_wallet(
        self,
        wallet_address: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let wallet = WalletRepo::update_wallet_update_at(core_pool.clone(), wallet_address).await?;

        if let Some(wallet) = wallet {
            let sn = self.ctx.get_sn();
            DeviceRepo::update_uid(core_pool.clone(), sn, Some(&wallet.uid)).await?;

            let Some(device) = DeviceRepo::get_device_info(core_pool, sn).await? else {
                return Err(crate::error::service::ServiceError::Business(
                    crate::error::business::BusinessError::Device(
                        crate::error::business::device::DeviceError::Uninitialized,
                    ),
                ));
            };

            let config = crate::app_state::APP_STATE.read().await;
            let language = config.language();

            let client_id = domain::app::DeviceDomain::client_id_by_device(&device)?;
            let language_req = wallet_transport_backend::request::LanguageInitReq {
                client_id,
                lan: language.to_string(),
            };
            let language_init_task_data = BackendApiTaskData::new(
                wallet_transport_backend::consts::endpoint::LANGUAGE_INIT,
                &language_req,
            )?;
            Tasks::new()
                .push(BackendApiTask::BackendApi(language_init_task_data))
                .send_with_ctx(self.ctx)
                .await?;
        }

        Ok(())
    }

    pub async fn edit_wallet_name(
        self,
        wallet_name: &str,
        wallet_address: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let wallet_list =
            WalletRepo::edit_wallet_name(core_pool.clone(), wallet_address, wallet_name).await?;
        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(core_pool.clone(), sn).await? else {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Device(
                    crate::error::business::device::DeviceError::Uninitialized,
                ),
            ));
        };

        for wallet in wallet_list {
            let keys_update_wallet_name =
                wallet_transport_backend::request::KeysUpdateWalletNameReq::new(
                    &wallet.uid,
                    &device.sn,
                    &wallet.name,
                );
            let keys_update_wallet_name = BackendApiTaskData::new(
                wallet_transport_backend::consts::endpoint::KEYS_UPDATE_WALLET_NAME,
                &keys_update_wallet_name,
            )?;
            Tasks::new()
                .push(BackendApiTask::BackendApi(keys_update_wallet_name))
                .send_with_ctx(self.ctx)
                .await?;
        }

        Ok(())
    }

    pub async fn import_derivation_path(
        self,
        path: &str,
        wallet_address: &str,
        wallet_password: &str,
        account_name: &str,
        is_default_name: bool,
    ) -> Result<
        crate::response_vo::standard_wallet::wallet::ImportDerivationPathRes,
        crate::error::service::ServiceError,
    > {
        let pool = self.ctx.core_pool()?;

        WalletApplication::validate_password(self.ctx, wallet_password).await?;
        let mut buf = String::new();
        wallet_utils::file_func::read(&mut buf, path)?;

        let exports: Vec<Export> = wallet_utils::serde_func::serde_from_str(&buf)?;
        let seed = WalletApplication::get_seed(self.ctx, wallet_address, wallet_password).await?;

        let wallet = WalletRepo::wallet_detail_by_address(pool.clone(), wallet_address)
            .await?
            .ok_or(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Wallet(
                    crate::error::business::wallet::WalletError::NotFound,
                ),
            ))?;

        let mut subkeys = Vec::<wallet_tree::file_ops::BulkSubkey>::new();
        let mut accounts = Vec::new();
        let mut address_batch_init_task_data = AddressBatchInitReq(Vec::new());
        for data in exports {
            let hd_path = wallet_chain_instance::derivation_path::get_account_hd_path_from_path(
                &data.derivation_path,
            )?;
            let account_index_map =
                wallet_utils::address::AccountIndexMap::from_account_id(hd_path.get_account_id()?)?;
            let Ok(node) = ChainDomain::get_node_with_ctx(self.ctx, &data.chain_code).await else {
                continue;
            };

            let instance = wallet_chain_instance::instance::ChainObject::new(
                &data.chain_code,
                data.address_type,
                crate::domain::chain::ChainDomain::network_kind_from_node_network(&node.network),
            )?;

            let (account, _, address_init_req) = AccountDomain::create_account_v2(
                self.ctx,
                &seed,
                &instance,
                Some(&data.derivation_path),
                &account_index_map,
                &wallet.uid,
                wallet_address,
                account_name,
                is_default_name,
            )
            .await?;

            if let Some(address_init_req) = address_init_req {
                address_batch_init_task_data.0.push(address_init_req);
            } else {
                tracing::info!("不上报： {}", account.address);
            };

            let keypair = instance
                .gen_keypair_with_index_address_type(&seed, account_index_map.input_index)
                .map_err(|e| {
                    crate::error::service::ServiceError::System(
                        crate::error::system::SystemError::Service(e.to_string()),
                    )
                })?;
            let pk = keypair.private_key_bytes()?;
            let subkey = wallet_tree::file_ops::BulkSubkey::new(
                account_index_map.clone(),
                &account.address,
                &data.chain_code,
                &data.derivation_path,
                pk,
            );
            subkeys.push(subkey);
            accounts.push(account.address);
        }

        WalletApplication::store_subkeys(self.ctx, wallet_address, subkeys, wallet_password)
            .await?;

        let address_init_task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::ADDRESS_BATCH_INIT,
            &address_batch_init_task_data,
        )?;
        Tasks::new()
            .push(BackendApiTask::BackendApi(address_init_task_data))
            .send_with_ctx(self.ctx)
            .await?;
        Ok(crate::response_vo::standard_wallet::wallet::ImportDerivationPathRes { accounts })
    }

    pub async fn export_derivation_path(
        &mut self,
        wallet_address: &str,
    ) -> Result<
        crate::response_vo::standard_wallet::wallet::ExportDerivationPathRes,
        crate::error::service::ServiceError,
    > {
        let core_pool = self.ctx.core_pool()?;
        let dirs = self.ctx.get_global_dirs();
        let account_list = AccountRepo::get_account_list_by_wallet_address_and_account_id(
            core_pool,
            Some(wallet_address),
            None,
        )
        .await?;
        let mut derivation_paths = Vec::new();
        for account in account_list.into_iter() {
            let address_type = account.address_type();
            let export = Export {
                chain_code: account.chain_code,
                derivation_path: account.derivation_path,
                address_type,
            };
            derivation_paths.push(export);
        }

        let json = wallet_utils::serde_func::serde_to_string(&derivation_paths)?;
        let path = dirs.get_export_dir().join(wallet_address);
        wallet_utils::file_func::write(&json, &path)?;

        Ok(crate::response_vo::standard_wallet::wallet::ExportDerivationPathRes {
            file_path: path.to_string_lossy().to_string(),
        })
    }

    pub async fn create_wallet(
        &mut self,
        language_code: u8,
        phrase: &str,
        salt: &str,
        wallet_name: &str,
        account_name: &str,
        is_default_name: bool,
        wallet_password: &str,
        invite_code: Option<String>,
    ) -> Result<CreateWalletRes, crate::error::service::ServiceError> {
        let start = std::time::Instant::now();

        let password_validation_start = std::time::Instant::now();
        WalletApplication::validate_password(self.ctx, wallet_password).await?;
        tracing::debug!("Password validation took: {:?}", password_validation_start.elapsed());

        let pool = self.ctx.core_pool()?;
        tracing::info!("Create wallet request received");
        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(pool.clone(), sn).await? else {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Device(
                    crate::error::business::device::DeviceError::Uninitialized,
                ),
            ));
        };

        let master_key_start = std::time::Instant::now();
        let wallet_tree::api::RootInfo { private_key: _, seed, address, phrase } =
            wallet_tree::api::KeystoreApi::generate_master_key_info(language_code, phrase, salt)?;
        tracing::debug!("Master key generation took: {:?}", master_key_start.elapsed());

        let address = &address.to_string();

        if WalletApplication::check_api_wallet_exist(self.ctx, address).await? {
            return Err(crate::error::service::ServiceError::Business(crate::error::business::BusinessError::Wallet(
                crate::error::business::wallet::WalletError::MnemonicAlreadyImportedIntoApiWalletSystem,
            )));
        }

        // let uid = wallet_utils::md5(&format!("{phrase}{salt}"));
        let pbkdf2_string_start = std::time::Instant::now();
        let uid = wallet_utils::pbkdf2_string(&format!("{phrase}{salt}"), salt, 100000, 32)?;
        tracing::debug!("Pbkdf2 string took: {:?}", pbkdf2_string_start.elapsed());

        // 检查是否是api钱包
        if ApiWalletDomain::new(self.ctx).check_keys_uid(&uid).await?.is_api_wallet() {
            return Err(crate::error::service::ServiceError::Business(crate::error::business::BusinessError::Wallet(
                crate::error::business::wallet::WalletError::MnemonicAlreadyImportedIntoApiWalletSystem,
            )));
        }

        let seed = seed.clone();

        let initialize_root_keystore_start = std::time::Instant::now();
        WalletApplication::store_root(
            self.ctx,
            address,
            RootData::new(&phrase, &seed),
            wallet_password,
        )
        .await?;
        // Do not reactivate existing DB records before a protected write succeeds.
        let account_ids = WalletApplication::restart_existing_wallet(pool.clone(), address).await?;
        tracing::debug!(
            "Initialize root keystore took: {:?}",
            initialize_root_keystore_start.elapsed()
        );
        WalletRepo::upsert_wallet(pool.clone(), address, &uid, wallet_name).await?;
        let default_chain_list = ChainRepo::get_chain_list(&pool).await?;
        let coins = CoinRepo::default_coin_list(&pool).await?;
        let default_chain_list =
            default_chain_list.into_iter().map(|chain| chain.chain_code).collect::<Vec<String>>();
        // tracing::info!("coins: {:?}", coins);
        let account_creation_start = std::time::Instant::now();
        let mut req: TokenQueryPriceReq = TokenQueryPriceReq(Vec::new());
        let mut subkeys = Vec::<wallet_tree::file_ops::BulkSubkey>::new();

        let mut address_init_task_data = AddressBatchInitReq(Vec::new());
        for account_id in account_ids {
            let account_index_map =
                wallet_utils::address::AccountIndexMap::from_account_id(account_id)?;

            ChainDomain::init_chains_assets(
                self.ctx,
                &coins,
                &mut req,
                &mut address_init_task_data,
                &mut subkeys,
                &default_chain_list,
                &seed,
                &account_index_map,
                None,
                &uid,
                address,
                account_name,
                is_default_name,
            )
            .await?;
        }
        tracing::info!(
            "Account creation and subkey generation took: {:?}",
            account_creation_start.elapsed()
        );

        let child_keystore_start = std::time::Instant::now();

        // 波场的地址
        let tron_address =
            subkeys.iter().find(|s| s.chain_code == chain_code::TRON).map(|s| s.address.clone());

        WalletApplication::store_subkeys(self.ctx, address, subkeys, wallet_password).await?;
        tracing::debug!("Child keystore initialization took: {:?}", child_keystore_start.elapsed());

        Tasks::new().push(CommonTask::QueryCoinPrice(req)).send_with_ctx(self.ctx).await?;
        let core_pool = self.ctx.core_pool()?;
        let sn = self.ctx.get_sn();
        DeviceRepo::update_uid(core_pool, sn, Some(&uid)).await?;

        let client_id = domain::app::DeviceDomain::client_id_by_device(&device)?;

        let language_req = {
            let config = crate::app_state::APP_STATE.read().await;
            LanguageInitReq::new(&client_id, config.language())
        };

        let keys_init_req = wallet_transport_backend::request::KeysInitReq::new(
            &uid,
            &device.sn,
            Some(client_id),
            Some(device.device_type),
            wallet_name,
            invite_code,
        );
        let keys_init_task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::KEYS_V2_INIT,
            &keys_init_req,
        )?;

        let language_init_task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::LANGUAGE_INIT,
            &language_req,
        )?;

        // let uids = tx
        //     .uid_list()
        //     .await?
        //     .into_iter()
        //     .map(|uid| uid.0)
        //     .collect::<Vec<String>>();
        // let device_delete_req = DeviceDeleteReq::new(&device.sn, &uids);

        // let device_delete_task_data =
        //     BackendApiTaskData::new(endpoint::DEVICE_DELETE, &device_delete_req)?;

        // let device_bind_address_task_data =
        //     domain::app::DeviceDomain::gen_device_bind_address_task_data().await?;

        // 恢复多签账号、多签队列
        let mut recover_data = RecoverDataBody::new(&uid);
        if let Some(tron_address) = tron_address {
            recover_data.tron_address = Some(tron_address);
        };
        let address_init_task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::ADDRESS_BATCH_INIT,
            &address_init_task_data,
        )?;
        Tasks::new()
            .push(BackendApiTask::BackendApi(keys_init_task_data))
            .push(BackendApiTask::BackendApi(language_init_task_data))
            .push(CommonTask::RecoverMultisigAccountData(recover_data))
            .push(BackendApiTask::BackendApi(address_init_task_data))
            .send_with_ctx(self.ctx)
            .await?;

        tracing::debug!("cose time: {}", start.elapsed().as_millis());
        Ok(CreateWalletRes { address: address.to_string() })
    }

    pub async fn get_phrase(
        &mut self,
        wallet_address: &str,
        password: &str,
    ) -> Result<
        crate::response_vo::standard_wallet::wallet::GetPhraseRes,
        crate::error::service::ServiceError,
    > {
        let root = WalletApplication::load_root(self.ctx, wallet_address, password).await?;
        Ok(crate::response_vo::standard_wallet::wallet::GetPhraseRes {
            phrase: root.phrase().to_owned(),
        })
    }

    pub(crate) fn generate_phrase(
        &self,
        language_code: u8,
        count: usize,
    ) -> Result<GeneratePhraseRes, crate::error::service::ServiceError> {
        let lang = wallet_core::language::Language::from_u8(language_code).map_err(|e| {
            crate::error::service::ServiceError::System(crate::error::system::SystemError::Service(
                e.to_string(),
            ))
        })?;

        let phrases = lang.gen_phrase(count).map_err(|e| {
            crate::error::service::ServiceError::System(crate::error::system::SystemError::Service(
                e.to_string(),
            ))
        })?;

        Ok(GeneratePhraseRes { phrases })
    }

    pub(crate) fn query_phrases(
        &self,
        language_code: u8,
        keyword: &str,
        mode: u8,
    ) -> Result<QueryPhraseRes, crate::error::service::ServiceError> {
        let wordlist_wrapper =
            wallet_core::language::WordlistWrapper::new(language_code).map_err(|e| {
                crate::error::service::ServiceError::System(
                    crate::error::system::SystemError::Service(e.to_string()),
                )
            })?;
        let mode = wallet_core::language::QueryMode::from_u8(mode).map_err(|e| {
            crate::error::service::ServiceError::System(crate::error::system::SystemError::Service(
                e.to_string(),
            ))
        })?;

        let phrases = wordlist_wrapper.query_phrase(keyword, mode);

        Ok(QueryPhraseRes { phrases })
    }

    pub(crate) fn exact_query_phrase(
        &self,
        language_code: u8,
        phrases: Vec<&str>,
    ) -> Result<Vec<String>, crate::error::service::ServiceError> {
        let wordlist_wrapper =
            wallet_core::language::WordlistWrapper::new(language_code).map_err(|e| {
                crate::error::service::ServiceError::System(
                    crate::error::system::SystemError::Service(e.to_string()),
                )
            })?;
        let res = phrases
            .iter()
            .map(|phrase| wordlist_wrapper.exact_query_phrase(phrase).unwrap_or_default())
            .collect();

        Ok(res)
    }

    pub async fn get_wallet_list(
        &mut self,
        wallet_address: Option<String>,
        chain_code: Option<String>,
        account_id: Option<u32>,
    ) -> Result<
        Vec<crate::response_vo::standard_wallet::wallet::WalletInfo>,
        crate::error::service::ServiceError,
    > {
        let pool = self.ctx.core_pool()?;
        let chains = ChainRepo::get_chain_list(&pool).await?;
        let chain_codes = if let Some(chain_code) = chain_code {
            vec![chain_code]
        } else {
            chains.iter().map(|chain| chain.chain_code.clone()).collect()
        };

        let chains: ChainCodeAndName = chains.into();

        let token_currencies = CoinDomain::get_token_currencies_v2_with_ctx(self.ctx).await?;
        // let service = Service::default();
        let wallet_list = if let Some(wallet_address) = &wallet_address {
            let wallet = WalletRepo::wallet_detail_by_address(pool.clone(), wallet_address)
                .await?
                .ok_or(crate::error::service::ServiceError::Business(
                    crate::error::business::BusinessError::Wallet(
                        crate::error::business::wallet::WalletError::NotFound,
                    ),
                ))?;
            vec![wallet]
        } else {
            WalletRepo::wallet_list(pool.clone()).await?
        };
        let mut res = Vec::new();
        for wallet_info in wallet_list {
            let list = AccountRepo::account_list_by_wallet_address_and_chain_code(
                pool.clone(),
                Some(&wallet_info.address),
                chain_codes.clone(),
                account_id,
            )
            .await?;
            let mut account_list =
                token_currencies.calculate_account_infos(self.ctx, list, &chains).await?;
            // let mut account_cal_list = std::collections::HashMap::new();
            let mut wallet_assets = BalanceInfo::new_without_amount(self.ctx).await?;
            for account in account_list.iter_mut() {
                let mut account_assets_entity = self
                    .assets_domain
                    .get_account_assets_entity(
                        &pool,
                        account.account_id,
                        &wallet_info.address,
                        chain_codes.clone(),
                        None,
                    )
                    .await?;

                let account_total_assets = token_currencies
                    .calculate_account_total_assets(self.ctx, &mut account_assets_entity)
                    .await?;
                let fiat_value = account_total_assets.fiat_value;
                let amount = account_total_assets.amount;
                account.balance.fiat_add(fiat_value);
                account.balance.amount_add(amount);
                wallet_assets.fiat_add(fiat_value);
                wallet_assets.amount_add(amount);
            }

            res.push(crate::response_vo::standard_wallet::wallet::WalletInfo {
                address: wallet_info.address,
                uid: wallet_info.uid,
                name: wallet_info.name,
                balance: wallet_assets,
                created_at: wallet_info.created_at,
                updated_at: wallet_info.updated_at,
                account_list,
            });
        }

        Ok(res)
    }

    pub async fn logic_delete(
        self,
        address: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let sn = self.ctx.get_sn();
        let core_pool = self.ctx.core_pool()?;

        let mut tx = core_pool
            .as_ref()
            .begin()
            .await
            .map_err(|e| wallet_database::Error::Database(e.into()))?;

        let wallet = WalletRepo::wallet_detail_by_address_with_executor(&mut tx, address).await?;
        WalletRepo::reset_with_executor(&mut tx, address).await?;
        AccountRepo::reset_with_executor(&mut tx, address).await?;

        let latest_wallet = WalletRepo::wallet_latest_with_executor(&mut tx).await?;
        let rest_uids = WalletRepo::uid_list_with_executor(&mut tx)
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect::<Vec<String>>();

        DeviceRepo::update_uid_with_executor(
            &mut tx,
            sn,
            latest_wallet.as_ref().map(|w| w.uid.as_str()),
        )
        .await?;

        tx.commit().await.map_err(|e| wallet_database::Error::Database(e.into()))?;

        if let Some(wallet) = wallet {
            let pool = core_pool.clone().into_inner();
            let members = MultisigMemberRepo::list_by_uid(&core_pool, &wallet.uid).await?;
            for member in members.0 {
                MultisigDomain::logic_delete_account(&member.account_id, pool.clone()).await?;
            }
            let Some(device) = DeviceRepo::get_device_info(core_pool.clone(), sn).await? else {
                return Err(crate::error::service::ServiceError::Business(
                    crate::error::business::BusinessError::Device(
                        crate::error::business::device::DeviceError::Uninitialized,
                    ),
                ));
            };
            let req = DeviceDeleteReq::new(&device.sn, &rest_uids);

            Tasks::new()
                .push(BackendApiTask::BackendApi(BackendApiTaskData::new(
                    endpoint::DEVICE_DELETE,
                    &req,
                )?))
                .send_with_ctx(self.ctx)
                .await?;
        };

        Ok(())
    }

    pub async fn physical_delete(
        self,
        address: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let sn = self.ctx.get_sn();
        let core_pool = self.ctx.core_pool()?;
        tracing::info!("delete wallet ------------ -3");

        WalletApplication::delete_wallet_files(self.ctx, Some(address)).await?;

        let mut tx = core_pool
            .as_ref()
            .begin()
            .await
            .map_err(|e| wallet_database::Error::Database(e.into()))?;

        let wallet = WalletRepo::wallet_detail_by_address_with_executor(&mut tx, address).await?;
        WalletRepo::physical_delete_with_executor(&mut tx, &[address]).await?;
        let accounts = AccountRepo::physical_delete_all_with_executor(&mut tx, &[address]).await?;
        let latest_wallet = WalletRepo::wallet_latest_with_executor(&mut tx).await?;
        DeviceRepo::update_uid_with_executor(
            &mut tx,
            sn,
            latest_wallet.as_ref().map(|w| w.uid.as_str()),
        )
        .await?;

        tx.commit().await.map_err(|e| wallet_database::Error::Database(e.into()))?;

        tracing::info!("delete wallet ------------ -2");
        let dirs = self.ctx.get_global_dirs();
        tracing::info!("delete wallet ------------ -1");

        let api_pool = self.ctx.api_wallet_pool()?;
        let rest_standard_uids = WalletRepo::uid_list(core_pool.clone())
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect::<Vec<String>>();
        let rest_api_uids = ApiWalletRepo::uid_list(&api_pool)
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect::<Vec<String>>();

        let rest_uids = rest_standard_uids
            .iter()
            .cloned()
            .chain(rest_api_uids.iter().cloned())
            .collect::<Vec<String>>();

        tracing::info!("rest_uids: {:?}", rest_uids);
        tracing::info!("delete wallet ------------ 0");

        if rest_standard_uids.is_empty() && rest_api_uids.is_empty() {
            KeystoreApi::remove_verify_file(&dirs.root_dir)?;
            DeviceRepo::update_password_proof(core_pool.clone(), sn, None).await?;
            ApiWalletDomain::new(self.ctx).clear_wallet_unlock_session().await?;
        }

        let pool = core_pool.clone().into_inner();
        if let Some(wallet) = wallet {
            tracing::info!("delete wallet ------------ 3");
            let req = DeviceDeleteReq::new(&sn, &rest_uids);

            let members = MultisigMemberRepo::list_by_uid(&core_pool, &wallet.uid).await?;
            tracing::info!("delete wallet ------------ 4");
            let multisig_accounts =
                MultisigDomain::physical_delete_wallet_account(members, &wallet.uid, pool.clone())
                    .await?;
            tracing::info!("delete wallet ------------ 5");
            let device_unbind_address_task = DeviceDomain::gen_device_unbind_all_address_task_data(
                &accounts,
                multisig_accounts,
                &sn,
            )
            .await?;
            tracing::info!("delete wallet ------------ 6");

            // FIXME: 这里的任务执行时间不能保证，比后续的设备初始化等接口快执行，所以暂时先用同步处理
            let backend = self.ctx.get_api_wallet_backend();
            backend.device_delete(&req).await?;

            Tasks::new()
                // .push(BackendApiTask::BackendApi(BackendApiTaskData::new(
                //     endpoint::DEVICE_DELETE,
                //     &req,
                // )?))
                .push(BackendApiTask::BackendApi(device_unbind_address_task))
                .send_with_ctx(self.ctx)
                .await?;
        };

        // find tron address and del permission
        let tron_address = accounts.iter().find(|a| a.chain_code == chain_code::TRON);
        tracing::warn!("tron address = {:?}", tron_address);
        if let Some(address) = tron_address {
            PermissionDomain::delete_by_address(&pool, &address.address).await?;
        }

        for uid in rest_uids {
            let body = RecoverDataBody::new(&uid);

            Tasks::new()
                .push(CommonTask::RecoverMultisigAccountData(body))
                .send_with_ctx(self.ctx)
                .await?;
        }
        Ok(())
    }

    pub async fn logic_reset(self) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(core_pool.clone(), sn).await? else {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Device(
                    crate::error::business::device::DeviceError::Uninitialized,
                ),
            ));
        };

        WalletApplication::delete_wallet_files(self.ctx, None).await?;

        let mut tx = core_pool
            .as_ref()
            .begin()
            .await
            .map_err(|e| wallet_database::Error::Database(e.into()))?;
        WalletRepo::reset_all_wallet_with_executor(&mut tx).await?;
        AccountRepo::reset_all_account_with_executor(&mut tx).await?;
        tx.commit().await.map_err(|e| wallet_database::Error::Database(e.into()))?;

        let req = DeviceDeleteReq::new(&device.sn, &[]);

        Tasks::new()
            .push(BackendApiTask::BackendApi(BackendApiTaskData::new(
                endpoint::DEVICE_DELETE,
                &req,
            )?))
            .send_with_ctx(self.ctx)
            .await?;

        Ok(())
    }

    pub async fn physical_reset(self) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let core_pool = self.ctx.core_pool()?;
        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(core_pool.clone(), sn).await? else {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Device(
                    crate::error::business::device::DeviceError::Uninitialized,
                ),
            ));
        };

        // 1. 首先递增Epoch，切换世代，这是reset的核心事实
        // 确保reset开始后，所有后续操作都使用新世代的Epoch
        ConfigDomain::bump_keys_reset_epoch(self.ctx).await?;
        // 获取新的epoch值用于日志
        let new_epoch = ConfigDomain::get_keys_reset_epoch(self.ctx).await?;
        tracing::info!(epoch = new_epoch, "physical_reset: Epoch bumped, generation switched");

        WalletApplication::delete_wallet_files(self.ctx, None).await?;

        let mut tx = core_pool
            .as_ref()
            .begin()
            .await
            .map_err(|e| wallet_database::Error::Database(e.into()))?;
        DeviceRepo::update_password_with_executor(&mut tx, sn, None).await?;
        WalletRepo::physical_delete_all_with_executor(&mut tx).await?;
        AccountRepo::physical_delete_all_with_executor(&mut tx, &[]).await?;
        tx.commit().await.map_err(|e| wallet_database::Error::Database(e.into()))?;

        ApiWalletRepo::physical_delete_all_wallet(&pool).await?;
        crate::domain::api_wallet::wallet_list_policy::invalidate_api_wallet_list();
        // 删除所有mqtt相关的任务
        // TaskQueueRepoTrait::delete_all(&mut tx, 2).await?;
        ApiAccountRepo::physical_delete_all(&pool, &[]).await?;
        AddressQueryStateRepo::delete_all(&pool).await?;

        let req = DeviceDeleteReq::new(&device.sn, &[]);
        // FIXME: 这里的任务执行时间不能保证，比后续的设备初始化等接口快执行，所以暂时先用同步处理
        let backend = self.ctx.get_api_wallet_backend();
        backend.device_delete(&req).await?;
        // let device_delete_task = BackendApiTaskData::new(endpoint::DEVICE_DELETE, &req)?;
        MultisigDomain::physical_delete_all_account(core_pool.clone()).await?;
        // let device_unbind_address_task = DeviceDomain::gen_device_unbind_all_address_task_data(
        //     &accounts,
        //     multisig_accounts,
        //     &device.sn,
        // )
        // .await?;
        let reset_task = BackendApiTaskData::new(
            endpoint::KEYS_RESET,
            &serde_json::json!({
                "sn": device.sn
            }),
        )?;

        Tasks::new()
            // .push(BackendApiTask::BackendApi(device_delete_task))
            .push(BackendApiTask::BackendApi(reset_task))
            .send_with_ctx(self.ctx)
            .await?;

        let dirs = self.ctx.get_global_dirs();
        ApiWalletDomain::new(self.ctx).clear_wallet_unlock_session().await?;
        KeystoreApi::remove_verify_file(&dirs.root_dir)?;
        DeviceRepo::update_password_proof(core_pool.clone(), sn, None).await?;

        Ok(())
    }

    pub async fn recover_multisig_data(
        self,
        _wallet_address: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        // 在创建钱包时，skd已经在任务里面添加了task 来恢复，这里没有必要给到前端一个接口再去执行一遍重复的逻辑
        // let mut tx = self.repo;
        // MultisigDomain::recover_multisig_account_and_queue_data(&mut tx, wallet_address).await?;

        Ok(())
    }

    pub async fn upgrade_algorithm(
        &self,
        password: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        WalletApplication::upgrade_algorithm(self.ctx, password).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use wallet_database::SqliteContext;

    use crate::domain::multisig::MultisigDomain;

    fn _uid(phrase: &str, salt: &str) -> String {
        let uid = format!("{phrase}{salt}");
        wallet_utils::md5(&uid)
    }

    fn uid_pbkdf2(phrase: &str, salt: &str) -> String {
        let uid = format!("{phrase}{salt}");
        wallet_utils::pbkdf2_string(&uid, salt, 100000, 32).unwrap()
    }

    #[test]
    fn wallet_uid_pbkdf2_matches_fixed_vector() {
        let phrase = "";
        let salt = "test-salt";

        let uid = uid_pbkdf2(phrase, salt);

        assert_eq!(uid, "65a22f8aed8a36225a75640865477f5f3b3769926f1d1102f5766970a4f4dadb");
    }

    #[tokio::test]
    async fn physical_reset_multisig_cleanup_uses_core_pool() -> Result<(), anyhow::Error> {
        let temp = tempdir()?;
        let root_dir = temp.path().to_string_lossy().to_string();
        let core_sqlite = SqliteContext::new(&root_dir, Some("data.db")).await?;
        let core_pool = wallet_database::CoreDbPool::new(core_sqlite.get_pool()?);

        let api_sqlite = SqliteContext::new(&root_dir, Some("api_wallet.db")).await?;
        let _api_pool = wallet_database::ApiWalletDbPool::new(api_sqlite.get_pool()?);

        MultisigDomain::physical_delete_all_account(core_pool.clone()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_reset_root() {
        // let phrase =
        //     "";
        // let phrase = "";
        // let phrase = "";
        // let phrase = "";

        let phrase =
            "";
        // let phrase =
        //     "";
        // let phrase = "";
        // let phrase = "";
        // let phrase = "";
        // let phrase = "";

        let salt = "12345678";
        // let salt = "1234qwer";
        let uid_md5 = _uid(phrase, salt);
        let uid = uid_pbkdf2(phrase, salt);

        println!("uid_md5: {}", uid_md5);
        println!("uid: {}", uid);
    }
}
