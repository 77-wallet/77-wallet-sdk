use crate::{
    context::{CONTEXT, Context, WalletUnlockMaterial, WalletUnlockSession},
    domain::{
        api_wallet::{
            assets::ApiAssetsDomain,
            unlock::{SeedEnvelopeCodec, WalletUnlockSessionCodec},
        },
        app::{DeviceDomain, config::ConfigDomain},
    },
    error::service::ServiceError,
    infrastructure::{
        expand_address::bootstrap::ExpandBootstrap, phrase_package::PhrasePackageCodec,
        unlock_session,
    },
    messaging::mqtt::topics::api_wallet::cmd::address_allock::{
        AddressAllockType, AwmCmdAddrExpandMsg, EXPAND_INDEX_LOCK,
    },
    response_vo::{
        api_wallet::wallet::{ApiWalletItem, ApiWalletList},
        standard_wallet::app::{ConfigApiWalletItem, ConfigApiWalletList, ConfigWalletInfo},
    },
};
use std::{collections::HashMap, time::Instant};
use wallet_crypto::{
    EncryptedJsonDecryptor as _, EncryptedJsonGenerator as _, KeystoreJsonDecryptor,
    KeystoreJsonGenerator,
};
use wallet_database::{
    entities::{api_wallet::ApiWalletType, device::DeviceEntity},
    repositories::{api_wallet::wallet::ApiWalletRepo, wallet::WalletRepo},
};
use wallet_transport_backend::{
    request::api_wallet::{
        address::ExpandAddressCompleteReq,
        wallet::{AppIdImportRechargeWalletReq, AppIdImportReq, AppIdUidUsageReq, BindAppIdReq},
    },
    response_vo::api_wallet::wallet::{
        KeysUidCheckRes, QueryUidBindInfoRes, QueryWalletActivationInfoResp, UidStatus,
    },
};
use wallet_tree::KdfAlgorithm;

mod persistence_timing;
use persistence_timing::PersistenceTiming;

pub struct ApiWalletDomain {
    ctx: &'static Context,
    persistence_timing: PersistenceTiming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiWalletImportStage {
    Initial = 0,
    SubaccountCreated = 1,
    WithdrawalPending = 2,
    Completed = 3,
}

impl ApiWalletImportStage {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
pub(crate) use super::unlock::{
    SEED_ENVELOPE_NONCE_BYTES, SEED_ENVELOPE_SALT_BYTES, SEED_ENVELOPE_VERSION_V1,
};

impl ApiWalletDomain {
    pub(crate) fn new(ctx: &'static Context) -> Self {
        Self { ctx, persistence_timing: PersistenceTiming::disabled() }
    }

    pub(crate) fn with_creation_diagnostics(mut self, call_id: u64) -> Self {
        self.persistence_timing = PersistenceTiming::new(self.ctx, call_id);
        self
    }

    pub(crate) async fn upsert_api_wallet(
        &self,
        uid: &str,
        wallet_name: &str,
        wallet_address: &str,
        password: &str,
        phrase: &str,
        seed: &[u8],
        api_wallet_type: ApiWalletType,
        binding_address: Option<&str>,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        // let phrase = wallet_utils::serde_func::serde_to_vec(&phrase)?;

        // let rng = rand::thread_rng();
        // let mut generator = KeystoreJsonGenerator::new(rng.clone(), algorithm.clone());
        // let phrase = generator.generate(password.as_bytes(), &phrase)?;
        // let phrase = wallet_utils::serde_func::serde_to_string(&phrase)?;
        // let seed =
        //     KeystoreJsonGenerator::new(rng, algorithm).generate(password.as_bytes(), seed)?;
        // let seed = wallet_utils::serde_func::serde_to_string(&seed)?;

        let phase = &self.persistence_timing;
        let phrase_enc = phase
            .run("encrypt_phrase", PhrasePackageCodec::encrypt_phrase(password, phrase))
            .await?;
        let seed_enc = phase.run("encrypt_seed", Self::encrypt_seed_bundle(password, seed)).await?;

        let sn = self.ctx.get_sn();
        let persisted = phase
            .run("repo_upsert", async {
                let (result, write_timing) = ApiWalletRepo::upsert_with_timing(
                    &pool,
                    &uid,
                    wallet_name,
                    wallet_address,
                    &phrase_enc,
                    &seed_enc,
                    api_wallet_type,
                    binding_address,
                    sn,
                    ApiWalletImportStage::Initial.as_u8(),
                )
                .await;
                phase.write_timing(write_timing);
                result
            })
            .await?;

        #[cfg(debug_assertions)]
        {
            let writer_visible = phase
                .run(
                    "visibility_writer",
                    sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM api_wallet WHERE address=?)",
                    )
                    .bind(wallet_address)
                    .fetch_one(pool.write_ref()),
                )
                .await;
            let reader_visible = phase
                .run(
                    "visibility_reader",
                    sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM api_wallet WHERE address=?)",
                    )
                    .bind(wallet_address)
                    .fetch_one(pool.read_ref()),
                )
                .await;
            tracing::info!(
                metric = "api_wallet_creation_visibility",
                persisted_address_matches = persisted.address == wallet_address,
                persisted_uid_matches = persisted.uid == uid,
                persisted_name_matches = persisted.name == wallet_name,
                writer_visible = ?writer_visible.ok(),
                reader_visible = ?reader_visible.ok(),
                "creation persistence visibility"
            );
        }
        #[cfg(not(debug_assertions))]
        let _ = persisted;
        tracing::info!("upsert api wallet uid: {:?}", uid);
        phase
            .run("binding", async {
                if let Some(binding_address) = binding_address {
                    if api_wallet_type == ApiWalletType::Withdrawal {
                        let recharge_wallet = phase
                            .run(
                                "binding_lookup",
                                ApiWalletRepo::find_by_address(&pool, binding_address),
                            )
                            .await?;

                        if let Some(recharge_wallet) = recharge_wallet {
                            if let Some(address) = recharge_wallet.binding_address {
                                tracing::info!(
                                    "address: {address}, wallet_address: {wallet_address}"
                                );
                                if address != wallet_address {
                                    phase
                                        .run(
                                            "binding_unbind",
                                            ApiWalletRepo::unbind_uid(&pool, &address),
                                        )
                                        .await?;
                                    phase
                                        .run(
                                            "binding_rebind",
                                            self.bind_uid_with_app_id(
                                                &wallet_address,
                                                recharge_wallet
                                                    .merchant_id
                                                    .as_deref()
                                                    .unwrap_or_default(),
                                                recharge_wallet.app_id.as_deref(),
                                                // &recharge_wallet.sn,
                                            ),
                                        )
                                        .await?;
                                }
                            }
                        }
                    }

                    phase
                        .run(
                            "binding_relation",
                            ApiWalletRepo::bind_withdraw_and_subaccount_relation(
                                &pool,
                                binding_address,
                                wallet_address,
                            ),
                        )
                        .await?;
                }

                Ok::<(), ServiceError>(())
            })
            .await
    }

    /// Seed envelopes are stored as opaque blob bytes.
    pub(crate) async fn encrypt_seed_bundle(
        password: &str,
        seed: &[u8],
    ) -> Result<Vec<u8>, ServiceError> {
        SeedEnvelopeCodec::encrypt_seed_bundle(password, seed).await
    }

    pub(crate) async fn encrypt_password_proof(
        algorithm: KdfAlgorithm,
        rng: rand::rngs::OsRng,
        password: &str,
        proof: &str,
    ) -> Result<String, ServiceError> {
        let mut gen1 = KeystoreJsonGenerator::new(rng, algorithm);
        let proof_keystore = gen1.generate(password.as_bytes(), proof.as_bytes())?;
        let proof_enc = wallet_utils::serde_func::serde_to_string(&proof_keystore)?;

        Ok(proof_enc)
    }

    async fn encrypt_phrase_and_seed(
        password: &str,
        phrase: &str,
        seed: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), ServiceError> {
        let phrase_enc = PhrasePackageCodec::encrypt_phrase(password, phrase).await?;
        let seed_enc = Self::encrypt_seed_bundle(password, seed).await?;

        Ok((phrase_enc, seed_enc))
    }

    pub(crate) async fn reset_api_wallet_seed(
        &self,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let wallets = ApiWalletRepo::list(&pool, None).await?;

        for wallet in wallets {
            let current = async {
                Ok::<_, ServiceError>((
                    ApiWalletDomain::decrypt_phrase(old_password, &wallet.phrase).await?,
                    ApiWalletDomain::decrypt_seed(old_password, &wallet.seed).await?,
                ))
            }
            .await;
            let (phrase, seed) = match current {
                Ok(value) => value,
                Err(old_error) => {
                    // A previous set_all_password attempt may already have committed
                    // this row. Verify both fields with the requested new password;
                    // otherwise preserve the original failure.
                    let already_rotated = async {
                        ApiWalletDomain::decrypt_phrase(new_password, &wallet.phrase).await?;
                        ApiWalletDomain::decrypt_seed(new_password, &wallet.seed).await?;
                        Ok::<_, ServiceError>(())
                    }
                    .await
                    .is_ok();
                    if already_rotated {
                        continue;
                    }
                    return Err(old_error);
                }
            };
            let (phrase_enc, seed_enc) =
                Self::encrypt_phrase_and_seed(new_password, &phrase, &seed).await?;
            ApiWalletRepo::update_seed_and_phrase(&pool, &wallet.uid, &phrase_enc, &seed_enc)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn get_seed(&self, wallet_address: &str) -> Result<Vec<u8>, ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let api_wallet =
            ApiWalletRepo::find_by_address(&pool, wallet_address).await?.ok_or_else(|| {
                crate::error::business::BusinessError::ApiWallet(
                    crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
                )
            })?;

        let unlock_material = self
            .ctx
            .wallet_unlock_session_snapshot()
            .await
            .and_then(|session| session.wallet_material(wallet_address).cloned())
            .ok_or(crate::error::system::SystemError::SystemNotReady)?;
        let envelope = SeedEnvelopeCodec::decrypt_seed_envelope_with_smk(
            unlock_material.smk(),
            &api_wallet.seed,
        )
        .await?;
        SeedEnvelopeCodec::decrypt_seed_bundle_with_smk(unlock_material.smk(), &envelope).await
    }

    pub(crate) async fn decrypt_seed(password: &str, seed: &[u8]) -> Result<Vec<u8>, ServiceError> {
        SeedEnvelopeCodec::decrypt_seed_bundle(password, seed).await
    }

    pub(crate) async fn decrypt_phrase(
        password: &str,
        phrase: &[u8],
    ) -> Result<String, ServiceError> {
        PhrasePackageCodec::decrypt_phrase(password, phrase).await
    }

    pub(crate) async fn decrypt_password_proof(
        password: &str,
        proof: &str,
    ) -> Result<String, ServiceError> {
        let data = KeystoreJsonDecryptor.decrypt(password.as_ref(), proof)?;
        let data = wallet_utils::conversion::vec_to_string(&data)?;
        Ok(data)
    }

    /// 检查这个地址是否曾经被创建为普通钱包
    pub(crate) async fn check_normal_wallet_exist(
        &self,
        address: &str,
    ) -> Result<bool, ServiceError> {
        let pool = self.ctx.core_pool()?;

        Ok(WalletRepo::detail(pool.clone(), address).await?.is_some())
    }

    /// 落盘数据：uid绑定数据
    pub(crate) async fn db_save_bind_data(
        &self,
        recharge_address: &str,
        withdrawal_address: &str,
        org_id: &str,
        app_id: &str,
    ) -> Result<(), ServiceError> {
        self.bind_uid_with_app_id(recharge_address, org_id, Some(app_id)).await?;
        self.bind_uid_with_app_id(withdrawal_address, org_id, Some(app_id)).await?;
        self.bind_withdraw_and_subaccount_relation(recharge_address, withdrawal_address).await?;
        self.ctx.app_warning_config_refresh.notify_one();

        Ok(())
    }

    pub(crate) async fn bind_uid_with_app_id(
        &self,
        address: &str,
        merchain_id: &str,
        org_app_id: Option<&str>,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        ApiWalletRepo::update_merchant_id(&pool, &address, merchain_id).await?;
        ApiWalletRepo::update_app_id(&pool, &address, org_app_id).await?;

        Ok(())
    }

    pub(crate) async fn db_save_sn_data(
        &self,
        recharge_address: &str,
        withdrawal_address: Option<&str>,
        sn: &str,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        ApiWalletRepo::update_sn(&pool, &recharge_address, sn).await?;
        if let Some(withdrawal_address) = withdrawal_address {
            ApiWalletRepo::update_sn(&pool, &withdrawal_address, sn).await?;
        }
        Ok(())
    }

    pub(crate) async fn bind_withdraw_and_subaccount_relation(
        &self,
        subaccount_uid: &str,
        withdraw_uid: &str,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;

        ApiWalletRepo::bind_withdraw_and_subaccount_relation(&pool, &subaccount_uid, &withdraw_uid)
            .await?;

        ApiWalletRepo::bind_withdraw_and_subaccount_relation(&pool, &withdraw_uid, &subaccount_uid)
            .await?;
        Ok(())
    }

    pub(crate) async fn unbind_uid(
        &self,
        uid: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let api_wallet = ApiWalletRepo::find_by_uid(&pool, uid).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;
        ApiWalletRepo::unbind_uid(&pool, &api_wallet.address).await?;

        Ok(())
    }

    pub(crate) async fn unbind_uid_by_address(
        &self,
        address: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let api_wallet = ApiWalletRepo::find_by_address(&pool, address).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;
        ApiWalletRepo::unbind_uid(&pool, &api_wallet.address).await?;

        Ok(())
    }

    pub(crate) async fn expand_address(
        &self,
        msg_id: &str,
        address_allock_type: &AddressAllockType,
        index: Option<i32>,
        uid: &str,
        chain_code: &str,
        number: u32,
        serial_no: &str,
        batch_id: &str,
    ) -> Result<(), ServiceError> {
        let _guard = EXPAND_INDEX_LOCK.lock().await;

        let pool = self.ctx.api_wallet_pool()?;
        let backend = self.ctx.get_api_wallet_backend();

        let Some(api_wallet) = ApiWalletRepo::find_by_uid(&pool, &uid).await? else {
            let req = ExpandAddressCompleteReq::new(
                uid,
                batch_id,
                serial_no,
                false,
                Some("api wallet not found"),
            );
            backend.expand_address_complete(req).await?;

            return Ok(());
        };

        let needed_indices = AwmCmdAddrExpandMsg::get_needed_indices(
            self.ctx,
            address_allock_type,
            &api_wallet.uid,
            chain_code,
            batch_id,
            number,
            index,
            Some(msg_id),
        )
        .await?;
        drop(_guard);

        tracing::info!("expand address index: {:?}", needed_indices);
        Ok(())
    }

    pub(crate) async fn get_wallet_unlock_token() -> Result<String, ServiceError> {
        unlock_session::wallet_unlock_token().await
    }

    pub(crate) async fn initialize_wallet_unlock_session(
        &self,
        wallet_password: &str,
    ) -> Result<(), ServiceError> {
        // The session assembly itself lives in unlock.rs; this wrapper only reads wallets
        // and writes the resulting unlock session back.
        let pool = self.ctx.api_wallet_pool()?;
        let wallets = ApiWalletRepo::list(&pool, None).await?;
        let mut wallet_materials = std::collections::HashMap::new();

        for wallet in wallets {
            let envelope =
                SeedEnvelopeCodec::decrypt_seed_envelope(wallet_password, &wallet.seed).await?;

            let smk = WalletUnlockSessionCodec::derive_smk(wallet_password, &envelope.salt).await?;
            wallet_materials
                .insert(wallet.address.clone(), WalletUnlockMaterial::new(smk.to_vec()));
        }

        let unlock_session = WalletUnlockSession::new(
            WalletUnlockSessionCodec::generate_unlock_token(),
            Instant::now() + WalletUnlockSessionCodec::unlock_session_rotation_interval(),
            wallet_materials,
        );
        tracing::info!("wallet unlock session initialized");
        self.ctx.set_wallet_unlock_session(unlock_session).await?;
        crate::domain::api_wallet::chain::ApiChainDomain::start_missing_chain_recovery(self.ctx)
            .await;
        ExpandBootstrap::start_after_first_wallet_unlock(self.ctx).await?;
        Ok(())
    }

    pub(crate) async fn clear_wallet_unlock_session(&self) -> Result<(), ServiceError> {
        self.ctx.clear_wallet_unlock_session().await?;
        Ok(())
    }

    pub(crate) async fn rotate_wallet_session_key(&self) -> Result<(), ServiceError> {
        // Rotation rewraps the seed envelope using the current unlock material and refreshes
        // the wallet-level unlock session without touching the plaintext password again.
        let pool = self.ctx.api_wallet_pool()?;
        let wallets = ApiWalletRepo::list(&pool, None).await?;
        let session = self.ctx.wallet_unlock_session_snapshot().await;
        let mut wallet_materials =
            session.as_ref().map(|session| session.wallet_materials_snapshot()).unwrap_or_default();

        for wallet in wallets {
            let unlock_material = session
                .as_ref()
                .and_then(|session| session.wallet_material(&wallet.address).cloned())
                .ok_or(crate::error::system::SystemError::SystemNotReady)?;
            let envelope = SeedEnvelopeCodec::decrypt_seed_envelope_with_smk(
                unlock_material.smk(),
                &wallet.seed,
            )
            .await?;
            let seed = match SeedEnvelopeCodec::decrypt_seed_bundle_with_smk(
                unlock_material.smk(),
                &envelope,
            )
            .await
            {
                Ok(seed) => seed,
                Err(err) => {
                    tracing::error!("wallet seed rotation decrypt failed: {:?}", err);
                    return Err(err);
                }
            };
            let next_rotation_counter = envelope.rotation_counter.saturating_add(1);
            // Keep the password-derived salt stable so the next unlock can derive the same
            // wallet material from the stored envelope after a restart.
            let salt = envelope.salt.clone();
            tracing::debug!("wallet seed rotation decrypt ok");
            let rotated_seed = SeedEnvelopeCodec::encrypt_seed_bundle_with_smk(
                unlock_material.smk(),
                &salt,
                &seed,
                next_rotation_counter,
            )
            .await?;

            ApiWalletRepo::update_seed_and_phrase(
                &pool,
                &wallet.uid,
                &wallet.phrase,
                &rotated_seed,
            )
            .await?;
            tracing::debug!("wallet seed rotated");

            wallet_materials.insert(
                wallet.address.clone(),
                WalletUnlockMaterial::new(unlock_material.smk().to_vec()),
            );
        }

        let unlock_session = WalletUnlockSession::new(
            WalletUnlockSessionCodec::generate_unlock_token(),
            Instant::now() + WalletUnlockSessionCodec::unlock_session_rotation_interval(),
            wallet_materials,
        );
        tracing::info!("wallet unlock session rotated");
        self.ctx.set_wallet_unlock_session(unlock_session).await?;
        crate::domain::api_wallet::chain::ApiChainDomain::start_missing_chain_recovery(self.ctx)
            .await;
        Ok(())
    }

    /// 设置uid为api钱包
    pub(crate) async fn set_api_wallet(
        &self,
        sn: &str,
        recharge_uid: Option<&str>,
        withdrawal_uid: Option<&str>,
    ) -> Result<(), ServiceError> {
        let backend = self.ctx.get_api_wallet_backend();
        let mut req = AppIdImportReq::new(sn);
        if let Some(recharge_uid) = recharge_uid {
            req.set_recharge_uid(recharge_uid);
        }
        if let Some(withdrawal_uid) = withdrawal_uid {
            req.set_withdrawal_uid(withdrawal_uid);
        }
        backend.init_api_wallet(req).await?;
        Ok(())
    }

    pub(crate) async fn keys_init(
        &self,
        uid: &str,
        device: &DeviceEntity,
        wallet_name: &str,
        invite_code: Option<String>,
    ) -> Result<(), ServiceError> {
        let status = ConfigDomain::get_keys_reset_status(self.ctx).await?;
        if let Some(status) = status
            && let Some(false) = status.status
        {
            return Err(crate::error::business::BusinessError::Config(
                crate::error::business::config::ConfigError::KeysNotReset,
            )
            .into());
        }

        let client_id = DeviceDomain::client_id_by_device(&device)?;
        let backend = self.ctx.get_api_wallet_backend();
        let keys_init_req = wallet_transport_backend::request::KeysInitReq::new(
            &uid,
            &device.sn,
            Some(client_id),
            Some(device.device_type.clone()),
            wallet_name,
            invite_code,
        );

        backend.old_keys_init(keys_init_req).await?;
        Ok(())
    }

    pub(crate) async fn check_keys_uid(&self, uid: &str) -> Result<KeysUidCheckRes, ServiceError> {
        let backend = self.ctx.get_api_wallet_backend();
        let uid_check = backend.keys_uid_check(&uid).await?;

        Ok(uid_check)
    }

    pub(crate) async fn change_withdrawal_wallet(
        &self,
        recharge_uid: &str,
        withdrawal_uid: &str,
        // app_id: &str,
    ) -> Result<(), ServiceError> {
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
        let Some(app_id) = recharge_wallet.app_id else {
            return Err(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::SubAccountWalletNotBound
                    .into(),
            )
            .into());
        };
        let backend = self.ctx.get_api_wallet_backend();
        backend.appid_withdrawal_wallet_change(withdrawal_uid, &app_id).await?;
        if let Some(binding_address) = recharge_wallet.binding_address {
            self.unbind_uid_by_address(&binding_address).await?;
        }

        self.db_save_bind_data(
            &recharge_wallet.address,
            &withdrawal_wallet.address,
            recharge_wallet.merchant_id.as_deref().unwrap_or_default(),
            &app_id,
        )
        .await?;
        if let Some(sn) = recharge_wallet.sn {
            ApiWalletRepo::update_sn(&pool, &withdrawal_wallet.address, &sn).await?;
        }

        super::wallet_list_policy::invalidate_api_wallet_list();

        Ok(())
    }

    /// 查询绑定信息
    pub(crate) async fn query_uid_bind_info(
        &self,
        uid: &str,
    ) -> Result<QueryUidBindInfoRes, ServiceError> {
        let backend = self.ctx.get_api_wallet_backend();
        Ok(backend.query_uid_bind_info(uid).await?)
    }

    pub async fn is_wallet_authorized_on_device(
        &self,
        wallet_address: &str,
        sn: &str,
    ) -> Result<bool, ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let wallet = ApiWalletRepo::find_by_address(&pool, wallet_address).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;

        if let Some(_sn) = wallet.sn {
            if _sn == *sn {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 查询钱包在uid下的使用状态
    pub(crate) async fn appid_uid_usage(
        &self,
        org_app_id: &str,
        uid: &str,
        wallet_type: UidStatus,
    ) -> Result<bool, ServiceError> {
        let req = AppIdUidUsageReq::new(org_app_id, uid, wallet_type);
        let backend = self.ctx.get_api_wallet_backend();
        Ok(backend.appid_uid_usage(req).await?.used)
    }

    /// 扫码绑定
    pub(crate) async fn scan_bind(
        &self,
        recharge_uid: &str,
        withdrawal_uid: &str,
        org_app_id: &str,
        sn: &str,
    ) -> Result<(), ServiceError> {
        let backend = self.ctx.get_api_wallet_backend();
        Ok(backend
            .wallet_bind_appid(BindAppIdReq::new(recharge_uid, withdrawal_uid, org_app_id, sn))
            .await?)
    }

    /// 导入钱包
    pub(crate) async fn appid_import(
        &self,
        sn: &str,
        recharge_uid: Option<&str>,
        withdrawal_uid: Option<&str>,
    ) -> Result<(), ServiceError> {
        let backend = self.ctx.get_api_wallet_backend();
        let mut req = AppIdImportReq::new(sn);

        if let Some(recharge_uid) = recharge_uid {
            req.set_recharge_uid(recharge_uid);
        }
        if let Some(withdrawal_uid) = withdrawal_uid {
            req.set_withdrawal_uid(withdrawal_uid);
        }
        backend.appid_import(req).await?;

        Ok(())
    }

    pub(crate) async fn appid_import_recharge_wallet(
        &self,
        sn: &str,
        recharge_uid: &str,
    ) -> Result<(), ServiceError> {
        let backend = self.ctx.get_api_wallet_backend();
        backend
            .appid_import_recharge_wallet(AppIdImportRechargeWalletReq::new(sn, recharge_uid))
            .await?;
        Ok(())
    }

    /// 查询激活信息
    pub async fn query_wallet_activation_info(
        &self,
        wallet_address: &str,
    ) -> Result<QueryWalletActivationInfoResp, crate::error::service::ServiceError> {
        let backend = self.ctx.get_api_wallet_backend();
        let pool = self.ctx.api_wallet_pool()?;
        let api_wallet = ApiWalletRepo::find_by_address(&pool, wallet_address).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ),
        )?;
        Ok(backend.query_wallet_activation_info(&api_wallet.uid).await?)
    }

    fn build_api_wallet_list(
        wallets: &[wallet_database::entities::api_wallet::ApiWalletEntity],
        balance_list: &HashMap<String, crate::response_vo::standard_wallet::account::BalanceInfo>,
        fill_balance: bool,
    ) -> ApiWalletList {
        let mut list = ApiWalletList::new();

        for e in wallets {
            let mut wallet: crate::response_vo::api_wallet::wallet::WalletInfo = e.into();
            if fill_balance {
                if let Some(balance) = balance_list.get(&e.address) {
                    wallet = wallet.with_balance(balance.clone());
                }
            } else {
                wallet = wallet.with_default_balance();
            }

            match e.api_wallet_type {
                ApiWalletType::SubAccount => {
                    if let Some(binding_address) = &e.binding_address
                        && let Some(item) = list.iter_mut().find(|item| {
                            item.withdraw_wallet
                                .as_ref()
                                .map(|w| &w.address == binding_address)
                                .unwrap_or(false)
                        })
                    {
                        item.recharge_wallet = Some(wallet);
                    } else {
                        list.push(ApiWalletItem {
                            recharge_wallet: Some(wallet),
                            withdraw_wallet: None,
                        });
                    }
                }
                ApiWalletType::Withdrawal => {
                    if let Some(binding_address) = &e.binding_address
                        && let Some(item) = list.iter_mut().find(|item| {
                            item.recharge_wallet
                                .as_ref()
                                .map(|r| &r.address == binding_address)
                                .unwrap_or(false)
                        })
                    {
                        item.withdraw_wallet = Some(wallet);
                    } else {
                        list.push(ApiWalletItem {
                            recharge_wallet: None,
                            withdraw_wallet: Some(wallet),
                        });
                    }
                }
            }
        }

        list
    }

    fn build_config_api_wallet_list(
        wallets: &[wallet_database::entities::api_wallet::ApiWalletEntity],
    ) -> ConfigApiWalletList {
        let mut list = ConfigApiWalletList::new();

        for e in wallets {
            let wallet = ConfigWalletInfo::from(e);

            match e.api_wallet_type {
                ApiWalletType::SubAccount => {
                    if let Some(binding_address) = &e.binding_address
                        && let Some(item) = list.iter_mut().find(|item| {
                            item.withdraw_wallet
                                .as_ref()
                                .map(|w| &w.address == binding_address)
                                .unwrap_or(false)
                        })
                    {
                        item.recharge_wallet = Some(wallet);
                    } else {
                        list.push(ConfigApiWalletItem {
                            recharge_wallet: Some(wallet),
                            withdraw_wallet: None,
                        });
                    }
                }
                ApiWalletType::Withdrawal => {
                    if let Some(binding_address) = &e.binding_address
                        && let Some(item) = list.iter_mut().find(|item| {
                            item.recharge_wallet
                                .as_ref()
                                .map(|r| &r.address == binding_address)
                                .unwrap_or(false)
                        })
                    {
                        item.withdraw_wallet = Some(wallet);
                    } else {
                        list.push(ConfigApiWalletItem {
                            recharge_wallet: None,
                            withdraw_wallet: Some(wallet),
                        });
                    }
                }
            }
        }

        list
    }

    pub async fn get_api_wallet_list(
        &self,
    ) -> Result<ApiWalletList, crate::error::service::ServiceError> {
        let load_start = std::time::Instant::now();
        let list = super::wallet_list_policy::load_api_wallet_list(|| async {
            self.get_api_wallet_list_uncached().await
        })
        .await?;
        tracing::info!(
            flow = "api_wallet_list_bind_probe",
            metric = "api_wallet_list_load",
            elapsed_ms = load_start.elapsed().as_millis(),
            wallet_pair_count = list.len(),
            complete_pair_count = list
                .iter()
                .filter(|item| item.recharge_wallet.is_some() && item.withdraw_wallet.is_some())
                .count(),
            recharge_app_id_count = list
                .iter()
                .filter(|item| item
                    .recharge_wallet
                    .as_ref()
                    .and_then(|wallet| wallet.app_id.as_deref())
                    .is_some_and(|app_id| !app_id.is_empty()))
                .count(),
            "API wallet list returned to caller"
        );
        Ok(list)
    }

    async fn get_api_wallet_list_uncached(
        &self,
    ) -> Result<ApiWalletList, crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let wallets = ApiWalletRepo::list(&pool, None).await?;
        let mut balance_list = HashMap::new();

        for wallet in &wallets {
            let balance =
                ApiAssetsDomain::get_api_wallet_assets(self.ctx, Some(&wallet.address), None, None)
                    .await?;
            balance_list.insert(wallet.address.clone(), balance);
        }

        let list = Self::build_api_wallet_list(&wallets, &balance_list, true);
        tracing::info!(
            flow = "api_wallet_list_bind_probe",
            source = "database",
            wallet_row_count = wallets.len(),
            wallet_pair_count = list.len(),
            "API wallet list rebuilt from database"
        );
        Ok(list)
    }

    pub async fn get_api_wallet_list_light(
        &self,
    ) -> Result<ApiWalletList, crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let wallets = ApiWalletRepo::list(&pool, None).await?;
        Ok(Self::build_api_wallet_list(&wallets, &HashMap::new(), false))
    }

    pub async fn get_config_api_wallet_list(
        &self,
    ) -> Result<ConfigApiWalletList, crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let wallets = ApiWalletRepo::list(&pool, None).await?;
        Ok(Self::build_config_api_wallet_list(&wallets))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Once},
        time::{Duration, Instant},
    };

    use crate::{
        context::{WalletUnlockMaterial, WalletUnlockSession},
        response_vo::standard_wallet::account::BalanceInfo,
    };
    use async_trait::async_trait;
    use once_cell::sync::Lazy;
    use serial_test::serial;
    use tempfile::TempDir;
    use tokio::sync::OnceCell;
    use wallet_database::{
        entities::{api_wallet::ApiWalletType, device::CreateDeviceEntity},
        repositories::{api_wallet::wallet::ApiWalletRepo, device::DeviceRepo},
    };
    use wallet_transport_backend::{
        request::{
            DeviceDeleteReq, KeysInitReq,
            api_wallet::{
                address::ExpandAddressCompleteReq,
                swap::{ApiInitSwapReq, ApiInitSwapResponse},
                wallet::{
                    AppIdImportRechargeWalletReq, AppIdImportReq, AppIdUidUsageReq, BindAppIdReq,
                },
            },
        },
        response_vo::api_wallet::wallet::{
            AppIdUidUsageRes, KeysUidCheckRes, QueryUidBindInfoRes, QueryWalletActivationInfoResp,
        },
    };

    use crate::{ApiWalletBackend, dirs::Dirs};

    use super::{
        ApiWalletDomain, SEED_ENVELOPE_NONCE_BYTES, SEED_ENVELOPE_SALT_BYTES,
        SEED_ENVELOPE_VERSION_V1, SeedEnvelopeCodec, WalletUnlockSessionCodec,
    };
    use sqlx::types::chrono::{TimeZone, Utc};
    use tokio::time::sleep;

    const TEST_SN: &str = "seed-cache-test-sn";
    const TEST_DEVICE_TYPE: &str = "ANDROID";
    const TEST_PASSWORD: &str = "q1111111";
    static TEST_TRACING: Once = Once::new();

    fn init_test_tracing() {
        TEST_TRACING.call_once(|| {
            let _ = tracing_subscriber::fmt()
                .with_test_writer()
                .with_max_level(tracing::Level::INFO)
                .try_init();
        });
    }

    fn make_api_wallet(
        id: i64,
        name: &str,
        uid: &str,
        address: &str,
        wallet_type: ApiWalletType,
        binding_address: Option<&str>,
    ) -> wallet_database::entities::api_wallet::ApiWalletEntity {
        wallet_database::entities::api_wallet::ApiWalletEntity {
            id,
            name: name.to_string(),
            uid: uid.to_string(),
            address: address.to_string(),
            phrase: Vec::new(),
            seed: Vec::new(),
            binding_address: binding_address.map(|s| s.to_string()),
            api_wallet_type: wallet_type,
            merchant_id: None,
            app_id: Some("app".to_string()),
            sn: Some("sn".to_string()),
            status: 1,
            is_init: 1,
            import_stage: 0,
            created_at: Utc.timestamp_opt(0, 0).single().unwrap(),
            updated_at: None,
        }
    }

    #[test]
    fn build_api_wallet_list_light_keeps_default_balance() {
        let wallets =
            vec![make_api_wallet(1, "recharge", "uid-1", "0x111", ApiWalletType::SubAccount, None)];

        let list = ApiWalletDomain::build_api_wallet_list(&wallets, &HashMap::new(), false);
        let item = list.0.first().expect("wallet item");
        let balance = &item.recharge_wallet.as_ref().expect("recharge wallet").balance;

        assert_eq!(balance.amount, 0.0);
        assert_eq!(balance.currency, "");
        assert_eq!(balance.fiat_value, None);
    }

    #[test]
    fn build_api_wallet_list_with_balance_fills_balance() {
        let wallets =
            vec![make_api_wallet(1, "recharge", "uid-1", "0x111", ApiWalletType::SubAccount, None)];
        let mut balance_list = HashMap::new();
        balance_list.insert(
            "0x111".to_string(),
            BalanceInfo {
                amount: 12.34,
                currency: "USD".to_string(),
                unit_price: Some(1.0),
                fiat_value: Some(12.34),
            },
        );

        let list = ApiWalletDomain::build_api_wallet_list(&wallets, &balance_list, true);
        let item = list.0.first().expect("wallet item");
        let balance = &item.recharge_wallet.as_ref().expect("recharge wallet").balance;

        assert_eq!(balance.amount, 12.34);
        assert_eq!(balance.currency, "USD");
        assert_eq!(balance.fiat_value, Some(12.34));
    }

    struct SeedCacheTestEnv {
        _tempdir: TempDir,
        wallet_address: String,
        context: &'static crate::context::Context,
    }

    #[derive(Default)]
    struct NoopApiWalletBackend;

    #[async_trait]
    impl ApiWalletBackend for NoopApiWalletBackend {
        async fn wallet_bind_appid(
            &self,
            _: BindAppIdReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }

        async fn init_api_wallet(
            &self,
            _: AppIdImportReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }

        async fn old_keys_init(
            &self,
            _: KeysInitReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }

        async fn appid_import(
            &self,
            _: AppIdImportReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }

        async fn appid_import_recharge_wallet(
            &self,
            _: AppIdImportRechargeWalletReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }

        async fn keys_uid_check(
            &self,
            uid: &str,
        ) -> Result<KeysUidCheckRes, crate::error::service::ServiceError> {
            Ok(KeysUidCheckRes {
                uid: uid.to_string(),
                status:
                    wallet_transport_backend::response_vo::api_wallet::wallet::UidStatus::ApiRaw,
            })
        }

        async fn query_uid_bind_info(
            &self,
            uid: &str,
        ) -> Result<QueryUidBindInfoRes, crate::error::service::ServiceError> {
            Ok(QueryUidBindInfoRes {
                app_id: String::new(),
                org_id: String::new(),
                bind_status: false,
                sn: uid.to_string(),
            })
        }

        async fn query_wallet_activation_info(
            &self,
            _: &str,
        ) -> Result<QueryWalletActivationInfoResp, crate::error::service::ServiceError> {
            Ok(QueryWalletActivationInfoResp(Vec::new()))
        }

        async fn appid_uid_usage(
            &self,
            _: AppIdUidUsageReq,
        ) -> Result<AppIdUidUsageRes, crate::error::service::ServiceError> {
            Ok(AppIdUidUsageRes { used: false })
        }

        async fn expand_address_complete(
            &self,
            _: ExpandAddressCompleteReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }

        async fn appid_withdrawal_wallet_change(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }

        async fn init_swap(
            &self,
            _: &ApiInitSwapReq,
        ) -> Result<ApiInitSwapResponse, crate::error::service::ServiceError> {
            Ok(ApiInitSwapResponse { success: true, code: None, msg: None, data: None })
        }

        async fn device_delete(
            &self,
            _: &DeviceDeleteReq,
        ) -> Result<Option<()>, crate::error::service::ServiceError> {
            Ok(Some(()))
        }
    }

    async fn seed_cache_test_env() -> SeedCacheTestEnv {
        init_test_tracing();
        let config = crate::config::Config::new(
            r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
aggregate_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
"#,
        )
        .expect("parse test config");
        let tempdir = TempDir::new().expect("create tempdir");
        let dirs =
            Dirs::new(tempdir.path().to_str().expect("utf8 temp path")).expect("create dirs");
        let context = Box::leak(Box::new(
            crate::context::Context::new_without_handles_for_test(dirs, config)
                .await
                .expect("create isolated test context"),
        ));
        let pool = context.api_wallet_pool().expect("api pool");
        let wallet_address = "0x00000000000000000000000000000000000000aa".to_string();
        let phrase_enc = crate::infrastructure::phrase_package::PhrasePackageCodec::encrypt_phrase(
            TEST_PASSWORD,
            "seed-cache-phrase",
        )
        .await
        .expect("generate phrase package");
        let seed_enc = ApiWalletDomain::encrypt_seed_bundle(TEST_PASSWORD, b"seed-cache-seed")
            .await
            .expect("generate seed envelope");
        ApiWalletRepo::upsert(
            &pool,
            "seed-cache-wallet-uid",
            "seed-cache-wallet",
            &wallet_address,
            &phrase_enc,
            &seed_enc,
            ApiWalletType::SubAccount,
            None,
            TEST_SN,
            0,
        )
        .await
        .expect("upsert wallet");

        SeedCacheTestEnv { _tempdir: tempdir, wallet_address, context }
    }

    async fn install_test_unlock_session(env: &SeedCacheTestEnv) -> String {
        let pool = env.context.api_wallet_pool().expect("api pool");
        let wallet = ApiWalletRepo::find_by_address(&pool, &env.wallet_address)
            .await
            .expect("load wallet")
            .expect("wallet exists");
        let envelope = SeedEnvelopeCodec::decrypt_seed_envelope(TEST_PASSWORD, &wallet.seed)
            .await
            .expect("decrypt seed envelope");
        let smk = WalletUnlockSessionCodec::derive_smk(TEST_PASSWORD, &envelope.salt)
            .await
            .expect("derive wallet key");
        let token = WalletUnlockSessionCodec::generate_unlock_token();
        let mut materials = HashMap::new();
        materials.insert(env.wallet_address.clone(), WalletUnlockMaterial::new(smk.to_vec()));
        env.context
            .set_wallet_unlock_session(WalletUnlockSession::new(
                token.clone(),
                Instant::now() + WalletUnlockSessionCodec::unlock_session_rotation_interval(),
                materials,
            ))
            .await
            .expect("install test unlock session");
        token
    }

    // Each session fixture owns process-global Context and unlock state.
    fn run_in_isolated_session_process(test_name: &str) -> bool {
        const CHILD: &str = "WALLET_SESSION_TEST_CHILD";
        if std::env::var(CHILD).as_deref() == Ok(test_name) {
            return false;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([test_name, "--exact", "--test-threads=1"])
            .env(CHILD, test_name)
            .output()
            .expect("run isolated session test");
        assert!(
            output.status.success(),
            "isolated session test failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        true
    }

    #[tokio::test]
    #[serial]
    async fn wallet_unlock_session_rotation_replaces_token() {
        if run_in_isolated_session_process(
            "domain::api_wallet::wallet::tests::wallet_unlock_session_rotation_replaces_token",
        ) {
            return;
        }
        init_test_tracing();
        let env = seed_cache_test_env().await;
        let context = env.context;
        let before = install_test_unlock_session(&env).await;
        assert!(!before.is_empty());
        assert_ne!(before, TEST_PASSWORD);

        ApiWalletDomain::new(context)
            .rotate_wallet_session_key()
            .await
            .expect("rotate session key");

        let after = context
            .wallet_unlock_session_snapshot()
            .await
            .expect("rotated session")
            .session_token()
            .to_string();
        assert!(!after.is_empty());
        assert_ne!(after, before);

        let _ = ApiWalletDomain::new(context).clear_wallet_unlock_session().await;
    }

    #[tokio::test]
    #[serial]
    async fn session_key_rotation_rewraps_seed_without_password() {
        if run_in_isolated_session_process(
            "domain::api_wallet::wallet::tests::session_key_rotation_rewraps_seed_without_password",
        ) {
            return;
        }
        init_test_tracing();
        let env = seed_cache_test_env().await;
        let context = env.context;
        install_test_unlock_session(&env).await;

        let pool = context.api_wallet_pool().expect("api pool");
        let before = ApiWalletRepo::find_by_address(&pool, &env.wallet_address)
            .await
            .expect("load wallet before rotation")
            .expect("wallet before rotation")
            .seed;

        ApiWalletDomain::new(context)
            .rotate_wallet_session_key()
            .await
            .expect("rotate session key");

        let after = ApiWalletRepo::find_by_address(&pool, &env.wallet_address)
            .await
            .expect("load wallet after rotation")
            .expect("wallet after rotation")
            .seed;

        assert_ne!(before, after);

        let seed = ApiWalletDomain::new(context)
            .get_seed(&env.wallet_address)
            .await
            .expect("decrypt seed after rotation");
        assert_eq!(seed, b"seed-cache-seed");

        let _ = ApiWalletDomain::new(context).clear_wallet_unlock_session().await;
    }

    #[tokio::test]
    async fn seed_envelope_roundtrip() {
        init_test_tracing();
        let encrypted: Vec<u8> =
            ApiWalletDomain::encrypt_seed_bundle(TEST_PASSWORD, b"seed-bundle-roundtrip")
                .await
                .expect("encrypt seed bundle");

        let envelope = SeedEnvelopeCodec::decrypt_seed_envelope(TEST_PASSWORD, &encrypted)
            .await
            .expect("parse seed envelope");
        assert_eq!(envelope.version, SEED_ENVELOPE_VERSION_V1);
        assert_eq!(envelope.salt.len(), SEED_ENVELOPE_SALT_BYTES);
        assert_eq!(envelope.session_nonce.len(), SEED_ENVELOPE_NONCE_BYTES);
        assert_eq!(envelope.seed_nonce.len(), SEED_ENVELOPE_NONCE_BYTES);
        assert!(!envelope.wrapped_dek.is_empty());
        assert!(!envelope.seed_cipher.is_empty());

        let decrypted =
            ApiWalletDomain::decrypt_seed(TEST_PASSWORD, &encrypted).await.expect("decrypt seed");
        assert_eq!(decrypted, b"seed-bundle-roundtrip");
    }

    #[tokio::test]
    async fn decrypt_seed_rejects_wrong_password_for_envelope() {
        init_test_tracing();
        let encrypted: Vec<u8> =
            ApiWalletDomain::encrypt_seed_bundle(TEST_PASSWORD, b"seed-bundle-roundtrip")
                .await
                .expect("encrypt seed bundle");

        let err = ApiWalletDomain::decrypt_seed("wrong-password", &encrypted)
            .await
            .expect_err("decrypt must fail with wrong password");
        let debug = format!("{err:?}");
        assert!(!debug.contains("seed-bundle-roundtrip"));
    }

    #[tokio::test]
    #[serial]
    async fn seed_cache_is_not_retained() {
        if run_in_isolated_session_process(
            "domain::api_wallet::wallet::tests::seed_cache_is_not_retained",
        ) {
            return;
        }
        init_test_tracing();
        let env = seed_cache_test_env().await;
        let context = env.context;
        install_test_unlock_session(&env).await;

        let seed = ApiWalletDomain::new(context)
            .get_seed(&env.wallet_address)
            .await
            .expect("decrypt seed");
        assert_eq!(seed, b"seed-cache-seed");
        ApiWalletDomain::new(context).clear_wallet_unlock_session().await.expect("clear password");
    }
}
