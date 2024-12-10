use wallet_database::{
    dao::{assets::CreateAssetsVo, multisig_member::MultisigMemberDaoV1},
    entities::assets::AssetsId,
    repositories::{
        account::AccountRepoTrait, assets::AssetsRepoTrait, chain::ChainRepoTrait,
        coin::CoinRepoTrait, device::DeviceRepoTrait, wallet::WalletRepoTrait, ResourcesRepo,
        TransactionTrait as _,
    },
};
use wallet_transport_backend::{
    consts::endpoint,
    request::{DeviceDeleteReq, LanguageInitReq, TokenQueryPriceReq},
};
use wallet_types::chain::{
    address::r#type::{AddressType, BTC_ADDRESS_TYPES},
    chain::ChainCode,
};

use crate::{
    domain::{
        self,
        account::AccountDomain,
        app::DeviceDomain,
        assets::AssetsDomain,
        coin::CoinDomain,
        multisig::{MultisigDomain, MultisigQueueDomain},
        task_queue::{BackendApiTask, CommonTask, Task, Tasks},
        wallet::WalletDomain,
    },
    response_vo::{
        account::BalanceInfo,
        chain::ChainCodeAndName,
        wallet::{CreateWalletRes, GeneratePhraseRes, QueryPhraseRes, ResetRootRes},
    },
};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Export {
    chain_code: String,
    derivation_path: String,
    address_type: Option<String>,
}

pub struct WalletService {
    pub repo: ResourcesRepo,
    wallet_domain: WalletDomain,
    account_domain: AccountDomain,
    assets_domain: AssetsDomain,
    coin_domain: CoinDomain,
}

impl WalletService {
    pub fn new(repo: ResourcesRepo) -> Self {
        Self {
            repo,
            wallet_domain: WalletDomain::new(),
            account_domain: AccountDomain::new(),
            assets_domain: AssetsDomain::new(),
            coin_domain: CoinDomain::new(),
        }
    }

    pub(crate) async fn encrypt_password(
        mut self,
        password: &str,
    ) -> Result<String, crate::ServiceError> {
        let Some(device) = self.repo.get_device_info().await? else {
            return Err(crate::BusinessError::Device(crate::DeviceError::Uninitialized).into());
        };

        let encrypted_password = WalletDomain::encrypt_password(password, &device.sn)?;
        Ok(encrypted_password)
    }

    pub(crate) async fn validate_password(
        mut self,
        encrypted_password: &str,
    ) -> Result<(), crate::ServiceError> {
        let Some(device) = self.repo.get_device_info().await? else {
            return Err(crate::BusinessError::Device(crate::DeviceError::Uninitialized).into());
        };
        let Some(password) = device.password else {
            return Err(crate::BusinessError::Wallet(crate::WalletError::PasswordNotSet).into());
        };

        if password != encrypted_password {
            return Err(crate::BusinessError::Wallet(crate::WalletError::PasswordIncorrect).into());
        }

        Ok(())
    }

    pub(crate) async fn switch_wallet(
        self,
        wallet_address: &str,
    ) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        let wallet = tx.update_wallet_update_at(wallet_address).await?;

        if let Some(wallet) = wallet {
            tx.update_uid(Some(&wallet.uid)).await?;
            let device = tx
                .get_device_info()
                .await?
                .ok_or(crate::BusinessError::Device(
                    crate::DeviceError::Uninitialized,
                ))?;
            let config = crate::app_state::APP_STATE.read().await;
            let language = config.language();

            let client_id = domain::app::DeviceDomain::client_id_by_device(&device)?;
            let language_req = wallet_transport_backend::request::LanguageInitReq {
                client_id,
                lan: language.to_string(),
            };
            let language_init_task_data = crate::domain::task_queue::BackendApiTaskData::new(
                wallet_transport_backend::consts::endpoint::LANGUAGE_INIT,
                &language_req,
            )?;
            Tasks::new()
                .push(Task::BackendApi(BackendApiTask::BackendApi(
                    language_init_task_data,
                )))
                .send()
                .await?;
        }

        Ok(())
    }

    pub async fn edit_wallet_name(
        self,
        wallet_name: &str,
        wallet_address: &str,
    ) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        let wallet_list = tx.edit_wallet_name(wallet_address, wallet_name).await?;

        let device = tx.get_device_info().await?;
        if let Some(device) = &device {
            let client_id = domain::app::DeviceDomain::client_id_by_device(device)?;

            for wallet in wallet_list {
                let keys_init_req = wallet_transport_backend::request::KeysInitReq::new(
                    &wallet.uid,
                    &device.sn,
                    Some(client_id.clone()),
                    device.app_id.clone(),
                    Some(device.device_type.clone()),
                    wallet_name,
                );
                let keys_init_task_data = domain::task_queue::BackendApiTaskData::new(
                    wallet_transport_backend::consts::endpoint::KEYS_INIT,
                    &keys_init_req,
                )?;
                domain::task_queue::Tasks::new()
                    .push(Task::BackendApi(BackendApiTask::BackendApi(
                        keys_init_task_data,
                    )))
                    .send()
                    .await?;
            }
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
    ) -> Result<crate::response_vo::wallet::ImportDerivationPathRes, crate::ServiceError> {
        let mut tx = self.repo;
        let Some(device) = tx.get_device_info().await? else {
            return Err(crate::BusinessError::Device(crate::DeviceError::Uninitialized).into());
        };
        WalletDomain::validate_password(&device, wallet_password)?;
        let dirs = crate::manager::Context::get_global_dirs()?;
        let mut buf = String::new();
        wallet_utils::file_func::read(&mut buf, path)?;

        let datas: Vec<Export> = wallet_utils::serde_func::serde_from_str(&buf)?;
        let seed_wallet =
            self.wallet_domain
                .get_seed_wallet(dirs, wallet_address, wallet_password)?;

        let wallet = tx
            .wallet_detail_by_address(wallet_address)
            .await?
            .ok_or(crate::BusinessError::Wallet(crate::WalletError::NotFound))?;

        let mut accounts = Vec::new();
        for data in datas {
            let hd_path = wallet_chain_instance::derivation_path::get_account_hd_path_from_path(
                &data.derivation_path,
            )?;
            let account_index_map =
                wallet_utils::address::AccountIndexMap::from_account_id(hd_path.get_account_id()?)?;
            let Some(chain) = tx.detail_with_node(&data.chain_code).await? else {
                continue;
            };
            let instance = wallet_chain_instance::instance::ChainObject::new(
                &data.chain_code,
                data.address_type,
                chain.network.as_str().into(),
            )?;

            let account = self
                .account_domain
                .create_account_with_derivation_path(
                    &mut tx,
                    dirs,
                    &seed_wallet.seed,
                    instance,
                    &Some(data.derivation_path),
                    &account_index_map,
                    &wallet.uid,
                    wallet_address,
                    wallet_password,
                    None,
                    account_name,
                    is_default_name,
                )
                .await?;
            accounts.push(account.address)
        }

        Ok(crate::response_vo::wallet::ImportDerivationPathRes { accounts })
    }

    pub async fn export_derivation_path(
        &mut self,
        wallet_address: &str,
    ) -> Result<crate::response_vo::wallet::ExportDerivationPathRes, crate::ServiceError> {
        let tx = &mut self.repo;
        let dirs = crate::manager::Context::get_global_dirs()?;
        let account_list = tx
            .get_account_list_by_wallet_address_and_account_id(Some(wallet_address), None)
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

        Ok(crate::response_vo::wallet::ExportDerivationPathRes {
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
        derive_password: Option<String>,
    ) -> Result<CreateWalletRes, crate::ServiceError> {
        let tx = &mut self.repo;

        let Some(device) = tx.get_device_info().await? else {
            return Err(crate::BusinessError::Device(crate::DeviceError::Uninitialized).into());
        };

        let encrypted_pw = WalletDomain::encrypt_password(wallet_password, &device.sn)?;
        // 如果有密码，就校验，否则更新密码
        if let Some(password) = &device.password {
            if password != &encrypted_pw {
                return Err(
                    crate::BusinessError::Wallet(crate::WalletError::PasswordIncorrect).into(),
                );
            }
        } else {
            tx.update_password(Some(&encrypted_pw)).await?;
        }

        let dirs = crate::manager::Context::get_global_dirs()?;

        let wallet_keystore::api::RootInfo {
            private_key,
            seed,
            address,
            phrase,
        } = wallet_keystore::api::KeystoreApi::generate_master_key_info(
            language_code,
            phrase,
            salt,
        )?;

        // let uid = wallet_utils::md5(&format!("{phrase}{salt}"));
        let uid = wallet_utils::pbkdf2_string(&format!("{phrase}{salt}"), salt, 100000, 32)?;

        let address = &address.to_string();
        let seed = seed.clone();

        // 检查钱包状态
        let account_ids = self
            .wallet_domain
            .restart_existing_wallet(tx, address)
            .await?;

        let storage_path = dirs.get_root_dir(address)?;
        wallet_utils::file_func::recreate_dir_all(&storage_path)?;

        wallet_keystore::api::KeystoreApi::initialize_root_keystore(
            address,
            &private_key,
            &seed,
            &phrase,
            &storage_path,
            wallet_password,
        )?;
        tx.upsert_wallet(address, &uid, wallet_name).await?;
        let default_chain_list = tx.get_chain_list().await?;
        let coins = tx.default_coin_list().await?;
        let default_chain_list = default_chain_list
            .into_iter()
            .map(|chain| chain.chain_code)
            .collect::<Vec<String>>();

        let mut req: TokenQueryPriceReq = TokenQueryPriceReq(Vec::new());
        for account_id in account_ids {
            let account_index_map =
                wallet_utils::address::AccountIndexMap::from_account_id(account_id)?;
            for chain_code in &default_chain_list {
                let btc_address_types = if chain_code == "btc" {
                    BTC_ADDRESS_TYPES.to_vec()
                } else {
                    vec![AddressType::Other]
                };

                let code: ChainCode = chain_code.as_str().try_into()?;
                for btc_address_type in btc_address_types {
                    let Some(chain) = tx.detail_with_node(chain_code).await? else {
                        continue;
                    };

                    let instance: wallet_chain_instance::instance::ChainObject =
                        (&code, &btc_address_type, chain.network.as_str().into()).try_into()?;

                    let address = self
                        .account_domain
                        .create_account_with_account_id(
                            tx,
                            dirs,
                            &seed,
                            instance,
                            &account_index_map,
                            &uid,
                            address,
                            wallet_password,
                            derive_password.clone(),
                            account_name,
                            is_default_name,
                        )
                        .await?;
                    for coin in &coins {
                        if chain_code == &coin.chain_code {
                            let assets_id =
                                AssetsId::new(&address.address, &coin.chain_code, &coin.symbol);
                            let assets = CreateAssetsVo::new(
                                assets_id,
                                coin.decimals,
                                coin.token_address.clone(),
                                coin.protocol.clone(),
                                0,
                            )
                            .with_name(&coin.name)
                            .with_u256(alloy::primitives::U256::default(), coin.decimals)?;
                            if coin.price.is_empty() {
                                req.insert(
                                    chain_code,
                                    &assets.token_address.clone().unwrap_or_default(),
                                );
                            }
                            tx.upsert_assets(assets).await?;
                        }
                    }
                }
            }
        }
        let task =
            domain::task_queue::Task::Common(domain::task_queue::CommonTask::QueryCoinPrice(req));
        Tasks::new().push(task).send().await?;
        tx.update_uid(Some(&uid)).await?;

        let config = crate::app_state::APP_STATE.read().await;
        let language = config.language();

        let client_id = domain::app::DeviceDomain::client_id_by_device(&device)?;

        let language_req = LanguageInitReq::new(&client_id, language);

        let keys_init_req = wallet_transport_backend::request::KeysInitReq::new(
            &uid,
            &device.sn,
            Some(client_id),
            device.app_id,
            Some(device.device_type),
            wallet_name,
        );
        let keys_init_task_data = domain::task_queue::BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::KEYS_INIT,
            &keys_init_req,
        )?;

        let language_init_task_data = domain::task_queue::BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::LANGUAGE_INIT,
            &language_req,
        )?;

        let uids = tx
            .uid_list()
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect::<Vec<String>>();
        let device_delete_req = DeviceDeleteReq::new(&device.sn, &uids);

        let device_delete_task_data = domain::task_queue::BackendApiTaskData::new(
            endpoint::DEVICE_DELETE,
            &device_delete_req,
        )?;

        let device_bind_address_task_data =
            domain::app::DeviceDomain::gen_device_bind_address_task_data(&device.sn).await?;

        let _ = domain::app::config::ConfigDomain::report_backend(&device.sn).await;

        domain::task_queue::Tasks::new()
            .push(Task::BackendApi(BackendApiTask::BackendApi(
                keys_init_task_data,
            )))
            .push(Task::BackendApi(BackendApiTask::BackendApi(
                language_init_task_data,
            )))
            .push(Task::BackendApi(BackendApiTask::BackendApi(
                device_delete_task_data,
            )))
            .push(Task::BackendApi(BackendApiTask::BackendApi(
                device_bind_address_task_data,
            )))
            .push(Task::Common(CommonTask::RecoverMultisigAccountData(uid)))
            .send()
            .await?;

        // let accounts = tx.get_account_list_by_wallet_address(Some(address)).await?;
        // tokio::spawn(async move {
        //     if let Err(e) = MultisigDomain::recover_uid_multisig_data(&uid).await {
        //         tracing::error!("recover multisig account data error: {}", e);
        //     };

        //     if let Err(e) = MultisigQueueDomain::recover_all_queue_data(&uid).await {
        //         tracing::error!("recover multisig queue data error: {}", e);
        //     }
        // });

        Ok(CreateWalletRes {
            address: address.to_string(),
        })
    }

    pub async fn get_phrase(
        &self,
        wallet_address: &str,
        password: &str,
    ) -> Result<crate::response_vo::wallet::GetPhraseRes, crate::ServiceError> {
        let dirs = crate::manager::Context::get_global_dirs()?;
        let root_dir = dirs.get_root_dir(wallet_address)?;
        let phrase_wallet =
            wallet_keystore::Keystore::load_phrase_keystore(wallet_address, &root_dir, password)?;
        Ok(crate::response_vo::wallet::GetPhraseRes {
            phrase: phrase_wallet.phrase,
        })
    }

    pub(crate) fn generate_phrase(
        &self,
        language_code: u8,
        count: usize,
    ) -> Result<GeneratePhraseRes, crate::ServiceError> {
        let lang = wallet_core::language::Language::from_u8(language_code)
            .map_err(|e| crate::SystemError::Service(e.to_string()))?;

        let phrases = lang
            .gen_phrase(count)
            .map_err(|e| crate::SystemError::Service(e.to_string()))?;

        Ok(GeneratePhraseRes { phrases })
    }

    pub(crate) fn query_phrases(
        &self,
        language_code: u8,
        keyword: &str,
        mode: u8,
    ) -> Result<QueryPhraseRes, crate::ServiceError> {
        let wordlist_wrapper = wallet_core::language::WordlistWrapper::new(language_code)
            .map_err(|e| crate::SystemError::Service(e.to_string()))?;
        let mode = wallet_core::language::QueryMode::from_u8(mode)
            .map_err(|e| crate::SystemError::Service(e.to_string()))?;

        let phrases = wordlist_wrapper.query_phrase(keyword, mode);

        Ok(QueryPhraseRes { phrases })
    }

    pub(crate) fn exact_query_phrase(
        &self,
        language_code: u8,
        phrases: Vec<&str>,
    ) -> Result<Vec<String>, crate::ServiceError> {
        let wordlist_wrapper = wallet_core::language::WordlistWrapper::new(language_code)
            .map_err(|e| crate::SystemError::Service(e.to_string()))?;
        let res = phrases
            .iter()
            .map(|phrase| {
                wordlist_wrapper
                    .exact_query_phrase(phrase)
                    .unwrap_or_default()
            })
            .collect();

        Ok(res)
    }

    pub async fn get_wallet_list(
        &mut self,
        wallet_address: Option<String>,
        chain_code: Option<String>,
    ) -> Result<Vec<crate::response_vo::wallet::WalletInfo>, crate::ServiceError> {
        let tx = &mut self.repo;
        let chains: ChainCodeAndName = tx.get_chain_list().await?.into();
        let token_currencies = self.coin_domain.get_token_currencies_v2(tx).await?;
        // let service = Service::default();
        let wallet_list = if let Some(wallet_address) = &wallet_address {
            let wallet = tx
                .wallet_detail_by_address(wallet_address)
                .await?
                .ok_or(crate::BusinessError::Wallet(crate::WalletError::NotFound))?;
            vec![wallet]
        } else {
            tx.wallet_list().await?
        };
        let mut res = Vec::new();
        for wallet_info in wallet_list {
            let list = tx
                .account_list_by_wallet_address_and_chain_code(
                    Some(&wallet_info.address),
                    chain_code.clone(),
                )
                .await?;
            let mut account_list = token_currencies
                .calculate_account_infos(list, &chains)
                .await?;

            // let mut account_cal_list = std::collections::HashMap::new();

            let mut wallet_assets = BalanceInfo::new_without_amount().await?;
            for account in account_list.iter_mut() {
                let mut account_assets_entity = self
                    .assets_domain
                    .get_account_assets_entity(tx, account.account_id, &wallet_info.address, None)
                    .await?;
                let account_total_assets = token_currencies
                    .calculate_account_total_assets(&mut account_assets_entity)
                    .await?;
                let fiat_value = account_total_assets.fiat_value;
                let amount = account_total_assets.amount;
                account.balance.fiat_add(fiat_value);
                account.balance.amount_add(amount);
                wallet_assets.fiat_add(fiat_value);
                wallet_assets.amount_add(amount);
            }

            res.push(crate::response_vo::wallet::WalletInfo {
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

    pub async fn logic_delete(self, address: &str) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        tx.begin_transaction().await?;
        let wallet = tx.wallet_detail_by_address(address).await?;
        WalletRepoTrait::reset(&mut tx, address).await?;
        AccountRepoTrait::reset(&mut tx, address).await?;
        let latest_wallet = tx.wallet_latest().await?;

        let rest_uids = tx
            .uid_list()
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect::<Vec<String>>();

        let uid = if let Some(latest_wallet) = latest_wallet {
            Some(latest_wallet.uid)
        } else {
            None
        };
        tx.update_uid(uid.as_deref()).await?;
        let device = tx.get_device_info().await?;

        tx.commit_transaction().await?;
        let pool = crate::Context::get_global_sqlite_pool()?;

        if let Some(device) = device
            && let Some(wallet) = wallet
        {
            let members = MultisigMemberDaoV1::list_by_uid(&wallet.uid, &*pool)
                .await
                .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;
            for member in members.0 {
                MultisigDomain::logic_delete_account(&member.account_id, pool.clone()).await?;
            }

            let req = DeviceDeleteReq::new(&device.sn, &rest_uids);

            let task = domain::task_queue::Task::BackendApi(
                domain::task_queue::BackendApiTask::BackendApi(
                    domain::task_queue::BackendApiTaskData::new(endpoint::DEVICE_DELETE, &req)?,
                ),
            );
            Tasks::new().push(task).send().await?;
        };

        Ok(())
    }

    pub async fn physical_delete(self, address: &str) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        tx.begin_transaction().await?;
        let wallet = tx.wallet_detail_by_address(address).await?;
        WalletRepoTrait::physical_delete(&mut tx, &[address]).await?;
        let accounts = AccountRepoTrait::physical_delete_all(&mut tx, &[address]).await?;
        let Some(device) = tx.get_device_info().await? else {
            return Err(crate::BusinessError::Device(crate::DeviceError::Uninitialized).into());
        };
        let dirs = crate::manager::Context::get_global_dirs()?;
        let wallet_dir = dirs.get_wallet_dir(Some(address));
        wallet_utils::file_func::remove_dir_all(wallet_dir)?;

        let latest_wallet = tx.wallet_latest().await?;

        let rest_uids = tx
            .uid_list()
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect::<Vec<String>>();

        let uid = if let Some(latest_wallet) = latest_wallet {
            Some(latest_wallet.uid)
        } else {
            None
        };
        tx.update_uid(uid.as_deref()).await?;
        tx.commit_transaction().await?;
        let pool = crate::Context::get_global_sqlite_pool()?;

        if let Some(wallet) = wallet {
            let req = DeviceDeleteReq::new(&device.sn, &rest_uids);
            let device_delete_task = domain::task_queue::Task::BackendApi(
                domain::task_queue::BackendApiTask::BackendApi(
                    domain::task_queue::BackendApiTaskData::new(endpoint::DEVICE_DELETE, &req)?,
                ),
            );

            let members = MultisigMemberDaoV1::list_by_uid(&wallet.uid, &*pool)
                .await
                .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;

            let multisig_accounts =
                MultisigDomain::physical_delete_wallet_account(members, &wallet.uid, pool.clone())
                    .await?;

            let device_unbind_address_task = DeviceDomain::gen_device_unbind_all_address_task_data(
                &accounts,
                multisig_accounts,
                &device.sn,
            )
            .await?;

            let device_unbind_address_task = domain::task_queue::Task::BackendApi(
                domain::task_queue::BackendApiTask::BackendApi(device_unbind_address_task),
            );
            Tasks::new()
                .push(device_delete_task)
                .push(device_unbind_address_task)
                .send()
                .await?;
        };
        // let accounts = tx.get_account_list_by_wallet_address(Some(address)).await?;
        tokio::spawn(async move {
            for uid in rest_uids.iter() {
                if let Err(e) = MultisigDomain::recover_uid_multisig_data(uid).await {
                    tracing::error!("recover multisig account data error: {}", e);
                };

                if let Err(e) = MultisigQueueDomain::recover_all_queue_data(uid).await {
                    tracing::error!("recover multisig queue data error: {}", e);
                }
            }
        });
        Ok(())
    }

    pub async fn logic_reset(self) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        tx.begin_transaction().await?;
        let device = tx.get_device_info().await?;

        WalletRepoTrait::reset_all_wallet(&mut tx).await?;
        AccountRepoTrait::reset_all_account(&mut tx).await?;

        let dirs = crate::manager::Context::get_global_dirs()?;
        let wallet_dir = dirs.get_wallet_dir(None);
        wallet_utils::file_func::remove_dir_all(wallet_dir)?;
        tx.commit_transaction().await?;

        if let Some(device) = &device {
            let req = DeviceDeleteReq::new(&device.sn, &[]);

            let task = domain::task_queue::Task::BackendApi(
                domain::task_queue::BackendApiTask::BackendApi(
                    domain::task_queue::BackendApiTaskData::new(endpoint::DEVICE_DELETE, &req)?,
                ),
            );
            Tasks::new().push(task).send().await?;
        };

        Ok(())
    }

    pub async fn physical_reset(self) -> Result<(), crate::ServiceError> {
        let pool = crate::Context::get_global_sqlite_pool()?;
        let mut tx = self.repo;
        let Some(device) = tx.get_device_info().await? else {
            return Err(crate::BusinessError::Device(crate::DeviceError::Uninitialized).into());
        };

        tx.update_password(None).await?;

        WalletRepoTrait::physical_delete_all(&mut tx).await?;
        let accounts = AccountRepoTrait::physical_delete_all(&mut tx, &[]).await?;

        let req = DeviceDeleteReq::new(&device.sn, &[]);
        let device_delete_task =
            domain::task_queue::BackendApiTaskData::new(endpoint::DEVICE_DELETE, &req)?;
        let multisig_accounts = MultisigDomain::physical_delete_all_account(pool).await?;

        let device_unbind_address_task = DeviceDomain::gen_device_unbind_all_address_task_data(
            &accounts,
            multisig_accounts,
            &device.sn,
        )
        .await?;

        Tasks::new()
            .push(domain::task_queue::Task::BackendApi(
                domain::task_queue::BackendApiTask::BackendApi(device_delete_task),
            ))
            .push(domain::task_queue::Task::BackendApi(
                domain::task_queue::BackendApiTask::BackendApi(device_unbind_address_task),
            ))
            .send()
            .await?;

        let dirs = crate::manager::Context::get_global_dirs()?;
        let wallet_dir = dirs.get_wallet_dir(None);
        wallet_utils::file_func::remove_dir_all(wallet_dir)?;
        Ok(())
    }

    pub async fn recover_multisig_data(
        self,
        wallet_address: &str,
    ) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        let wallet = WalletRepoTrait::detail(&mut tx, wallet_address)
            .await?
            .ok_or(crate::ServiceError::Business(crate::BusinessError::Wallet(
                crate::WalletError::NotFound,
            )))?;

        MultisigDomain::recover_uid_multisig_data(&wallet.uid).await?;
        MultisigQueueDomain::recover_all_queue_data(&wallet.uid).await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn reset_root(
        &mut self,
        language_code: u8,
        phrase: &str,
        salt: &str,
        _address: &str,
        new_password: &str,
        subkey_password: Option<String>,
    ) -> Result<ResetRootRes, crate::ServiceError> {
        // let service = Service::default();
        let dirs = crate::manager::Context::get_global_dirs()?;
        let wallet_keystore::api::RootInfo {
            private_key,
            seed,
            address,
            phrase,
        } = wallet_keystore::api::KeystoreApi::generate_master_key_info(
            language_code,
            phrase,
            salt,
        )
        .map_err(|e| crate::SystemError::Service(e.to_string()))?;
        let address = address.to_string();

        // Get the path to the root directory for the given wallet name.
        let root_path = dirs.get_root_dir(&address)?;

        // Get the path to the subkeys directory for the given wallet name.
        let subs_path = dirs.get_subs_dir(&address)?;

        // Traverse the directory structure to obtain the current wallet tree.
        let wallet_tree =
            wallet_tree::wallet_tree::WalletTree::traverse_directory_structure(&dirs.wallet_dir)?;

        // Call the reset_root function from the wallet manager handler,
        // passing in the root path, subs path, wallet tree, wallet name,
        // language code, phrase, salt, address, new password, and subkey password.
        let req = crate::request::wallet::ResetRootReq {
            language_code,
            phrase: phrase.to_string(),
            salt: salt.to_string(),
            wallet_address: address.to_string(),
            new_password: new_password.to_string(),
            subkey_password,
        };

        self.wallet_domain
            .reset_root(
                &mut self.repo,
                root_path,
                subs_path,
                wallet_tree,
                private_key,
                seed,
                req,
            )
            .await?;

        // Traverse the directory structure again to update the wallet tree after resetting the root key.
        let wallet_tree =
            wallet_tree::wallet_tree::WalletTree::traverse_directory_structure(&dirs.wallet_dir)?;

        // Return the updated wallet tree as part of the ResetRootRes response.
        Ok(ResetRootRes { wallet_tree })
    }
}

#[cfg(test)]
mod tests {

    fn _uid(phrase: &str, salt: &str) -> String {
        let uid = format!("{phrase}{salt}");
        wallet_utils::md5(&uid)
    }

    fn uid_pbkdf2(phrase: &str, salt: &str) -> String {
        let uid = format!("{phrase}{salt}");
        wallet_utils::pbkdf2_string(&uid, salt, 100000, 32).unwrap()
    }

    #[tokio::test]
    async fn test_reset_root() {
        // let phrase =
        //     "chuckle practice chicken permit swarm giant improve absurd melt kitchen oppose scrub";
        // let phrase = "arrest hover fury mercy slim answer hospital area morning student riot deal";
        // let phrase = "spoil first width hat submit inflict impact quantum love funny warrior spike";
        // let phrase = "fetch bronze forward wish only gentle picture noise vocal essay devote steel";

        let phrase =
            "will match face problem tongue fortune rebuild stool moon assist virtual lounge";
        // let phrase =
        //     "drum planet ugly present absorb chair simple shiver honey object captain unable";
        // let phrase = "loan tiny planet lucky rigid clip coil recall praise obvious debris dilemma";
        // let phrase = "divorce word join around degree mother quiz math just custom lunar angle";
        // let phrase = "nose bird celery bread slice hero black session tonight winner pitch foot";
        // let phrase = "fan swamp loop mesh enact tennis priority artefact canal hour skull joy";

        let salt = "12345678";
        // let salt = "1234qwer";
        let uid_md5 = _uid(phrase, salt);
        let uid = uid_pbkdf2(phrase, salt);

        println!("uid_md5: {}", uid_md5);
        println!("uid: {}", uid);
    }
}
