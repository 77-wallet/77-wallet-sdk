//! Operational migration progress, not a cryptographically trusted policy anchor.
//! Absence does not authorize legacy reads or new hardware-key provisioning.
use crate::CoreDbPool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationState {
    Pending,
    DeviceRequired,
    /// A brand-new record was authorized before its first file publication.
    CreationAuthorized,
    /// Explicit deletion permits protected recreation only after file/index removal.
    RecreationAllowed,
}

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("Invalid migration scope")]
    InvalidScope,
    #[error("Invalid migration state")]
    InvalidState,
    #[error("Migration has not started")]
    NotStarted,
    #[error("Migration state database operation failed: {0}")]
    Database(#[from] crate::Error),
}

pub struct MigrationRepo;

impl MigrationRepo {
    pub async fn get(
        scope: &str,
        pool: &CoreDbPool,
    ) -> Result<Option<MigrationState>, MigrationError> {
        let key = state_key(scope)?;
        let Some((value, types)) =
            sqlx::query_as::<_, (String, i64)>("SELECT value, types FROM config WHERE key=?")
                .bind(&key)
                .fetch_optional(pool.read_ref())
                .await
                .map_err(db_error)?
        else {
            return Ok(None);
        };
        if types != 1 {
            return Err(MigrationError::InvalidState);
        }
        match value.as_str() {
            "migrating-v1" => Ok(Some(MigrationState::Pending)),
            "device-required-v1" => Ok(Some(MigrationState::DeviceRequired)),
            "creation-authorized-v1" => Ok(Some(MigrationState::CreationAuthorized)),
            "recreation-allowed-v1" => Ok(Some(MigrationState::RecreationAllowed)),
            _ => Err(MigrationError::InvalidState),
        }
    }

    /// Insert initial progress without ever overwriting an existing state.
    /// Scope is a stable non-secret identifier; caller must first assess stored data.
    pub async fn begin(scope: &str, pool: &CoreDbPool) -> Result<MigrationState, MigrationError> {
        let key = state_key(scope)?;
        sqlx::query(
            "INSERT INTO config (key,value,types,created_at,updated_at)
             VALUES (?, 'migrating-v1', 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))
             ON CONFLICT(key) DO NOTHING",
        ).bind(&key).execute(pool.write_ref()).await.map_err(db_error)?;
        Self::get(scope, pool).await?.ok_or(MigrationError::NotStarted)
    }

    /// Persist first-creation intent without overwriting existing record history.
    pub async fn authorize_creation(
        scope: &str,
        pool: &CoreDbPool,
    ) -> Result<MigrationState, MigrationError> {
        let key = state_key(scope)?;
        sqlx::query(
            "INSERT INTO config (key,value,types,created_at,updated_at)
             VALUES (?, 'creation-authorized-v1', 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))
             ON CONFLICT(key) DO NOTHING",
        )
        .bind(&key)
        .execute(pool.write_ref())
        .await
        .map_err(db_error)?;
        match Self::get(scope, pool).await? {
            Some(MigrationState::CreationAuthorized) => Ok(MigrationState::CreationAuthorized),
            _ => Err(MigrationError::NotStarted),
        }
    }

    /// Record an explicit account deletion before removing its ciphertext/index.
    /// This never authorizes legacy writes or device-key provisioning. Callers must
    /// serialize deletion/recreation and require BOTH file and index absence before
    /// creating a protected replacement. Unknown state/version is never reset.
    pub async fn authorize_recreation(
        scope: &str,
        pool: &CoreDbPool,
    ) -> Result<MigrationState, MigrationError> {
        let key = state_key(scope)?;
        sqlx::query(
            "INSERT INTO config (key,value,types,created_at,updated_at)
             VALUES (?, 'recreation-allowed-v1', 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))
             ON CONFLICT(key) DO UPDATE SET value='recreation-allowed-v1', updated_at=excluded.updated_at
             WHERE config.types=1 AND config.value IN ('migrating-v1', 'device-required-v1', 'recreation-allowed-v1')",
        ).bind(&key).execute(pool.write_ref()).await.map_err(db_error)?;
        match Self::get(scope, pool).await? {
            Some(MigrationState::RecreationAllowed) => Ok(MigrationState::RecreationAllowed),
            _ => Err(MigrationError::NotStarted),
        }
    }

    /// Caller must verify every protected record in this scope before completion.
    /// This row alone cannot prove file contents, hardware availability or migration.
    pub async fn complete(
        scope: &str,
        pool: &CoreDbPool,
    ) -> Result<MigrationState, MigrationError> {
        let key = state_key(scope)?;
        sqlx::query(
            "UPDATE config SET value='device-required-v1', updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE key=? AND value IN ('migrating-v1', 'creation-authorized-v1', 'recreation-allowed-v1') AND types=1",
        ).bind(&key).execute(pool.write_ref()).await.map_err(db_error)?;
        match Self::get(scope, pool).await? {
            Some(MigrationState::DeviceRequired) => Ok(MigrationState::DeviceRequired),
            Some(
                MigrationState::Pending
                | MigrationState::CreationAuthorized
                | MigrationState::RecreationAllowed,
            )
            | None => Err(MigrationError::NotStarted),
        }
    }
}

fn state_key(scope: &str) -> Result<String, MigrationError> {
    if scope.is_empty()
        || scope.len() > 256
        || scope.trim() != scope
        || scope.chars().any(char::is_control)
    {
        return Err(MigrationError::InvalidScope);
    }
    Ok(format!("device_protection.file.v1:{scope}"))
}

fn db_error(error: sqlx::Error) -> MigrationError {
    MigrationError::Database(crate::Error::Database(error.into()))
}

#[cfg(test)]
mod tests;
