use crate::file_ops::{IoStrategy, RootData, v2::ModernIo};
use wallet_crypto::{EncryptedJsonDecryptor, KdfAlgorithm};

fn fixture() -> tempfile::TempDir {
    let parent =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/root-protection-tests");
    std::fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}

#[tokio::test]
async fn root_roundtrip_uses_existing_password_format_and_path() {
    let dir = fixture();
    let root = RootData::new("synthetic root fixture", b"synthetic seed");
    ModernIo
        .store_root_with_protection(
            dir.path(),
            root,
            "fixture-password",
            KdfAlgorithm::Scrypt,
            |inner| async move {
                tokio::task::yield_now().await;
                // This marker is a test transform, not hardware encryption.
                let mut stored = b"test-envelope:".to_vec();
                stored.extend(inner);
                Ok::<_, crate::Error>(stored)
            },
        )
        .await
        .unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert!(
        std::fs::read(dir.path().join("root.keystore")).unwrap().starts_with(b"test-envelope:")
    );
    let loaded = ModernIo
        .load_root_with_protection(dir.path(), |stored| async move {
            tokio::task::yield_now().await;
            let inner =
                std::str::from_utf8(stored.strip_prefix(b"test-envelope:").unwrap()).unwrap();
            let plain = wallet_crypto::KeystoreJsonDecryptor.decrypt(b"fixture-password", inner)?;
            Ok::<RootData, crate::Error>(wallet_utils::serde_func::serde_from_slice(&plain)?)
        })
        .await
        .unwrap();
    assert!(loaded.phrase() == "synthetic root fixture");
    assert!(loaded.seed() == b"synthetic seed");
}

#[tokio::test]
async fn failed_protection_keeps_existing_root_and_creates_no_files() {
    let dir = fixture();
    ModernIo
        .store_root(
            "fixture",
            b"old synthetic seed",
            "old synthetic phrase",
            &dir.path(),
            "fixture-password",
            KdfAlgorithm::Scrypt,
        )
        .unwrap();
    let original = std::fs::read(dir.path().join("root.keystore")).unwrap();
    let result = ModernIo
        .store_root_with_protection(
            dir.path(),
            RootData::new("new synthetic phrase", b"new synthetic seed"),
            "fixture-password",
            KdfAlgorithm::Scrypt,
            |_| async { Err::<Vec<u8>, _>(crate::Error::InvalidKeyFormat) },
        )
        .await;
    assert!(result.is_err());
    assert!(std::fs::read(dir.path().join("root.keystore")).unwrap() == original);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn failed_decoder_never_changes_existing_root() {
    let dir = fixture();
    let bytes = b"test-envelope:synthetic bytes";
    std::fs::write(dir.path().join("root.keystore"), bytes).unwrap();
    let result = ModernIo
        .load_root_with_protection(dir.path(), |_| async {
            Err::<RootData, _>(crate::Error::InvalidKeyFormat)
        })
        .await;
    assert!(result.is_err());
    assert!(std::fs::read(dir.path().join("root.keystore")).unwrap() == bytes);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn oversized_file_is_rejected_before_decoder() {
    let dir = fixture();
    let file = std::fs::File::create(dir.path().join("root.keystore")).unwrap();
    file.set_len(2 * 1024 * 1024).unwrap();
    let result = ModernIo
        .load_root_with_protection(dir.path(), |_| async {
            panic!("oversized data reached decoder");
            #[allow(unreachable_code)]
            Ok::<RootData, crate::Error>(RootData::new("", &[]))
        })
        .await;
    assert!(result.is_err());
    assert_eq!(file.metadata().unwrap().len(), 2 * 1024 * 1024);
}

#[tokio::test]
async fn empty_protection_result_is_rejected_without_truncating_root() {
    let dir = fixture();
    let path = dir.path().join("root.keystore");
    std::fs::write(&path, b"existing synthetic ciphertext").unwrap();
    let result = ModernIo
        .store_root_with_protection(
            dir.path(),
            RootData::new("synthetic phrase", b"synthetic seed"),
            "fixture-password",
            KdfAlgorithm::Scrypt,
            |_| async { Ok::<_, crate::Error>(Vec::new()) },
        )
        .await;
    assert!(result.is_err());
    assert!(std::fs::read(path).unwrap() == b"existing synthetic ciphertext");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn failed_publication_cleans_temporary_ciphertext() {
    let dir = fixture();
    let destination = dir.path().join("root.keystore");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("sentinel"), b"unchanged").unwrap();
    let result = ModernIo
        .store_root_with_protection(
            dir.path(),
            RootData::new("synthetic phrase", b"synthetic seed"),
            "fixture-password",
            KdfAlgorithm::Scrypt,
            |_| async { Ok::<_, crate::Error>(b"synthetic protected bytes".to_vec()) },
        )
        .await;
    assert!(result.is_err());
    assert!(std::fs::read(destination.join("sentinel")).unwrap() == b"unchanged");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn replacement_waits_for_protection_before_touching_old_root() {
    let dir = fixture();
    let destination = dir.path().join("root.keystore");
    std::fs::write(&destination, b"old synthetic ciphertext").unwrap();
    ModernIo
        .store_root_with_protection(
            dir.path(),
            RootData::new("synthetic phrase", b"synthetic seed"),
            "fixture-password",
            KdfAlgorithm::Scrypt,
            |_| async {
                assert!(std::fs::read(&destination).unwrap() == b"old synthetic ciphertext");
                tokio::task::yield_now().await;
                assert!(std::fs::read(&destination).unwrap() == b"old synthetic ciphertext");
                assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
                Ok::<_, crate::Error>(b"new synthetic ciphertext".to_vec())
            },
        )
        .await
        .unwrap();
    assert!(std::fs::read(destination).unwrap() == b"new synthetic ciphertext");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
