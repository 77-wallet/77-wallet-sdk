//! Read-only discovery of existing keystore files. Empty is not provisioning permission.
use crate::{
    Error,
    naming::{FileType, NamingStrategy, v1::LegacyNaming},
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RecordIdentity {
    Root,
    LegacySeed,
    LegacyPhrase,
    LegacyPrivateKey,
    Derived { account_id: u32 },
    LegacyDerived { chain: String, address: String, derivation: String },
    Verify,
}

#[derive(Debug)]
pub struct InventoryEntry {
    pub path: PathBuf,
    pub identity: RecordIdentity,
}

/// Scan one existing wallet directory. Caller supplies its stable wallet address.
/// Covers root/subs, including mixed legacy/modern layouts during upgrade.
/// Unknown entries are errors; they must be assessed rather than silently omitted.
/// Run on a blocking worker and exclude concurrent inventory changes during rollout.
pub fn scan_wallet(wallet_dir: &Path, wallet_address: &str) -> Result<Vec<InventoryEntry>, Error> {
    if wallet_address.is_empty() {
        return Err(Error::FilenameInvalid);
    }
    ensure_directory(wallet_dir)?;
    let mut result = Vec::new();
    let mut identities = HashSet::new();
    for directory in std::fs::read_dir(wallet_dir)? {
        let directory = directory?;
        let name = directory.file_name();
        let name = name.to_str().ok_or(Error::FilenameInvalid)?;
        let root = match name {
            "root" => true,
            "subs" => false,
            _ => return Err(Error::FilenameInvalid),
        };
        let base = directory.path();
        ensure_directory(&base)?;
        for entry in std::fs::read_dir(base)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                return Err(Error::FilenameInvalid);
            }
            let name = entry.file_name();
            let name = name.to_str().ok_or(Error::FilenameInvalid)?;
            if !root && name == "derived_meta.json" {
                continue;
            }
            let identity = classify(name, root, wallet_address)?;
            if !identities.insert(identity.clone()) {
                return Err(Error::FilenameInvalid);
            }
            result.push(InventoryEntry { path: entry.path(), identity });
        }
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

/// The legacy verify file lives at the App root, outside wallet_data/<wallet>.
/// SQLite password_proof/other secrets are intentionally outside this file inventory.
pub fn scan_verify(app_root: &Path) -> Result<Option<InventoryEntry>, Error> {
    ensure_directory(app_root)?;
    let path = app_root.join("verify");
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => {
            Ok(Some(InventoryEntry { path, identity: RecordIdentity::Verify }))
        }
        Ok(_) => Err(Error::FilenameInvalid),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn ensure_directory(path: &Path) -> Result<(), Error> {
    if !std::fs::symlink_metadata(path)?.is_dir() {
        return Err(Error::FilenameInvalid);
    }
    Ok(())
}

fn classify(name: &str, root: bool, wallet_address: &str) -> Result<RecordIdentity, Error> {
    if root && name == "root.keystore" {
        return Ok(RecordIdentity::Root);
    }
    if !root {
        if let Some(index) = name.strip_prefix("key").and_then(|s| s.strip_suffix(".keystore")) {
            let account_id: u32 = index.parse().map_err(|_| Error::FilenameInvalid)?;
            if name != format!("key{account_id}.keystore") {
                return Err(Error::FilenameInvalid);
            }
            return Ok(RecordIdentity::Derived { account_id });
        }
    }
    let meta = LegacyNaming::decode("", name)?;
    if root {
        if meta.address().as_deref() != Some(wallet_address) {
            return Err(Error::FilenameInvalid);
        }
        match meta.file_type() {
            FileType::Seed => Ok(RecordIdentity::LegacySeed),
            FileType::Phrase => Ok(RecordIdentity::LegacyPhrase),
            FileType::PrivateKey => Ok(RecordIdentity::LegacyPrivateKey),
            _ => Err(Error::FilenameInvalid),
        }
    } else {
        if meta.file_type() != &FileType::DerivedData {
            return Err(Error::FilenameInvalid);
        }
        let chain = meta.chain_code().filter(|s| !s.is_empty()).ok_or(Error::FilenameInvalid)?;
        let address = meta.address().filter(|s| !s.is_empty()).ok_or(Error::FilenameInvalid)?;
        let derivation =
            meta.derivation_path().filter(|s| !s.is_empty()).ok_or(Error::FilenameInvalid)?;
        Ok(RecordIdentity::LegacyDerived { chain, address, derivation })
    }
}

#[cfg(test)]
mod tests;
