use super::*;

fn fixture() -> tempfile::TempDir {
    let parent =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/keystore-inventory-tests");
    std::fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}

#[test]
fn scans_all_existing_modern_and_legacy_keystores_without_reading_contents() {
    let dir = fixture();
    let wallet = dir.path().join("wallet");
    std::fs::create_dir_all(wallet.join("root")).unwrap();
    std::fs::create_dir_all(wallet.join("subs")).unwrap();
    for name in ["root.keystore", "fixturewallet-seed", "fixturewallet-phrase", "fixturewallet-pk"]
    {
        std::fs::write(wallet.join("root").join(name), b"opaque fixture").unwrap();
    }
    std::fs::write(wallet.join("subs/key0.keystore"), b"opaque fixture").unwrap();
    std::fs::write(wallet.join("subs/sol-fixtureaddress-m%2F0-pk"), b"opaque fixture").unwrap();
    std::fs::write(wallet.join("subs/derived_meta.json"), b"{}").unwrap();
    std::fs::write(dir.path().join("verify"), b"opaque fixture").unwrap();
    let entries = scan_wallet(&wallet, "fixturewallet").unwrap();
    let identities: std::collections::HashSet<_> =
        entries.iter().map(|entry| entry.identity.clone()).collect();
    assert_eq!(identities.len(), 6);
    assert!(identities.contains(&RecordIdentity::Root));
    assert!(identities.contains(&RecordIdentity::LegacySeed));
    assert!(identities.contains(&RecordIdentity::LegacyPhrase));
    assert!(identities.contains(&RecordIdentity::LegacyPrivateKey));
    assert!(identities.contains(&RecordIdentity::Derived { account_id: 0 }));
    assert!(identities.contains(&RecordIdentity::LegacyDerived {
        chain: "sol".into(),
        address: "fixtureaddress".into(),
        derivation: "m/0".into()
    }));
    assert_eq!(scan_verify(dir.path()).unwrap().unwrap().identity, RecordIdentity::Verify);
    for entry in entries {
        assert!(std::fs::read(entry.path).unwrap() == b"opaque fixture");
    }
}

#[test]
fn unknown_file_and_wrong_wallet_root_are_not_silently_skipped() {
    let dir = fixture();
    std::fs::create_dir(dir.path().join("root")).unwrap();
    let path = dir.path().join("root/forgotten.backup");
    std::fs::write(&path, b"opaque fixture").unwrap();
    assert!(scan_wallet(dir.path(), "fixturewallet").is_err());
    assert!(std::fs::read(&path).unwrap() == b"opaque fixture");
    std::fs::rename(&path, dir.path().join("root/otherwallet-seed")).unwrap();
    assert!(scan_wallet(dir.path(), "fixturewallet").is_err());
}

#[test]
fn empty_directories_do_not_create_data_or_progress() {
    let dir = fixture();
    assert!(scan_wallet(dir.path(), "fixturewallet").unwrap().is_empty());
    assert!(scan_verify(dir.path()).unwrap().is_none());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn symlink_is_rejected_without_following_or_replacing_it() {
    let dir = fixture();
    let external = fixture();
    std::fs::write(external.path().join("source"), b"opaque fixture").unwrap();
    std::fs::create_dir(dir.path().join("root")).unwrap();
    std::os::unix::fs::symlink(
        external.path().join("source"),
        dir.path().join("root/root.keystore"),
    )
    .unwrap();
    assert!(scan_wallet(dir.path(), "fixturewallet").is_err());
    assert!(std::fs::read(external.path().join("source")).unwrap() == b"opaque fixture");
}

#[test]
fn duplicate_decoded_identity_is_rejected_without_changing_files() {
    let dir = fixture();
    let subs = dir.path().join("subs");
    std::fs::create_dir(&subs).unwrap();
    // Distinct filenames decode to the same record even on case-insensitive filesystems.
    let paths =
        [subs.join("sol-fixtureaddress-m%2F0-pk"), subs.join("sol-fixtureaddress-%6D%2F0-pk")];
    for path in &paths {
        std::fs::write(path, b"opaque fixture").unwrap();
    }
    assert!(scan_wallet(dir.path(), "fixturewallet").is_err());
    for path in paths {
        assert_eq!(std::fs::read(path).unwrap(), b"opaque fixture");
    }
}
