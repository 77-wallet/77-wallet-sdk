use super::*;
use crate::device_protection::{
    DeviceProtection, DeviceProtectionProvider, ProtectionError, ProtectionFuture,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use wallet_crypto::EncryptedJsonGenerator;

struct NativeFixture {
    fail_second: AtomicBool,
    seals: AtomicUsize,
}
impl DeviceProtectionProvider for NativeFixture {
    fn protect<'a>(
        &'a self,
        context: &'a ProtectionContext,
        input: &'a [u8],
    ) -> ProtectionFuture<'a> {
        Box::pin(async move {
            if context == &ProtectionContext::new("fixture-wallet", "derived:0").unwrap()
                && self.fail_second.load(Ordering::SeqCst)
            {
                return Err(ProtectionError::Locked);
            }
            self.seals.fetch_add(1, Ordering::SeqCst);
            let mut bytes = context.authenticated_data();
            bytes.extend(input);
            Ok(bytes)
        })
    }
    fn unprotect<'a>(
        &'a self,
        context: &'a ProtectionContext,
        input: &'a [u8],
    ) -> ProtectionFuture<'a> {
        Box::pin(async move {
            input
                .strip_prefix(context.authenticated_data().as_slice())
                .map(Vec::from)
                .ok_or(ProtectionError::AuthenticationFailed)
        })
    }
}

fn fixture() -> tempfile::TempDir {
    let parent =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/tracked-migration-tests");
    std::fs::create_dir_all(&parent).unwrap();
    tempfile::tempdir_in(parent).unwrap()
}
async fn database(dir: &std::path::Path) -> wallet_database::CoreDbPool {
    wallet_database::SqliteContext::new(dir.to_str().unwrap(), Some("data.db"))
        .await
        .unwrap()
        .into_core_db_pool()
        .unwrap()
}
async fn close(pool: wallet_database::CoreDbPool) {
    pool.read_ref().close().await;
    pool.write_ref().close().await;
}
fn legacy() -> Vec<u8> {
    let encrypted = wallet_crypto::KeystoreJsonGenerator::new(
        rand::thread_rng(),
        wallet_crypto::KdfAlgorithm::Scrypt,
    )
    .generate(b"fixture-password", b"synthetic payload")
    .unwrap();
    serde_json::to_vec(&encrypted).unwrap()
}

#[tokio::test]
async fn tracked_prepared_migration_completes_without_a_second_password_input() {
    let dir = fixture();
    let pool = database(dir.path()).await;
    let path = dir.path().join("root.keystore");
    std::fs::write(&path, legacy()).unwrap();
    let context = ProtectionContext::new("fixture-wallet", "root-keystore").unwrap();
    let native =
        Arc::new(NativeFixture { fail_second: AtomicBool::new(false), seals: AtomicUsize::new(0) });
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    let prepared = codec
        .prepare_file_migration(
            &path,
            &context,
            "fixture-password",
            ReadPolicy::LegacyMigrationAllowed,
        )
        .await
        .unwrap();
    let files = [PreparedMigrationFile { path: &path, context: &context, prepared }];

    let summary = codec.migrate_prepared_files(&pool, files).await.unwrap();

    assert_eq!(summary.converted, 1);
    assert_eq!(summary.already_protected, 0);
    assert_eq!(native.seals.load(Ordering::SeqCst), 1);
    assert_eq!(
        MigrationRepo::get(&record_scope(&context), &pool).await.unwrap(),
        Some(MigrationState::DeviceRequired)
    );
    close(pool).await;
}

#[tokio::test]
async fn prepared_migration_freezes_every_scope_before_first_publication() {
    let dir = fixture();
    let pool = database(dir.path()).await;
    let first_path = dir.path().join("key0.keystore");
    let second_path = dir.path().join("root.keystore");
    std::fs::write(&first_path, legacy()).unwrap();
    std::fs::write(&second_path, legacy()).unwrap();
    let first = ProtectionContext::new("fixture-wallet", "derived:0").unwrap();
    let second = ProtectionContext::new("fixture-wallet", "root-keystore").unwrap();
    let native =
        Arc::new(NativeFixture { fail_second: AtomicBool::new(true), seals: AtomicUsize::new(0) });
    let codec = KeystoreProtection::new(DeviceProtection::new(native));
    let first_prepared = codec
        .prepare_file_migration(
            &first_path,
            &first,
            "fixture-password",
            ReadPolicy::LegacyMigrationAllowed,
        )
        .await
        .unwrap();
    let second_prepared = codec
        .prepare_file_migration(
            &second_path,
            &second,
            "fixture-password",
            ReadPolicy::LegacyMigrationAllowed,
        )
        .await
        .unwrap();

    assert!(
        codec
            .migrate_prepared_files(
                &pool,
                [
                    PreparedMigrationFile {
                        path: &first_path,
                        context: &first,
                        prepared: first_prepared,
                    },
                    PreparedMigrationFile {
                        path: &second_path,
                        context: &second,
                        prepared: second_prepared,
                    },
                ],
            )
            .await
            .is_err()
    );
    assert_eq!(
        MigrationRepo::get(&record_scope(&second), &pool).await.unwrap(),
        Some(MigrationState::Pending)
    );
    close(pool).await;
}

#[tokio::test]
async fn partial_migration_resumes_after_database_reopen_without_rewrapping() {
    let dir = fixture();
    let pool = database(dir.path()).await;
    let first_path = dir.path().join("root.keystore");
    let second_path = dir.path().join("key0.keystore");
    let original = legacy();
    std::fs::write(&first_path, &original).unwrap();
    std::fs::write(&second_path, &original).unwrap();
    let first = ProtectionContext::new("fixture-wallet", "root-keystore").unwrap();
    let second = ProtectionContext::new("fixture-wallet", "derived:0").unwrap();
    let files = [
        MigrationFile { path: &first_path, context: &first },
        MigrationFile { path: &second_path, context: &second },
    ];
    let native =
        Arc::new(NativeFixture { fail_second: AtomicBool::new(true), seals: AtomicUsize::new(0) });
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));

    assert!(matches!(
        codec
            .migrate_files(&pool, &files, "fixture-password", ReadPolicy::LegacyMigrationAllowed)
            .await,
        Err(TrackedMigrationError::Keystore(KeystoreProtectionError::Protection(
            ProtectionError::Locked
        )))
    ));
    let first_bytes = std::fs::read(&first_path).unwrap();
    assert!(first_bytes.starts_with(b"WDK\x01"));
    assert!(std::fs::read(&second_path).unwrap() == original);
    close(pool).await;
    let pool = database(dir.path()).await;
    assert_eq!(
        MigrationRepo::get(&record_scope(&first), &pool).await.unwrap(),
        Some(MigrationState::DeviceRequired)
    );
    assert_eq!(
        MigrationRepo::get(&record_scope(&second), &pool).await.unwrap(),
        Some(MigrationState::Pending)
    );

    native.fail_second.store(false, Ordering::SeqCst);
    let result = codec
        .migrate_files(&pool, &files, "fixture-password", ReadPolicy::LegacyMigrationAllowed)
        .await
        .unwrap();
    assert_eq!(result.converted, 1);
    assert_eq!(result.already_protected, 1);
    assert!(std::fs::read(&first_path).unwrap() == first_bytes);
    assert_eq!(native.seals.load(Ordering::SeqCst), 2);
    assert_eq!(
        MigrationRepo::get(&record_scope(&second), &pool).await.unwrap(),
        Some(MigrationState::DeviceRequired)
    );
    close(pool).await;
}

#[tokio::test]
async fn completed_record_rejects_legacy_even_when_caller_allows_migration() {
    let dir = fixture();
    let pool = database(dir.path()).await;
    let path = dir.path().join("verify");
    let original = legacy();
    std::fs::write(&path, &original).unwrap();
    let context = ProtectionContext::new("fixture-wallet", "verify").unwrap();
    let files = [MigrationFile { path: &path, context: &context }];
    let native =
        Arc::new(NativeFixture { fail_second: AtomicBool::new(false), seals: AtomicUsize::new(0) });
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    codec
        .migrate_files(&pool, &files, "fixture-password", ReadPolicy::LegacyMigrationAllowed)
        .await
        .unwrap();
    std::fs::write(&path, &original).unwrap();
    assert!(matches!(
        codec
            .migrate_files(&pool, &files, "fixture-password", ReadPolicy::LegacyMigrationAllowed)
            .await,
        Err(TrackedMigrationError::Keystore(KeystoreProtectionError::LegacyForbidden))
    ));
    assert!(std::fs::read(path).unwrap() == original);
    assert_eq!(
        MigrationRepo::get(&record_scope(&context), &pool).await.unwrap(),
        Some(MigrationState::DeviceRequired)
    );
    assert_eq!(native.seals.load(Ordering::SeqCst), 1);
    close(pool).await;
}

#[tokio::test]
async fn invalid_inventory_does_not_create_progress_or_change_files() {
    let dir = fixture();
    let pool = database(dir.path()).await;
    let path = dir.path().join("verify");
    let original = legacy();
    std::fs::write(&path, &original).unwrap();
    let context = ProtectionContext::new("fixture-wallet", "verify").unwrap();
    let codec = KeystoreProtection::new(DeviceProtection::default());
    let duplicate = [
        MigrationFile { path: &path, context: &context },
        MigrationFile { path: &path, context: &context },
    ];
    let other = ProtectionContext::new("fixture-wallet", "other-record").unwrap();
    let duplicate_path = [
        MigrationFile { path: &path, context: &context },
        MigrationFile { path: &path, context: &other },
    ];
    let missing_path = dir.path().join("missing.keystore");
    let missing = [MigrationFile { path: &missing_path, context: &context }];
    let directory = [MigrationFile { path: dir.path(), context: &context }];
    for files in [&[][..], &duplicate[..], &duplicate_path[..], &missing[..], &directory[..]] {
        assert!(matches!(
            codec
                .migrate_files(&pool, files, "fixture-password", ReadPolicy::LegacyMigrationAllowed)
                .await,
            Err(TrackedMigrationError::InvalidInventory)
        ));
    }
    #[cfg(unix)]
    {
        let link = dir.path().join("link.keystore");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        let files = [MigrationFile { path: &link, context: &context }];
        assert!(matches!(
            codec
                .migrate_files(
                    &pool,
                    &files,
                    "fixture-password",
                    ReadPolicy::LegacyMigrationAllowed
                )
                .await,
            Err(TrackedMigrationError::InvalidInventory)
        ));
    }
    assert_eq!(MigrationRepo::get(&record_scope(&context), &pool).await.unwrap(), None);
    assert_eq!(MigrationRepo::get(&record_scope(&other), &pool).await.unwrap(), None);
    assert!(std::fs::read(path).unwrap() == original);
    close(pool).await;
}

#[tokio::test]
async fn new_protected_file_is_recorded_without_legacy_conversion() {
    let dir = fixture();
    let pool = database(dir.path()).await;
    let path = dir.path().join("verify");
    let context = ProtectionContext::new("fixture-wallet", "verify").unwrap();
    let native =
        Arc::new(NativeFixture { fail_second: AtomicBool::new(false), seals: AtomicUsize::new(0) });
    let codec = KeystoreProtection::new(DeviceProtection::new(native.clone()));
    codec
        .store_file(
            &path,
            &context,
            zeroize::Zeroizing::new(b"synthetic payload".to_vec()),
            "fixture-password",
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let original = std::fs::read(&path).unwrap();
    let files = [MigrationFile { path: &path, context: &context }];
    let result = codec
        .migrate_files(&pool, &files, "fixture-password", ReadPolicy::DeviceRequired)
        .await
        .unwrap();
    assert_eq!(result.converted, 0);
    assert_eq!(result.already_protected, 1);
    assert_eq!(
        MigrationRepo::get(&record_scope(&context), &pool).await.unwrap(),
        Some(MigrationState::DeviceRequired)
    );
    assert!(std::fs::read(path).unwrap() == original);
    assert_eq!(native.seals.load(Ordering::SeqCst), 1);
    close(pool).await;
}
