use crate::{
    context::Context,
    device_protection::keystore_format::{
        KeystoreProtectionError, read_legacy_file, verify_legacy_bytes,
    },
    domain::api_wallet::wallet::ApiWalletDomain,
    error::service::ServiceError,
};
use std::collections::BTreeMap;
use wallet_tree::file_ops::inventory::{InventoryEntry, RecordIdentity, scan_verify, scan_wallet};

/// Called with the application storage gate held. First-use provisioning is
/// allowed only after every discovered record is positively read as legacy.
pub(super) async fn verify_storage(ctx: &Context, password: &str) -> Result<(), ServiceError> {
    reject_migration_history(ctx).await?;
    let paths = snapshot(ctx).await?;
    for (_, record, expected) in paths {
        let plain = read_legacy_file(&record.path, password).await?;
        verify_payload(&record.identity, &plain, &expected)?;
    }
    verify_non_file_storage(ctx, password).await
}

async fn reject_migration_history(ctx: &Context) -> Result<(), ServiceError> {
    let core = ctx.core_pool()?;
    let migrated: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM config WHERE key LIKE 'device_protection.file.v1:%')",
    )
    .fetch_one(core.read_ref())
    .await
    .map_err(|_| state_error())?;
    if migrated {
        return Err(KeystoreProtectionError::LegacyForbidden.into());
    }
    Ok(())
}

/// File payloads have already been password-verified by the rollout preparation.
/// Check every remaining password-bound store before device-key provisioning.
pub(super) async fn verify_non_file_storage(
    ctx: &Context,
    password: &str,
) -> Result<(), ServiceError> {
    reject_migration_history(ctx).await?;
    let core = ctx.core_pool()?;
    let proofs: Vec<String> =
        sqlx::query_scalar("SELECT password_proof FROM device WHERE password_proof IS NOT NULL")
            .fetch_all(core.read_ref())
            .await
            .map_err(|_| state_error())?;
    for proof in proofs {
        verify_legacy_bytes(proof.as_bytes(), password).await?;
    }

    let api = ctx.api_wallet_pool()?;
    let orphaned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM api_account a WHERE NOT EXISTS(SELECT 1 FROM api_wallet w WHERE w.address=a.wallet_address))")
        .fetch_one(api.read_ref()).await.map_err(|_| state_error())?;
    if orphaned {
        return Err(KeystoreProtectionError::Storage.into());
    }
    let packages: Vec<(Vec<u8>, Vec<u8>)> = sqlx::query_as("SELECT phrase, seed FROM api_wallet")
        .fetch_all(api.read_ref())
        .await
        .map_err(|_| state_error())?;
    for (phrase, seed) in packages {
        if phrase.starts_with(b"WDK") || seed.starts_with(b"WDK") {
            return Err(KeystoreProtectionError::LegacyForbidden.into());
        }
        // Existing package codecs perform the password checks; do not invent a
        // second decoder. Drop zeroizing plaintext immediately, never persist it.
        let _phrase = zeroize::Zeroizing::new(
            ApiWalletDomain::decrypt_phrase(password, &phrase)
                .await
                .map_err(|_| KeystoreProtectionError::PasswordDecryption)?,
        );
        let _seed = zeroize::Zeroizing::new(
            ApiWalletDomain::decrypt_seed(password, &seed)
                .await
                .map_err(|_| KeystoreProtectionError::PasswordDecryption)?,
        );
    }
    Ok(())
}

fn require_root(records: &[InventoryEntry]) -> Result<(), ServiceError> {
    let has = |identity| records.iter().any(|record| record.identity == identity);
    if has(RecordIdentity::Root)
        || (has(RecordIdentity::LegacySeed) && has(RecordIdentity::LegacyPhrase))
    {
        Ok(())
    } else {
        Err(KeystoreProtectionError::Storage.into())
    }
}

fn state_error() -> ServiceError {
    ServiceError::Context("Cannot inspect legacy wallet storage".into())
}

/// Caller holds the application storage gate; reused by legacy preflight and resume.
pub(super) async fn snapshot(
    ctx: &Context,
) -> Result<Vec<(String, InventoryEntry, Vec<wallet_tree::KeyMeta>)>, ServiceError> {
    let core = ctx.core_pool()?;
    let wallets: Vec<String> = sqlx::query_scalar("SELECT address FROM wallet")
        .fetch_all(core.read_ref())
        .await
        .map_err(|_| state_error())?;
    let accounts: Vec<(String, u32, String, String, String)> = sqlx::query_as(
        "SELECT wallet_address, account_id, chain_code, address, derivation_path FROM account",
    )
    .fetch_all(core.read_ref())
    .await
    .map_err(|_| state_error())?;
    let dirs = ctx.get_global_dirs().clone();
    tokio::task::spawn_blocking(move || -> Result<_, ServiceError> {
        let mut expected = BTreeMap::<(String, u32), Vec<wallet_tree::KeyMeta>>::new();
        let mut inventory = BTreeMap::<String, Vec<InventoryEntry>>::new();
        for entry in
            std::fs::read_dir(&dirs.wallet_dir).map_err(|_| KeystoreProtectionError::Storage)?
        {
            let entry = entry.map_err(|_| KeystoreProtectionError::Storage)?;
            let address = entry
                .file_name()
                .into_string()
                .map_err(|_| KeystoreProtectionError::InvalidFormat)?;
            let records = scan_wallet(&entry.path(), &address)?;
            if !records.is_empty() {
                require_root(&records)?;
            }
            match std::fs::read(entry.path().join("subs/derived_meta.json")) {
                Ok(bytes) => {
                    let index: super::derived_store::Metadata = serde_json::from_slice(&bytes)
                        .map_err(|_| KeystoreProtectionError::InvalidFormat)?;
                    for (id, keys) in index.accounts {
                        if !records.iter().any(|record| {
                            record.identity == (RecordIdentity::Derived { account_id: id })
                        }) {
                            return Err(KeystoreProtectionError::Storage.into());
                        }
                        expected.entry((address.clone(), id)).or_default().extend(keys);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(KeystoreProtectionError::Storage.into()),
            }
            inventory.insert(address, records);
        }
        for wallet in wallets {
            require_root(inventory.get(&wallet).ok_or(KeystoreProtectionError::Storage)?)?;
        }
        for (wallet, id, chain, address, derivation) in accounts {
            let records = inventory.get(&wallet).ok_or(KeystoreProtectionError::Storage)?;
            require_root(records)?;
            if records
                .iter()
                .any(|record| record.identity == (RecordIdentity::Derived { account_id: id }))
            {
                expected.entry((wallet.clone(), id)).or_default().push(wallet_tree::KeyMeta {
                    chain_code: chain.clone(),
                    address: address.clone(),
                    derivation_path: derivation.clone(),
                });
            }
            if !records.iter().any(|record| match &record.identity {
                RecordIdentity::Derived { account_id } => *account_id == id,
                RecordIdentity::LegacyDerived { chain: c, address: a, derivation: d } => {
                    c == &chain && a == &address && d == &derivation
                }
                _ => false,
            }) {
                return Err(KeystoreProtectionError::Storage.into());
            }
        }
        let mut paths = Vec::new();
        for (wallet, records) in inventory {
            for record in records {
                let keys = match record.identity {
                    RecordIdentity::Derived { account_id } => {
                        expected.remove(&(wallet.clone(), account_id)).unwrap_or_default()
                    }
                    _ => Vec::new(),
                };
                paths.push((wallet.clone(), record, keys));
            }
        }
        if let Some(verify) = scan_verify(&dirs.root_dir)? {
            paths.push((String::new(), verify, Vec::new()));
        }
        Ok(paths)
    })
    .await
    .map_err(|_| KeystoreProtectionError::Worker)?
}

pub(super) fn verify_payload(
    identity: &RecordIdentity,
    plain: &[u8],
    expected: &[wallet_tree::KeyMeta],
) -> Result<(), ServiceError> {
    if matches!(identity, RecordIdentity::Derived { .. }) {
        let mut keys = super::decode_account_data(plain)?;
        let complete =
            expected.iter().all(|meta| keys.get(meta).is_some_and(|key| !key.is_empty()));
        for key in keys.values_mut() {
            zeroize::Zeroize::zeroize(key);
        }
        if !complete {
            return Err(KeystoreProtectionError::Storage.into());
        }
    }
    Ok(())
}
