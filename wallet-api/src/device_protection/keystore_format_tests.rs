use super::*;
use crate::device_protection::{DeviceProtectionProvider, ProtectionFuture};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use wallet_crypto::EncryptedJsonGenerator;

#[derive(Default)]
struct Fixture {
    protects: AtomicUsize,
    opens: AtomicUsize,
}
impl DeviceProtectionProvider for Fixture {
    fn protect<'a>(&'a self, ctx: &'a ProtectionContext, input: &'a [u8]) -> ProtectionFuture<'a> {
        Box::pin(async move {
            self.protects.fetch_add(1, Ordering::SeqCst);
            let mut bytes = ctx.authenticated_data();
            bytes.extend_from_slice(input);
            Ok(bytes)
        })
    }
    fn unprotect<'a>(
        &'a self,
        ctx: &'a ProtectionContext,
        input: &'a [u8],
    ) -> ProtectionFuture<'a> {
        Box::pin(async move {
            self.opens.fetch_add(1, Ordering::SeqCst);
            input
                .strip_prefix(ctx.authenticated_data().as_slice())
                .map(Vec::from)
                .ok_or(ProtectionError::AuthenticationFailed)
        })
    }
}
fn legacy() -> Vec<u8> {
    let json = wallet_crypto::KeystoreJsonGenerator::new(
        rand::thread_rng(),
        wallet_crypto::KdfAlgorithm::Scrypt,
    )
    .generate(b"fixture-password", b"synthetic-payload")
    .unwrap();
    serde_json::to_vec(&json).unwrap()
}
fn context() -> ProtectionContext {
    ProtectionContext::new("wallet-fixture", "root-keystore").unwrap()
}

fn root_dir() -> tempfile::TempDir {
    let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/sdk-root-tests");
    std::fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}

#[tokio::test]
async fn password_rotation_retry_accepts_an_already_published_new_password() {
    let dir = root_dir();
    let path = dir.path().join("root.keystore");
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    codec
        .store_file(
            &path,
            &context(),
            Zeroizing::new(b"synthetic-payload".to_vec()),
            "fixture-password",
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();

    codec
        .change_file_password_if_needed(
            &path,
            &context(),
            "fixture-password",
            "rotated-password",
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let first_publication = std::fs::read(&path).unwrap();

    codec
        .change_file_password_if_needed(
            &path,
            &context(),
            "fixture-password",
            "rotated-password",
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), first_publication);
    assert_eq!(
        codec.load_file(&path, &context(), "rotated-password").await.unwrap().as_slice(),
        b"synthetic-payload"
    );
    assert!(matches!(
        codec.load_file(&path, &context(), "fixture-password").await,
        Err(KeystoreProtectionError::PasswordDecryption)
    ));
    assert_eq!(native.protects.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn prepared_migration_publishes_without_repeating_password_decryption() {
    let dir = root_dir();
    let path = dir.path().join("root.keystore");
    let original = legacy();
    std::fs::write(&path, &original).unwrap();
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));

    let prepared = codec
        .prepare_file_migration(
            &path,
            &context(),
            "fixture-password",
            ReadPolicy::LegacyMigrationAllowed,
        )
        .await
        .unwrap();
    assert_eq!(prepared.plaintext(), b"synthetic-payload");
    assert!(codec.publish_prepared_file(&path, &context(), prepared).await.unwrap());

    let stored = std::fs::read(&path).unwrap();
    assert!(stored.starts_with(b"WDK\x01"));
    assert_eq!(native.protects.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn authenticated_rollout_wraps_legacy_ciphertext_without_a_file_password() {
    let dir = root_dir();
    let path = dir.path().join("root.keystore");
    let original = legacy();
    std::fs::write(&path, &original).unwrap();
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));

    let prepared = codec
        .prepare_file_wrapping(&path, &context(), ReadPolicy::LegacyMigrationAllowed)
        .await
        .unwrap();
    assert!(codec.publish_prepared_file(&path, &context(), prepared).await.unwrap());

    let stored = std::fs::read(&path).unwrap();
    assert!(stored.starts_with(b"WDK\x01"));
    assert_eq!(native.unprotect(&context(), &stored[4..]).await.unwrap(), original);
}

#[tokio::test]
async fn prepared_migration_rejects_a_source_changed_before_publication() {
    let dir = root_dir();
    let path = dir.path().join("root.keystore");
    std::fs::write(&path, legacy()).unwrap();
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    let prepared = codec
        .prepare_file_migration(
            &path,
            &context(),
            "fixture-password",
            ReadPolicy::LegacyMigrationAllowed,
        )
        .await
        .unwrap();
    let changed = b"changed after validation";
    std::fs::write(&path, changed).unwrap();

    assert!(matches!(
        codec.publish_prepared_file(&path, &context(), prepared).await,
        Err(KeystoreProtectionError::Storage)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), changed);
    assert_eq!(native.protects.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn file_migration_converts_each_kind_once_after_password_verification() {
    let dir = root_dir();
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    for (name, purpose) in [
        ("root.keystore", "root-keystore"),
        ("key0.keystore", "derived:0"),
        ("verify", "password-verify"),
    ] {
        let path = dir.path().join(name);
        let context = ProtectionContext::new("wallet-fixture", purpose).unwrap();
        let original = legacy();
        std::fs::write(&path, &original).unwrap();
        assert!(
            codec
                .migrate_file(
                    &path,
                    &context,
                    "fixture-password",
                    ReadPolicy::LegacyMigrationAllowed
                )
                .await
                .unwrap()
        );
        let migrated = std::fs::read(&path).unwrap();
        assert!(migrated.starts_with(b"WDK\x01"));
        let inner = native.unprotect(&context, &migrated[4..]).await.unwrap();
        assert!(inner == original, "migration changed the existing password ciphertext");
        let plain = codec.load_file(&path, &context, "fixture-password").await.unwrap();
        assert!(plain.as_slice() == b"synthetic-payload");
        for policy in [ReadPolicy::LegacyMigrationAllowed, ReadPolicy::DeviceRequired] {
            assert!(
                !codec.migrate_file(&path, &context, "fixture-password", policy).await.unwrap()
            );
            assert!(std::fs::read(&path).unwrap() == migrated);
        }
    }
    assert_eq!(native.protects.load(Ordering::SeqCst), 3);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[tokio::test]
async fn file_migration_failure_never_changes_original_or_downgrades_policy() {
    let dir = root_dir();
    let path = dir.path().join("key0.keystore");
    let original = legacy();
    std::fs::write(&path, &original).unwrap();
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    assert!(matches!(
        codec
            .migrate_file(&path, &context(), "wrong-password", ReadPolicy::LegacyMigrationAllowed)
            .await,
        Err(KeystoreProtectionError::PasswordDecryption)
    ));
    assert!(std::fs::read(&path).unwrap() == original);
    assert_eq!(native.protects.load(Ordering::SeqCst), 0);
    assert!(matches!(
        codec.migrate_file(&path, &context(), "fixture-password", ReadPolicy::DeviceRequired).await,
        Err(KeystoreProtectionError::LegacyForbidden)
    ));
    assert!(std::fs::read(&path).unwrap() == original);
    assert_eq!(native.protects.load(Ordering::SeqCst), 0);
    let unavailable = KeystoreProtection::new(DeviceProtection::default());
    assert!(matches!(
        unavailable
            .migrate_file(&path, &context(), "fixture-password", ReadPolicy::LegacyMigrationAllowed)
            .await,
        Err(KeystoreProtectionError::Protection(ProtectionError::NotConfigured))
    ));
    assert!(std::fs::read(path).unwrap() == original);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn common_file_codec_roundtrips_every_keystore_kind_without_rewrap() {
    let dir = root_dir();
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    for (file, purpose) in [
        ("root.keystore", "root-keystore"),
        ("key0.keystore", "derived:0"),
        ("verify", "password-verify"),
    ] {
        let path = dir.path().join(file);
        let context = ProtectionContext::new("wallet-fixture", purpose).unwrap();
        codec
            .store_file(
                &path,
                &context,
                Zeroizing::new(vec![0, 255, 128, 42]),
                "fixture-password",
                wallet_crypto::KdfAlgorithm::Scrypt,
            )
            .await
            .unwrap();
        let stored = std::fs::read(&path).unwrap();
        assert!(stored.starts_with(b"WDK\x01"));
        for _ in 0..2 {
            let plain = codec.load_file(&path, &context, "fixture-password").await.unwrap();
            assert!(plain.as_slice() == [0, 255, 128, 42]);
        }
        assert!(std::fs::read(path).unwrap() == stored);
    }
    assert_eq!(native.protects.load(Ordering::SeqCst), 3);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[tokio::test]
async fn common_file_codec_rejects_cross_record_substitution() {
    let dir = root_dir();
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    let root_context = ProtectionContext::new("wallet-fixture", "root-keystore").unwrap();
    let derived_context = ProtectionContext::new("wallet-fixture", "derived:0").unwrap();
    let root = dir.path().join("root.keystore");
    let derived = dir.path().join("key0.keystore");
    codec
        .store_file(
            &root,
            &root_context,
            Zeroizing::new(vec![1, 2, 3]),
            "fixture-password",
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    std::fs::copy(&root, &derived).unwrap();
    let original = std::fs::read(&derived).unwrap();
    assert!(matches!(
        codec.load_file(&derived, &derived_context, "fixture-password").await,
        Err(KeystoreProtectionError::Protection(ProtectionError::AuthenticationFailed))
    ));
    assert!(std::fs::read(derived).unwrap() == original);
    assert_eq!(native.protects.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn root_file_roundtrip_uses_device_codec_and_survives_reopening() {
    let dir = root_dir();
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    codec
        .store_root(
            dir.path(),
            "wallet-fixture",
            wallet_tree::file_ops::RootData::new("synthetic phrase", b"synthetic seed"),
            "fixture-password",
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let path = dir.path().join("root.keystore");
    let stored = std::fs::read(&path).unwrap();
    assert!(stored.starts_with(b"WDK\x01"));
    drop(codec);
    let reopened = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    let root = reopened.load_root(dir.path(), "wallet-fixture", "fixture-password").await.unwrap();
    assert!(root.phrase() == "synthetic phrase");
    assert!(root.seed() == b"synthetic seed");
    assert!(std::fs::read(path).unwrap() == stored);
    assert_eq!(native.protects.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn root_read_errors_preserve_file_and_error_category() {
    let dir = root_dir();
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    codec
        .store_root(
            dir.path(),
            "wallet-fixture",
            wallet_tree::file_ops::RootData::new("synthetic phrase", b"synthetic seed"),
            "fixture-password",
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let path = dir.path().join("root.keystore");
    let stored = std::fs::read(&path).unwrap();
    assert!(matches!(
        codec.load_root(dir.path(), "wallet-fixture", "wrong-password").await,
        Err(KeystoreProtectionError::PasswordDecryption)
    ));
    assert!(matches!(
        codec.load_root(dir.path(), "another-wallet", "fixture-password").await,
        Err(KeystoreProtectionError::Protection(ProtectionError::AuthenticationFailed))
    ));
    let unavailable = KeystoreProtection::new(DeviceProtection::default());
    assert!(matches!(
        unavailable.load_root(dir.path(), "wallet-fixture", "fixture-password").await,
        Err(KeystoreProtectionError::Protection(ProtectionError::NotConfigured))
    ));
    assert!(std::fs::read(path).unwrap() == stored);
    assert_eq!(native.protects.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn root_missing_provider_write_does_not_replace_existing_file() {
    let dir = root_dir();
    let path = dir.path().join("root.keystore");
    std::fs::write(&path, b"existing fixture ciphertext").unwrap();
    let codec = KeystoreProtection::new(DeviceProtection::default());
    let result = codec
        .store_root(
            dir.path(),
            "wallet-fixture",
            wallet_tree::file_ops::RootData::new("synthetic phrase", b"synthetic seed"),
            "fixture-password",
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await;
    assert!(matches!(
        result,
        Err(KeystoreProtectionError::Protection(ProtectionError::NotConfigured))
    ));
    assert!(std::fs::read(path).unwrap() == b"existing fixture ciphertext");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn root_read_rejects_legacy_without_migration_or_rewrite() {
    use wallet_tree::file_ops::IoStrategy;
    let dir = root_dir();
    wallet_tree::wallet_hierarchy::v2::ModernWalletTree::default()
        .io
        .store_root(
            "wallet-fixture",
            b"synthetic seed",
            "synthetic phrase",
            &dir.path(),
            "fixture-password",
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .unwrap();
    let path = dir.path().join("root.keystore");
    let stored = std::fs::read(&path).unwrap();
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    assert!(matches!(
        codec.load_root(dir.path(), "wallet-fixture", "fixture-password").await,
        Err(KeystoreProtectionError::LegacyForbidden)
    ));
    assert_eq!(native.protects.load(Ordering::SeqCst), 0);
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
    assert!(std::fs::read(path).unwrap() == stored);
}

#[tokio::test]
async fn new_file_never_enters_migration_or_double_wraps() {
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    let sealed = codec.seal(&context(), &legacy()).await.unwrap();
    for policy in [ReadPolicy::LegacyMigrationAllowed, ReadPolicy::DeviceRequired] {
        let opened = codec.open(&context(), &sealed, "fixture-password", policy).await.unwrap();
        assert_eq!(opened.plaintext.as_slice(), b"synthetic-payload");
        assert!(opened.replacement.is_none());
    }
    assert_eq!(native.protects.load(Ordering::SeqCst), 1);
    assert!(matches!(
        codec.seal(&context(), &sealed).await,
        Err(KeystoreProtectionError::InvalidFormat)
    ));
}

#[tokio::test]
async fn old_file_upgrades_once_only_after_correct_password() {
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    let original = legacy();
    assert!(matches!(
        codec.open(&context(), &original, "wrong", ReadPolicy::LegacyMigrationAllowed).await,
        Err(KeystoreProtectionError::PasswordDecryption)
    ));
    assert_eq!(native.protects.load(Ordering::SeqCst), 0);
    let opened = codec
        .open(&context(), &original, "fixture-password", ReadPolicy::LegacyMigrationAllowed)
        .await
        .unwrap();
    let upgraded = opened.replacement.unwrap();
    let again = codec
        .open(&context(), &upgraded, "fixture-password", ReadPolicy::DeviceRequired)
        .await
        .unwrap();
    assert!(again.replacement.is_none());
    assert_eq!(again.plaintext.as_slice(), b"synthetic-payload");
    assert_eq!(native.protects.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn required_policy_and_unknown_formats_never_fall_back() {
    let codec = KeystoreProtection::new(DeviceProtection::default());
    assert!(matches!(
        codec.open(&context(), &legacy(), "fixture-password", ReadPolicy::DeviceRequired).await,
        Err(KeystoreProtectionError::LegacyForbidden)
    ));
    for bytes in [b"WDK\x02unknown".as_slice(), b"WDK\x01", b"damaged"] {
        assert!(matches!(
            codec
                .open(&context(), bytes, "fixture-password", ReadPolicy::LegacyMigrationAllowed)
                .await,
            Err(KeystoreProtectionError::InvalidFormat)
        ));
    }
    assert!(matches!(
        codec
            .open(
                &context(),
                b"WDK\x01native",
                "fixture-password",
                ReadPolicy::LegacyMigrationAllowed
            )
            .await,
        Err(KeystoreProtectionError::Protection(ProtectionError::NotConfigured))
    ));
}

#[tokio::test]
async fn native_failure_does_not_release_legacy_migration_or_downgrade_current_data() {
    let codec = KeystoreProtection::new(DeviceProtection::default());
    assert!(matches!(
        codec
            .open(&context(), &legacy(), "fixture-password", ReadPolicy::LegacyMigrationAllowed)
            .await,
        Err(KeystoreProtectionError::Protection(ProtectionError::NotConfigured))
    ));
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    let sealed = codec.seal(&context(), &legacy()).await.unwrap();
    let wrong = ProtectionContext::new("different-wallet", "root-keystore").unwrap();
    assert!(matches!(
        codec.open(&wrong, &sealed, "fixture-password", ReadPolicy::LegacyMigrationAllowed).await,
        Err(KeystoreProtectionError::Protection(ProtectionError::AuthenticationFailed))
    ));
    assert_eq!(native.protects.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn excessive_kdf_cost_is_rejected_without_native_calls() {
    let native = Arc::new(Fixture::default());
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    let mut json: serde_json::Value = serde_json::from_slice(&legacy()).unwrap();
    json["crypto"]["kdfparams"]["n"] = serde_json::json!(2147483648u32);
    let malicious = serde_json::to_vec(&json).unwrap();
    assert!(matches!(
        codec
            .open(&context(), &malicious, "fixture-password", ReadPolicy::LegacyMigrationAllowed)
            .await,
        Err(KeystoreProtectionError::UnsupportedKdfCost)
    ));
    assert_eq!(native.protects.load(Ordering::SeqCst), 0);
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
}

#[test]
fn current_generated_kdf_profiles_remain_supported() {
    let mut json: serde_json::Value = serde_json::from_slice(&legacy()).unwrap();
    for algorithm in [
        wallet_crypto::KdfAlgorithm::Argon2id,
        wallet_crypto::KdfAlgorithm::Scrypt,
        wallet_crypto::KdfAlgorithm::Pbkdf2,
    ] {
        let kdf = wallet_crypto::kdf::factory::KdfFactory::create(&algorithm, &[1; 32]).unwrap();
        json["crypto"]["kdf"] = serde_json::to_value(algorithm).unwrap();
        json["crypto"]["kdfparams"] = serde_json::to_value(kdf.params()).unwrap();
        validate_inner(&serde_json::to_vec(&json).unwrap()).unwrap();
    }
}
