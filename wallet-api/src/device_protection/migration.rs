//! Resumable file-level migration with operational SQLite progress.
//! Callers still own authoritative inventory, safe key provisioning and rollout.
use super::{
    ProtectionContext,
    keystore_format::{KeystoreProtection, KeystoreProtectionError, PreparedKeystore, ReadPolicy},
};
use sha2::Digest;
use std::{collections::HashSet, path::Path};
use wallet_database::{
    CoreDbPool,
    repositories::device_protection::{MigrationRepo, MigrationState},
};

pub struct MigrationFile<'a> {
    pub path: &'a Path,
    /// Expected stable identity, never taken from an untrusted file header.
    pub context: &'a ProtectionContext,
}

pub(crate) struct PreparedMigrationFile<'a> {
    pub path: &'a Path,
    pub context: &'a ProtectionContext,
    pub prepared: PreparedKeystore,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MigrationSummary {
    pub converted: usize,
    pub already_protected: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum TrackedMigrationError {
    #[error("Invalid or unavailable migration inventory")]
    InvalidInventory,
    #[error("Migration progress could not be read or recorded")]
    State,
    #[error(transparent)]
    Keystore(#[from] KeystoreProtectionError),
}

impl KeystoreProtection {
    pub(crate) async fn migrate_prepared_files(
        &self,
        pool: &CoreDbPool,
        files: impl IntoIterator<Item = PreparedMigrationFile<'_>>,
    ) -> Result<MigrationSummary, TrackedMigrationError> {
        let files: Vec<_> = files.into_iter().collect();
        validate_prepared_inventory(&files).await?;
        let mut states = Vec::with_capacity(files.len());
        for file in &files {
            states.push(
                MigrationRepo::begin(&record_scope(file.context), pool)
                    .await
                    .map_err(|_| TrackedMigrationError::State)?,
            );
        }
        let mut summary = MigrationSummary::default();
        for (file, state) in files.into_iter().zip(states) {
            let scope = record_scope(file.context);
            let converted =
                self.publish_prepared_file(file.path, file.context, file.prepared).await?;
            if state != MigrationState::DeviceRequired {
                MigrationRepo::complete(&scope, pool)
                    .await
                    .map_err(|_| TrackedMigrationError::State)?;
            }
            if converted {
                summary.converted += 1;
            } else {
                summary.already_protected += 1;
            }
        }
        Ok(summary)
    }

    /// Migrate and record each file in an authoritative inventory. Success covers
    /// only this inventory, not omitted files or SQLite secrets. Caller must exclude
    /// legacy writers and inventory changes throughout this operation.
    /// Absence of progress never grants legacy access: caller policy is explicit.
    pub async fn migrate_files(
        &self,
        pool: &CoreDbPool,
        files: &[MigrationFile<'_>],
        password: &str,
        policy: ReadPolicy,
    ) -> Result<MigrationSummary, TrackedMigrationError> {
        validate_inventory(files).await?;
        let mut summary = MigrationSummary::default();
        for file in files {
            let scope = record_scope(file.context);
            let state = MigrationRepo::begin(&scope, pool)
                .await
                .map_err(|_| TrackedMigrationError::State)?;
            let effective_policy = match (state, policy) {
                (
                    MigrationState::DeviceRequired
                    | MigrationState::CreationAuthorized
                    | MigrationState::RecreationAllowed,
                    _,
                )
                | (_, ReadPolicy::DeviceRequired) => ReadPolicy::DeviceRequired,
                _ => ReadPolicy::LegacyMigrationAllowed,
            };
            let converted =
                self.migrate_file(file.path, file.context, password, effective_policy).await?;
            // File publication succeeded (or current ciphertext was verified). A DB
            // failure leaves a retryable Pending record, never an early completion.
            if state != MigrationState::DeviceRequired {
                MigrationRepo::complete(&scope, pool)
                    .await
                    .map_err(|_| TrackedMigrationError::State)?;
            }
            if converted {
                summary.converted += 1;
            } else {
                summary.already_protected += 1;
            }
        }
        Ok(summary)
    }
}

async fn validate_prepared_inventory(
    files: &[PreparedMigrationFile<'_>],
) -> Result<(), TrackedMigrationError> {
    if files.is_empty() {
        return Err(TrackedMigrationError::InvalidInventory);
    }
    let mut identities = HashSet::new();
    for file in files {
        if !identities.insert(record_scope(file.context)) {
            return Err(TrackedMigrationError::InvalidInventory);
        }
    }
    let paths: Vec<_> = files.iter().map(|file| file.path.to_owned()).collect();
    validate_paths(paths).await
}

pub(crate) fn record_scope(context: &ProtectionContext) -> String {
    hex::encode(sha2::Sha256::digest(context.authenticated_data()))
}

async fn validate_inventory(files: &[MigrationFile<'_>]) -> Result<(), TrackedMigrationError> {
    if files.is_empty() {
        return Err(TrackedMigrationError::InvalidInventory);
    }
    let mut identities = HashSet::new();
    for file in files {
        if !identities.insert(record_scope(file.context)) {
            return Err(TrackedMigrationError::InvalidInventory);
        }
    }
    let paths: Vec<_> = files.iter().map(|file| file.path.to_owned()).collect();
    validate_paths(paths).await
}

async fn validate_paths(paths: Vec<std::path::PathBuf>) -> Result<(), TrackedMigrationError> {
    tokio::task::spawn_blocking(move || {
        let mut unique = HashSet::new();
        for path in paths {
            // Replacing a symlink could leave its legacy target unprotected.
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|_| TrackedMigrationError::InvalidInventory)?;
            if !metadata.is_file()
                || !unique.insert(
                    path.canonicalize().map_err(|_| TrackedMigrationError::InvalidInventory)?,
                )
            {
                return Err(TrackedMigrationError::InvalidInventory);
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| TrackedMigrationError::InvalidInventory)?
}

#[cfg(test)]
mod tests;
