mod derived_store;
mod legacy_initialization;
mod legacy_upgrade;
mod protection_initialization;
mod protection_rollout;
mod protection_startup;
mod unlock_timing;
mod wallet_deletion;

use std::collections::HashSet;

use wallet_database::{
    CoreDbPool,
    entities::config::config_key::{KEYSTORE_KDF_ALGORITHM, WALLET_TREE_STRATEGY},
    repositories::{
        account::AccountRepo,
        api_wallet::wallet::ApiWalletRepo,
        device::DeviceRepo,
        device_protection::{MigrationRepo, MigrationState},
        wallet::WalletRepo,
    },
};
use wallet_tree::{KdfAlgorithm, WalletTreeStrategy};

use crate::{
    context::Context,
    device_protection::{
        ProtectionContext,
        keystore_format::{KeystoreProtection, KeystoreProtectionError, has_device_header},
        migration::record_scope,
    },
    domain::{api_wallet::wallet::ApiWalletDomain, app::config::ConfigDomain},
    error::service::ServiceError,
};

pub struct WalletApplication;

impl WalletApplication {
    pub(crate) async fn load_root(
        ctx: &Context,
        wallet_address: &str,
        password: &str,
    ) -> Result<wallet_tree::file_ops::RootData, ServiceError> {
        let dirs = ctx.get_global_dirs();
        let root_dir = dirs.get_root_dir(wallet_address)?;
        let binding = ProtectionContext::new(wallet_address, "root-keystore")
            .map_err(KeystoreProtectionError::from)?;
        if Self::record_requires_device(ctx, &binding, &root_dir.join("root.keystore")).await? {
            return Ok(KeystoreProtection::new(ctx.device_protection().clone())
                .load_root(&root_dir, wallet_address, password)
                .await?);
        }

        let strategy = ConfigDomain::get_wallet_tree_strategy(ctx).await?;
        let wallet_tree = strategy.get_wallet_tree(&dirs.wallet_dir)?;
        Ok(wallet_tree::api::KeystoreApi::load_root(
            &*wallet_tree,
            &root_dir,
            wallet_address,
            password,
        )?)
    }

    pub(crate) async fn store_root(
        ctx: &Context,
        wallet_address: &str,
        root: wallet_tree::file_ops::RootData,
        password: &str,
    ) -> Result<(), ServiceError> {
        let guard = derived_store::lock().await;
        let dirs = ctx.get_global_dirs();
        let root_dir = dirs.get_root_dir(wallet_address)?;
        let binding = ProtectionContext::new(wallet_address, "root-keystore")
            .map_err(KeystoreProtectionError::from)?;
        let algorithm = ConfigDomain::get_keystore_kdf_algorithm(ctx).await?;
        if Self::record_requires_device(ctx, &binding, &root_dir.join("root.keystore")).await? {
            let pool = ctx.core_pool()?;
            let scope = record_scope(&binding);
            let state = MigrationRepo::get(&scope, &pool).await.map_err(|_| {
                ServiceError::Context("Cannot read keystore migration progress".into())
            })?;
            let mut recreate = state == Some(MigrationState::RecreationAllowed);
            let active = protection_rollout::files_active(ctx).await?;
            let exists = root_dir
                .join("root.keystore")
                .try_exists()
                .map_err(|_| KeystoreProtectionError::Storage)?;
            if active && exists && !has_device_header(&root_dir.join("root.keystore")).await? {
                return Err(KeystoreProtectionError::LegacyForbidden.into());
            }
            if active && !exists && state.is_none() {
                let known: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM wallet WHERE address=?) OR EXISTS(SELECT 1 FROM account WHERE wallet_address=?)")
                    .bind(wallet_address).bind(wallet_address).fetch_one(pool.read_ref()).await
                    .map_err(|_| ServiceError::Context("Cannot inspect new wallet storage".into()))?;
                let wallet_dir = root_dir.parent().ok_or(KeystoreProtectionError::Storage)?;
                if known
                    || !wallet_tree::file_ops::inventory::scan_wallet(wallet_dir, wallet_address)?
                        .is_empty()
                    || wallet_dir
                        .join("subs/derived_meta.json")
                        .try_exists()
                        .map_err(|_| KeystoreProtectionError::Storage)?
                {
                    return Err(KeystoreProtectionError::Storage.into());
                }
                // Explicit fresh creation owns retry permission before publication.
                MigrationRepo::authorize_recreation(&scope, &pool)
                    .await
                    .map_err(|_| ServiceError::Context("Cannot record new root creation".into()))?;
                recreate = true;
            }
            if !root_dir
                .join("root.keystore")
                .try_exists()
                .map_err(|_| KeystoreProtectionError::Storage)?
                && !recreate
            {
                return Err(KeystoreProtectionError::Storage.into());
            }
            let codec = KeystoreProtection::new(ctx.device_protection().clone());
            let address = wallet_address.to_owned();
            let password = zeroize::Zeroizing::new(password.to_owned());
            return tokio::spawn(async move {
                let _guard = guard;
                codec.store_root(&root_dir, &address, root, &password, algorithm).await?;
                if recreate {
                    MigrationRepo::complete(&scope, &pool).await.map_err(|_| {
                        ServiceError::Context("Cannot complete root recreation".into())
                    })?;
                }
                Ok::<_, ServiceError>(())
            })
            .await
            .map_err(|_| KeystoreProtectionError::Worker)?;
        }

        // Preserve the existing unprotected creation path until provisioning/rollout is wired.
        let strategy = ConfigDomain::get_wallet_tree_strategy(ctx).await?;
        let wallet_tree = strategy.get_wallet_tree(&dirs.wallet_dir)?;
        wallet_utils::file_func::recreate_dir_all(&root_dir)?;
        Ok(wallet_tree::api::KeystoreApi::initialize_root_keystore(
            wallet_tree,
            wallet_address,
            root,
            &root_dir,
            password,
            algorithm,
        )?)
    }

    // Active file policy forbids legacy fallback even for untracked records.
    async fn record_requires_device(
        ctx: &Context,
        binding: &ProtectionContext,
        path: &std::path::Path,
    ) -> Result<bool, ServiceError> {
        let recorded = Self::record_has_device_requirement(ctx, binding, path).await?;
        Ok(protection_rollout::files_active(ctx).await? || recorded)
    }

    // Per-record history, used to distinguish new derived files from lost ones.
    async fn record_has_device_requirement(
        ctx: &Context,
        binding: &ProtectionContext,
        path: &std::path::Path,
    ) -> Result<bool, ServiceError> {
        let state = MigrationRepo::get(&record_scope(binding), &ctx.core_pool()?)
            .await
            .map_err(|_| ServiceError::Context("Cannot read keystore migration progress".into()))?;
        let protected = has_device_header(path).await?;
        // Completion forbids legacy fallback, even when the protected file is missing.
        Ok(protected
            || matches!(
                state,
                Some(
                    MigrationState::DeviceRequired
                        | MigrationState::CreationAuthorized
                        | MigrationState::RecreationAllowed
                )
            ))
    }

    pub(crate) async fn change_root_password(
        ctx: &Context,
        wallet_address: &str,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), ServiceError> {
        let guard = derived_store::lock().await;
        let dirs = ctx.get_global_dirs();
        let root_dir = dirs.get_root_dir(wallet_address)?;
        let algorithm = ConfigDomain::get_keystore_kdf_algorithm(ctx).await?;
        let binding = ProtectionContext::new(wallet_address, "root-keystore")
            .map_err(KeystoreProtectionError::from)?;
        if Self::record_requires_device(ctx, &binding, &root_dir.join("root.keystore")).await? {
            let codec = KeystoreProtection::new(ctx.device_protection().clone());
            let old_password = zeroize::Zeroizing::new(old_password.to_owned());
            let new_password = zeroize::Zeroizing::new(new_password.to_owned());
            return Ok(tokio::spawn(async move {
                let _guard = guard;
                codec
                    .change_file_password(
                        &root_dir.join("root.keystore"),
                        &binding,
                        &old_password,
                        &new_password,
                        algorithm,
                    )
                    .await
            })
            .await
            .map_err(|_| KeystoreProtectionError::Worker)??);
        }

        let strategy = ConfigDomain::get_wallet_tree_strategy(ctx).await?;
        let wallet_tree = strategy.get_wallet_tree(&dirs.wallet_dir)?;
        wallet_tree::api::KeystoreApi::update_root_password(
            root_dir,
            wallet_tree,
            wallet_address,
            old_password,
            new_password,
            algorithm,
        )
        .map_err(|error| crate::error::system::SystemError::Service(error.to_string()))?;
        Ok(())
    }

    pub(crate) async fn resume_root_password_change(
        ctx: &Context,
        wallet_address: &str,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), ServiceError> {
        let dirs = ctx.get_global_dirs();
        let root_dir = dirs.get_root_dir(wallet_address)?;
        let path = root_dir.join("root.keystore");
        let binding = ProtectionContext::new(wallet_address, "root-keystore")
            .map_err(KeystoreProtectionError::from)?;
        if !Self::record_requires_device(ctx, &binding, &path).await? {
            return Self::change_root_password(ctx, wallet_address, old_password, new_password)
                .await;
        }
        let guard = derived_store::lock().await;
        let codec = KeystoreProtection::new(ctx.device_protection().clone());
        let algorithm = ConfigDomain::get_keystore_kdf_algorithm(ctx).await?;
        let old_password = zeroize::Zeroizing::new(old_password.to_owned());
        let new_password = zeroize::Zeroizing::new(new_password.to_owned());
        Ok(tokio::spawn(async move {
            let _guard = guard;
            codec
                .change_file_password_if_needed(
                    &path,
                    &binding,
                    &old_password,
                    &new_password,
                    algorithm,
                )
                .await
        })
        .await
        .map_err(|_| KeystoreProtectionError::Worker)??)
    }

    pub(crate) async fn change_account_password(
        ctx: &Context,
        wallet_address: &str,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), ServiceError> {
        let guard = derived_store::lock().await;
        let dirs = ctx.get_global_dirs();
        let subs_dir = dirs.get_subs_dir(wallet_address)?;
        let path = subs_dir.join(format!("key{}.keystore", account_index_map.account_id));
        let binding = ProtectionContext::new(
            wallet_address,
            format!("derived:{}", account_index_map.account_id),
        )
        .map_err(KeystoreProtectionError::from)?;
        let algorithm = ConfigDomain::get_keystore_kdf_algorithm(ctx).await?;
        if Self::record_requires_device(ctx, &binding, &path).await? {
            let codec = KeystoreProtection::new(ctx.device_protection().clone());
            let old_password = zeroize::Zeroizing::new(old_password.to_owned());
            let new_password = zeroize::Zeroizing::new(new_password.to_owned());
            // Keep deletion/append blocked through publication even if the
            // caller cancels while the native operation or disk worker runs.
            return Ok(tokio::spawn(async move {
                let _guard = guard;
                codec
                    .change_file_password(&path, &binding, &old_password, &new_password, algorithm)
                    .await
            })
            .await
            .map_err(|_| KeystoreProtectionError::Worker)??);
        }

        let strategy = ConfigDomain::get_wallet_tree_strategy(ctx).await?;
        let wallet_tree = strategy.get_wallet_tree(&dirs.wallet_dir)?;
        wallet_tree::api::KeystoreApi::update_account_password(
            wallet_tree,
            &subs_dir,
            account_index_map,
            old_password,
            new_password,
            algorithm,
        )
        .map_err(|error| crate::error::system::SystemError::Service(error.to_string()))?;
        Ok(())
    }

    pub(crate) async fn resume_account_password_change(
        ctx: &Context,
        wallet_address: &str,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), ServiceError> {
        let dirs = ctx.get_global_dirs();
        let path = dirs
            .get_subs_dir(wallet_address)?
            .join(format!("key{}.keystore", account_index_map.account_id));
        let binding = ProtectionContext::new(
            wallet_address,
            format!("derived:{}", account_index_map.account_id),
        )
        .map_err(KeystoreProtectionError::from)?;
        if !Self::record_requires_device(ctx, &binding, &path).await? {
            return Self::change_account_password(
                ctx,
                wallet_address,
                account_index_map,
                old_password,
                new_password,
            )
            .await;
        }
        let guard = derived_store::lock().await;
        let codec = KeystoreProtection::new(ctx.device_protection().clone());
        let algorithm = ConfigDomain::get_keystore_kdf_algorithm(ctx).await?;
        let old_password = zeroize::Zeroizing::new(old_password.to_owned());
        let new_password = zeroize::Zeroizing::new(new_password.to_owned());
        Ok(tokio::spawn(async move {
            let _guard = guard;
            codec
                .change_file_password_if_needed(
                    &path,
                    &binding,
                    &old_password,
                    &new_password,
                    algorithm,
                )
                .await
        })
        .await
        .map_err(|_| KeystoreProtectionError::Worker)??)
    }

    pub(crate) async fn load_account_data(
        ctx: &Context,
        wallet_address: &str,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        password: &str,
    ) -> Result<wallet_tree::file_ops::AccountData, ServiceError> {
        let dirs = ctx.get_global_dirs();
        let subs_dir = dirs.get_subs_dir(wallet_address)?;
        let path = subs_dir.join(format!("key{}.keystore", account_index_map.account_id));
        let binding = ProtectionContext::new(
            wallet_address,
            format!("derived:{}", account_index_map.account_id),
        )
        .map_err(KeystoreProtectionError::from)?;
        if Self::record_requires_device(ctx, &binding, &path).await? {
            return Self::load_protected_account(ctx, &binding, &path, password).await;
        }
        let strategy = ConfigDomain::get_wallet_tree_strategy(ctx).await?;
        let wallet_tree = strategy.get_wallet_tree(&dirs.wallet_dir)?;
        Ok(wallet_tree::api::KeystoreApi::load_account_pk(
            &*wallet_tree,
            account_index_map,
            &subs_dir,
            password,
        )?)
    }

    pub(crate) async fn load_sub_key(
        ctx: &Context,
        wallet_address: &str,
        account_index_map: &wallet_utils::address::AccountIndexMap,
        expected: &wallet_tree::KeyMeta,
        password: &str,
    ) -> Result<Vec<u8>, ServiceError> {
        let dirs = ctx.get_global_dirs();
        let subs_dir = dirs.get_subs_dir(wallet_address)?;
        let path = subs_dir.join(format!("key{}.keystore", account_index_map.account_id));
        let binding = ProtectionContext::new(
            wallet_address,
            format!("derived:{}", account_index_map.account_id),
        )
        .map_err(KeystoreProtectionError::from)?;
        if Self::record_requires_device(ctx, &binding, &path).await? {
            let mut keys = Self::load_protected_account(ctx, &binding, &path, password).await?;
            return keys
                .remove(expected)
                .ok_or_else(|| wallet_tree::Error::PrivateKeyNotFound.into());
        }
        let strategy = ConfigDomain::get_wallet_tree_strategy(ctx).await?;
        let wallet_tree = strategy.get_wallet_tree(&dirs.wallet_dir)?;
        Ok(wallet_tree::api::KeystoreApi::load_sub_pk(
            &*wallet_tree,
            Some(account_index_map),
            &subs_dir,
            &expected.address,
            &expected.chain_code,
            &expected.derivation_path,
            password,
        )?)
    }

    async fn load_protected_account(
        ctx: &Context,
        binding: &ProtectionContext,
        path: &std::path::Path,
        password: &str,
    ) -> Result<wallet_tree::file_ops::AccountData, ServiceError> {
        let plain = KeystoreProtection::new(ctx.device_protection().clone())
            .load_file(path, binding, password)
            .await?;
        Ok(decode_account_data(&plain)?)
    }

    async fn validate_password_inner(
        ctx: &'static Context,
        password: &str,
    ) -> Result<Option<String>, ServiceError> {
        let core_pool = ctx.core_pool()?;
        let api_pool = ctx.api_wallet_pool()?;

        let sn = ctx.get_sn();
        let device = {
            let _timing = unlock_timing::Phase::start("device_lookup");
            DeviceRepo::get_device_info(core_pool.clone(), sn).await?
        };
        let Some(device) = device else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };

        if device.password.is_some() {
            let _timing = unlock_timing::Phase::start("legacy_upgrade");
            let strategy = ConfigDomain::get_wallet_tree_strategy(ctx).await?;
            if matches!(strategy, WalletTreeStrategy::V2) {
                legacy_upgrade::resume_published_v2(ctx, password).await?;
                return Ok(None);
            }
            // A real V1 wallet authenticates through its source files in upgrade_algorithm.
            // With no V1 wallet, authenticate proof/API ciphertext before an empty conversion.
            if !legacy_upgrade::has_v1_wallets(ctx).await? {
                legacy_initialization::verify_non_file_storage(ctx, password).await?;
            }
            Self::upgrade_algorithm(ctx, password).await?;
            return Ok(None);
        }

        let has_wallets = {
            let _timing = unlock_timing::Phase::start("wallet_lookup");
            WalletRepo::wallet_latest(core_pool.clone()).await?.is_some()
                || ApiWalletRepo::wallet_latest(&api_pool).await?.is_some()
        };
        if !has_wallets {
            return Ok(None);
        }

        match &device.password_proof {
            Some(proof) => {
                let _timing = unlock_timing::Phase::start("proof_decrypt");
                if let Err(_) = Self::decrypt_password_proof(proof, password).await {
                    tracing::info!("password validation failed");
                    return Err(crate::error::business::BusinessError::Wallet(
                        crate::error::business::wallet::WalletError::PasswordIncorrect,
                    )
                    .into());
                }
                Ok(Some(proof.clone()))
            }
            None => {
                let _timing = unlock_timing::Phase::start("password_fallback");
                tracing::info!("password_proof is None, trying fallback validation");
                if Self::try_decrypt_wallet_db(ctx, password).await? {
                    let proof = Self::generate_password_proof(ctx, password).await?;
                    DeviceRepo::update_password_proof(core_pool.clone(), sn, Some(&proof)).await?;
                    tracing::info!("password_proof generated and stored");
                    Ok(Some(proof))
                } else {
                    tracing::info!("password validation failed");
                    return Err(crate::error::business::BusinessError::Wallet(
                        crate::error::business::wallet::WalletError::PasswordIncorrect,
                    )
                    .into());
                }
            }
        }
    }

    pub(crate) async fn upgrade_algorithm(
        ctx: &'static Context,
        password: &str,
    ) -> Result<(), ServiceError> {
        let _guard = derived_store::lock().await;
        legacy_upgrade::preflight(ctx).await?;
        let dirs = ctx.get_global_dirs();

        let mut legacy_wallet_tree = WalletTreeStrategy::V1.get_wallet_tree(&dirs.wallet_dir)?;
        #[derive(Debug, Clone, Eq, PartialEq, Hash)]
        struct AccountInfo {
            wallet_address: String,
            address: String,
            chain_code: String,
            derivation_path: String,
        }

        let modern_wallet_tree = WalletTreeStrategy::V2.get_wallet_tree(&dirs.wallet_dir)?;
        let mut account_data = std::collections::HashMap::<AccountInfo, Vec<u8>>::new();
        let legacy_wallet_count = legacy_wallet_tree.iter().count();
        tracing::info!(legacy_wallet_count, "legacy wallet tree loaded for algorithm upgrade");

        let mut delete_roots = Vec::new();
        let mut delete_subs = Vec::new();
        for (k, v) in legacy_wallet_tree.iter() {
            let root_dir = dirs.get_root_dir(k)?;
            let subs_dir = dirs.get_subs_dir(k)?;
            let root_data = legacy_wallet_tree.io().load_root(k, &root_dir, password)?;
            modern_wallet_tree.io().store_root(
                k,
                root_data.seed(),
                root_data.phrase(),
                &root_dir,
                password,
                wallet_tree::KdfAlgorithm::Argon2id,
            )?;
            // Never schedule removal of the source unless its replacement reads
            // back correctly. Failed/partial conversions retain the legacy files.
            let verified = modern_wallet_tree.io().load_root(k, &root_dir, password)?;
            if verified.seed() != root_data.seed() || verified.phrase() != root_data.phrase() {
                return Err(KeystoreProtectionError::InvalidFormat.into());
            }

            for account in v.get_accounts().into_iter() {
                let address = account.get_address();
                let chain_code = account.chain_code().unwrap_or_default();
                let derivation_path = account.derivation_path().unwrap_or_default();

                let pk = legacy_wallet_tree.io().load_subkey(
                    None,
                    address,
                    &chain_code,
                    &derivation_path,
                    &subs_dir,
                    password,
                )?;

                account_data.insert(
                    AccountInfo {
                        wallet_address: k.to_string(),
                        address: address.to_string(),
                        chain_code,
                        derivation_path,
                    },
                    pk,
                );
            }
            delete_roots.push(k);
        }

        modern_wallet_tree.io().store(
            "verify",
            &"data",
            &dirs.root_dir,
            password,
            wallet_tree::KdfAlgorithm::Argon2id,
        )?;

        let mut subkeys = std::collections::HashMap::new();
        for (info, d) in account_data {
            let hd_path = wallet_chain_instance::derivation_path::get_account_hd_path_from_path(
                &info.derivation_path,
            )?;
            let account_index_map =
                wallet_utils::address::AccountIndexMap::from_account_id(hd_path.get_account_id()?)?;

            let subkey = wallet_tree::file_ops::BulkSubkey::new(
                account_index_map.clone(),
                &info.address,
                &info.chain_code,
                &info.derivation_path,
                d,
            );

            subkeys.entry(info.wallet_address.clone()).or_insert_with(Vec::new).push(subkey);
            delete_subs.push(SubsKeyInfo {
                wallet_address: info.wallet_address,
                address: info.address,
                chain_code: info.chain_code,
            });
        }

        for (wallet_address, subkey) in subkeys {
            let subs_dir = dirs.get_subs_dir(&wallet_address)?;
            modern_wallet_tree.io().store_subkeys_bulk(
                subkey,
                &subs_dir,
                password,
                wallet_tree::KdfAlgorithm::Argon2id,
            )?;
        }

        let keystore_kdf_algorithm = wallet_database::entities::config::KeystoreKdfAlgorithm {
            keystore_kdf_algorithm: KdfAlgorithm::Argon2id,
        };
        let wallet_tree_strategy = wallet_database::entities::config::WalletTreeStrategy {
            wallet_tree_strategy: wallet_tree::WalletTreeStrategy::V2,
        };
        crate::domain::app::config::ConfigDomain::set_config(
            ctx,
            KEYSTORE_KDF_ALGORITHM,
            &keystore_kdf_algorithm.to_json_str()?,
        )
        .await?;
        crate::domain::app::config::ConfigDomain::set_config(
            ctx,
            WALLET_TREE_STRATEGY,
            &wallet_tree_strategy.to_json_str()?,
        )
        .await?;

        for k in delete_roots {
            let root_dir = dirs.get_root_dir(k)?;
            legacy_wallet_tree.io().delete_root(k, &root_dir)?;
        }

        for k in delete_subs {
            let subs_dir = dirs.get_subs_dir(&k.wallet_address)?;
            legacy_wallet_tree.delete_subkey(
                &k.wallet_address,
                &k.address,
                k.chain_code.as_str(),
                &subs_dir,
                password,
            )?;
        }

        let sn = ctx.get_sn();
        let core_pool = ctx.core_pool()?;
        DeviceRepo::update_password(core_pool, sn, None).await?;

        Ok(())
    }

    pub(crate) async fn get_seed(
        ctx: &'static Context,
        wallet_address: &str,
        wallet_password: &str,
    ) -> Result<Vec<u8>, ServiceError> {
        Ok(Self::load_root(ctx, wallet_address, wallet_password).await?.seed().to_vec())
    }

    pub(crate) async fn restart_existing_wallet(
        core_pool: CoreDbPool,
        address: &str,
    ) -> Result<HashSet<u32>, ServiceError> {
        let mut account_ids = HashSet::new();
        if let Some(wallet) = WalletRepo::detail_all_status(core_pool.clone(), address).await? {
            if wallet.status == 2 {
                WalletRepo::restart(core_pool.clone(), &[address]).await?;
                for account in AccountRepo::restart(core_pool.clone(), address).await? {
                    account_ids.insert(account.account_id);
                }
            }
        }
        if account_ids.is_empty() {
            account_ids.insert(1);
        }
        Ok(account_ids)
    }

    pub(crate) async fn check_api_wallet_exist(
        ctx: &'static Context,
        address: &str,
    ) -> Result<bool, ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let res = ApiWalletRepo::find_by_address(&pool, address).await?;
        Ok(res.is_some())
    }

    pub(crate) async fn generate_password_proof(
        ctx: &'static Context,
        password: &str,
    ) -> Result<String, ServiceError> {
        const PROOF_STRING: &str = "wallet-sdk-password-proof";

        let algorithm =
            crate::domain::app::config::ConfigDomain::get_keystore_kdf_algorithm(ctx).await?;
        let rng = rand::rngs::OsRng::default();

        let proof =
            ApiWalletDomain::encrypt_password_proof(algorithm, rng, password, PROOF_STRING).await?;

        Ok(proof)
    }

    pub(crate) async fn decrypt_password_proof(
        proof: &str,
        password: &str,
    ) -> Result<(), ServiceError> {
        const PROOF_STRING: &str = "wallet-sdk-password-proof";

        let decrypted = ApiWalletDomain::decrypt_password_proof(password, proof).await?;
        if decrypted == PROOF_STRING {
            Ok(())
        } else {
            Err(crate::error::business::BusinessError::Wallet(
                crate::error::business::wallet::WalletError::PasswordIncorrect,
            )
            .into())
        }
    }

    async fn try_decrypt_wallet_db(
        ctx: &'static Context,
        password: &str,
    ) -> Result<bool, ServiceError> {
        let core_pool = ctx.core_pool()?;
        let pool = ctx.api_wallet_pool()?;

        if let Some(wallet) = WalletRepo::wallet_latest(core_pool.clone()).await? {
            match Self::load_root(ctx, &wallet.address, password).await {
                Ok(_) => {
                    tracing::info!("standard wallet decryption succeeded");
                    return Ok(true);
                }
                // Preserve password fallback, but never hide a device or policy failure.
                Err(ServiceError::DeviceProtection(
                    KeystoreProtectionError::PasswordDecryption,
                ))
                | Err(ServiceError::Tree(_))
                | Err(ServiceError::Keystore(_)) => {}
                Err(error) => return Err(error),
            }
        } else {
            tracing::info!("no WalletRepo wallet_latest");
        }

        if let Some(wallet) = ApiWalletRepo::wallet_latest(&pool).await? {
            match ApiWalletDomain::decrypt_phrase(password, &wallet.phrase).await {
                Ok(_) => {
                    tracing::info!("API wallet phrase decryption succeeded");
                    return Ok(true);
                }
                Err(e) => {
                    tracing::warn!("API wallet seed decryption error {:?}", e,);
                }
            }
            match ApiWalletDomain::decrypt_seed(password, &wallet.seed).await {
                Ok(_) => {
                    tracing::info!("API wallet seed decryption succeeded");
                    return Ok(true);
                }
                Err(e) => {
                    tracing::warn!("API wallet seed decryption error {:?}", e);
                }
            }
        } else {
            tracing::info!("no ApiWalletRepo wallet_latest");
        }

        tracing::info!("all wallet decryption attempts failed");
        Ok(false)
    }
}

struct SubsKeyInfo {
    pub wallet_address: String,
    pub address: String,
    pub chain_code: String,
}

// Visit entries directly: a normal map deserializer discards duplicate JSON keys.
fn decode_account_data(
    plain: &[u8],
) -> Result<wallet_tree::file_ops::AccountData, KeystoreProtectionError> {
    use serde::de::{Error as _, MapAccess, Visitor};
    struct AccountVisitor;
    impl<'de> Visitor<'de> for AccountVisitor {
        type Value = wallet_tree::file_ops::AccountData;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an account key map")
        }

        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut keys = Self::Value::default();
            while let Some((encoded, key)) = map.next_entry::<String, Vec<u8>>()? {
                let meta = wallet_tree::KeyMeta::decode(&encoded)
                    .map_err(|_| M::Error::custom("invalid account metadata"))?;
                if keys.insert(meta, key).is_some() {
                    return Err(M::Error::custom("duplicate account metadata"));
                }
            }
            Ok(keys)
        }
    }
    let mut decoder = serde_json::Deserializer::from_slice(plain);
    let keys = serde::de::Deserializer::deserialize_map(&mut decoder, AccountVisitor)
        .map_err(|_| KeystoreProtectionError::InvalidFormat)?;
    decoder.end().map_err(|_| KeystoreProtectionError::InvalidFormat)?;
    Ok(keys)
}

#[cfg(test)]
#[path = "wallet_protection_tests.rs"]
mod protection_tests;
