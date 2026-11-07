use wallet_database::{
    entities::api_wallet::ApiWalletType,
    repositories::{
        api_wallet::{
            account::ApiAccountRepo, address_query_state::AddressQueryStateRepo,
            chain::ApiChainRepo, wallet::ApiWalletRepo,
        },
        device::DeviceRepo,
        wallet::WalletRepo,
    },
};
use wallet_ecdh::GLOBAL_KEY;
use wallet_transport_backend::{
    request::{
        DeviceDeleteReq, LanguageInitReq,
        api_wallet::{address::AddressListReq, swap::ApiInitSwapReq},
    },
    response_vo::api_wallet::wallet::{
        QueryUidBindInfoRes, QueryWalletActivationInfoResp, UidStatus,
    },
};
use wallet_tree::api::KeystoreApi;

mod create_timing;

use crate::{
    api::ReturnType,
    context::Context,
    domain::{
        api_wallet::{
            account::ApiAccountDomain,
            adapter_factory::ApiChainAdapterFactory,
            chain::ApiChainDomain,
            wallet::{ApiWalletDomain, ApiWalletImportStage},
        },
        app::{DeviceDomain, mqtt::MqttDomain},
        wallet::WalletDomain,
    },
    error::{
        business::{BusinessError, api_wallet::ApiWalletError},
        service::ServiceError,
    },
    infrastructure::task_queue::{
        backend::{BackendApiTask, BackendApiTaskData},
        initialization::InitializationTask,
        task::Tasks,
    },
    response_vo::api_wallet::wallet::ApiWalletList,
};

/// RAII guard: pauses recovery Workers on creation, resumes on drop.
struct RecoveryPauseGuard {
    ctx: &'static Context,
}
impl RecoveryPauseGuard {
    fn new(ctx: &'static Context) -> Self {
        ctx.set_recovery_paused(true);
        Self { ctx }
    }
}
impl Drop for RecoveryPauseGuard {
    fn drop(&mut self) {
        self.ctx.set_recovery_paused(false);
    }
}

static WALLET_DELETE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub struct ApiWalletService {
    ctx: &'static Context,
}

struct WalletBindingRelation {
    wallet_address: String,
    binding_address: String,
    remove_binding_wallet: bool,
}

struct ReimportBindingPlan {
    binding_address: Option<String>,
    stale_relation: Option<WalletBindingRelation>,
}

impl ReimportBindingPlan {
    fn from_request(binding_address: Option<&str>) -> Self {
        Self { binding_address: binding_address.map(str::to_owned), stale_relation: None }
    }
}

impl ApiWalletService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    async fn plan_subaccount_reimport_binding(
        pool: &wallet_database::ApiWalletDbPool,
        subaccount_address: &str,
        current_sn: &str,
        requested_binding_address: Option<&str>,
    ) -> Result<ReimportBindingPlan, ServiceError> {
        let binding_address = match requested_binding_address {
            Some(address) => Some(address.to_owned()),
            None => ApiWalletRepo::find_by_address(pool, subaccount_address)
                .await?
                .and_then(|wallet| wallet.binding_address),
        };

        let Some(withdrawal_address) = binding_address else {
            return Ok(ReimportBindingPlan::from_request(None));
        };
        let withdrawal_wallet = ApiWalletRepo::find_by_address(pool, &withdrawal_address).await?;
        let relation_is_current = withdrawal_wallet.as_ref().is_some_and(|wallet| {
            wallet.sn.as_deref() == Some(current_sn)
                && wallet.binding_address.as_deref() == Some(subaccount_address)
        });

        if relation_is_current {
            return Ok(ReimportBindingPlan {
                binding_address: Some(withdrawal_address),
                stale_relation: None,
            });
        }

        Ok(ReimportBindingPlan {
            binding_address: None,
            stale_relation: Some(WalletBindingRelation {
                wallet_address: subaccount_address.to_owned(),
                binding_address: withdrawal_address,
                remove_binding_wallet: true,
            }),
        })
    }

    fn stale_withdrawal_reimport_relation(
        recharge_wallet: &wallet_database::entities::api_wallet::ApiWalletEntity,
        withdrawal_address: &str,
        current_sn: &str,
    ) -> Option<WalletBindingRelation> {
        let relation_is_current = recharge_wallet.sn.as_deref() == Some(current_sn)
            && recharge_wallet.binding_address.as_deref() == Some(withdrawal_address);
        (!relation_is_current).then(|| WalletBindingRelation {
            wallet_address: recharge_wallet.address.clone(),
            binding_address: recharge_wallet
                .binding_address
                .clone()
                .unwrap_or_else(|| withdrawal_address.to_owned()),
            remove_binding_wallet: false,
        })
    }

    pub async fn init_api_swap(&self) -> ReturnType<()> {
        if self.ctx.is_init_api_swap().await {
            tracing::warn!("init_api_swap already initialized, skip re-init");
            return Ok(());
        }

        let backend = self.ctx.get_api_wallet_backend();
        let req = ApiInitSwapReq {
            sn: self.ctx.get_sn().to_string(),
            client_pub_key: GLOBAL_KEY.secret_pub_key(),
        };
        let res = backend.init_swap(&req).await?;
        if let Some(data) = res.data {
            GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
        }

        // 初始化API钱包MQTT
        #[cfg(feature = "api-mqtt")]
        MqttDomain::init_api_mqtt(self.ctx).await?;
        self.ctx.set_init_api_swap(true).await;

        tracing::info!(
            "init api swap successful=================================================="
        );

        let ctx = self.ctx;
        tokio::spawn(async move {
            if let Err(e) = Self::init_data(ctx).await {
                tracing::error!("初始化数据失败: {:?}", e);
            }
        });

        Ok(())
    }

    async fn init_data(ctx: &'static Context) -> ReturnType<()> {
        // 初始化API_CHAIN_ADAPTER_FACTORY全局单例
        tracing::info!("初始化API_CHAIN_ADAPTER_FACTORY全局单例");
        let factory = ApiChainAdapterFactory::get_instance();
        // 预初始化所有链和节点的适配器
        tracing::info!("预初始化所有链和节点的适配器");
        factory.pre_init_all_adapters_with_ctx(ctx).await?;

        ApiChainDomain::init_api_chain_info(ctx).await?;
        Tasks::new().push(InitializationTask::PullApiWalletCoins).send_with_ctx(ctx).await?;
        Ok(())
    }

    pub async fn get_api_wallet_list(&self) -> ReturnType<ApiWalletList> {
        ApiWalletDomain::new(self.ctx).get_api_wallet_list().await
    }

    pub async fn create_wallet(
        self,
        language_code: u8,
        phrase: &str,
        salt: &str,
        wallet_name: &str,
        // account_name: &str,
        // is_default_name: bool,
        wallet_password: &str,
        invite_code: Option<String>,
        api_wallet_type: ApiWalletType,
        binding_address: Option<&str>,
    ) -> Result<String, ServiceError> {
        // The worker owns both inputs and credential guards. Dropping the caller
        // must not release a guard while SQLite/native work can still finish.
        let ctx = self.ctx;
        let phrase = zeroize::Zeroizing::new(phrase.to_owned());
        let salt = zeroize::Zeroizing::new(salt.to_owned());
        let wallet_name = wallet_name.to_owned();
        let wallet_password = zeroize::Zeroizing::new(wallet_password.to_owned());
        let binding_address = binding_address.map(str::to_owned);
        tokio::spawn(async move {
            Self::new(ctx)
                .create_wallet_inner(
                    language_code,
                    &phrase,
                    &salt,
                    &wallet_name,
                    &wallet_password,
                    invite_code,
                    api_wallet_type,
                    binding_address.as_deref(),
                )
                .await
        })
        .await
        .map_err(|_| {
            crate::error::system::SystemError::Internal("wallet creation worker failed".to_owned())
        })?
    }

    async fn create_wallet_inner(
        self,
        language_code: u8,
        phrase: &str,
        salt: &str,
        wallet_name: &str,
        // account_name: &str,
        // is_default_name: bool,
        wallet_password: &str,
        invite_code: Option<String>,
        api_wallet_type: ApiWalletType,
        binding_address: Option<&str>,
    ) -> Result<String, ServiceError> {
        let timing = create_timing::CreateTiming::new(api_wallet_type).with_context(self.ctx);
        let creation = async {
            let start = std::time::Instant::now();
            let credential_prepare_guard = timing
                .run("credential_prepare_wait", async {
                    Ok::<_, ServiceError>(self.ctx.api_wallet_credential_gate.lock().await)
                })
                .await?;

            let password_validation_start = std::time::Instant::now();
            let authenticated_proof = timing
                .run(
                    "validate_password",
                    WalletDomain::validate_password_with_proof_status(self.ctx, wallet_password),
                )
                .await?;

            tracing::debug!("Password validation took: {:?}", password_validation_start.elapsed());
            let pool = self.ctx.api_wallet_pool()?;
            let core_pool = self.ctx.core_pool()?;

            let sn = self.ctx.get_sn();
            // Protection preparation may upgrade the KDF after authenticating.
            // Compare that exact proof snapshot with the final policy, not an
            // arbitrary proof read after validation (e.g. during password rotation).
            let reuse_proof = match authenticated_proof {
                Some(ref proof) => {
                    WalletDomain::password_proof_matches_current_policy(self.ctx, proof).await?
                }
                None => false,
            };
            if !reuse_proof {
                let password_proof = timing
                    .run(
                        "generate_password_proof",
                        WalletDomain::generate_password_proof(self.ctx, wallet_password),
                    )
                    .await?;
                timing
                    .run(
                        "db_device_proof",
                        DeviceRepo::update_password_proof(
                            core_pool.clone(),
                            sn,
                            Some(&password_proof),
                        ),
                    )
                    .await?;
            }
            let Some(device) = timing
                .run("db_device_lookup", DeviceRepo::get_device_info(core_pool.clone(), sn))
                .await?
            else {
                return Err(crate::error::business::BusinessError::Device(
                    crate::error::business::device::DeviceError::Uninitialized,
                )
                .into());
            };

            // No credential mutation is allowed during authentication/proof repair.
            // Release for the read-only UID query so rotation may overtake this request.
            drop(credential_prepare_guard);
            let master_key_start = std::time::Instant::now();
            let wallet_tree::api::RootInfo { private_key: _, seed, address, phrase } = timing
                .run("generate_master_key", async {
                    wallet_tree::api::KeystoreApi::generate_master_key_info(
                        language_code,
                        phrase,
                        salt,
                    )
                })
                .await?;
            let address = &address.to_string();

            if timing
                .run(
                    "db_normal_wallet_check",
                    ApiWalletDomain::new(self.ctx).check_normal_wallet_exist(address),
                )
                .await?
            {
                return Err(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::MnemonicAlreadyImportedIntoNormalWalletSystem.into(),
            )
            .into());
            }

            tracing::debug!("Master key generation took: {:?}", master_key_start.elapsed());

            // let uid = wallet_utils::md5(&format!("{phrase}{salt}"));
            let pbkdf2_string_start = std::time::Instant::now();
            let uid = timing
                .run("derive_uid_pbkdf2", async {
                    wallet_utils::pbkdf2_string(&format!("{phrase}{salt}"), salt, 100000, 32)
                })
                .await?;

            // 检查是否是普通钱包
            let status = timing
                .run("http_uid_check", ApiWalletDomain::new(self.ctx).check_keys_uid(&uid))
                .await?;
            if status.is_normal_wallet() {
                return Err(ServiceError::Business(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::MnemonicAlreadyImportedIntoNormalWalletSystem.into(),
            )));
            }

            // Compare the authenticated/initialized proof revision under the same
            // gate used by password rotation, and retain it through all creation writes.
            let _credential_commit_guard = timing
                .run("credential_commit_wait", async {
                    Ok::<_, ServiceError>(self.ctx.api_wallet_credential_gate.lock().await)
                })
                .await?;
            let current_device = timing
                .run("db_credential_recheck", DeviceRepo::get_device_info(core_pool.clone(), sn))
                .await?;
            let Some(current_proof) =
                current_device.as_ref().and_then(|value| value.password_proof.as_deref())
            else {
                return Err(crate::error::business::BusinessError::Wallet(
                    crate::error::business::wallet::WalletError::PasswordIncorrect,
                )
                .into());
            };
            if device.password_proof.as_deref() != Some(current_proof) {
                // Another empty-inventory creation can refresh the same password's
                // random proof. Authenticate that revision rather than reject it by
                // byte inequality alone. Changed passwords still fail before writes.
                timing
                    .run(
                        "credential_reauthenticate",
                        WalletDomain::revalidate_password_proof(current_proof, wallet_password),
                    )
                    .await?;
            }

            tracing::debug!("Pbkdf2 string took: {:?}", pbkdf2_string_start.elapsed());

            tracing::info!(
                "[import_wallet] Wallet type check completed, wallet type: {:?}, uid: {}",
                api_wallet_type,
                uid
            );

            let seed = seed.clone();

            let initialize_root_keystore_start = std::time::Instant::now();

            let (recharge_uid, withdrawal_uid) = match api_wallet_type {
                ApiWalletType::SubAccount => (Some(uid.as_str()), None),
                ApiWalletType::Withdrawal => (None, Some(uid.as_str())),
            };
            timing
                .run(
                    "http_init_wallet",
                    ApiWalletDomain::new(self.ctx).set_api_wallet(
                        &device.sn,
                        recharge_uid,
                        withdrawal_uid,
                    ),
                )
                .await?;

            let old = match api_wallet_type {
                ApiWalletType::SubAccount => None,
                ApiWalletType::Withdrawal => {
                    if let Some(binding_address) = binding_address {
                        let recharge_wallet = timing
                            .run(
                                "db_old_binding_lookup",
                                ApiWalletRepo::find_by_address(&pool, binding_address),
                            )
                            .await?;
                        if let Some(recharge_wallet) = recharge_wallet {
                            if let Some(binding_address) = recharge_wallet.binding_address {
                                Some(binding_address)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            };

            timing
                .run(
                    "wallet_upsert",
                    ApiWalletDomain::new(self.ctx)
                        .with_creation_diagnostics(timing.call_id())
                        .upsert_api_wallet(
                            &uid,
                            wallet_name,
                            address,
                            wallet_password,
                            &phrase,
                            &seed,
                            api_wallet_type,
                            binding_address,
                        ),
                )
                .await?;
            #[cfg(debug_assertions)]
            tracing::info!(
                metric = "api_wallet_creation_context",
                runtime_matches = ?crate::infrastructure::unlock_session::diagnostic_context_matches(self.ctx),
                "creation unlock context check"
            );
            timing
                .run(
                    "unlock_material",
                    crate::infrastructure::unlock_session::upsert_wallet_unlock_material(
                        address,
                        wallet_password,
                    ),
                )
                .await?;

            tracing::info!(
                "[import_wallet] API wallet inserted/updated successfully, uid: {}, wallet_name: {}",
                uid,
                wallet_name
            );

            tracing::debug!(
                "Initialize root keystore took: {:?}",
                initialize_root_keystore_start.elapsed()
            );

            match api_wallet_type {
                ApiWalletType::SubAccount => {
                    // let info = ApiWalletDomain::query_uid_bind_info(&uid).await?;
                    // ApiWalletDomain::new(self.ctx).bind_uid_with_app_id(
                    //     address,
                    //     &info.org_id,
                    //     Some(info.app_id.as_str()),
                    // )
                    // .await?;
                }
                ApiWalletType::Withdrawal => {
                    if let Some(old) = old {
                        timing
                            .run(
                                "db_remove_old_wallet",
                                ApiWalletRepo::physical_delete(&pool, &[&old]),
                            )
                            .await?;
                    }
                }
            }

            let client_id = DeviceDomain::client_id_by_device(&device)?;

            let language_req = timing
                .run("language_config", async {
                    let config = crate::app_state::APP_STATE.read().await;
                    Ok::<_, ServiceError>(LanguageInitReq::new(&client_id, config.language()))
                })
                .await?;

            let language_init_task_data = BackendApiTaskData::new(
                wallet_transport_backend::consts::endpoint::LANGUAGE_INIT,
                &language_req,
            )?;
            timing
                .run(
                    "http_keys_init",
                    ApiWalletDomain::new(self.ctx).keys_init(
                        &uid,
                        &device,
                        wallet_name,
                        invite_code,
                    ),
                )
                .await?;
            timing.run("db_mark_init", ApiWalletRepo::mark_init(&pool, &uid)).await?;
            let domain = ApiWalletDomain::new(self.ctx);

            match api_wallet_type {
                ApiWalletType::SubAccount => {
                    let info =
                        timing.run("http_bind_info", domain.query_uid_bind_info(&uid)).await?;
                    if info.bind_status {
                        timing
                            .run(
                                "db_bind_uid",
                                ApiWalletDomain::new(self.ctx).bind_uid_with_app_id(
                                    address,
                                    &info.org_id,
                                    Some(info.app_id.as_str()),
                                ),
                            )
                            .await?;
                    }
                }
                ApiWalletType::Withdrawal => {
                    if let Some(binding_address) = binding_address {
                        let recharge_wallet = timing
                            .run(
                                "db_binding_lookup",
                                ApiWalletRepo::find_by_address(&pool, binding_address),
                            )
                            .await?;

                        if let Some(recharge_wallet) = recharge_wallet {
                            let info = timing
                                .run(
                                    "http_bind_info",
                                    domain.query_uid_bind_info(&recharge_wallet.uid),
                                )
                                .await?;

                            if info.bind_status {
                                timing
                                    .run(
                                        "http_appid_import",
                                        domain.appid_import(
                                            sn,
                                            Some(&recharge_wallet.uid),
                                            Some(&uid),
                                        ),
                                    )
                                    .await?;
                                timing
                                    .run(
                                        "db_bind_withdrawal_uid",
                                        ApiWalletDomain::new(self.ctx).bind_uid_with_app_id(
                                            address,
                                            &info.org_id,
                                            Some(info.app_id.as_str()),
                                        ),
                                    )
                                    .await?;
                            }

                            timing
                                .run(
                                    "db_save_binding",
                                    ApiWalletDomain::new(self.ctx).db_save_bind_data(
                                        &recharge_wallet.address,
                                        &address,
                                        &info.org_id,
                                        &info.app_id,
                                    ),
                                )
                                .await?;
                            timing
                                .run(
                                    "db_save_binding_sn",
                                    ApiWalletDomain::new(self.ctx).db_save_sn_data(
                                        &recharge_wallet.address,
                                        Some(address),
                                        sn,
                                    ),
                                )
                                .await?;

                            if info.bind_status {
                                let default_chain_list = timing
                                    .run("db_default_chains", ApiChainRepo::get_chain_list(&pool))
                                    .await?;
                                let chains: Vec<String> = default_chain_list
                                    .iter()
                                    .map(|chain| chain.chain_code.clone())
                                    .collect();
                                timing
                                    .run(
                                        "create_default_accounts",
                                        ApiAccountDomain::create_withdrawal_account_with_ctx(
                                            self.ctx, address, chains, "账户", true, false,
                                        ),
                                    )
                                    .await?;
                            }
                        }
                    }
                }
            }

            timing.run("db_update_sn", ApiWalletRepo::update_sn(&pool, &address, sn)).await?;

            timing
                .run(
                    "enqueue_tasks",
                    Tasks::new()
                        // .push(BackendApiTask::BackendApi(keys_init_task_data))
                        .push(BackendApiTask::BackendApi(language_init_task_data))
                        .send_with_ctx(self.ctx),
                )
                .await?;

            crate::domain::api_wallet::wallet_list_policy::invalidate_api_wallet_list();
            if let Ok(Some(entity)) =
                timing.run("db_cache_lookup", ApiWalletRepo::find_by_address(&pool, address)).await
            {
                crate::domain::api_wallet::wallet_list_policy::add_wallets_to_cache(&[entity]);
            }

            tracing::debug!("cose time: {}", start.elapsed().as_millis());
            Ok(uid)
        };
        timing.run("total", creation).await
    }

    pub async fn import_wallet(
        self,
        language_code: u8,
        phrase: &str,
        salt: &str,
        wallet_name: &str,
        // account_name: &str,
        // is_default_name: bool,
        wallet_password: &str,
        invite_code: Option<String>,
        api_wallet_type: ApiWalletType,
        binding_address: Option<&str>,
    ) -> Result<String, crate::error::service::ServiceError> {
        tracing::info!(
            "[import_wallet] Start importing API wallet, type: {:?}, name: {}",
            api_wallet_type,
            wallet_name
        );

        let password_validation_start = std::time::Instant::now();
        WalletDomain::validate_password_with_context(self.ctx, wallet_password).await?;

        tracing::debug!("Password validation took: {:?}", password_validation_start.elapsed());

        let pool = self.ctx.api_wallet_pool()?;
        let core_pool = self.ctx.core_pool()?;
        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(core_pool.clone(), sn).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };

        tracing::info!("[import_wallet] Device info retrieved successfully, sn: {}", device.sn);

        // 检查是否是api钱包，是就恢复，不是就报错
        let master_key_start = std::time::Instant::now();
        let wallet_tree::api::RootInfo { private_key: _, seed, address, phrase } =
            wallet_tree::api::KeystoreApi::generate_master_key_info(language_code, phrase, salt)?;
        let address = &address.to_string();

        tracing::info!(
            "[import_wallet] Master key information generated successfully, wallet address: {}",
            address
        );

        // 1.校验uid，是否本地已有普通钱包
        if ApiWalletDomain::new(self.ctx).check_normal_wallet_exist(address).await? {
            return Err(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::AlreadyImported.into(),
            )
            .into());
        }

        tracing::debug!("Master key generation took: {:?}", master_key_start.elapsed());

        // let uid = wallet_utils::md5(&format!("{phrase}{salt}"));
        let pbkdf2_string_start = std::time::Instant::now();
        let uid = wallet_utils::pbkdf2_string(&format!("{phrase}{salt}"), salt, 100000, 32)?;
        tracing::debug!("Pbkdf2 string took: {:?}", pbkdf2_string_start.elapsed());

        // Acquire foreground priority before backend/type/binding validation so
        // unrelated recovery cannot start another large page during preflight.
        // Any validation error drops this provisional lease immediately.
        let foreground_lease = self.ctx.recovery_coordinator().begin_wallet_import(&uid);
        self.ctx.recovery_coordinator().wait_for_turn(Some(&uid)).await;

        // 检查钱包类型和后端是否一致，不一致就报错
        let status = ApiWalletDomain::new(self.ctx).check_keys_uid(&uid).await?;
        let domain = ApiWalletDomain::new(self.ctx);

        if status.is_not_found() {
            return Err(crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::ApiWallet(
                    crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
                ),
            ));
        }
        match api_wallet_type {
            ApiWalletType::SubAccount => {
                if !status.is_sub_account_wallet() {
                    return Err(crate::error::service::ServiceError::Business(crate::error::business::BusinessError::ApiWallet(
                        crate::error::business::api_wallet::wallet::WalletError::ImportNotSupportedForThisWalletType.into(),
                            )));
                }
            }
            ApiWalletType::Withdrawal => {
                if !status.is_withdrawal_wallet() {
                    return Err(crate::error::service::ServiceError::Business(crate::error::business::BusinessError::ApiWallet(
                        crate::error::business::api_wallet::wallet::WalletError::ImportNotSupportedForThisWalletType.into(),
                            )));
                }

                if let Some(binding_address) = binding_address {
                    let recharge_wallet =
                        ApiWalletRepo::find_by_address(&pool, binding_address).await?;

                    if let Some(recharge_wallet) = recharge_wallet {
                        let info = domain.query_uid_bind_info(&recharge_wallet.uid).await?;
                        if !info.app_id.is_empty()
                            && !domain
                                .appid_uid_usage(&info.app_id, &uid, UidStatus::ApiWaw)
                                .await?
                        {
                            // 该出款钱包未在该appId下使用过
                            return Err(
                                BusinessError::ApiWallet(
                                    ApiWalletError::Wallet(
                                        crate::error::business::api_wallet::wallet::WalletError
                                        ::WithdrawalWalletNotUsed
                                    )
                                ).into()
                            );
                        }
                    }
                }
            }
        }

        // Stop unrelated paged recovery from starting another DB-heavy batch
        // while this valid foreground import is being persisted.
        // Validation succeeded: retain priority for the import's initialization
        // and deferred recovery work.
        foreground_lease.commit();

        let seed = seed.clone();
        let initialize_root_keystore_start = std::time::Instant::now();

        let (recharge_uid, withdrawal_uid) = match api_wallet_type {
            ApiWalletType::SubAccount => (Some(uid.as_str()), None),
            ApiWalletType::Withdrawal => (None, Some(uid.as_str())),
        };

        ApiWalletDomain::new(self.ctx)
            .set_api_wallet(&device.sn, recharge_uid, withdrawal_uid)
            .await?;

        tracing::info!(
            "[import_wallet] API wallet settings completed, wallet type: {:?}, recharge_uid: {:?}, withdrawal_uid: {:?}",
            api_wallet_type,
            recharge_uid,
            withdrawal_uid
        );

        let client_id = DeviceDomain::client_id_by_device(&device)?;

        let language_req = {
            let config = crate::app_state::APP_STATE.read().await;
            LanguageInitReq::new(&client_id, config.language())
        };

        let language_init_task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::LANGUAGE_INIT,
            &language_req,
        )?;

        // 先做所有会失败的前置查询/初始化，避免本地先写入半完成钱包。
        let mut subaccount_bind_info: Option<QueryUidBindInfoRes> = None;
        let mut withdrawal_recharge_address: Option<String> = None;
        let mut withdrawal_bind_info: Option<QueryUidBindInfoRes> = None;
        let mut reimport_binding_plan = ReimportBindingPlan::from_request(binding_address);

        match api_wallet_type {
            ApiWalletType::SubAccount => {
                let info = domain.query_uid_bind_info(&uid).await?;
                tracing::info!(
                    flow = "api_wallet_import_bind_probe",
                    wallet_type = "subaccount",
                    bind_status = info.bind_status,
                    app_id_present = !info.app_id.is_empty(),
                    org_id_present = !info.org_id.is_empty(),
                    "backend bind info received before subaccount persistence"
                );
                if info.bind_status {
                    domain.appid_import_recharge_wallet(sn, &uid).await?;
                }
                subaccount_bind_info = Some(info);
                reimport_binding_plan =
                    Self::plan_subaccount_reimport_binding(&pool, address, sn, binding_address)
                        .await?;
            }
            ApiWalletType::Withdrawal => {
                if let Some(binding_address) = binding_address {
                    let recharge_wallet =
                        ApiWalletRepo::find_by_address(&pool, binding_address).await?;

                    if let Some(recharge_wallet) = recharge_wallet {
                        reimport_binding_plan.stale_relation =
                            Self::stale_withdrawal_reimport_relation(&recharge_wallet, address, sn);

                        let info = domain.query_uid_bind_info(&recharge_wallet.uid).await?;
                        tracing::info!(
                            flow = "api_wallet_import_bind_probe",
                            wallet_type = "withdrawal",
                            bind_status = info.bind_status,
                            app_id_present = !info.app_id.is_empty(),
                            org_id_present = !info.org_id.is_empty(),
                            "backend bind info received before withdrawal persistence"
                        );

                        if !info.app_id.is_empty()
                            && !domain
                                .appid_uid_usage(&info.app_id, &uid, UidStatus::ApiWaw)
                                .await?
                        {
                            return Err(
                                BusinessError::ApiWallet(
                                    ApiWalletError::Wallet(
                                        crate::error::business::api_wallet::wallet::WalletError
                                        ::WithdrawalWalletNotUsed,
                                    ),
                                )
                                .into(),
                            );
                        }

                        if info.bind_status {
                            domain.appid_import(sn, Some(&recharge_wallet.uid), Some(&uid)).await?;
                        }

                        withdrawal_recharge_address = Some(recharge_wallet.address.clone());
                        withdrawal_bind_info = Some(info);
                    }
                }
            }
        }

        ApiWalletDomain::new(self.ctx).keys_init(&uid, &device, wallet_name, invite_code).await?;

        if let Some(relation) = reimport_binding_plan.stale_relation.as_ref() {
            tracing::info!(
                uid = %uid,
                wallet_address = %relation.wallet_address,
                binding_address = %relation.binding_address,
                "clearing stale or incomplete API wallet relation during re-import"
            );
            ApiWalletRepo::clear_binding_relation(
                &pool,
                &relation.wallet_address,
                &relation.binding_address,
            )
            .await?;
            if relation.remove_binding_wallet {
                if let Some(stale_wallet) =
                    ApiWalletRepo::find_by_address(&pool, &relation.binding_address).await?
                {
                    ApiWalletRepo::physical_delete(&pool, &[relation.binding_address.as_str()])
                        .await?;
                    AddressQueryStateRepo::delete_by_uid(&pool, &stale_wallet.uid).await?;
                    ApiAccountRepo::physical_delete_all(
                        &pool,
                        &[relation.binding_address.as_str()],
                    )
                    .await?;
                    crate::domain::api_wallet::wallet_list_policy::remove_wallets_from_cache(&[
                        relation.binding_address.as_str(),
                    ]);
                }
            }
        }

        ApiWalletDomain::new(self.ctx)
            .upsert_api_wallet(
                &uid,
                wallet_name,
                address,
                wallet_password,
                &phrase,
                &seed,
                api_wallet_type,
                reimport_binding_plan.binding_address.as_deref(),
            )
            .await?;
        ApiWalletRepo::mark_init(&pool, &uid).await?;
        if reimport_binding_plan.stale_relation.is_some() {
            crate::domain::api_wallet::wallet_list_policy::invalidate_api_wallet_list();
        }
        crate::infrastructure::unlock_session::upsert_wallet_unlock_material(
            address,
            wallet_password,
        )
        .await?;

        tracing::info!(
            "[import_wallet] API wallet inserted/updated successfully, uid: {}, wallet_name: {}",
            uid,
            wallet_name
        );

        tracing::debug!(
            "Initialize root keystore took: {:?}",
            initialize_root_keystore_start.elapsed()
        );

        ApiWalletRepo::update_import_stage(
            &pool,
            &uid,
            match api_wallet_type {
                ApiWalletType::SubAccount => ApiWalletImportStage::SubaccountCreated.as_u8(),
                ApiWalletType::Withdrawal => ApiWalletImportStage::WithdrawalPending.as_u8(),
            },
        )
        .await?;

        let imported_wallet = ApiWalletRepo::find_by_uid(&pool, &uid).await?;
        match api_wallet_type {
            ApiWalletType::SubAccount => {
                if let Some(info) = subaccount_bind_info {
                    ApiWalletDomain::new(self.ctx)
                        .bind_uid_with_app_id(address, &info.org_id, Some(info.app_id.as_str()))
                        .await?;
                    ApiWalletRepo::update_import_stage(
                        &pool,
                        &uid,
                        ApiWalletImportStage::Completed.as_u8(),
                    )
                    .await?;
                }
            }
            ApiWalletType::Withdrawal => {
                let already_completed = imported_wallet
                    .as_ref()
                    .map(|wallet| wallet.import_stage >= ApiWalletImportStage::Completed.as_u8())
                    .unwrap_or(false);
                if let (Some(recharge_address), Some(info)) =
                    (withdrawal_recharge_address.as_ref(), withdrawal_bind_info.as_ref())
                {
                    let Some(recharge_wallet) =
                        ApiWalletRepo::find_by_address(&pool, recharge_address).await?
                    else {
                        return Err(
                            BusinessError::ApiWallet(
                                ApiWalletError::Wallet(
                                    crate::error::business::api_wallet::wallet::WalletError::SubAccountWalletNotBoundWithdrawalWalletAddress,
                                ),
                            )
                            .into(),
                        );
                    };

                    if recharge_wallet.import_stage < ApiWalletImportStage::Completed.as_u8() {
                        if info.bind_status {
                            ApiWalletDomain::new(self.ctx)
                                .bind_uid_with_app_id(
                                    &recharge_wallet.address,
                                    &info.org_id,
                                    Some(info.app_id.as_str()),
                                )
                                .await?;
                            ApiWalletRepo::update_import_stage(
                                &pool,
                                &recharge_wallet.uid,
                                ApiWalletImportStage::Completed.as_u8(),
                            )
                            .await?;
                        }

                        let Some(recharge_wallet) =
                            ApiWalletRepo::find_by_address(&pool, recharge_address).await?
                        else {
                            return Err(
                                BusinessError::ApiWallet(
                                    ApiWalletError::Wallet(
                                        crate::error::business::api_wallet::wallet::WalletError::SubAccountWalletNotBoundWithdrawalWalletAddress,
                                    ),
                                )
                                .into(),
                            );
                        };

                        if recharge_wallet.import_stage < ApiWalletImportStage::Completed.as_u8() {
                            return Err(
                                BusinessError::ApiWallet(
                                    ApiWalletError::Wallet(
                                        crate::error::business::api_wallet::wallet::WalletError::SubAccountWalletImportNotCompleted,
                                    ),
                                )
                                .into(),
                            );
                        }
                    }

                    if info.bind_status {
                        ApiWalletDomain::new(self.ctx)
                            .bind_uid_with_app_id(address, &info.org_id, Some(info.app_id.as_str()))
                            .await?;
                    }

                    // Re-import must repair the durable two-way relation even when the wallet's
                    // import stage was already completed before a device takeover.
                    ApiWalletDomain::new(self.ctx)
                        .db_save_bind_data(recharge_address, address, &info.org_id, &info.app_id)
                        .await?;
                    ApiWalletDomain::new(self.ctx)
                        .db_save_sn_data(recharge_address, Some(address), sn)
                        .await?;

                    let persisted_recharge =
                        ApiWalletRepo::find_by_address(&pool, recharge_address).await?;
                    let persisted_withdrawal =
                        ApiWalletRepo::find_by_address(&pool, address).await?;
                    tracing::info!(
                        flow = "api_wallet_import_bind_probe",
                        recharge_app_id_present = persisted_recharge
                            .as_ref()
                            .and_then(|wallet| wallet.app_id.as_deref())
                            .is_some_and(|app_id| !app_id.is_empty()),
                        withdrawal_app_id_present = persisted_withdrawal
                            .as_ref()
                            .and_then(|wallet| wallet.app_id.as_deref())
                            .is_some_and(|app_id| !app_id.is_empty()),
                        relation_is_symmetric = persisted_recharge
                            .as_ref()
                            .and_then(|wallet| wallet.binding_address.as_deref())
                            == Some(address)
                            && persisted_withdrawal
                                .as_ref()
                                .and_then(|wallet| wallet.binding_address.as_deref())
                                == Some(recharge_address.as_str()),
                        "paired wallet state persisted after withdrawal import"
                    );

                    if !already_completed {
                        let default_chain_list = ApiChainRepo::get_chain_list(&pool).await?;
                        let chains: Vec<String> = default_chain_list
                            .iter()
                            .map(|chain| chain.chain_code.clone())
                            .collect();
                        ApiAccountDomain::create_withdrawal_account_with_ctx(
                            self.ctx, address, chains, "账户", true, false,
                        )
                        .await?;
                        ApiWalletRepo::update_import_stage(
                            &pool,
                            &uid,
                            ApiWalletImportStage::Completed.as_u8(),
                        )
                        .await?;
                    }
                }
            }
        }

        ApiWalletRepo::update_sn(&pool, &address, sn).await?;

        let mut tasks = Tasks::new();
        let default_chain_list = ApiChainRepo::get_chain_list(&pool).await?;
        let chains: Vec<String> =
            default_chain_list.iter().map(|chain| chain.chain_code.clone()).collect();

        let wallet = ApiWalletRepo::find_by_uid(&pool, &uid).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;

        if wallet.app_id.is_some() {
            for chain_code in chains {
                let query_address_list_req = AddressListReq::new(&uid, &chain_code, 0, 1000);

                let query_address_list_task_data = BackendApiTaskData::new(
                    wallet_transport_backend::consts::endpoint::api_wallet::QUERY_ADDRESS_LIST,
                    &query_address_list_req,
                )?;
                tasks = tasks.push(BackendApiTask::BackendApi(query_address_list_task_data));
            }
        }

        tracing::info!("[import_wallet] Sending backend tasks");

        tasks
            // .push(BackendApiTask::BackendApi(keys_init_task_data))
            .push(BackendApiTask::BackendApi(language_init_task_data))
            .send_with_ctx(self.ctx)
            .await?;

        tracing::info!("[import_wallet] Backend tasks sent successfully");
        tracing::info!("[import_wallet] Wallet import completed successfully, uid: {}", uid);

        crate::domain::api_wallet::wallet_list_policy::invalidate_api_wallet_list();
        if let Ok(Some(entity)) = ApiWalletRepo::find_by_uid(&pool, &uid).await {
            let paired_wallet = match entity.binding_address.as_deref() {
                Some(binding_address) => {
                    ApiWalletRepo::find_by_address(&pool, binding_address).await.ok().flatten()
                }
                None => None,
            };
            let mut refreshed_wallets = Vec::with_capacity(2);
            if let Some(paired_wallet) = paired_wallet {
                refreshed_wallets.push(paired_wallet);
            }
            refreshed_wallets.push(entity);
            tracing::info!(
                flow = "api_wallet_import_bind_probe",
                refreshed_wallet_count = refreshed_wallets.len(),
                refreshed_app_id_count = refreshed_wallets
                    .iter()
                    .filter(|wallet| wallet
                        .app_id
                        .as_deref()
                        .is_some_and(|app_id| !app_id.is_empty()))
                    .count(),
                "refreshing imported wallet list cache from persisted entities"
            );
            crate::domain::api_wallet::wallet_list_policy::add_wallets_to_cache(&refreshed_wallets);
        }

        Ok(uid)
    }

    pub async fn scan_bind(
        self,
        app_id: &str,
        org_id: &str,
        recharge_uid: &str,
        withdrawal_uid: &str,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let sn = self.ctx.get_sn();

        let recharge_wallet = ApiWalletRepo::find_by_uid(&pool, recharge_uid).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;
        let withdrawal_wallet = ApiWalletRepo::find_by_uid(&pool, withdrawal_uid).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;

        ApiWalletDomain::new(self.ctx).scan_bind(recharge_uid, withdrawal_uid, app_id, sn).await?;
        ApiWalletDomain::new(self.ctx)
            .db_save_bind_data(&recharge_wallet.address, &withdrawal_wallet.address, org_id, app_id)
            .await?;
        ApiWalletDomain::new(self.ctx)
            .db_save_sn_data(&recharge_wallet.address, Some(&withdrawal_wallet.address), &sn)
            .await?;

        tracing::info!(sn=%sn, "sn ------------ ==============================");
        let default_chain_list = ApiChainRepo::get_chain_list(&pool).await?;

        let chains: Vec<String> =
            default_chain_list.iter().map(|chain| chain.chain_code.clone()).collect();
        if chains.is_empty() {
            tracing::warn!("scan_bind is empty");
        }
        ApiAccountDomain::create_withdrawal_account_with_ctx(
            self.ctx,
            &withdrawal_wallet.address,
            chains,
            "账户",
            true,
            false,
        )
        .await?;

        crate::domain::api_wallet::wallet_list_policy::invalidate_api_wallet_list();

        tracing::info!("bind merchant success");
        Ok(())
    }

    pub async fn import_bind(
        self,
        sn: &str,
        org_id: &str,
        app_id: &str,
        recharge_uid: &str,
        withdrawal_uid: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;

        let recharge_wallet = ApiWalletRepo::find_by_uid(&pool, recharge_uid).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;
        let withdrawal_wallet = ApiWalletRepo::find_by_uid(&pool, withdrawal_uid).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;
        ApiWalletDomain::new(self.ctx)
            .appid_import(sn, Some(recharge_uid), Some(withdrawal_uid))
            .await?;

        ApiWalletDomain::new(self.ctx)
            .db_save_bind_data(&recharge_wallet.address, &withdrawal_wallet.address, org_id, app_id)
            .await?;

        ApiWalletDomain::new(self.ctx)
            .db_save_sn_data(&recharge_wallet.address, Some(&withdrawal_wallet.address), sn)
            .await?;

        crate::domain::api_wallet::wallet_list_policy::invalidate_api_wallet_list();
        crate::domain::api_wallet::wallet_list_policy::merge_wallets_in_cache(
            &recharge_wallet.address,
            &withdrawal_wallet.address,
        );

        // let default_chain_list = ApiChainRepo::get_chain_list(&pool).await?;

        // let chains: Vec<String> =
        //     default_chain_list.iter().map(|chain| chain.chain_code.clone()).collect();
        // ApiAccountDomain::create_withdrawal_account(
        //     &withdrawal_wallet.address,
        //     chains,
        //     "账户",
        //     true,
        //     false,
        // )
        // .await?;
        Ok(())
    }

    pub async fn unbind_merchant(
        self,
        recharge_uid: &str,
        withdrawal_uid: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        ApiWalletDomain::new(self.ctx).unbind_uid(recharge_uid).await?;
        ApiWalletDomain::new(self.ctx).unbind_uid(withdrawal_uid).await?;

        todo!();
        Ok(())
    }

    pub async fn edit_wallet_name(
        self,
        address: &str,
        name: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        ApiWalletRepo::edit_name(&pool, address, name).await?;
        crate::domain::api_wallet::wallet_list_policy::rename_wallet_in_cache(address, name);
        Ok(())
    }

    pub async fn set_passwd_cache(
        self,
        wallet_password: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        WalletDomain::validate_password_with_context(self.ctx, wallet_password).await?;
        ApiWalletDomain::new(self.ctx).initialize_wallet_unlock_session(wallet_password).await?;
        crate::infrastructure::system_ready::mark_system_ready();

        Ok(())
    }

    pub async fn query_wallet_activation_info(
        self,
        wallet_address: &str,
    ) -> Result<QueryWalletActivationInfoResp, crate::error::service::ServiceError> {
        ApiWalletDomain::new(self.ctx).query_wallet_activation_info(wallet_address).await
    }

    pub async fn get_phrase(
        &mut self,
        wallet_address: &str,
        password: &str,
    ) -> Result<
        crate::response_vo::standard_wallet::wallet::GetPhraseRes,
        crate::error::service::ServiceError,
    > {
        let pool = self.ctx.api_wallet_pool()?;
        let api_wallet = ApiWalletRepo::find_by_address(&pool, wallet_address).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;

        let phrase = ApiWalletDomain::decrypt_phrase(password, &api_wallet.phrase).await?;

        Ok(crate::response_vo::standard_wallet::wallet::GetPhraseRes { phrase })
    }

    pub async fn physical_delete(
        self,
        address: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        // Serialize repeated deletes so each request sees committed facts.
        let _delete_guard = WALLET_DELETE_LOCK.lock().await;
        let _pause_guard = RecoveryPauseGuard::new(self.ctx);
        let pool = self.ctx.api_wallet_pool()?;
        let core_pool = self.ctx.core_pool()?;
        let mut deletion_source = ApiWalletRepo::deletion_source(&pool).await?;
        let mut deletion_read = deletion_source.begin_read().await?;
        let cleanup_tasks = if let Some(wallet) = deletion_read.find_by_address(address).await? {
            let mut selected = vec![wallet];
            if selected[0].api_wallet_type == ApiWalletType::SubAccount
                && let Some(partner_address) = selected[0].binding_address.as_deref()
                && let Some(partner) = deletion_read.find_by_address(partner_address).await?
            {
                if partner.api_wallet_type == selected[0].api_wallet_type
                    || partner.binding_address.as_deref() != Some(address)
                {
                    return Err(ServiceError::Parameter("inconsistent API wallet binding".into()));
                }
                selected.push(partner);
            }
            let deleted_addresses: Vec<_> =
                selected.iter().map(|wallet| wallet.address.as_str()).collect();
            let deleted_uids: std::collections::HashSet<_> =
                selected.iter().map(|wallet| wallet.uid.as_str()).collect();
            let rest_api_uids = deletion_read
                .uid_list()
                .await?
                .into_iter()
                .map(|(uid,)| uid)
                .filter(|uid| !deleted_uids.contains(uid.as_str()));
            let rest_standard_uids =
                WalletRepo::uid_list(core_pool.clone()).await?.into_iter().map(|(uid,)| uid);
            let rest_uids: Vec<_> = rest_standard_uids.chain(rest_api_uids).collect();
            let sn = self.ctx.get_sn();

            // A failed/unknown backend response must leave the local retry facts intact.
            let stage = std::time::Instant::now();
            self.ctx
                .get_api_wallet_backend()
                .device_delete(&DeviceDeleteReq::new(sn, &rest_uids))
                .await?;
            tracing::info!(target: "wallet_deletion_perf", stage="backend_delete", elapsed_ms=stage.elapsed().as_millis() as u64);
            let (cleanup_tasks, account_snapshot) = super::wallet_deletion::persist_cleanup(
                self.ctx,
                &mut deletion_read,
                &deleted_addresses,
            )
            .await?;
            // End the old WAL view but retain its physical reader/token until
            // BEGIN IMMEDIATE. A changed token triggers full identity validation.
            deletion_read.close().await?;
            let stage = std::time::Instant::now();
            ApiWalletRepo::delete_wallet_data_with_source(
                &pool,
                &selected,
                &account_snapshot,
                deletion_source,
            )
            .await?;
            tracing::info!(target: "wallet_deletion_perf", stage="local_delete", elapsed_ms=stage.elapsed().as_millis() as u64);
            crate::domain::api_wallet::wallet_list_policy::remove_wallets_from_cache(
                &deleted_addresses,
            );

            cleanup_tasks
        } else {
            deletion_read.close().await?;
            drop(deletion_source);
            // Re-enter device/session cleanup if a previous local deletion already committed.
            Vec::new()
        };
        let stage = std::time::Instant::now();
        let sn = self.ctx.get_sn();

        let dirs = self.ctx.get_global_dirs();

        let latest_wallet = ApiWalletRepo::wallet_latest(&pool).await?;

        let rest_api_uids = ApiWalletRepo::uid_list(&pool)
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect::<Vec<String>>();

        let rest_standard_uids = WalletRepo::uid_list(core_pool.clone())
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect::<Vec<String>>();

        // Check if both standard wallets and API wallets are empty before consuming the vectors
        let has_standard_wallets = !rest_standard_uids.is_empty();
        let has_api_wallets = !rest_api_uids.is_empty();

        let uid = if let Some(latest_wallet) = latest_wallet {
            Some(latest_wallet.uid)
        } else {
            // Only remove verify file if both standard wallets and API wallets are deleted
            if !has_standard_wallets && !has_api_wallets {
                KeystoreApi::remove_verify_file(&dirs.root_dir)?;
                DeviceRepo::update_password_proof(core_pool.clone(), sn, None).await?;
                ApiWalletDomain::new(self.ctx).clear_wallet_unlock_session().await?;
            }

            // tx.update_password(None).await?;
            None
        };

        DeviceRepo::update_uid(core_pool, sn, uid.as_deref()).await?;

        tracing::info!(target: "wallet_deletion_perf", stage="finalize_device", elapsed_ms=stage.elapsed().as_millis() as u64);
        let stage = std::time::Instant::now();
        super::wallet_deletion::dispatch_cleanup(self.ctx, cleanup_tasks).await?;
        tracing::info!(target: "wallet_deletion_perf", stage="dispatch", elapsed_ms=stage.elapsed().as_millis() as u64);

        Ok(())
    }

    pub async fn change_withdrawal_wallet(
        &self,
        recharge_uid: &str,
        withdrawal_uid: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        // Do not change the cascade relation while deletion is awaiting its backend.
        let _delete_guard = WALLET_DELETE_LOCK.lock().await;
        ApiWalletDomain::new(self.ctx).change_withdrawal_wallet(recharge_uid, withdrawal_uid).await
    }

    pub async fn query_uid_bind_info(
        &self,
        uid: &str,
    ) -> Result<QueryUidBindInfoRes, crate::error::service::ServiceError> {
        ApiWalletDomain::new(self.ctx).query_uid_bind_info(uid).await
    }

    pub async fn is_wallet_authorized_on_device(
        &self,
        wallet_address: &str,
    ) -> Result<bool, crate::error::service::ServiceError> {
        let sn = self.ctx.get_sn();
        ApiWalletDomain::new(self.ctx).is_wallet_authorized_on_device(wallet_address, sn).await
    }

    //     pub async fn physical_delete(self, address: &str) -> Result<(), crate::ServiceError> {
    //         let mut tx = self.repo;

    //         tx.begin_transaction().await?;
    //         let wallet = tx.wallet_detail_by_address(address).await?;
    //         ApiWalletRepo::delete(&mut tx, &[address]).await?;
    //         let accounts = AccountRepoTrait::physical_delete_all(&mut tx, &[address]).await?;
    //         let Some(device) = tx.get_device_info().await? else {
    //             return Err(crate::BusinessError::Device(crate::DeviceError::Uninitialized).into());
    //         };
    //         let dirs = crate::manager::Context::get_global_dirs()?;
    //         let wallet_dir = dirs.get_wallet_dir(Some(address));
    //         wallet_utils::file_func::remove_dir_all(wallet_dir)?;

    //         let latest_wallet = tx.wallet_latest().await?;

    //         let rest_uids = tx
    //             .uid_list()
    //             .await?
    //             .into_iter()
    //             .map(|uid| uid.0)
    //             .collect::<Vec<String>>();

    //         let uid = if let Some(latest_wallet) = latest_wallet {
    //             Some(latest_wallet.uid)
    //         } else {
    //             KeystoreApi::remove_verify_file(&dirs.root_dir)?;
    //             tx.update_password(None).await?;
    //             None
    //         };
    //         tx.update_uid(uid.as_deref()).await?;
    //         tx.commit_transaction().await?;
    //         let pool = crate::Context::api_wallet_pool()?;

    //         // find tron address and del permission
    //         let tron_address = accounts.iter().find(|a| a.chain_code == chain_code::TRON);
    //         tracing::warn!("tron address = {:?}", tron_address);
    //         if let Some(address) = tron_address {
    //             PermissionDomain::delete_by_address(&pool, &address.address).await?;
    //         }

    //         for uid in rest_uids {
    //             let body = RecoverDataBody::new(&uid);

    //             Tasks::new()
    //                 .push(Task::Common(CommonTask::RecoverMultisigAccountData(body)))
    //                 .send()
    //                 .await?;
    //         }
    //         Ok(())
    //     }
}
