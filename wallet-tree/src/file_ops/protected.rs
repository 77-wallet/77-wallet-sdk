//! Shared byte-level keystore I/O. All record types use the same protection boundary.
use std::{
    collections::HashMap,
    future::Future,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

use secrecy::zeroize::Zeroizing;
use wallet_crypto::{EncryptedJsonGenerator, KdfAlgorithm, KeystoreJsonGenerator};

use crate::Error;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

const MAX_INNER: usize = 1024 * 1024;
const MAX_STORED: usize = MAX_INNER + 128 + 4;

/// Password-encrypt opaque keystore contents, then await outer protection and
/// atomically publish at the existing path. Does not inspect root/derived/verify data.
/// The callback must bind the ciphertext to the expected record and verify protection.
/// Caller owns provisioning/authorization. Common store/rewrite calls are serialized
/// per canonical parent directory in this process; legacy writers and other processes are not covered.
/// The callback must not await another store/rewrite in the same directory.
pub async fn store<F, Fut, E>(
    path: &Path,
    plain: Zeroizing<Vec<u8>>,
    password: &str,
    algorithm: KdfAlgorithm,
    protect: F,
) -> Result<(), E>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, E>>,
    E: From<Error>,
{
    let guard = lock_directory(path).await.map_err(E::from)?;
    let password = Zeroizing::new(password.as_bytes().to_vec());
    let inner = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, Error> {
        if plain.len() > MAX_INNER {
            return Err(Error::InvalidKeyFormat);
        }
        // Same generator as KeystoreBuilder::save; only the disk write is delayed.
        let encrypted = KeystoreJsonGenerator::new(rand::thread_rng(), algorithm)
            .generate(&password, &plain)?;
        let inner = wallet_utils::serde_func::serde_to_vec(&encrypted)?;
        if inner.len() > MAX_INNER {
            return Err(Error::InvalidKeyFormat);
        }
        Ok(inner)
    })
    .await
    .map_err(|_| E::from(Error::KeystoreWorker))?
    .map_err(E::from)?;

    let stored = protect(inner).await?;
    publish(path, stored, guard).await.map_err(E::from)
}

// The guard is owned by the publication worker, including after waiter cancellation.
async fn publish(path: &Path, stored: Vec<u8>, guard: OwnedMutexGuard<()>) -> Result<(), Error> {
    if stored.is_empty() || stored.len() > MAX_STORED {
        return Err(Error::InvalidKeyFormat);
    }
    let path = path.to_owned();
    let parent = path.parent().ok_or(Error::InvalidKeyFormat)?;
    let parent = if parent.as_os_str().is_empty() { Path::new(".") } else { parent }.to_owned();
    tokio::task::spawn_blocking(move || -> Result<(), Error> {
        let _guard = guard;
        // Only the outer ciphertext reaches disk, including this temporary file.
        let mut pending = tempfile::NamedTempFile::new_in(&parent)?;
        pending.write_all(&stored)?;
        pending.as_file().sync_all()?;
        pending.seek(SeekFrom::Start(0))?;
        let mut verified = Vec::new();
        pending.read_to_end(&mut verified)?;
        if verified != stored {
            return Err(Error::InvalidKeyFormat);
        }
        pending.persist(&path).map_err(|error| Error::KeystoreIo(error.error))?;
        // A failure here is an uncertain durability outcome, not a rollback.
        #[cfg(unix)]
        std::fs::File::open(&parent)?.sync_all()?;
        Ok(())
    })
    .await
    .map_err(|_| Error::KeystoreWorker)?
}

/// Bounded binary read followed by the caller's asynchronous decoder. The decoder
/// owns device/version/password policy and the result type; no automatic migration.
pub async fn load<T, F, Fut, E>(path: &Path, decode: F) -> Result<T, E>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: From<Error>,
{
    let path = path.to_owned();
    let stored = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, Error> {
        let file = std::fs::File::open(path)?;
        let mut stored = Vec::new();
        file.take(MAX_STORED as u64 + 1).read_to_end(&mut stored)?;
        if stored.is_empty() || stored.len() > MAX_STORED {
            return Err(Error::InvalidKeyFormat);
        }
        Ok(stored)
    })
    .await
    .map_err(|_| E::from(Error::KeystoreWorker))?
    .map_err(E::from)?;
    decode(stored).await
}

/// Replace existing bytes only when the caller supplies a validated replacement.
/// The callback owns format/password verification; None means no write is needed.
/// Callbacks must not reenter store/rewrite for the same directory.
/// Does not advance durable migration state. Exclude legacy/external writers before use.
pub async fn rewrite<F, Fut, E>(path: &Path, transform: F) -> Result<bool, E>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: Future<Output = Result<Option<Vec<u8>>, E>>,
    E: From<Error>,
{
    let guard = lock_directory(path).await.map_err(E::from)?;
    let stored = load(path, |bytes| async { Ok::<_, Error>(bytes) }).await.map_err(E::from)?;
    match transform(stored).await? {
        Some(replacement) => {
            publish(path, replacement, guard).await.map_err(E::from)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

async fn lock_directory(path: &Path) -> Result<OwnedMutexGuard<()>, Error> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<AsyncMutex<()>>>>> = OnceLock::new();
    let path = path.to_owned();
    let key = tokio::task::spawn_blocking(move || -> Result<PathBuf, Error> {
        path.file_name().ok_or(Error::InvalidKeyFormat)?;
        let parent = path.parent().ok_or(Error::InvalidKeyFormat)?;
        let parent = if parent.as_os_str().is_empty() { Path::new(".") } else { parent };
        Ok(parent.canonicalize()?)
    })
    .await
    .map_err(|_| Error::KeystoreWorker)??;
    let lock = {
        let mut locks = LOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| Error::KeystoreWorker)?;
        locks.retain(|_, value| value.strong_count() != 0);
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            lock
        } else {
            let lock = Arc::new(AsyncMutex::new(()));
            locks.insert(key, Arc::downgrade(&lock));
            lock
        }
    };
    Ok(lock.lock_owned().await)
}

#[cfg(test)]
#[path = "protected_tests.rs"]
mod tests;
