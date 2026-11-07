use std::collections::{BTreeMap, BTreeSet};

use super::{WalletApplication, derived_store};
use crate::{
    context::Context,
    device_protection::{
        ProtectionContext,
        keystore_format::{KeystoreProtectionError, has_device_header},
        migration::record_scope,
    },
    error::service::ServiceError,
};
use wallet_database::repositories::{account::AccountRepo, device_protection::MigrationRepo};
use wallet_tree::file_ops::inventory::{RecordIdentity, scan_wallet};

impl WalletApplication {
    /// Called only after explicit wallet deletion/reset authorization. Keep all
    /// storage work alive if the service stops waiting; no device key is deleted.
    pub(crate) async fn delete_wallet_files(
        ctx: &'static Context,
        wallet_address: Option<&str>,
    ) -> Result<(), ServiceError> {
        let selected = wallet_address.map(str::to_owned);
        tokio::spawn(async move {
            let guard = derived_store::lock().await;
            let dirs = ctx.get_global_dirs().clone();
            if let Some(address) = &selected {
                dirs.get_wallet_dir(Some(address))?;
            }
            let pool = ctx.core_pool()?;
            let mut wallets = BTreeMap::<String, BTreeSet<u32>>::new();
            if let Some(address) = &selected {
                wallets.entry(address.clone()).or_default();
            }
            // Reset must also cover inactive wallets whose directories are missing.
            let addresses: Vec<String> = sqlx::query_scalar("SELECT address FROM wallet")
                .fetch_all(pool.read_ref())
                .await
                .map_err(|_| ServiceError::Context("Cannot read wallets for deletion".into()))?;
            for address in addresses {
                if selected.as_ref().is_none_or(|s| s == &address) {
                    wallets.entry(address).or_default();
                }
            }
            for account in AccountRepo::list(pool.clone()).await? {
                if selected.as_ref().is_none_or(|s| s == &account.wallet_address) {
                    wallets.entry(account.wallet_address).or_default().insert(account.account_id);
                }
            }
            let scan_dirs = dirs.clone();
            let snapshots = tokio::task::spawn_blocking(move || -> Result<_, ServiceError> {
                if selected.is_none() {
                    for entry in std::fs::read_dir(&scan_dirs.wallet_dir)
                        .map_err(|_| KeystoreProtectionError::Storage)?
                    {
                        let entry = entry.map_err(|_| KeystoreProtectionError::Storage)?;
                        let name = entry
                            .file_name()
                            .into_string()
                            .map_err(|_| KeystoreProtectionError::InvalidFormat)?;
                        wallets.entry(name).or_default();
                    }
                }
                let mut snapshots = Vec::new();
                for (address, mut accounts) in wallets {
                    let path = scan_dirs.get_wallet_dir(Some(&address))?;
                    let records = match std::fs::symlink_metadata(&path) {
                        Ok(_) => scan_wallet(&path, &address)?,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                        Err(_) => return Err(KeystoreProtectionError::Storage.into()),
                    };
                    for record in &records {
                        if let RecordIdentity::Derived { account_id } = record.identity {
                            accounts.insert(account_id);
                        }
                    }
                    match std::fs::read(path.join("subs/derived_meta.json")) {
                        Ok(bytes) => {
                            let metadata: derived_store::Metadata = serde_json::from_slice(&bytes)
                                .map_err(|_| KeystoreProtectionError::InvalidFormat)?;
                            accounts.extend(metadata.accounts.keys().copied());
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(_) => return Err(KeystoreProtectionError::Storage.into()),
                    }
                    snapshots.push((address, path, accounts, records));
                }
                Ok(snapshots)
            })
            .await
            .map_err(|_| KeystoreProtectionError::Worker)??;
            let mut authorizations = Vec::new();
            let mut paths = Vec::new();
            for (address, path, accounts, records) in snapshots {
                let root = ProtectionContext::new(&address, "root-keystore")
                    .map_err(KeystoreProtectionError::from)?;
                let mut protected =
                    Self::record_requires_device(ctx, &root, &path.join("root/root.keystore"))
                        .await?;
                for record in records {
                    protected |= has_device_header(&record.path).await?;
                }
                let mut children = Vec::new();
                for id in accounts {
                    let binding = ProtectionContext::new(&address, format!("derived:{id}"))
                        .map_err(KeystoreProtectionError::from)?;
                    protected |= Self::record_requires_device(
                        ctx,
                        &binding,
                        &path.join(format!("subs/key{id}.keystore")),
                    )
                    .await?;
                    children.push(binding);
                }
                if protected {
                    authorizations.push(root);
                    authorizations.extend(children);
                }
                paths.push(path);
            }
            // Persist every permission before deleting any directory. Failure may
            // leave permissions, but keeps files available for deletion retry.
            for binding in authorizations {
                MigrationRepo::authorize_recreation(&record_scope(&binding), &pool)
                    .await
                    .map_err(|_| ServiceError::Context("Cannot record wallet deletion".into()))?;
            }
            tokio::task::spawn_blocking(move || {
                let _guard = guard;
                for path in paths {
                    match std::fs::remove_dir_all(path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(_) => return Err(KeystoreProtectionError::Storage),
                    }
                }
                std::fs::File::open(&dirs.wallet_dir)
                    .and_then(|f| f.sync_all())
                    .map_err(|_| KeystoreProtectionError::Storage)
            })
            .await
            .map_err(|_| KeystoreProtectionError::Worker)??;
            Ok::<_, ServiceError>(())
        })
        .await
        .map_err(|_| KeystoreProtectionError::Worker)?
    }
}
