//! Ordinary V2 file migration and default file-write policy. Does not migrate SQLite.
use super::{WalletApplication, derived_store, legacy_initialization, protection_initialization};
use crate::{
    context::Context,
    device_protection::{
        ProtectionContext,
        keystore_format::{
            KeystoreProtection, KeystoreProtectionError, ReadPolicy, has_device_header,
        },
        migration::{MigrationSummary, PreparedMigrationFile, record_scope},
    },
    domain::app::config::ConfigDomain,
    error::service::ServiceError,
};
use std::collections::HashSet;
use wallet_database::repositories::device_protection::{MigrationRepo, MigrationState};
use wallet_tree::file_ops::inventory::RecordIdentity;

impl WalletApplication {
    /// Explicit password-bearing operation; callers finish the existing V1 upgrade
    /// first. The owned task retains the writer gates after caller cancellation.
    pub(crate) async fn migrate_wallet_file_protection(
        ctx: &'static Context,
        _password: &str,
    ) -> Result<MigrationSummary, ServiceError> {
        tokio::spawn(async move {
            let _guard = derived_store::lock().await;
            let api = ctx.api_wallet_pool()?;
            let _api_guard = api.lock_write_with_metric("device_protection_file_migration").await;
            if !matches!(
                ConfigDomain::get_wallet_tree_strategy(ctx).await?,
                wallet_tree::WalletTreeStrategy::V2
            ) {
                return Err(KeystoreProtectionError::LegacyForbidden.into());
            }
            let active = files_active(ctx).await?;
            let mut inventory = legacy_initialization::snapshot(ctx).await?;
            inventory.sort_by(|left, right| left.1.path.cmp(&right.1.path));
            let bindings: Vec<_> = inventory
                .iter()
                .map(|(wallet, record, _)| {
                    let context = match record.identity {
                        RecordIdentity::Root => ProtectionContext::new(wallet, "root-keystore"),
                        RecordIdentity::Derived { account_id } => {
                            ProtectionContext::new(wallet, format!("derived:{account_id}"))
                        }
                        RecordIdentity::Verify => {
                            ProtectionContext::new("application", "password-verify")
                        }
                        _ => return Err(KeystoreProtectionError::LegacyForbidden),
                    };
                    context.map_err(KeystoreProtectionError::from)
                })
                .collect::<Result<_, _>>()?;
            let pool = ctx.core_pool()?;
            let scopes: HashSet<_> = bindings.iter().map(record_scope).collect();
            // Include progress for records with no DB/index entry (e.g. verify).
            // An intentional deletion may leave recreation permission without a file.
            let progress: Vec<String> = sqlx::query_scalar(
                "SELECT key FROM config WHERE key LIKE 'device_protection.file.v1:%'",
            )
            .fetch_all(pool.read_ref())
            .await
            .map_err(|_| state_error())?;
            for key in progress {
                let scope =
                    key.strip_prefix("device_protection.file.v1:").ok_or_else(state_error)?;
                let state = MigrationRepo::get(scope, &pool).await.map_err(|_| state_error())?;
                if !scopes.contains(scope)
                    && !matches!(
                        state,
                        Some(
                            MigrationState::CreationAuthorized | MigrationState::RecreationAllowed
                        )
                    )
                {
                    return Err(KeystoreProtectionError::Storage.into());
                }
            }
            let codec = KeystoreProtection::new(ctx.device_protection().clone());
            // Password authentication is owned by the startup entry point. Validate each
            // encrypted envelope here and retain its source until atomic publication.
            let mut prepared = Vec::with_capacity(inventory.len());
            for ((_, record, _), binding) in inventory.iter().zip(&bindings) {
                let state = MigrationRepo::get(&record_scope(binding), &pool)
                    .await
                    .map_err(|_| state_error())?;
                let required = active
                    || has_device_header(&record.path).await?
                    || matches!(
                        state,
                        Some(
                            MigrationState::DeviceRequired
                                | MigrationState::CreationAuthorized
                                | MigrationState::RecreationAllowed
                        )
                    );
                let file = codec
                    .prepare_file_wrapping(
                        &record.path,
                        binding,
                        if required {
                            ReadPolicy::DeviceRequired
                        } else {
                            ReadPolicy::LegacyMigrationAllowed
                        },
                    )
                    .await?;
                prepared.push(PreparedMigrationFile {
                    path: &record.path,
                    context: binding,
                    prepared: file,
                });
            }
            protection_initialization::initialize_under_gate_after_password_authentication(ctx)
                .await?;
            if inventory.is_empty() {
                mark_active(ctx).await?;
                return Ok(MigrationSummary::default());
            }
            let summary =
                codec.migrate_prepared_files(&pool, prepared).await.map_err(
                    |error| match error {
                        crate::device_protection::migration::TrackedMigrationError::Keystore(
                            error,
                        ) => error.into(),
                        _ => state_error(),
                    },
                )?;
            mark_active(ctx).await?;
            Ok(summary)
        })
        .await
        .map_err(|_| KeystoreProtectionError::Worker)?
    }
}

fn state_error() -> ServiceError {
    ServiceError::Context("Cannot validate wallet file migration progress".into())
}

const FILES_KEY: &str = "device_protection.files";

pub(super) async fn files_active(ctx: &Context) -> Result<bool, ServiceError> {
    let state: Option<(String, i64)> =
        sqlx::query_as("SELECT value, types FROM config WHERE key=?")
            .bind(FILES_KEY)
            .fetch_optional(ctx.core_pool()?.read_ref())
            .await
            .map_err(|_| state_error())?;
    match state {
        None => Ok(false),
        Some((value, 1)) if value == "active-v1" => Ok(true),
        _ => Err(KeystoreProtectionError::InvalidFormat.into()),
    }
}

async fn mark_active(ctx: &Context) -> Result<(), ServiceError> {
    wallet_database::repositories::config::ConfigRepo::upsert(
        FILES_KEY,
        "active-v1",
        Some(1),
        &ctx.core_pool()?,
    )
    .await?;
    Ok(())
}
