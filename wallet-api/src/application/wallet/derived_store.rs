use std::{
    collections::BTreeMap,
    io::Write,
    sync::{Arc, LazyLock},
};

use crate::device_protection::migration::record_scope;
use wallet_database::repositories::device_protection::{MigrationRepo, MigrationState};
use wallet_tree::{
    KeyMeta,
    file_ops::{AccountData, BulkSubkey},
};
use zeroize::Zeroizing;

use super::{WalletApplication, decode_account_data};
use crate::{
    context::Context,
    device_protection::{
        ProtectionContext,
        keystore_format::{KeystoreProtection, KeystoreProtectionError},
    },
    domain::app::config::ConfigDomain,
    error::service::ServiceError,
};

// Shared by application bulk writes and account password changes, whose legacy
// writer also modifies the index. The codec separately locks ciphertext rewrites.
static WRITES: LazyLock<Arc<tokio::sync::Mutex<()>>> =
    LazyLock::new(|| Arc::new(tokio::sync::Mutex::new(())));
pub(super) async fn lock() -> Arc<tokio::sync::OwnedMutexGuard<()>> {
    Arc::new(WRITES.clone().lock_owned().await)
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub(super) struct Metadata {
    pub(super) accounts: BTreeMap<u32, Vec<KeyMeta>>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

impl WalletApplication {
    /// The service validates user intent/password before entering. Keep the
    /// operation alive through state, file and index writes if its waiter cancels.
    pub(crate) async fn delete_account_files(
        ctx: &'static Context,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<(), ServiceError> {
        let wallet_address = wallet_address.to_owned();
        tokio::spawn(async move {
            let guard = lock().await;
            let index = wallet_utils::address::AccountIndexMap::from_account_id(account_id)?;
            let dirs = ctx.get_global_dirs();
            let subs = dirs.get_subs_dir(&wallet_address)?;
            let path = subs.join(format!("key{account_id}.keystore"));
            let binding = ProtectionContext::new(&wallet_address, format!("derived:{account_id}"))
                .map_err(KeystoreProtectionError::from)?;
            let root_binding = ProtectionContext::new(&wallet_address, "root-keystore")
                .map_err(KeystoreProtectionError::from)?;
            let protected = Self::record_requires_device(ctx, &binding, &path).await?
                || Self::record_requires_device(
                    ctx,
                    &root_binding,
                    &dirs.get_root_dir(&wallet_address)?.join("root.keystore"),
                )
                .await?;
            if !protected {
                let strategy = ConfigDomain::get_wallet_tree_strategy(ctx).await?;
                strategy.get_wallet_tree(&dirs.wallet_dir)?.io().delete_account(&index, &subs)?;
                return Ok::<_, ServiceError>(());
            }
            let meta_path = subs.join("derived_meta.json");
            let mut metadata: Metadata = serde_json::from_slice(
                &std::fs::read(&meta_path).map_err(|_| KeystoreProtectionError::Storage)?,
            )
            .map_err(|_| KeystoreProtectionError::InvalidFormat)?;
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_file() => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(KeystoreProtectionError::Storage.into()),
            }
            // Record intent first: a crash after removal must not erase the device
            // requirement. An old index entry blocks recreation until deletion retries.
            MigrationRepo::authorize_recreation(&record_scope(&binding), &ctx.core_pool()?)
                .await
                .map_err(|_| ServiceError::Context("Cannot record account deletion".into()))?;
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(KeystoreProtectionError::Storage.into()),
            }
            metadata.accounts.remove(&account_id);
            publish_metadata(meta_path, &metadata, guard).await?;
            Ok(())
        })
        .await
        .map_err(|_| KeystoreProtectionError::Worker)?
    }

    /// Once accepted, finish ciphertext and index publication even if the caller
    /// stops waiting. Otherwise a detached file publisher can outlive this gate.
    pub(crate) async fn store_subkeys(
        ctx: &'static Context,
        wallet_address: &str,
        subkeys: Vec<BulkSubkey>,
        password: &str,
    ) -> Result<(), ServiceError> {
        let wallet_address = wallet_address.to_owned();
        let password = Zeroizing::new(password.to_owned());
        tokio::spawn(async move {
            Self::store_subkeys_inner(ctx, &wallet_address, subkeys, &password).await
        })
        .await
        .map_err(|_| KeystoreProtectionError::Worker)?
    }

    async fn store_subkeys_inner(
        ctx: &Context,
        wallet_address: &str,
        subkeys: Vec<BulkSubkey>,
        password: &str,
    ) -> Result<(), ServiceError> {
        if subkeys.is_empty() {
            return Ok(());
        }
        let guard = lock().await;
        let dirs = ctx.get_global_dirs();
        let subs = dirs.get_subs_dir(wallet_address)?;
        let meta_path = subs.join("derived_meta.json");
        let mut metadata: Metadata = match std::fs::read(&meta_path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| KeystoreProtectionError::InvalidFormat)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Metadata::default(),
            Err(_) => return Err(KeystoreProtectionError::Storage.into()),
        };
        let root_binding = ProtectionContext::new(wallet_address, "root-keystore")
            .map_err(KeystoreProtectionError::from)?;
        let root_required = Self::record_requires_device(
            ctx,
            &root_binding,
            &dirs.get_root_dir(wallet_address)?.join("root.keystore"),
        )
        .await?;
        let algorithm = ConfigDomain::get_keystore_kdf_algorithm(ctx).await?;
        let codec = KeystoreProtection::new(ctx.device_protection().clone());
        let mut grouped = BTreeMap::<u32, Vec<BulkSubkey>>::new();
        for key in subkeys {
            grouped.entry(key.account_index_map.account_id).or_default().push(key);
        }
        let mut legacy = Vec::new();
        for (id, additions) in grouped {
            let path = subs.join(format!("key{id}.keystore"));
            let binding = ProtectionContext::new(wallet_address, format!("derived:{id}"))
                .map_err(KeystoreProtectionError::from)?;
            let required = Self::record_has_device_requirement(ctx, &binding, &path).await?;
            if !required && !root_required {
                legacy.extend(additions);
                continue;
            }
            let state =
                MigrationRepo::get(&record_scope(&binding), &ctx.core_pool()?).await.map_err(
                    |_| ServiceError::Context("Cannot read keystore migration progress".into()),
                )?;
            let exists = path.try_exists().map_err(|_| KeystoreProtectionError::Storage)?;
            if !exists && state == Some(MigrationState::Pending) {
                return Err(KeystoreProtectionError::Storage.into());
            }
            let keys = if exists {
                MigrationRepo::begin(&record_scope(&binding), &ctx.core_pool()?).await.map_err(
                    |_| ServiceError::Context("Cannot record protected account write".into()),
                )?;
                codec
                    .update_file(
                        &path,
                        &binding,
                        password,
                        password,
                        algorithm.clone(),
                        move |plain| merge(decode_account_data(&plain)?, additions),
                    )
                    .await?
            } else {
                // Never replace a lost known file with only the new subset of keys.
                if metadata.accounts.contains_key(&id)
                    || (required
                        && !matches!(
                            state,
                            None | Some(MigrationState::CreationAuthorized)
                                | Some(MigrationState::RecreationAllowed)
                        ))
                {
                    return Err(KeystoreProtectionError::Storage.into());
                }
                if state.is_none() {
                    MigrationRepo::authorize_creation(&record_scope(&binding), &ctx.core_pool()?)
                        .await
                        .map_err(|_| {
                            ServiceError::Context(
                                "Cannot authorize protected account creation".into(),
                            )
                        })?;
                }
                let (plain, keys) = merge(AccountData::default(), additions)?;
                codec.store_file(&path, &binding, plain, password, algorithm.clone()).await?;
                keys
            };
            metadata.accounts.insert(id, keys);
            publish_metadata(meta_path.clone(), &metadata, guard.clone()).await?;
            MigrationRepo::complete(&record_scope(&binding), &ctx.core_pool()?).await.map_err(
                |_| ServiceError::Context("Cannot complete protected account write".into()),
            )?;
        }
        if !legacy.is_empty() {
            let strategy = ConfigDomain::get_wallet_tree_strategy(ctx).await?;
            wallet_tree::api::KeystoreApi::initialize_child_keystores(
                strategy.get_wallet_tree(&dirs.wallet_dir)?,
                legacy,
                subs,
                password,
                algorithm,
            )?;
        }
        Ok(())
    }
}

fn merge(
    mut data: AccountData,
    additions: Vec<BulkSubkey>,
) -> Result<(Zeroizing<Vec<u8>>, Vec<KeyMeta>), KeystoreProtectionError> {
    for key in additions {
        data.insert(
            KeyMeta {
                chain_code: key.chain_code,
                address: key.address,
                derivation_path: key.derivation_path,
            },
            key.data,
        );
    }
    let metadata = data.keys().cloned().collect();
    let encoded: BTreeMap<_, _> = data.iter().map(|(key, value)| (key.encode(), value)).collect();
    let bytes = serde_json::to_vec(&encoded).map_err(|_| KeystoreProtectionError::InvalidFormat)?;
    Ok((Zeroizing::new(bytes), metadata))
}

async fn publish_metadata(
    destination: std::path::PathBuf,
    metadata: &Metadata,
    guard: Arc<tokio::sync::OwnedMutexGuard<()>>,
) -> Result<(), KeystoreProtectionError> {
    let bytes = serde_json::to_vec(metadata).map_err(|_| KeystoreProtectionError::InvalidFormat)?;
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        let parent = destination.parent().ok_or(KeystoreProtectionError::Storage)?;
        let publish = || -> std::io::Result<()> {
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            temp.write_all(&bytes)?;
            temp.as_file().sync_all()?;
            temp.persist(&destination).map_err(|e| e.error)?;
            std::fs::File::open(parent)?.sync_all()
        };
        publish().map_err(|_| KeystoreProtectionError::Storage)
    })
    .await
    .map_err(|_| KeystoreProtectionError::Worker)?
}
