use super::*;
use secrecy::zeroize::Zeroizing;
use wallet_crypto::{EncryptedJsonDecryptor, KdfAlgorithm};

fn fixture() -> tempfile::TempDir {
    let parent =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/shared-keystore-tests");
    std::fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}

#[tokio::test]
async fn same_io_handles_root_derived_and_verify_as_opaque_bytes() {
    let dir = fixture();
    // Synthetic binary payload deliberately unrelated to RootData or any JSON schema.
    for name in ["root.keystore", "key0.keystore", "verify"] {
        let path = dir.path().join(name);
        let payload = vec![0, 255, 128, 1, 2];
        store(
            &path,
            Zeroizing::new(payload.clone()),
            "fixture-password",
            KdfAlgorithm::Scrypt,
            |inner| async move {
                tokio::task::yield_now().await;
                let mut bytes = b"fixture-envelope:".to_vec();
                bytes.extend(inner);
                Ok::<_, crate::Error>(bytes)
            },
        )
        .await
        .unwrap();
        assert!(std::fs::read(&path).unwrap().starts_with(b"fixture-envelope:"));
        let decoded = load(&path, |stored| async move {
            let json =
                std::str::from_utf8(stored.strip_prefix(b"fixture-envelope:").unwrap()).unwrap();
            Ok::<_, crate::Error>(
                wallet_crypto::KeystoreJsonDecryptor.decrypt(b"fixture-password", json)?,
            )
        })
        .await
        .unwrap();
        assert!(decoded == payload);
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[tokio::test]
async fn failed_protection_preserves_each_existing_file() {
    let dir = fixture();
    for name in ["root.keystore", "key0.keystore", "verify"] {
        let path = dir.path().join(name);
        std::fs::write(&path, b"existing synthetic ciphertext").unwrap();
        let result = store(
            &path,
            Zeroizing::new(vec![0, 255]),
            "fixture-password",
            KdfAlgorithm::Scrypt,
            |_| async { Err::<Vec<u8>, _>(crate::Error::InvalidKeyFormat) },
        )
        .await;
        assert!(result.is_err());
        assert!(std::fs::read(path).unwrap() == b"existing synthetic ciphertext");
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[tokio::test]
async fn rewrite_commits_replacement_and_skips_current_bytes() {
    let dir = fixture();
    let path = dir.path().join("key0.keystore");
    std::fs::write(&path, b"old synthetic ciphertext").unwrap();
    assert!(
        rewrite(&path, |bytes| async move {
            assert!(bytes == b"old synthetic ciphertext");
            Ok::<_, crate::Error>(Some(b"new synthetic ciphertext".to_vec()))
        })
        .await
        .unwrap()
    );
    assert!(
        !rewrite(&path, |bytes| async move {
            assert!(bytes == b"new synthetic ciphertext");
            Ok::<_, crate::Error>(None)
        })
        .await
        .unwrap()
    );
    assert!(std::fs::read(path).unwrap() == b"new synthetic ciphertext");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn rejected_rewrite_keeps_original_file() {
    let dir = fixture();
    let path = dir.path().join("verify");
    std::fs::write(&path, b"old synthetic ciphertext").unwrap();
    assert!(
        rewrite(&path, |_| async { Err::<Option<Vec<u8>>, _>(crate::Error::InvalidKeyFormat) })
            .await
            .is_err()
    );
    assert!(std::fs::read(path).unwrap() == b"old synthetic ciphertext");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn concurrent_rewrites_observe_the_previous_committed_version() {
    let dir = fixture();
    let path = dir.path().join("root.keystore");
    std::fs::write(&path, b"0").unwrap();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let first_path = path.clone();
    let first = tokio::spawn(async move {
        rewrite(&first_path, |bytes| async move {
            assert!(bytes == b"0");
            started_tx.send(()).unwrap();
            release_rx.await.unwrap();
            Ok::<_, crate::Error>(Some(b"1".to_vec()))
        })
        .await
        .unwrap()
    });
    started_rx.await.unwrap();
    // On case-insensitive filesystems both spellings address the same destination.
    let alias = dir.path().join("ROOT.KEYSTORE");
    let second_path = if alias.exists() { alias } else { path.clone() };
    let second = tokio::spawn(async move {
        rewrite(&second_path, |bytes| async move {
            assert!(bytes == b"1", "second rewrite read stale bytes");
            Ok::<_, crate::Error>(Some(b"2".to_vec()))
        })
        .await
        .unwrap()
    });
    // Give the second operation a chance to acquire/read while the first is suspended.
    tokio::task::yield_now().await;
    assert!(std::fs::read(&path).unwrap() == b"0");
    release_tx.send(()).unwrap();
    assert!(first.await.unwrap());
    assert!(second.await.unwrap());
    assert!(std::fs::read(path).unwrap() == b"2");
}
