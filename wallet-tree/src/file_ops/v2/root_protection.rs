use super::ModernIo;
use crate::{
    Error,
    file_ops::{RootData, protected},
};
use secrecy::zeroize::Zeroizing;
use std::{future::Future, path::Path};
use wallet_crypto::KdfAlgorithm;

impl ModernIo {
    /// Compatibility entry: root serialization only; protection and I/O are shared.
    pub async fn store_root_with_protection<F, Fut, E>(
        &self,
        root_dir: &Path,
        root: RootData,
        password: &str,
        algorithm: KdfAlgorithm,
        protect: F,
    ) -> Result<(), E>
    where
        F: FnOnce(Vec<u8>) -> Fut,
        Fut: Future<Output = Result<Vec<u8>, E>>,
        E: From<Error>,
    {
        let plain = Zeroizing::new(
            wallet_utils::serde_func::serde_to_vec(&root).map_err(Error::from).map_err(E::from)?,
        );
        drop(root);
        protected::store(&root_dir.join("root.keystore"), plain, password, algorithm, protect).await
    }

    /// Compatibility entry: selects the existing filename, then uses common I/O.
    pub async fn load_root_with_protection<F, Fut, E>(
        &self,
        root_dir: &Path,
        decode: F,
    ) -> Result<RootData, E>
    where
        F: FnOnce(Vec<u8>) -> Fut,
        Fut: Future<Output = Result<RootData, E>>,
        E: From<Error>,
    {
        protected::load(&root_dir.join("root.keystore"), decode).await
    }
}

#[cfg(test)]
mod tests;
