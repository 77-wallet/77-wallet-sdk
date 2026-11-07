use super::*;
use std::path::PathBuf;
use wallet_utils::address::AccountIndexMap;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/derived-key-tests")
            .join(format!("{}-{}", std::process::id(), rand::random::<u64>()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn store(&self, address: &str, password: &str, bytes: &[u8]) -> Result<(), crate::Error> {
        ModernIo.store_subkey(
            &AccountIndexMap::from_account_id(0)?,
            address,
            "sol",
            "fixture-path",
            &bytes,
            &self.0,
            password,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn failed_append_preserves_ciphertext_and_metadata() {
    // Arrange: real encrypted keystore and real index, synthetic data only.
    let fixture = Fixture::new();
    fixture.store("first", "fixture-password", b"first fixture").unwrap();
    let snapshot: Vec<_> = std::fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    assert_eq!(snapshot.len(), 2);

    // Act: the existing key cannot be unlocked; no index may be published.
    let result = fixture.store("second", "wrong-password", b"second fixture");

    // Assert: both files remain exactly as before the failed operation.
    assert!(result.is_err());
    assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 2);
    for (path, original) in snapshot {
        assert!(std::fs::read(path).unwrap() == original, "failed append changed storage");
    }
}

#[test]
fn successful_append_keeps_both_keys_and_publishes_index() {
    // Arrange.
    let fixture = Fixture::new();
    fixture.store("first", "fixture-password", b"first fixture").unwrap();

    // Act.
    fixture.store("second", "fixture-password", b"second fixture").unwrap();

    // Assert using the existing read API and the persisted index.
    let index = AccountIndexMap::from_account_id(0).unwrap();
    for (address, expected) in
        [("first", b"first fixture".as_slice()), ("second", b"second fixture".as_slice())]
    {
        let loaded = ModernIo
            .load_subkey(
                Some(&index),
                address,
                "sol",
                "fixture-path",
                &fixture.0,
                "fixture-password",
            )
            .unwrap();
        assert!(loaded == expected);
    }
    let metadata: DerivedMetadata =
        serde_json::from_slice(&std::fs::read(fixture.0.join("derived_meta.json")).unwrap())
            .unwrap();
    let keys = metadata.accounts.get(&0).unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].address, "first");
    assert_eq!(keys[1].address, "second");
}
