use crate::{
    context::Context,
    device_protection::keystore_format::{
        KeystoreProtectionError, has_device_header, read_legacy_file,
    },
    error::service::ServiceError,
};
use std::collections::HashSet;
use wallet_database::repositories::device::DeviceRepo;
use wallet_tree::file_ops::inventory::RecordIdentity;

/// V1-to-V2 conversion must finish before device protection starts. Checking only
/// extant file headers would miss a deleted protected file with durable progress.
pub(super) async fn preflight(ctx: &Context) -> Result<(), ServiceError> {
    let pool = ctx.core_pool()?;
    let started: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM config WHERE key LIKE ?)")
        .bind("device_protection.file.v1:%")
        .fetch_one(pool.read_ref())
        .await
        .map_err(|_| ServiceError::Context("Cannot read keystore migration progress".into()))?;
    if started {
        return Err(KeystoreProtectionError::LegacyForbidden.into());
    }

    let dirs = ctx.get_global_dirs().clone();
    let paths = tokio::task::spawn_blocking(
        move || -> Result<Vec<std::path::PathBuf>, wallet_tree::Error> {
            use wallet_tree::file_ops::inventory::{scan_verify, scan_wallet};
            let mut paths = Vec::new();
            // Reject unknown entries/symlinks rather than overlooking protected data.
            for entry in std::fs::read_dir(&dirs.wallet_dir)? {
                let entry = entry?;
                let name = entry.file_name();
                let address = name.to_str().ok_or(wallet_tree::Error::FilenameInvalid)?;
                paths.extend(
                    scan_wallet(&entry.path(), address)?.into_iter().map(|record| record.path),
                );
            }
            if let Some(verify) = scan_verify(&dirs.root_dir)? {
                paths.push(verify.path);
            }
            Ok(paths)
        },
    )
    .await
    .map_err(|_| KeystoreProtectionError::Worker)??;
    for path in paths {
        if has_device_header(&path).await? {
            return Err(KeystoreProtectionError::LegacyForbidden.into());
        }
    }
    Ok(())
}

pub(super) async fn has_v1_wallets(ctx: &Context) -> Result<bool, ServiceError> {
    let wallet_dir = ctx.get_global_dirs().wallet_dir.clone();
    tokio::task::spawn_blocking(move || {
        Ok::<_, ServiceError>(
            wallet_tree::WalletTreeStrategy::V1
                .get_wallet_tree(&wallet_dir)?
                .iter()
                .next()
                .is_some(),
        )
    })
    .await
    .map_err(|_| KeystoreProtectionError::Worker)?
}

/// Finish a V1-to-V2 conversion after the V2 strategy was durably published but legacy
/// cleanup or the final marker update was interrupted. Every referenced V2 replacement is
/// password-verified before any remaining V1 source is removed.
pub(super) async fn resume_published_v2(ctx: &Context, password: &str) -> Result<(), ServiceError> {
    let _guard = super::derived_store::lock().await;
    // V2 files authenticate ordinary wallets below. Proof/API packages cover the
    // API-only case and prevent accepting a verify file created with a wrong password.
    super::legacy_initialization::verify_non_file_storage(ctx, password).await?;
    let inventory = super::legacy_initialization::snapshot(ctx).await?;
    let pool = ctx.core_pool()?;
    let wallets: Vec<String> = sqlx::query_scalar("SELECT address FROM wallet")
        .fetch_all(pool.read_ref())
        .await
        .map_err(|_| state_error())?;
    let accounts: Vec<(String, u32)> =
        sqlx::query_as("SELECT wallet_address, account_id FROM account")
            .fetch_all(pool.read_ref())
            .await
            .map_err(|_| state_error())?;
    let roots: HashSet<_> = inventory
        .iter()
        .filter_map(|(wallet, record, _)| {
            (record.identity == RecordIdentity::Root).then_some(wallet.clone())
        })
        .collect();
    let derived: HashSet<_> = inventory
        .iter()
        .filter_map(|(wallet, record, _)| match record.identity {
            RecordIdentity::Derived { account_id } => Some((wallet.clone(), account_id)),
            _ => None,
        })
        .collect();
    if wallets.iter().any(|wallet| !roots.contains(wallet))
        || accounts.iter().any(|account| !derived.contains(account))
        || !inventory.iter().any(|(_, record, _)| record.identity == RecordIdentity::Verify)
    {
        return Err(KeystoreProtectionError::Storage.into());
    }

    let mut obsolete = Vec::new();
    for (_, record, expected) in &inventory {
        match record.identity {
            RecordIdentity::Root => {
                let plain = read_legacy_file(&record.path, password).await?;
                let _: wallet_tree::file_ops::RootData = serde_json::from_slice(&plain)
                    .map_err(|_| KeystoreProtectionError::InvalidFormat)?;
            }
            RecordIdentity::Derived { .. } => {
                let plain = read_legacy_file(&record.path, password).await?;
                super::legacy_initialization::verify_payload(&record.identity, &plain, expected)?;
            }
            RecordIdentity::Verify => {
                let _ = read_legacy_file(&record.path, password).await?;
            }
            RecordIdentity::LegacySeed
            | RecordIdentity::LegacyPhrase
            | RecordIdentity::LegacyPrivateKey
            | RecordIdentity::LegacyDerived { .. } => obsolete.push(record.path.clone()),
            _ => return Err(KeystoreProtectionError::InvalidFormat.into()),
        }
    }
    for path in obsolete {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(KeystoreProtectionError::Storage.into()),
        }
    }
    DeviceRepo::update_password(pool, ctx.get_sn(), None).await?;
    Ok(())
}

fn state_error() -> ServiceError {
    ServiceError::Context("Cannot resume wallet algorithm upgrade".into())
}
