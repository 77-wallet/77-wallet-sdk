use super::{
    WalletApplication, derived_store,
    unlock_timing::{self, Phase},
};
use crate::{
    context::Context,
    device_protection::{ProtectionContext, keystore_format::KeystoreProtectionError},
    error::service::ServiceError,
};
use std::time::Instant;
use wallet_database::repositories::config::ConfigRepo;

const KEY: &str = "device_protection.key";
const PENDING: &str = "provisioning-v1";
const READY: &str = "ready-v1";

impl WalletApplication {
    /// Explicit first-install initialization, not a migration/recovery shortcut.
    /// Does not enable storage writes. The startup integration must call this
    /// before accepting wallet operations; old storage needs a separate migration.
    pub(crate) async fn initialize_device_protection(
        ctx: &'static Context,
    ) -> Result<(), ServiceError> {
        Self::initialize_device_protection_inner(ctx, None).await
    }

    /// Verify legacy data before first provisioning. READY only checks key usability;
    /// this is not a password-validation API and does not migrate any records.
    pub(crate) async fn initialize_device_protection_for_legacy(
        ctx: &'static Context,
        password: &str,
    ) -> Result<(), ServiceError> {
        Self::initialize_device_protection_inner(
            ctx,
            Some(zeroize::Zeroizing::new(password.to_owned())),
        )
        .await
    }

    async fn initialize_device_protection_inner(
        ctx: &'static Context,
        legacy_password: Option<zeroize::Zeroizing<String>>,
    ) -> Result<(), ServiceError> {
        let dispatched_at = Instant::now();
        let parent_task_id = tokio::task::try_id();
        tokio::spawn(async move {
            unlock_timing::task_started("device_key_initialization", parent_task_id, dispatched_at);
            let _total = Phase::start("initialization_total");
            let file_wait = Phase::start("file_lock_wait");
            let _guard = derived_store::lock().await;
            drop(file_wait);
            // READY only probes the existing native key. Keep file lifecycle
            // exclusion, but do not queue login behind unrelated API DB writers.
            let state = {
                let _timing = Phase::start("device_key_state_read");
                key_state(ctx).await?
            };
            if state == Some(KeyState::Ready) {
                return probe_key_usability(ctx).await;
            }
            let api_pool = ctx.api_wallet_pool()?;
            let api_wait = Phase::start("api_writer_wait");
            let _api_write_guard =
                api_pool.lock_write_with_metric("device_protection_initialization").await;
            drop(api_wait);
            initialize_under_gate(ctx, legacy_password.as_deref().map(|p| p.as_str())).await
        })
        .await
        .map_err(|_| KeystoreProtectionError::Worker)?
    }
}

async fn require_empty_storage(ctx: &Context) -> Result<(), ServiceError> {
    let core = ctx.core_pool()?;
    let occupied: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM wallet) OR EXISTS(SELECT 1 FROM account) OR EXISTS(SELECT 1 FROM device WHERE password_proof IS NOT NULL) OR EXISTS(SELECT 1 FROM config WHERE key LIKE 'device_protection.file.v1:%')",
    ).fetch_one(core.read_ref()).await
        .map_err(|_| ServiceError::Context("Cannot inspect wallet initialization state".into()))?;
    let api = ctx.api_wallet_pool()?;
    let api_occupied: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM api_wallet) OR EXISTS(SELECT 1 FROM api_account)",
    )
    .fetch_one(api.read_ref())
    .await
    .map_err(|_| ServiceError::Context("Cannot inspect API wallet initialization state".into()))?;
    if occupied || api_occupied {
        return Err(KeystoreProtectionError::LegacyForbidden.into());
    }
    let dirs = ctx.get_global_dirs().clone();
    tokio::task::spawn_blocking(move || -> Result<(), ServiceError> {
        use wallet_tree::file_ops::inventory::{scan_verify, scan_wallet};
        if scan_verify(&dirs.root_dir)?.is_some() {
            return Err(KeystoreProtectionError::LegacyForbidden.into());
        }
        for entry in
            std::fs::read_dir(&dirs.wallet_dir).map_err(|_| KeystoreProtectionError::Storage)?
        {
            let entry = entry.map_err(|_| KeystoreProtectionError::Storage)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| KeystoreProtectionError::InvalidFormat)?;
            if !scan_wallet(&entry.path(), &name)?.is_empty() {
                return Err(KeystoreProtectionError::LegacyForbidden.into());
            }
            // A stale public index is evidence of previous accounts, even if
            // every private-key file has disappeared. Do not infer fresh storage.
            if entry
                .path()
                .join("subs/derived_meta.json")
                .try_exists()
                .map_err(|_| KeystoreProtectionError::Storage)?
            {
                return Err(KeystoreProtectionError::LegacyForbidden.into());
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| KeystoreProtectionError::Worker)?
}

/// Caller owns both application file and API database writer gates.
pub(super) async fn initialize_under_gate(
    ctx: &Context,
    legacy_password: Option<&str>,
) -> Result<(), ServiceError> {
    initialize_under_gate_inner(ctx, legacy_password, false).await
}

/// Caller has already authenticated the wallet password at the application entry point and
/// prepared the complete encrypted-file inventory while holding both storage gates.
pub(super) async fn initialize_under_gate_after_password_authentication(
    ctx: &Context,
) -> Result<(), ServiceError> {
    initialize_under_gate_inner(ctx, None, true).await
}

async fn initialize_under_gate_inner(
    ctx: &Context,
    legacy_password: Option<&str>,
    password_authenticated: bool,
) -> Result<(), ServiceError> {
    let pool = ctx.core_pool()?;
    let ready = key_state(ctx).await? == Some(KeyState::Ready);
    if !ready {
        if super::protection_rollout::files_active(ctx).await? {
            return Err(KeystoreProtectionError::LegacyForbidden.into());
        }
        if password_authenticated {
            // The startup entry point already verified password_proof (or its database
            // fallback). Repeating that KDF here would add no authentication value.
        } else if let Some(password) = legacy_password {
            super::legacy_initialization::verify_storage(ctx, password).await?;
        } else {
            require_empty_storage(ctx).await?;
        }
        ConfigRepo::upsert(KEY, PENDING, Some(1), &pool).await?;
        ctx.device_protection()
            .provision_for_new_storage()
            .await
            .map_err(KeystoreProtectionError::from)?;
    }
    probe_key_usability(ctx).await?;
    if !ready {
        ConfigRepo::upsert(KEY, READY, Some(1), &pool).await?;
    }
    Ok(())
}

/// Check usability without provisioning or changing storage state. Callers keep
/// the file gate; first provisioning/migration also retains the API writer gate.
async fn probe_key_usability(ctx: &Context) -> Result<(), ServiceError> {
    let _total = Phase::start("device_key_probe");
    // READY retries deliberately never provision, including after KeyMissing.
    // This non-secret challenge checks usability; it is not attestation and
    // does not prove that any existing wallet ciphertext can be recovered.
    use rand::RngCore;
    let mut challenge = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut challenge);
    let binding = ProtectionContext::new("device-protection", "initialization-probe-v1")
        .map_err(KeystoreProtectionError::from)?;
    let blob = {
        let _timing = Phase::start("native_protect");
        ctx.device_protection()
            .protect(&binding, &challenge)
            .await
            .map_err(KeystoreProtectionError::from)?
    };
    let opened = {
        let _timing = Phase::start("native_unprotect");
        ctx.device_protection()
            .unprotect(&binding, &blob)
            .await
            .map_err(KeystoreProtectionError::from)?
    };
    if opened != challenge {
        return Err(KeystoreProtectionError::InvalidFormat.into());
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyState {
    Pending,
    Ready,
}

pub(super) async fn key_state(ctx: &Context) -> Result<Option<KeyState>, ServiceError> {
    let state: Option<(String, i64)> =
        sqlx::query_as("SELECT value, types FROM config WHERE key=?")
            .bind(KEY)
            .fetch_optional(ctx.core_pool()?.read_ref())
            .await
            .map_err(|_| {
                ServiceError::Context("Cannot read device key initialization state".into())
            })?;
    match state {
        None => Ok(None),
        Some((value, 1)) if value == PENDING => Ok(Some(KeyState::Pending)),
        Some((value, 1)) if value == READY => Ok(Some(KeyState::Ready)),
        _ => Err(KeystoreProtectionError::InvalidFormat.into()),
    }
}
