use super::WalletApplication;
use crate::{
    context::Context,
    device_protection::{
        DeviceProtection, DeviceProtectionProvider, ProtectionContext, ProtectionError,
        ProtectionFuture,
        keystore_format::{KeystoreProtection, KeystoreProtectionError, ReadPolicy},
        migration::MigrationFile,
    },
    dirs::Dirs,
    error::service::ServiceError,
    service::wallet::WalletService,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use wallet_tree::file_ops::RootData;

#[path = "wallet_unlock_contention_tests.rs"]
mod unlock_contention_tests;

#[path = "wallet_unlock_timing_tests.rs"]
mod unlock_timing_tests;

#[derive(Default)]
struct Native {
    protect_error: std::sync::Mutex<Option<ProtectionError>>,
    locked: AtomicBool,
    fail_protect: AtomicBool,
    opens: AtomicUsize,
    pause_protect: AtomicBool,
    protect_entered: tokio::sync::Notify,
    protect_resume: tokio::sync::Notify,
    provisions: AtomicUsize,
    key_missing: AtomicBool,
}
impl DeviceProtectionProvider for Native {
    fn provision_for_new_storage(&self) -> crate::device_protection::ProvisionFuture<'_> {
        Box::pin(async move {
            self.provisions.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn protect<'a>(&'a self, ctx: &'a ProtectionContext, input: &'a [u8]) -> ProtectionFuture<'a> {
        Box::pin(async move {
            if let Some(error) = *self.protect_error.lock().unwrap() {
                return Err(error);
            }
            if self.pause_protect.swap(false, Ordering::SeqCst) {
                self.protect_entered.notify_one();
                self.protect_resume.notified().await;
            }
            if self.key_missing.load(Ordering::SeqCst) {
                return Err(ProtectionError::KeyMissing);
            }
            if self.fail_protect.load(Ordering::SeqCst) {
                return Err(ProtectionError::PlatformFailure);
            }
            let mut result = ctx.authenticated_data();
            result.extend_from_slice(input);
            Ok(result)
        })
    }
    fn unprotect<'a>(&'a self, ctx: &'a ProtectionContext, blob: &'a [u8]) -> ProtectionFuture<'a> {
        Box::pin(async move {
            self.opens.fetch_add(1, Ordering::SeqCst);
            if self.locked.load(Ordering::SeqCst) {
                return Err(ProtectionError::Locked);
            }
            blob.strip_prefix(ctx.authenticated_data().as_slice())
                .map(Vec::from)
                .ok_or(ProtectionError::AuthenticationFailed)
        })
    }
}
const WALLET: &str = "fixturewallet";
const PASSWORD: &str = "fixture-password";
const PHRASE: &str = "synthetic phrase payload";
fn directory() -> tempfile::TempDir {
    let base =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/phrase-service-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::tempdir_in(base).unwrap()
}
async fn context(path: &std::path::Path, native: Arc<Native>) -> &'static Context {
    context_with_protection(path, DeviceProtection::new(native)).await
}

async fn context_with_protection(
    path: &std::path::Path,
    protection: DeviceProtection,
) -> &'static Context {
    let config = serde_json::from_value(serde_json::json!({
        "backend_api": {"dev_url":"http://127.0.0.1:1", "test_url":"http://127.0.0.1:1", "prod_url":"http://127.0.0.1:1"},
        "aggregate_api": {"dev_url":"http://127.0.0.1:1", "test_url":"http://127.0.0.1:1", "prod_url":"http://127.0.0.1:1"},
        "crypto": {"aes_key":"00000000000000000000000000000000", "aes_iv":"0000000000000000"},
        "app_code":"fixture"
    })).unwrap();
    let ctx =
        Context::new_without_handles_for_test(Dirs::new(path.to_str().unwrap()).unwrap(), config)
            .await
            .unwrap()
            .with_device_protection_for_test(protection);
    Box::leak(Box::new(ctx))
}

#[tokio::test]
async fn protected_phrase_uses_actual_service_after_context_reopen() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    let codec = KeystoreProtection::new(ctx.device_protection().clone());
    codec
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let before = std::fs::read(root.join("root.keystore")).unwrap();
    let response = WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await.unwrap();
    assert!(response.phrase == PHRASE);
    let reopened = context(dir.path(), native.clone()).await;
    assert!(
        WalletService::new(reopened).get_phrase(WALLET, PASSWORD).await.unwrap().phrase == PHRASE
    );
    let wrong = WalletService::new(reopened).get_phrase(WALLET, "wrong-fixture-password").await;
    assert!(matches!(
        wrong,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::PasswordDecryption))
    ));
    native.locked.store(true, Ordering::SeqCst);
    let result = WalletService::new(reopened).get_phrase(WALLET, PASSWORD).await;
    assert!(matches!(
        result,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::Protection(
            ProtectionError::Locked
        )))
    ));
    assert!(std::fs::read(root.join("root.keystore")).unwrap() == before);
}

#[tokio::test]
async fn legacy_phrase_stays_readable_but_completed_migration_blocks_rollback() {
    use wallet_tree::wallet_hierarchy::{WalletTreeOps, v1::LegacyWalletTree};
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    LegacyWalletTree::default()
        .io()
        .store_root(
            WALLET,
            b"synthetic seed",
            PHRASE,
            &root,
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .unwrap();
    let legacy = std::fs::read(root.join("fixturewallet-phrase")).unwrap();
    assert!(WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await.unwrap().phrase == PHRASE);
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
    let codec = KeystoreProtection::new(ctx.device_protection().clone());
    codec
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let binding = ProtectionContext::new(WALLET, "root-keystore").unwrap();
    let path = root.join("root.keystore");
    codec
        .migrate_files(
            &ctx.core_pool().unwrap(),
            &[MigrationFile { path: &path, context: &binding }],
            PASSWORD,
            ReadPolicy::DeviceRequired,
        )
        .await
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    // Older root files remain valid but may not be used once this root completed migration.
    assert!(WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await.is_err());
    assert!(std::fs::read(root.join("fixturewallet-phrase")).unwrap() == legacy);
}

#[tokio::test]
async fn modern_password_only_phrase_uses_existing_reader_without_native_calls() {
    use wallet_tree::wallet_hierarchy::{WalletTreeOps, v2::ModernWalletTree};
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    wallet_database::repositories::config::ConfigRepo::upsert(
        "wallet_tree_strategy",
        r#"{"wallet_tree_strategy":"v2"}"#,
        Some(0),
        &ctx.core_pool().unwrap(),
    )
    .await
    .unwrap();
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    ModernWalletTree::default()
        .io()
        .store_root(
            WALLET,
            b"synthetic seed",
            PHRASE,
            &root,
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .unwrap();
    let path = root.join("root.keystore");
    let before = std::fs::read(&path).unwrap();
    assert!(WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await.unwrap().phrase == PHRASE);
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
    assert!(std::fs::read(&path).unwrap() == before);
    std::fs::write(&path, b"WDK\x02unsupported-fixture").unwrap();
    let failure = WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await;
    assert!(matches!(
        failure,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::InvalidFormat))
    ));
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
    assert!(std::fs::read(&path).unwrap() == b"WDK\x02unsupported-fixture");
}

#[tokio::test]
async fn protected_seed_and_password_fallback_use_device_reader() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let dirs = ctx.get_global_dirs();
    let root = dirs.get_root_dir(WALLET).unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    wallet_database::repositories::wallet::WalletRepo::upsert_wallet(
        ctx.core_pool().unwrap(),
        WALLET,
        "fixture-uid",
        "fixture",
    )
    .await
    .unwrap();
    let before = std::fs::read(root.join("root.keystore")).unwrap();
    let seed = WalletApplication::get_seed(ctx, WALLET, PASSWORD).await.unwrap();
    assert!(seed == b"synthetic seed");
    assert!(WalletApplication::try_decrypt_wallet_db(ctx, PASSWORD).await.unwrap());
    assert!(
        !WalletApplication::try_decrypt_wallet_db(ctx, "wrong-fixture-password").await.unwrap()
    );
    native.locked.store(true, Ordering::SeqCst);
    let failure = WalletApplication::try_decrypt_wallet_db(ctx, PASSWORD).await;
    assert!(matches!(
        failure,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::Protection(
            ProtectionError::Locked
        )))
    ));
    assert!(std::fs::read(root.join("root.keystore")).unwrap() == before);
}

#[tokio::test]
async fn legacy_password_fallback_preserves_success_and_wrong_password_behavior() {
    use wallet_tree::wallet_hierarchy::{WalletTreeOps, v1::LegacyWalletTree};
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    LegacyWalletTree::default()
        .io()
        .store_root(
            WALLET,
            b"synthetic seed",
            PHRASE,
            &root,
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .unwrap();
    wallet_database::repositories::wallet::WalletRepo::upsert_wallet(
        ctx.core_pool().unwrap(),
        WALLET,
        "fixture-uid",
        "fixture",
    )
    .await
    .unwrap();
    assert!(WalletApplication::try_decrypt_wallet_db(ctx, PASSWORD).await.unwrap());
    assert!(
        !WalletApplication::try_decrypt_wallet_db(ctx, "wrong-fixture-password").await.unwrap()
    );
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
}

async fn register_wallet_for_password_change(ctx: &Context) {
    use wallet_database::{
        entities::config::{KeystoreKdfAlgorithm, config_key::KEYSTORE_KDF_ALGORITHM},
        repositories::{config::ConfigRepo, wallet::WalletRepo},
    };
    let pool = ctx.core_pool().unwrap();
    WalletRepo::upsert_wallet(pool.clone(), WALLET, "fixture-uid", "fixture").await.unwrap();
    let config =
        KeystoreKdfAlgorithm { keystore_kdf_algorithm: wallet_crypto::KdfAlgorithm::Scrypt };
    ConfigRepo::upsert(KEYSTORE_KDF_ALGORITHM, &config.to_json_str().unwrap(), Some(0), &pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn root_password_change_preserves_wdk_and_recovers_from_native_failure() {
    use crate::domain::account::AccountDomain;
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    register_wallet_for_password_change(ctx).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    let codec = KeystoreProtection::new(ctx.device_protection().clone());
    codec
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let path = root.join("root.keystore");
    let before = std::fs::read(&path).unwrap();
    native.fail_protect.store(true, Ordering::SeqCst);
    let failure =
        AccountDomain::set_root_password(ctx, WALLET, PASSWORD, "new-fixture-password").await;
    assert!(matches!(
        failure,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::Protection(
            ProtectionError::PlatformFailure
        )))
    ));
    assert!(std::fs::read(&path).unwrap() == before);
    assert!(WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await.unwrap().phrase == PHRASE);
    native.fail_protect.store(false, Ordering::SeqCst);
    let wrong = AccountDomain::set_root_password(
        ctx,
        WALLET,
        "wrong-fixture-password",
        "new-fixture-password",
    )
    .await;
    assert!(matches!(
        wrong,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::PasswordDecryption))
    ));
    assert!(std::fs::read(&path).unwrap() == before);
    AccountDomain::set_root_password(ctx, WALLET, PASSWORD, "new-fixture-password").await.unwrap();
    let after = std::fs::read(&path).unwrap();
    assert!(after.starts_with(b"WDK\x01") && after != before);
    assert!(
        WalletService::new(ctx).get_phrase(WALLET, "new-fixture-password").await.unwrap().phrase
            == PHRASE
    );
    assert!(
        WalletApplication::get_seed(ctx, WALLET, "new-fixture-password").await.unwrap()
            == b"synthetic seed"
    );
    assert!(matches!(
        WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::PasswordDecryption))
    ));
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    // Competing changes using the same old password cannot overwrite each other.
    let (left, right) = tokio::join!(
        AccountDomain::set_root_password(ctx, WALLET, "new-fixture-password", "next-fixture-a"),
        AccountDomain::set_root_password(ctx, WALLET, "new-fixture-password", "next-fixture-b"),
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let winner = if left.is_ok() { "next-fixture-a" } else { "next-fixture-b" };
    assert!(WalletService::new(ctx).get_phrase(WALLET, winner).await.unwrap().phrase == PHRASE);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
}

#[tokio::test]
async fn completed_root_password_change_rejects_legacy_rollback() {
    use crate::domain::account::AccountDomain;
    use wallet_tree::wallet_hierarchy::{WalletTreeOps, v1::LegacyWalletTree};
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native).await;
    register_wallet_for_password_change(ctx).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    LegacyWalletTree::default()
        .io()
        .store_root(
            WALLET,
            b"synthetic seed",
            PHRASE,
            &root,
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .unwrap();
    let original = std::fs::read(root.join("fixturewallet-phrase")).unwrap();
    let codec = KeystoreProtection::new(ctx.device_protection().clone());
    codec
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let path = root.join("root.keystore");
    let binding = ProtectionContext::new(WALLET, "root-keystore").unwrap();
    codec
        .migrate_files(
            &ctx.core_pool().unwrap(),
            &[MigrationFile { path: &path, context: &binding }],
            PASSWORD,
            ReadPolicy::DeviceRequired,
        )
        .await
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(
        AccountDomain::set_root_password(ctx, WALLET, PASSWORD, "new-fixture-password")
            .await
            .is_err()
    );
    assert!(std::fs::read(root.join("fixturewallet-phrase")).unwrap() == original);
}

#[tokio::test]
async fn legacy_root_password_change_keeps_existing_format() {
    use crate::domain::account::AccountDomain;
    use wallet_tree::wallet_hierarchy::{WalletTreeOps, v1::LegacyWalletTree};
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    register_wallet_for_password_change(ctx).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    LegacyWalletTree::default()
        .io()
        .store_root(
            WALLET,
            b"synthetic seed",
            PHRASE,
            &root,
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .unwrap();
    AccountDomain::set_root_password(ctx, WALLET, PASSWORD, "new-fixture-password").await.unwrap();
    assert!(
        WalletService::new(ctx).get_phrase(WALLET, "new-fixture-password").await.unwrap().phrase
            == PHRASE
    );
    assert!(WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await.is_err());
    assert!(!root.join("root.keystore").exists());
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
}

async fn derived_password_fixture(ctx: &Context) -> std::path::PathBuf {
    use wallet_tree::{
        file_ops::BulkSubkey,
        wallet_hierarchy::{WalletTreeOps, v2::ModernWalletTree},
    };
    use wallet_utils::address::AccountIndexMap;
    register_wallet_for_password_change(ctx).await;
    wallet_database::repositories::config::ConfigRepo::upsert(
        "wallet_tree_strategy",
        r#"{"wallet_tree_strategy":"v2"}"#,
        Some(0),
        &ctx.core_pool().unwrap(),
    )
    .await
    .unwrap();
    ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    let subs = ctx.get_global_dirs().get_subs_dir(WALLET).unwrap();
    let account = AccountIndexMap::from_account_id(0).unwrap();
    let other = AccountIndexMap::from_account_id(1).unwrap();
    ModernWalletTree::default()
        .io()
        .store_subkeys_bulk(
            vec![
                BulkSubkey::new(
                    account.clone(),
                    "fixture-eth",
                    "eth",
                    "m/0",
                    b"synthetic eth key".to_vec(),
                ),
                BulkSubkey::new(
                    account,
                    "fixture-sol",
                    "sol",
                    "m/1",
                    b"synthetic sol key".to_vec(),
                ),
                BulkSubkey::new(
                    other,
                    "fixture-other",
                    "eth",
                    "m/2",
                    b"synthetic other key".to_vec(),
                ),
            ],
            &subs,
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .unwrap();
    subs
}

#[tokio::test]
async fn derived_password_change_preserves_payload_index_and_other_accounts() {
    use crate::domain::account::AccountDomain;
    use wallet_crypto::EncryptedJsonDecryptor;
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    let path = subs.join("key0.keystore");
    let index = std::fs::read(subs.join("derived_meta.json")).unwrap();
    let other = std::fs::read(subs.join("key1.keystore")).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    let plain =
        wallet_crypto::KeystoreJsonDecryptor.decrypt(PASSWORD.as_bytes(), &original).unwrap();
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    let codec = KeystoreProtection::new(ctx.device_protection().clone());
    codec
        .migrate_file(&path, &binding, PASSWORD, ReadPolicy::LegacyMigrationAllowed)
        .await
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let account = wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap();
    let wrong = AccountDomain::set_account_password(
        ctx,
        WALLET,
        &account,
        "wrong-fixture-password",
        "new-fixture-password",
    )
    .await;
    assert!(matches!(
        wrong,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::PasswordDecryption))
    ));
    assert!(std::fs::read(&path).unwrap() == before);
    native.fail_protect.store(true, Ordering::SeqCst);
    let failed = AccountDomain::set_account_password(
        ctx,
        WALLET,
        &account,
        PASSWORD,
        "new-fixture-password",
    )
    .await;
    assert!(matches!(
        failed,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::Protection(
            ProtectionError::PlatformFailure
        )))
    ));
    assert!(std::fs::read(&path).unwrap() == before);
    assert!(std::fs::read(subs.join("derived_meta.json")).unwrap() == index);
    native.fail_protect.store(false, Ordering::SeqCst);
    AccountDomain::set_account_password(ctx, WALLET, &account, PASSWORD, "new-fixture-password")
        .await
        .unwrap();
    assert!(
        codec.load_file(&path, &binding, "new-fixture-password").await.unwrap().as_slice() == plain
    );
    assert!(matches!(
        codec.load_file(&path, &binding, PASSWORD).await,
        Err(KeystoreProtectionError::PasswordDecryption)
    ));
    assert!(std::fs::read(&path).unwrap().starts_with(b"WDK\x01"));
    assert!(std::fs::read(subs.join("derived_meta.json")).unwrap() == index);
    assert!(std::fs::read(subs.join("key1.keystore")).unwrap() == other);
    assert_eq!(std::fs::read_dir(&subs).unwrap().count(), 3);
}

#[tokio::test]
async fn completed_derived_password_change_rejects_legacy_rollback() {
    use crate::domain::account::AccountDomain;
    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    let subs = derived_password_fixture(ctx).await;
    let path = subs.join("key0.keystore");
    let original = std::fs::read(&path).unwrap();
    let index = std::fs::read(subs.join("derived_meta.json")).unwrap();
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .migrate_files(
            &ctx.core_pool().unwrap(),
            &[MigrationFile { path: &path, context: &binding }],
            PASSWORD,
            ReadPolicy::LegacyMigrationAllowed,
        )
        .await
        .unwrap();
    std::fs::write(&path, &original).unwrap();
    let account = wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap();
    let failed = AccountDomain::set_account_password(
        ctx,
        WALLET,
        &account,
        PASSWORD,
        "new-fixture-password",
    )
    .await;
    assert!(matches!(
        failed,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::LegacyForbidden))
    ));
    assert!(std::fs::read(&path).unwrap() == original);
    assert!(std::fs::read(subs.join("derived_meta.json")).unwrap() == index);
}

#[tokio::test]
async fn password_only_derived_change_remains_compatible() {
    use crate::domain::account::AccountDomain;
    use wallet_crypto::EncryptedJsonDecryptor;
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    let path = subs.join("key0.keystore");
    let original = std::fs::read_to_string(&path).unwrap();
    let plain =
        wallet_crypto::KeystoreJsonDecryptor.decrypt(PASSWORD.as_bytes(), &original).unwrap();
    let account = wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap();
    AccountDomain::set_account_password(ctx, WALLET, &account, PASSWORD, "new-fixture-password")
        .await
        .unwrap();
    let updated = std::fs::read_to_string(&path).unwrap();
    assert!(
        wallet_crypto::KeystoreJsonDecryptor.decrypt(b"new-fixture-password", &updated).unwrap()
            == plain
    );
    assert!(wallet_crypto::KeystoreJsonDecryptor.decrypt(PASSWORD.as_bytes(), &updated).is_err());
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn protected_sub_password_change_updates_the_grouped_account_file() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    let path = subs.join("key0.keystore");
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .migrate_file(&path, &binding, PASSWORD, ReadPolicy::LegacyMigrationAllowed)
        .await
        .unwrap();

    crate::service::account::change_sub_password(ctx, WALLET, 0, PASSWORD, "new-fixture-password")
        .await
        .unwrap();

    assert!(std::fs::read(&path).unwrap().starts_with(b"WDK\x01"));
    let account = wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap();
    let changed =
        WalletApplication::load_account_data(ctx, WALLET, &account, "new-fixture-password")
            .await
            .unwrap();
    assert_eq!(changed.len(), 2);
    assert!(changed.iter().any(|(meta, key)| {
        meta.address == "fixture-eth" && key.as_slice() == b"synthetic eth key"
    }));
    assert!(changed.iter().any(|(meta, key)| {
        meta.address == "fixture-sol" && key.as_slice() == b"synthetic sol key"
    }));
    assert!(WalletApplication::load_account_data(ctx, WALLET, &account, PASSWORD).await.is_err());
}

#[tokio::test]
async fn private_key_export_propagates_device_failure_before_conversion() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    let path = subs.join("key0.keystore");
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .migrate_file(&path, &binding, PASSWORD, ReadPolicy::LegacyMigrationAllowed)
        .await
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    native.locked.store(true, Ordering::SeqCst);
    let account = wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap();
    let failed =
        crate::domain::account::open_accounts_pk_with_password(ctx, &account, WALLET, PASSWORD)
            .await;
    assert!(matches!(
        failed,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::Protection(
            ProtectionError::Locked
        )))
    ));
    assert!(std::fs::read(&path).unwrap() == before);
}

#[tokio::test]
async fn derived_private_key_reads_match_exact_metadata_and_preserve_files() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    let path = subs.join("key0.keystore");
    let index = std::fs::read(subs.join("derived_meta.json")).unwrap();
    let account = wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap();
    let expected = wallet_tree::KeyMeta {
        chain_code: "eth".into(),
        address: "fixture-eth".into(),
        derivation_path: "m/0".into(),
    };
    let legacy =
        WalletApplication::load_account_data(ctx, WALLET, &account, PASSWORD).await.unwrap();
    assert_eq!(legacy.len(), 2);
    assert!(
        WalletApplication::load_sub_key(ctx, WALLET, &account, &expected, PASSWORD).await.unwrap()
            == b"synthetic eth key"
    );
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .migrate_file(&path, &binding, PASSWORD, ReadPolicy::LegacyMigrationAllowed)
        .await
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let loaded =
        WalletApplication::load_account_data(ctx, WALLET, &account, PASSWORD).await.unwrap();
    assert!(loaded.into_inner() == legacy.into_inner());
    assert!(
        WalletApplication::load_sub_key(ctx, WALLET, &account, &expected, PASSWORD).await.unwrap()
            == b"synthetic eth key"
    );
    let wrong_path =
        wallet_tree::KeyMeta { derivation_path: "m/unknown".into(), ..expected.clone() };
    assert!(matches!(
        WalletApplication::load_sub_key(ctx, WALLET, &account, &wrong_path, PASSWORD).await,
        Err(ServiceError::Tree(wallet_tree::Error::PrivateKeyNotFound))
    ));
    let wrong_password =
        WalletApplication::load_sub_key(ctx, WALLET, &account, &expected, "wrong-fixture-password")
            .await;
    assert!(matches!(
        wrong_password,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::PasswordDecryption))
    ));
    native.locked.store(true, Ordering::SeqCst);
    assert!(matches!(
        WalletApplication::load_sub_key(ctx, WALLET, &account, &expected, PASSWORD).await,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::Protection(
            ProtectionError::Locked
        )))
    ));
    assert!(std::fs::read(&path).unwrap() == before);
    assert!(std::fs::read(subs.join("derived_meta.json")).unwrap() == index);
}

#[tokio::test]
async fn malformed_protected_account_payload_is_rejected_without_rewrite() {
    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    let subs = derived_password_fixture(ctx).await;
    let path = subs.join("key0.keystore");
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    let codec = KeystoreProtection::new(ctx.device_protection().clone());
    for payload in [
        br#"{"invalid-key":[1,2,3]}"#.as_slice(),
        br#"{"eth::address::m/0":[1],"eth::address::m/0":[2]}"#.as_slice(),
        br#"{"eth::addr:1::m/0":[1],"eth::addr\\c1::m/0":[2]}"#.as_slice(),
    ] {
        codec
            .store_file(
                &path,
                &binding,
                zeroize::Zeroizing::new(payload.to_vec()),
                PASSWORD,
                wallet_crypto::KdfAlgorithm::Scrypt,
            )
            .await
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let account = wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap();
        assert!(matches!(
            WalletApplication::load_account_data(ctx, WALLET, &account, PASSWORD).await,
            Err(ServiceError::DeviceProtection(KeystoreProtectionError::InvalidFormat))
        ));
        assert!(std::fs::read(&path).unwrap() == before);
    }
}

#[tokio::test]
async fn root_store_preserves_existing_protection_and_files_on_failure() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    register_wallet_for_password_change(ctx).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let path = root.join("root.keystore");
    let before = std::fs::read(&path).unwrap();
    let sibling = root.join("fixture-public-marker");
    std::fs::write(&sibling, b"keep").unwrap();
    native.fail_protect.store(true, Ordering::SeqCst);
    let failed = WalletApplication::store_root(
        ctx,
        WALLET,
        RootData::new(PHRASE, b"synthetic seed"),
        PASSWORD,
    )
    .await;
    assert!(matches!(
        failed,
        Err(ServiceError::DeviceProtection(KeystoreProtectionError::Protection(
            ProtectionError::PlatformFailure
        )))
    ));
    assert!(std::fs::read(&path).unwrap() == before);
    assert!(std::fs::read(&sibling).unwrap() == b"keep");
    assert!(WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await.unwrap().phrase == PHRASE);
    native.fail_protect.store(false, Ordering::SeqCst);
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    assert!(std::fs::read(&path).unwrap().starts_with(b"WDK\x01"));
    assert!(WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await.unwrap().phrase == PHRASE);
    assert!(std::fs::read(&sibling).unwrap() == b"keep");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
}

#[tokio::test]
async fn root_store_retains_legacy_creation_until_activation() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    register_wallet_for_password_change(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    assert!(WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await.unwrap().phrase == PHRASE);
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
    assert!(
        ctx.get_global_dirs().get_root_dir(WALLET).unwrap().join("fixturewallet-seed").exists()
    );
}

fn appended_subkey(account: u32, address: &str) -> wallet_tree::file_ops::BulkSubkey {
    wallet_tree::file_ops::BulkSubkey::new(
        wallet_utils::address::AccountIndexMap::from_account_id(account).unwrap(),
        address,
        "eth",
        "m/3",
        b"synthetic appended key".to_vec(),
    )
}

#[tokio::test]
async fn derived_bulk_protection_preserves_keys_and_index_on_failure_and_retry() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    let codec = KeystoreProtection::new(ctx.device_protection().clone());
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    codec
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    codec
        .migrate_file(
            &subs.join("key0.keystore"),
            &binding,
            PASSWORD,
            ReadPolicy::LegacyMigrationAllowed,
        )
        .await
        .unwrap();
    let original = std::fs::read(subs.join("key0.keystore")).unwrap();
    let index = std::fs::read(subs.join("derived_meta.json")).unwrap();
    let other = std::fs::read(subs.join("key1.keystore")).unwrap();
    native.fail_protect.store(true, Ordering::SeqCst);
    assert!(
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![appended_subkey(0, "new-a"), appended_subkey(2, "new-b")],
            PASSWORD
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read(subs.join("key0.keystore")).unwrap(), original);
    assert_eq!(std::fs::read(subs.join("derived_meta.json")).unwrap(), index);
    assert!(!subs.join("key2.keystore").exists());
    native.fail_protect.store(false, Ordering::SeqCst);
    for _ in 0..2 {
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![appended_subkey(0, "new-a"), appended_subkey(2, "new-b")],
            PASSWORD,
        )
        .await
        .unwrap();
    }
    let keys = WalletApplication::load_account_data(
        ctx,
        WALLET,
        &wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap(),
        PASSWORD,
    )
    .await
    .unwrap();
    assert_eq!(keys.len(), 3);
    assert!(keys.values().any(|v| v == b"synthetic eth key"));
    assert!(keys.values().any(|v| v == b"synthetic sol key"));
    assert_eq!(
        WalletApplication::load_account_data(
            ctx,
            WALLET,
            &wallet_utils::address::AccountIndexMap::from_account_id(2).unwrap(),
            PASSWORD
        )
        .await
        .unwrap()
        .len(),
        1
    );
    assert!(std::fs::read(subs.join("key2.keystore")).unwrap().starts_with(b"WDK\x01"));
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(subs.join("derived_meta.json")).unwrap()).unwrap();
    assert_eq!(index["accounts"]["0"].as_array().unwrap().len(), 3);
    assert_eq!(index["accounts"]["2"].as_array().unwrap().len(), 1);
    assert_eq!(std::fs::read(subs.join("key1.keystore")).unwrap(), other);
}

#[tokio::test]
async fn derived_bulk_protection_serializes_appends_and_rejects_damaged_index() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native).await;
    let subs = derived_password_fixture(ctx).await;
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .migrate_file(
            &subs.join("key0.keystore"),
            &binding,
            PASSWORD,
            ReadPolicy::LegacyMigrationAllowed,
        )
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![appended_subkey(0, "concurrent-a")],
            PASSWORD
        ),
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![appended_subkey(0, "concurrent-b")],
            PASSWORD
        ),
    );
    a.unwrap();
    b.unwrap();
    let keys = WalletApplication::load_account_data(
        ctx,
        WALLET,
        &wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap(),
        PASSWORD,
    )
    .await
    .unwrap();
    assert_eq!(keys.len(), 4);
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(subs.join("derived_meta.json")).unwrap()).unwrap();
    assert_eq!(metadata["accounts"]["0"].as_array().unwrap().len(), 4);
    let original = std::fs::read(subs.join("key0.keystore")).unwrap();
    std::fs::write(subs.join("derived_meta.json"), b"invalid index").unwrap();
    assert!(
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![appended_subkey(0, "rejected")],
            PASSWORD
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read(subs.join("key0.keystore")).unwrap(), original);
    assert_eq!(std::fs::read(subs.join("derived_meta.json")).unwrap(), b"invalid index");
}

#[tokio::test]
async fn derived_bulk_protection_retains_legacy_compatibility() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    native.fail_protect.store(true, Ordering::SeqCst);
    WalletApplication::store_subkeys(
        ctx,
        WALLET,
        vec![appended_subkey(0, "legacy-append")],
        PASSWORD,
    )
    .await
    .unwrap();
    let keys = WalletApplication::load_account_data(
        ctx,
        WALLET,
        &wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap(),
        PASSWORD,
    )
    .await
    .unwrap();
    assert_eq!(keys.len(), 3);
    assert!(!std::fs::read(subs.join("key0.keystore")).unwrap().starts_with(b"WDK"));
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn derived_bulk_protection_finishes_accepted_write_after_waiter_cancellation() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    native.pause_protect.store(true, Ordering::SeqCst);
    let first = tokio::spawn(WalletApplication::store_subkeys(
        ctx,
        WALLET,
        vec![appended_subkey(2, "first")],
        PASSWORD,
    ));
    tokio::time::timeout(std::time::Duration::from_secs(60), native.protect_entered.notified())
        .await
        .unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    native.protect_resume.notify_one();
    WalletApplication::store_subkeys(ctx, WALLET, vec![appended_subkey(2, "second")], PASSWORD)
        .await
        .unwrap();
    let keys = WalletApplication::load_account_data(
        ctx,
        WALLET,
        &wallet_utils::address::AccountIndexMap::from_account_id(2).unwrap(),
        PASSWORD,
    )
    .await
    .unwrap();
    assert_eq!(
        keys.len(),
        2,
        "accepted operation must retain its guard through ciphertext and index publication"
    );
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(subs.join("derived_meta.json")).unwrap()).unwrap();
    assert_eq!(index["accounts"]["2"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn derived_bulk_protection_refuses_legacy_or_missing_known_file_under_protected_root() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native).await;
    let subs = derived_password_fixture(ctx).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let before = std::fs::read(subs.join("key0.keystore")).unwrap();
    let index = std::fs::read(subs.join("derived_meta.json")).unwrap();
    assert!(
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![appended_subkey(0, "refused")],
            PASSWORD
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read(subs.join("key0.keystore")).unwrap(), before);
    std::fs::remove_file(subs.join("key0.keystore")).unwrap();
    assert!(
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![appended_subkey(0, "refused")],
            PASSWORD
        )
        .await
        .is_err()
    );
    assert!(!subs.join("key0.keystore").exists());
    assert_eq!(std::fs::read(subs.join("derived_meta.json")).unwrap(), index);
}

fn legacy_upgrade_fixture(ctx: &Context) -> std::path::PathBuf {
    use wallet_tree::wallet_hierarchy::{WalletTreeOps, v1::LegacyWalletTree};
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    ctx.get_global_dirs().get_subs_dir(WALLET).unwrap();
    LegacyWalletTree::default()
        .io()
        .store_root(
            WALLET,
            b"synthetic seed",
            PHRASE,
            &root,
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .unwrap();
    root
}

#[tokio::test]
async fn protection_legacy_upgrade_wrong_password_keeps_originals_and_config() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let root = legacy_upgrade_fixture(ctx);
    let seed = std::fs::read(root.join("fixturewallet-seed")).unwrap();
    let phrase = std::fs::read(root.join("fixturewallet-phrase")).unwrap();
    assert!(WalletApplication::upgrade_algorithm(ctx, "wrong-password").await.is_err());
    assert_eq!(std::fs::read(root.join("fixturewallet-seed")).unwrap(), seed);
    assert_eq!(std::fs::read(root.join("fixturewallet-phrase")).unwrap(), phrase);
    assert!(!root.join("root.keystore").exists());
    assert!(!ctx.get_global_dirs().root_dir.join("verify").exists());
    assert!(
        wallet_database::repositories::config::ConfigRepo::find_by_key(
            "wallet_tree_strategy",
            &ctx.core_pool().unwrap()
        )
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn protection_legacy_upgrade_does_not_overwrite_protected_root() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native).await;
    let root = legacy_upgrade_fixture(ctx);
    KeystoreProtection::new(ctx.device_protection().clone())
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let before = std::fs::read(root.join("root.keystore")).unwrap();
    let legacy = std::fs::read(root.join("fixturewallet-seed")).unwrap();
    assert!(WalletApplication::upgrade_algorithm(ctx, PASSWORD).await.is_err());
    assert_eq!(std::fs::read(root.join("root.keystore")).unwrap(), before);
    assert_eq!(std::fs::read(root.join("fixturewallet-seed")).unwrap(), legacy);
    assert!(!ctx.get_global_dirs().root_dir.join("verify").exists());
}

#[tokio::test]
async fn protection_legacy_upgrade_retains_successful_conversion() {
    use wallet_tree::wallet_hierarchy::{WalletTreeOps, v2::ModernWalletTree};
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let root = legacy_upgrade_fixture(ctx);
    WalletApplication::upgrade_algorithm(ctx, PASSWORD).await.unwrap();
    let converted = ModernWalletTree::default().io().load_root(WALLET, &root, PASSWORD).unwrap();
    assert!(converted.phrase() == PHRASE);
    assert!(converted.seed() == b"synthetic seed");
    assert!(!root.join("fixturewallet-seed").exists());
    assert!(!root.join("fixturewallet-phrase").exists());
    assert!(ctx.get_global_dirs().root_dir.join("verify").is_file());
    let config = wallet_database::repositories::config::ConfigRepo::find_by_key(
        "wallet_tree_strategy",
        &ctx.core_pool().unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(config.value, r#"{"wallet_tree_strategy":"v2"}"#);
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn protection_password_entry_resumes_partially_cleaned_published_v2_upgrade() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native).await;
    protection_device_fixture(ctx).await;
    register_wallet_for_password_change(ctx).await;
    let root = legacy_upgrade_fixture(ctx);
    let legacy_phrase = std::fs::read(root.join("fixturewallet-phrase")).unwrap();
    WalletApplication::upgrade_algorithm(ctx, PASSWORD).await.unwrap();

    // Recreate the durable state left by a crash after one legacy source was removed
    // but before the final legacy marker was cleared.
    std::fs::write(root.join("fixturewallet-phrase"), legacy_phrase).unwrap();
    wallet_database::repositories::device::DeviceRepo::update_password(
        ctx.core_pool().unwrap(),
        ctx.get_sn(),
        Some("legacy"),
    )
    .await
    .unwrap();

    WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
    assert!(!root.join("fixturewallet-phrase").exists());
    let opened = WalletApplication::load_root(ctx, WALLET, PASSWORD).await.unwrap();
    assert_eq!(opened.phrase(), PHRASE);
    let device = wallet_database::repositories::device::DeviceRepo::get_device_info(
        ctx.core_pool().unwrap(),
        ctx.get_sn(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(device.password.is_none());
}

#[tokio::test]
async fn protection_legacy_upgrade_refuses_migration_progress_even_if_file_missing() {
    use wallet_database::repositories::device_protection::{MigrationRepo, MigrationState};
    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    let root = legacy_upgrade_fixture(ctx);
    let binding = ProtectionContext::new(WALLET, "root-keystore").unwrap();
    let scope = crate::device_protection::migration::record_scope(&binding);
    let pool = ctx.core_pool().unwrap();
    let original = std::fs::read(root.join("fixturewallet-seed")).unwrap();
    MigrationRepo::begin(&scope, &pool).await.unwrap();
    for completed in [false, true] {
        if completed {
            MigrationRepo::complete(&scope, &pool).await.unwrap();
        }
        assert!(WalletApplication::upgrade_algorithm(ctx, PASSWORD).await.is_err());
        assert_eq!(std::fs::read(root.join("fixturewallet-seed")).unwrap(), original);
        assert!(!root.join("root.keystore").exists());
        assert!(!ctx.get_global_dirs().root_dir.join("verify").exists());
        assert_eq!(
            MigrationRepo::get(&scope, &pool).await.unwrap(),
            Some(if completed { MigrationState::DeviceRequired } else { MigrationState::Pending })
        );
    }
}

#[tokio::test]
async fn protection_legacy_upgrade_checks_derived_and_verify_before_any_write() {
    for verify in [false, true] {
        let dir = directory();
        let ctx = context(dir.path(), Arc::new(Native::default())).await;
        let root = legacy_upgrade_fixture(ctx);
        let path = if verify {
            ctx.get_global_dirs().root_dir.join("verify")
        } else {
            ctx.get_global_dirs().get_subs_dir(WALLET).unwrap().join("key0.keystore")
        };
        // Even an unsupported protected envelope must never reach a legacy writer.
        std::fs::write(&path, b"WDK\x7fsynthetic invalid envelope").unwrap();
        let original = std::fs::read(root.join("fixturewallet-seed")).unwrap();
        assert!(WalletApplication::upgrade_algorithm(ctx, PASSWORD).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"WDK\x7fsynthetic invalid envelope");
        assert_eq!(std::fs::read(root.join("fixturewallet-seed")).unwrap(), original);
        assert!(!root.join("root.keystore").exists());
        assert!(
            wallet_database::repositories::config::ConfigRepo::find_by_key(
                "wallet_tree_strategy",
                &ctx.core_pool().unwrap()
            )
            .await
            .unwrap()
            .is_none()
        );
    }
}

#[tokio::test]
async fn protection_account_delete_recreate_preserves_other_accounts_and_device_requirement() {
    use wallet_database::repositories::device_protection::{MigrationRepo, MigrationState};
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    let scope = crate::device_protection::migration::record_scope(&binding);
    let codec = KeystoreProtection::new(ctx.device_protection().clone());
    codec
        .migrate_file(
            &subs.join("key0.keystore"),
            &binding,
            PASSWORD,
            ReadPolicy::LegacyMigrationAllowed,
        )
        .await
        .unwrap();
    let other = std::fs::read(subs.join("key1.keystore")).unwrap();
    WalletApplication::delete_account_files(ctx, WALLET, 0).await.unwrap();
    // Retrying a deletion must remain harmless and keep recreation permission.
    WalletApplication::delete_account_files(ctx, WALLET, 0).await.unwrap();
    assert!(!subs.join("key0.keystore").exists());
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(subs.join("derived_meta.json")).unwrap()).unwrap();
    assert!(index["accounts"].get("0").is_none());
    assert_eq!(
        MigrationRepo::get(&scope, &ctx.core_pool().unwrap()).await.unwrap(),
        Some(MigrationState::RecreationAllowed)
    );
    // Root is still legacy: the deletion record itself must enforce new protection.
    native.fail_protect.store(true, Ordering::SeqCst);
    assert!(
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![appended_subkey(0, "recreated")],
            PASSWORD
        )
        .await
        .is_err()
    );
    assert!(!subs.join("key0.keystore").exists());
    native.fail_protect.store(false, Ordering::SeqCst);
    WalletApplication::store_subkeys(ctx, WALLET, vec![appended_subkey(0, "recreated")], PASSWORD)
        .await
        .unwrap();
    assert!(std::fs::read(subs.join("key0.keystore")).unwrap().starts_with(b"WDK\x01"));
    assert_eq!(
        MigrationRepo::get(&scope, &ctx.core_pool().unwrap()).await.unwrap(),
        Some(MigrationState::DeviceRequired)
    );
    assert_eq!(std::fs::read(subs.join("key1.keystore")).unwrap(), other);
    let index = wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap();
    let data = WalletApplication::load_account_data(ctx, WALLET, &index, PASSWORD).await.unwrap();
    assert_eq!(data.len(), 1);
    assert!(data.keys().all(|key| key.address == "recreated"));
    // A later accidental loss is not another authorized deletion.
    std::fs::remove_file(subs.join("key0.keystore")).unwrap();
    assert!(
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![appended_subkey(0, "must-not-recreate")],
            PASSWORD
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn protection_account_delete_rejects_damaged_index_without_side_effects() {
    use wallet_database::repositories::device_protection::MigrationRepo;
    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    let subs = derived_password_fixture(ctx).await;
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .migrate_file(
            &subs.join("key0.keystore"),
            &binding,
            PASSWORD,
            ReadPolicy::LegacyMigrationAllowed,
        )
        .await
        .unwrap();
    let original = std::fs::read(subs.join("key0.keystore")).unwrap();
    std::fs::write(subs.join("derived_meta.json"), b"bad index").unwrap();
    assert!(WalletApplication::delete_account_files(ctx, WALLET, 0).await.is_err());
    assert_eq!(std::fs::read(subs.join("key0.keystore")).unwrap(), original);
    assert_eq!(std::fs::read(subs.join("derived_meta.json")).unwrap(), b"bad index");
    assert_eq!(
        MigrationRepo::get(
            &crate::device_protection::migration::record_scope(&binding),
            &ctx.core_pool().unwrap()
        )
        .await
        .unwrap(),
        None
    );
}

#[tokio::test]
async fn protection_account_delete_partial_removal_requires_retry_and_blocks_legacy_migration() {
    use wallet_database::repositories::device_protection::MigrationRepo;
    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    let subs = derived_password_fixture(ctx).await;
    let path = subs.join("key0.keystore");
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    let scope = crate::device_protection::migration::record_scope(&binding);
    MigrationRepo::authorize_recreation(&scope, &ctx.core_pool().unwrap()).await.unwrap();
    let original = std::fs::read(&path).unwrap();
    assert!(
        KeystoreProtection::new(ctx.device_protection().clone())
            .migrate_files(
                &ctx.core_pool().unwrap(),
                &[MigrationFile { path: &path, context: &binding }],
                PASSWORD,
                ReadPolicy::LegacyMigrationAllowed,
            )
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_file(&path).unwrap();
    // Represents termination between file removal and index publication.
    assert!(
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![appended_subkey(0, "too-early")],
            PASSWORD
        )
        .await
        .is_err()
    );
    WalletApplication::delete_account_files(ctx, WALLET, 0).await.unwrap();
    WalletApplication::store_subkeys(
        ctx,
        WALLET,
        vec![appended_subkey(0, "after-retry")],
        PASSWORD,
    )
    .await
    .unwrap();
    assert!(std::fs::read(&path).unwrap().starts_with(b"WDK\x01"));
}

#[tokio::test]
async fn protection_account_delete_retains_legacy_behavior() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    let other = std::fs::read(subs.join("key1.keystore")).unwrap();
    WalletApplication::delete_account_files(ctx, WALLET, 0).await.unwrap();
    assert!(!subs.join("key0.keystore").exists());
    assert_eq!(std::fs::read(subs.join("key1.keystore")).unwrap(), other);
    assert_eq!(native.opens.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn protection_account_delete_state_write_failure_preserves_file_and_index() {
    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    let subs = derived_password_fixture(ctx).await;
    let path = subs.join("key0.keystore");
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .migrate_file(&path, &binding, PASSWORD, ReadPolicy::LegacyMigrationAllowed)
        .await
        .unwrap();
    let original = std::fs::read(&path).unwrap();
    let metadata = std::fs::read(subs.join("derived_meta.json")).unwrap();
    let pool = ctx.core_pool().unwrap();
    sqlx::query("CREATE TRIGGER reject_deletion_state BEFORE INSERT ON config WHEN NEW.key LIKE 'device_protection.file.v1:%' BEGIN SELECT RAISE(ABORT, 'fixture refusal'); END")
        .execute(pool.write_ref()).await.unwrap();
    assert!(WalletApplication::delete_account_files(ctx, WALLET, 0).await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read(subs.join("derived_meta.json")).unwrap(), metadata);
    sqlx::query("DROP TRIGGER reject_deletion_state").execute(pool.write_ref()).await.unwrap();
    WalletApplication::delete_account_files(ctx, WALLET, 0).await.unwrap();
    assert!(!path.exists());
}

#[tokio::test]
async fn protection_account_delete_waits_for_cancelled_password_change_publication() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    let path = subs.join("key0.keystore");
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    let codec = KeystoreProtection::new(ctx.device_protection().clone());
    codec
        .migrate_file(&path, &binding, PASSWORD, ReadPolicy::LegacyMigrationAllowed)
        .await
        .unwrap();
    native.pause_protect.store(true, Ordering::SeqCst);
    let changing = tokio::spawn(async move {
        let index = wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap();
        WalletApplication::change_account_password(
            ctx,
            WALLET,
            &index,
            PASSWORD,
            "new-fixture-password",
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(60), native.protect_entered.notified())
        .await
        .unwrap();
    changing.abort();
    assert!(changing.await.unwrap_err().is_cancelled());
    native.protect_resume.notify_one();
    // Deletion uses this same gate. It must not acquire it ahead of publication.
    let guard = super::derived_store::lock().await;
    assert!(codec.load_file(&path, &binding, "new-fixture-password").await.is_ok());
    drop(guard);
    WalletApplication::delete_account_files(ctx, WALLET, 0).await.unwrap();
    assert!(!path.exists());
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(subs.join("derived_meta.json")).unwrap()).unwrap();
    assert!(metadata["accounts"].get("0").is_none());
}

#[tokio::test]
async fn protection_wallet_delete_reimports_root_and_all_account_files() {
    use wallet_database::repositories::device_protection::{MigrationRepo, MigrationState};
    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    let subs = derived_password_fixture(ctx).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    let codec = KeystoreProtection::new(ctx.device_protection().clone());
    codec
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    for id in 0..2 {
        codec
            .migrate_file(
                &subs.join(format!("key{id}.keystore")),
                &ProtectionContext::new(WALLET, format!("derived:{id}")).unwrap(),
                PASSWORD,
                ReadPolicy::LegacyMigrationAllowed,
            )
            .await
            .unwrap();
    }
    WalletApplication::delete_wallet_files(ctx, Some(WALLET)).await.unwrap();
    WalletApplication::delete_wallet_files(ctx, Some(WALLET)).await.unwrap();
    assert!(!ctx.get_global_dirs().get_wallet_dir(Some(WALLET)).unwrap().exists());
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    WalletApplication::store_subkeys(
        ctx,
        WALLET,
        vec![appended_subkey(0, "reimport-a"), appended_subkey(1, "reimport-b")],
        PASSWORD,
    )
    .await
    .unwrap();
    assert!(std::fs::read(root.join("root.keystore")).unwrap().starts_with(b"WDK\x01"));
    for purpose in ["root-keystore", "derived:0", "derived:1"] {
        let binding = ProtectionContext::new(WALLET, purpose).unwrap();
        assert_eq!(
            MigrationRepo::get(
                &crate::device_protection::migration::record_scope(&binding),
                &ctx.core_pool().unwrap()
            )
            .await
            .unwrap(),
            Some(MigrationState::DeviceRequired)
        );
    }
    for id in 0..2 {
        assert!(
            std::fs::read(subs.join(format!("key{id}.keystore"))).unwrap().starts_with(b"WDK\x01")
        );
    }
    assert!(WalletApplication::load_root(ctx, WALLET, PASSWORD).await.unwrap().phrase() == PHRASE);
}

#[tokio::test]
async fn protection_wallet_delete_state_failure_preserves_directory() {
    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    let subs = derived_password_fixture(ctx).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    KeystoreProtection::new(ctx.device_protection().clone())
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    let original = std::fs::read(root.join("root.keystore")).unwrap();
    let child = std::fs::read(subs.join("key0.keystore")).unwrap();
    sqlx::query("CREATE TRIGGER reject_wallet_deletion BEFORE INSERT ON config WHEN NEW.key LIKE 'device_protection.file.v1:%' BEGIN SELECT RAISE(ABORT, 'fixture refusal'); END").execute(ctx.core_pool().unwrap().write_ref()).await.unwrap();
    assert!(WalletApplication::delete_wallet_files(ctx, Some(WALLET)).await.is_err());
    assert_eq!(std::fs::read(root.join("root.keystore")).unwrap(), original);
    assert_eq!(std::fs::read(subs.join("key0.keystore")).unwrap(), child);
}

#[tokio::test]
async fn protection_wallet_delete_reset_keeps_base_and_unrelated_directories() {
    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    derived_password_fixture(ctx).await;
    WalletApplication::store_root(
        ctx,
        "otherwallet",
        RootData::new(PHRASE, b"synthetic seed"),
        PASSWORD,
    )
    .await
    .unwrap();
    let unrelated = ctx.get_global_dirs().export_dir.join("public-fixture");
    std::fs::write(&unrelated, b"keep").unwrap();
    WalletApplication::delete_wallet_files(ctx, None).await.unwrap();
    assert!(ctx.get_global_dirs().wallet_dir.is_dir());
    assert_eq!(std::fs::read_dir(&ctx.get_global_dirs().wallet_dir).unwrap().count(), 0);
    assert_eq!(std::fs::read(&unrelated).unwrap(), b"keep");
}

#[tokio::test]
async fn protection_wallet_delete_waits_for_cancelled_root_write() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    derived_password_fixture(ctx).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap();
    let codec = KeystoreProtection::new(ctx.device_protection().clone());
    codec
        .store_root(
            &root,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .await
        .unwrap();
    native.pause_protect.store(true, Ordering::SeqCst);
    let writing = tokio::spawn(WalletApplication::store_root(
        ctx,
        WALLET,
        RootData::new("synthetic replacement", b"replacement seed"),
        PASSWORD,
    ));
    tokio::time::timeout(std::time::Duration::from_secs(60), native.protect_entered.notified())
        .await
        .unwrap();
    writing.abort();
    assert!(writing.await.unwrap_err().is_cancelled());
    native.protect_resume.notify_one();
    let guard = super::derived_store::lock().await;
    assert!(
        codec.load_root(&root, WALLET, PASSWORD).await.unwrap().phrase() == "synthetic replacement"
    );
    drop(guard);
    WalletApplication::delete_wallet_files(ctx, Some(WALLET)).await.unwrap();
    assert!(!root.exists());
}

#[tokio::test]
async fn protection_wallet_delete_reset_includes_inactive_missing_wallets() {
    use wallet_database::repositories::device_protection::{MigrationRepo, MigrationState};
    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    register_wallet_for_password_change(ctx).await;
    let binding = ProtectionContext::new(WALLET, "root-keystore").unwrap();
    let scope = crate::device_protection::migration::record_scope(&binding);
    let pool = ctx.core_pool().unwrap();
    MigrationRepo::begin(&scope, &pool).await.unwrap();
    MigrationRepo::complete(&scope, &pool).await.unwrap();
    // A missing protected root cannot be silently recreated by ordinary writes.
    assert!(
        WalletApplication::store_root(
            ctx,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD
        )
        .await
        .is_err()
    );
    std::fs::remove_dir_all(ctx.get_global_dirs().get_wallet_dir(Some(WALLET)).unwrap()).unwrap();
    sqlx::query("UPDATE wallet SET status=2 WHERE address=?")
        .bind(WALLET)
        .execute(pool.write_ref())
        .await
        .unwrap();
    WalletApplication::delete_wallet_files(ctx, None).await.unwrap();
    assert_eq!(
        MigrationRepo::get(&scope, &pool).await.unwrap(),
        Some(MigrationState::RecreationAllowed)
    );
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    assert!(WalletApplication::load_root(ctx, WALLET, PASSWORD).await.unwrap().phrase() == PHRASE);
}

#[tokio::test]
async fn protection_initialization_empty_storage_is_verified_and_never_reprovisions_ready_key() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    WalletApplication::initialize_device_protection(ctx).await.unwrap();
    WalletApplication::initialize_device_protection(ctx).await.unwrap();
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
    native.key_missing.store(true, Ordering::SeqCst);
    assert!(WalletApplication::initialize_device_protection(ctx).await.is_err());
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read_dir(&ctx.get_global_dirs().wallet_dir).unwrap().count(), 0);
}

#[tokio::test]
async fn protection_initialization_refuses_existing_data_and_migration_history() {
    use wallet_database::repositories::device_protection::MigrationRepo;
    for scenario in 0..4 {
        let dir = directory();
        let native = Arc::new(Native::default());
        let ctx = context(dir.path(), native.clone()).await;
        match scenario {
            0 => {
                legacy_upgrade_fixture(ctx);
            }
            1 => {
                register_wallet_for_password_change(ctx).await;
            }
            2 => {
                MigrationRepo::begin("previous-record", &ctx.core_pool().unwrap()).await.unwrap();
            }
            _ => {
                std::fs::write(ctx.get_global_dirs().root_dir.join("verify"), b"existing verify")
                    .unwrap();
            }
        }
        assert!(WalletApplication::initialize_device_protection(ctx).await.is_err());
        assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn protection_initialization_probe_failure_is_retryable_only_before_ready() {
    use wallet_database::repositories::config::ConfigRepo;
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    native.fail_protect.store(true, Ordering::SeqCst);
    assert!(WalletApplication::initialize_device_protection(ctx).await.is_err());
    assert_eq!(
        ConfigRepo::find_by_key("device_protection.key", &ctx.core_pool().unwrap())
            .await
            .unwrap()
            .unwrap()
            .value,
        "provisioning-v1"
    );
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
    native.fail_protect.store(false, Ordering::SeqCst);
    WalletApplication::initialize_device_protection(ctx).await.unwrap();
    let reopened = context(dir.path(), native.clone()).await;
    WalletApplication::initialize_device_protection(reopened).await.unwrap();
    assert_eq!(native.provisions.load(Ordering::SeqCst), 2);
    assert_eq!(
        ConfigRepo::find_by_key("device_protection.key", &ctx.core_pool().unwrap())
            .await
            .unwrap()
            .unwrap()
            .value,
        "ready-v1"
    );
}

#[tokio::test]
async fn protection_initialization_refuses_api_wallets_and_unknown_state() {
    use wallet_database::repositories::{api_wallet::wallet::ApiWalletRepo, config::ConfigRepo};
    for scenario in 0..3 {
        let dir = directory();
        let native = Arc::new(Native::default());
        let ctx = context(dir.path(), native.clone()).await;
        if scenario == 0 {
            ApiWalletRepo::upsert(
                &ctx.api_wallet_pool().unwrap(),
                "fixture-uid",
                "fixture-name",
                "fixture-address",
                b"synthetic phrase",
                b"synthetic seed",
                wallet_database::entities::api_wallet::ApiWalletType::SubAccount,
                None,
                "fixture-sn",
                0,
            )
            .await
            .unwrap();
        } else {
            ConfigRepo::upsert(
                "device_protection.key",
                if scenario == 1 { "future-version" } else { "ready-v1" },
                Some(if scenario == 1 { 1 } else { 0 }),
                &ctx.core_pool().unwrap(),
            )
            .await
            .unwrap();
        }
        assert!(WalletApplication::initialize_device_protection(ctx).await.is_err());
        assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn protection_legacy_initialization_verifies_password_without_rewriting_files() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let root = legacy_upgrade_fixture(ctx);
    let original = std::fs::read(root.join("fixturewallet-seed")).unwrap();
    assert!(
        WalletApplication::initialize_device_protection_for_legacy(ctx, "wrong-password")
            .await
            .is_err()
    );
    assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    WalletApplication::initialize_device_protection_for_legacy(ctx, PASSWORD).await.unwrap();
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read(root.join("fixturewallet-seed")).unwrap(), original);
    assert!(!root.join("root.keystore").exists());
}

#[tokio::test]
async fn protection_legacy_initialization_rejects_protected_and_missing_files() {
    for protected in [false, true] {
        let dir = directory();
        let native = Arc::new(Native::default());
        let ctx = context(dir.path(), native.clone()).await;
        let subs = derived_password_fixture(ctx).await;
        WalletApplication::store_root(
            ctx,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD,
        )
        .await
        .unwrap();
        if protected {
            let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
            KeystoreProtection::new(ctx.device_protection().clone())
                .migrate_file(
                    &subs.join("key0.keystore"),
                    &binding,
                    PASSWORD,
                    ReadPolicy::LegacyMigrationAllowed,
                )
                .await
                .unwrap();
        } else {
            std::fs::remove_file(subs.join("key1.keystore")).unwrap();
        }
        assert!(
            WalletApplication::initialize_device_protection_for_legacy(ctx, PASSWORD)
                .await
                .is_err()
        );
        assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn protection_legacy_initialization_v2_retry_keeps_all_ciphertext() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    let original = std::fs::read(subs.join("key0.keystore")).unwrap();
    native.fail_protect.store(true, Ordering::SeqCst);
    assert!(
        WalletApplication::initialize_device_protection_for_legacy(ctx, PASSWORD).await.is_err()
    );
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
    native.fail_protect.store(false, Ordering::SeqCst);
    WalletApplication::initialize_device_protection_for_legacy(ctx, PASSWORD).await.unwrap();
    assert_eq!(native.provisions.load(Ordering::SeqCst), 2);
    assert_eq!(std::fs::read(subs.join("key0.keystore")).unwrap(), original);
}

#[tokio::test]
async fn protection_legacy_initialization_verifies_existing_api_packages_without_changes() {
    use crate::{
        domain::api_wallet::wallet::ApiWalletDomain,
        infrastructure::phrase_package::PhrasePackageCodec,
    };
    use wallet_database::{
        entities::api_wallet::ApiWalletType, repositories::api_wallet::wallet::ApiWalletRepo,
    };
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let phrase = PhrasePackageCodec::encrypt_phrase(PASSWORD, PHRASE).await.unwrap();
    let seed = ApiWalletDomain::encrypt_seed_bundle(PASSWORD, b"synthetic api seed").await.unwrap();
    let pool = ctx.api_wallet_pool().unwrap();
    ApiWalletRepo::upsert(
        &pool,
        "fixture-uid",
        "fixture-name",
        "fixture-address",
        &phrase,
        &seed,
        ApiWalletType::SubAccount,
        None,
        "fixture-sn",
        0,
    )
    .await
    .unwrap();
    assert!(
        WalletApplication::initialize_device_protection_for_legacy(ctx, "wrong-password")
            .await
            .is_err()
    );
    assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    WalletApplication::initialize_device_protection_for_legacy(ctx, PASSWORD).await.unwrap();
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
    let stored: (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT phrase, seed FROM api_wallet WHERE address='fixture-address'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(stored, (phrase, seed));
}

#[tokio::test]
async fn protection_legacy_initialization_refuses_history_and_missing_db_wallet() {
    use wallet_database::repositories::device_protection::MigrationRepo;
    for history in [true, false] {
        let dir = directory();
        let native = Arc::new(Native::default());
        let ctx = context(dir.path(), native.clone()).await;
        if history {
            legacy_upgrade_fixture(ctx);
            MigrationRepo::begin("old-device-record", &ctx.core_pool().unwrap()).await.unwrap();
        } else {
            register_wallet_for_password_change(ctx).await;
        }
        assert!(
            WalletApplication::initialize_device_protection_for_legacy(ctx, PASSWORD)
                .await
                .is_err()
        );
        assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn protection_legacy_initialization_rejects_missing_keys_inside_v2_file() {
    use wallet_tree::wallet_hierarchy::{WalletTreeOps, v2::ModernWalletTree};
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    ModernWalletTree::default()
        .io()
        .store(
            "key0.keystore",
            &b"{}".to_vec(),
            &subs,
            PASSWORD,
            wallet_crypto::KdfAlgorithm::Scrypt,
        )
        .unwrap();
    let before = std::fs::read(subs.join("key0.keystore")).unwrap();
    assert!(
        WalletApplication::initialize_device_protection_for_legacy(ctx, PASSWORD).await.is_err()
    );
    assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(subs.join("key0.keystore")).unwrap(), before);
}

#[tokio::test]
async fn protection_rollout_migrates_all_v2_files_once_and_preserves_inner_data() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    let first = WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.unwrap();
    assert_eq!(first.converted, 3);
    let before = std::fs::read(subs.join("key0.keystore")).unwrap();
    assert!(before.starts_with(b"WDK\x01"));
    let second = WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.unwrap();
    assert_eq!(second.converted, 0);
    assert_eq!(second.already_protected, 3);
    assert_eq!(std::fs::read(subs.join("key0.keystore")).unwrap(), before);
    assert_eq!(WalletService::new(ctx).get_phrase(WALLET, PASSWORD).await.unwrap().phrase, PHRASE);
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protection_rollout_wraps_authenticated_inventory_without_per_file_password_kdf() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    let path = subs.join("key0.keystore");
    let before = std::fs::read(&path).unwrap();
    WalletApplication::migrate_wallet_file_protection(ctx, "already-authenticated").await.unwrap();
    let wrapped = std::fs::read(&path).unwrap();
    assert!(wrapped.starts_with(b"WDK\x01"));
    assert_ne!(wrapped, before);
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protection_rollout_resumes_published_file_after_progress_write_failure() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER fail_protection_completion BEFORE UPDATE ON config WHEN NEW.value='device-required-v1' BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END")
        .execute(ctx.core_pool().unwrap().write_ref()).await.unwrap();
    assert!(WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.is_err());
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap().join("root.keystore");
    let published = std::fs::read(&root).unwrap();
    assert!(published.starts_with(b"WDK\x01"));
    assert!(!std::fs::read(subs.join("key0.keystore")).unwrap().starts_with(b"WDK"));
    sqlx::query("DROP TRIGGER fail_protection_completion")
        .execute(ctx.core_pool().unwrap().write_ref())
        .await
        .unwrap();
    let result = WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.unwrap();
    assert_eq!(result.converted, 2);
    assert_eq!(result.already_protected, 1);
    assert_eq!(std::fs::read(root).unwrap(), published);
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
    assert_eq!(native.opens.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn protection_rollout_refuses_v1_and_missing_unindexed_protected_record() {
    use wallet_tree::wallet_hierarchy::{WalletTreeOps, v2::ModernWalletTree};
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    legacy_upgrade_fixture(ctx);
    assert!(WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.is_err());
    assert_eq!(native.provisions.load(Ordering::SeqCst), 0);

    let other = directory();
    let ctx = context(other.path(), native.clone()).await;
    derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    ModernWalletTree::default()
        .io()
        .store(
            "verify",
            &"data",
            &ctx.get_global_dirs().root_dir,
            PASSWORD,
            wallet_tree::KdfAlgorithm::Scrypt,
        )
        .unwrap();
    assert_eq!(
        WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.unwrap().converted,
        4
    );
    std::fs::remove_file(ctx.get_global_dirs().root_dir.join("verify")).unwrap();
    assert!(WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.is_err());
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protection_rollout_empty_activation_protects_new_root_and_derived() {
    use wallet_tree::file_ops::BulkSubkey;
    use wallet_utils::address::AccountIndexMap;
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    wallet_database::repositories::config::ConfigRepo::upsert(
        "wallet_tree_strategy",
        r#"{"wallet_tree_strategy":"v2"}"#,
        Some(0),
        &ctx.core_pool().unwrap(),
    )
    .await
    .unwrap();
    WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.unwrap();
    native.fail_protect.store(true, Ordering::SeqCst);
    assert!(
        WalletApplication::store_root(
            ctx,
            WALLET,
            RootData::new(PHRASE, b"synthetic seed"),
            PASSWORD
        )
        .await
        .is_err()
    );
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap().join("root.keystore");
    assert!(!root.exists());
    native.fail_protect.store(false, Ordering::SeqCst);
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    assert!(std::fs::read(&root).unwrap().starts_with(b"WDK\x01"));
    native.fail_protect.store(true, Ordering::SeqCst);
    assert!(
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![BulkSubkey::new(
                AccountIndexMap::from_account_id(0).unwrap(),
                "fixture-eth",
                "eth",
                "m/0",
                b"synthetic private key".to_vec(),
            )],
            PASSWORD,
        )
        .await
        .is_err()
    );
    native.fail_protect.store(false, Ordering::SeqCst);
    WalletApplication::store_subkeys(
        ctx,
        WALLET,
        vec![BulkSubkey::new(
            AccountIndexMap::from_account_id(0).unwrap(),
            "fixture-eth",
            "eth",
            "m/0",
            b"synthetic private key".to_vec(),
        )],
        PASSWORD,
    )
    .await
    .unwrap();
    let binding = ProtectionContext::new(WALLET, "derived:0").unwrap();
    assert_eq!(
        wallet_database::repositories::device_protection::MigrationRepo::get(
            &crate::device_protection::migration::record_scope(&binding),
            &ctx.core_pool().unwrap()
        )
        .await
        .unwrap(),
        Some(wallet_database::repositories::device_protection::MigrationState::DeviceRequired)
    );
    assert!(
        std::fs::read(ctx.get_global_dirs().get_subs_dir(WALLET).unwrap().join("key0.keystore"))
            .unwrap()
            .starts_with(b"WDK\x01")
    );
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protection_rollout_active_mode_rejects_new_legacy_file() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    let subs = derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    let legacy = std::fs::read(subs.join("key0.keystore")).unwrap();
    WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.unwrap();
    // Valid old-format ciphertext with no pre-existing progress/index entry.
    std::fs::write(subs.join("key99.keystore"), &legacy).unwrap();
    assert!(WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.is_err());
    assert_eq!(std::fs::read(subs.join("key99.keystore")).unwrap(), legacy);
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protection_rollout_pending_derived_loss_never_rebuilds_from_partial_keys() {
    use wallet_database::repositories::device_protection::MigrationRepo;
    use wallet_tree::file_ops::BulkSubkey;
    use wallet_utils::address::AccountIndexMap;
    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.unwrap();
    let binding = ProtectionContext::new(WALLET, "derived:7").unwrap();
    // Durable state after publication but before metadata/complete, then file loss.
    MigrationRepo::begin(
        &crate::device_protection::migration::record_scope(&binding),
        &ctx.core_pool().unwrap(),
    )
    .await
    .unwrap();
    let subs = ctx.get_global_dirs().get_subs_dir(WALLET).unwrap();
    let before = std::fs::read(subs.join("derived_meta.json")).unwrap();
    assert!(
        WalletApplication::store_subkeys(
            ctx,
            WALLET,
            vec![BulkSubkey::new(
                AccountIndexMap::from_account_id(7).unwrap(),
                "partial-eth",
                "eth",
                "m/7",
                b"synthetic partial key".to_vec()
            )],
            PASSWORD
        )
        .await
        .is_err()
    );
    assert!(!subs.join("key7.keystore").exists());
    assert_eq!(std::fs::read(subs.join("derived_meta.json")).unwrap(), before);
}

#[tokio::test]
async fn protected_derived_first_creation_resumes_after_prepublication_restart() {
    use wallet_database::repositories::device_protection::{MigrationRepo, MigrationState};
    use wallet_tree::file_ops::BulkSubkey;
    use wallet_utils::address::AccountIndexMap;

    let dir = directory();
    let ctx = context(dir.path(), Arc::new(Native::default())).await;
    derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD).await.unwrap();
    let binding = ProtectionContext::new(WALLET, "derived:7").unwrap();
    let scope = crate::device_protection::migration::record_scope(&binding);
    MigrationRepo::authorize_creation(&scope, &ctx.core_pool().unwrap()).await.unwrap();

    WalletApplication::store_subkeys(
        ctx,
        WALLET,
        vec![BulkSubkey::new(
            AccountIndexMap::from_account_id(7).unwrap(),
            "resumed-eth",
            "eth",
            "m/7",
            b"synthetic resumed key".to_vec(),
        )],
        PASSWORD,
    )
    .await
    .unwrap();

    let subs = ctx.get_global_dirs().get_subs_dir(WALLET).unwrap();
    assert!(std::fs::read(subs.join("key7.keystore")).unwrap().starts_with(b"WDK\x01"));
    assert_eq!(
        MigrationRepo::get(&scope, &ctx.core_pool().unwrap()).await.unwrap(),
        Some(MigrationState::DeviceRequired)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(subs.join("derived_meta.json")).unwrap()).unwrap();
    assert!(metadata["accounts"].get("7").is_some());
}

async fn protection_device_fixture(ctx: &Context) {
    wallet_database::repositories::device::DeviceRepo::upsert(
        ctx.core_pool().unwrap(),
        wallet_database::entities::device::CreateDeviceEntity {
            device_type: "fixture".into(),
            sn: ctx.get_sn().to_owned(),
            code: "fixture".into(),
            system_ver: "fixture".into(),
            iemi: None,
            meid: None,
            iccid: None,
            mem: None,
            app_id: None,
            is_init: 1,
            language_init: 1,
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn protection_startup_defers_provisioning_and_never_replaces_a_missing_key() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    WalletApplication::prepare_device_protection_startup(ctx).await.unwrap();
    assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    WalletApplication::initialize_device_protection(ctx).await.unwrap();
    WalletApplication::prepare_device_protection_startup(ctx).await.unwrap();
    native.key_missing.store(true, Ordering::SeqCst);
    assert!(WalletApplication::prepare_device_protection_startup(ctx).await.is_err());
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protection_password_entry_activates_v2_once_without_extra_frontend_call() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    protection_device_fixture(ctx).await;
    let subs = derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    WalletApplication::prepare_device_protection_startup(ctx).await.unwrap();
    assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    assert!(WalletApplication::validate_password(ctx, "wrong password").await.is_err());
    assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
    let before = std::fs::read(subs.join("key0.keystore")).unwrap();
    assert!(before.starts_with(b"WDK\x01"));
    WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
    assert_eq!(std::fs::read(subs.join("key0.keystore")).unwrap(), before);
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protection_password_entry_new_install_and_token_cannot_bootstrap() {
    use crate::domain::api_wallet::unlock::WalletUnlockSession;
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    protection_device_fixture(ctx).await;
    ctx.set_wallet_unlock_session(WalletUnlockSession::new(
        "synthetic-token".into(),
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        Default::default(),
    ))
    .await
    .unwrap();
    assert!(WalletApplication::validate_password(ctx, "synthetic-token").await.is_err());
    assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
    assert!(!ctx.get_global_dirs().root_dir.join("verify").exists());
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    assert!(
        std::fs::read(ctx.get_global_dirs().get_root_dir(WALLET).unwrap().join("root.keystore"))
            .unwrap()
            .starts_with(b"WDK\x01")
    );
    WalletApplication::validate_password(ctx, "synthetic-token").await.unwrap();
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protection_password_entry_native_failure_preserves_files_and_retries() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    protection_device_fixture(ctx).await;
    let subs = derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    let before = std::fs::read(subs.join("key0.keystore")).unwrap();
    native.fail_protect.store(true, Ordering::SeqCst);
    assert!(
        crate::service::api_wallet::wallet::ApiWalletService::new(ctx)
            .set_passwd_cache(PASSWORD)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(subs.join("key0.keystore")).unwrap(), before);
    assert!(ctx.wallet_unlock_session_snapshot().await.is_none());
    native.fail_protect.store(false, Ordering::SeqCst);
    WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
    assert!(std::fs::read(subs.join("key0.keystore")).unwrap().starts_with(b"WDK\x01"));
}

#[tokio::test]
async fn protection_password_entry_upgrades_v1_then_protects_it() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    protection_device_fixture(ctx).await;
    let root = legacy_upgrade_fixture(ctx);
    register_wallet_for_password_change(ctx).await;
    WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
    assert!(std::fs::read(root.join("root.keystore")).unwrap().starts_with(b"WDK\x01"));
    assert!(!root.join("fixturewallet-seed").exists());
    assert!(!root.join("fixturewallet-phrase").exists());
    assert!(
        std::fs::read(ctx.get_global_dirs().root_dir.join("verify"))
            .unwrap()
            .starts_with(b"WDK\x01")
    );
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protection_password_entry_authenticates_v1_files_without_active_db_wallet() {
    use wallet_database::repositories::device::DeviceRepo;
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    protection_device_fixture(ctx).await;
    DeviceRepo::update_password(ctx.core_pool().unwrap(), ctx.get_sn(), Some("legacy"))
        .await
        .unwrap();
    let root = legacy_upgrade_fixture(ctx);

    WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
    assert!(std::fs::read(root.join("root.keystore")).unwrap().starts_with(b"WDK\x01"));
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protection_password_entry_rejects_wrong_password_for_api_only_legacy_state() {
    use crate::{
        domain::api_wallet::wallet::ApiWalletDomain,
        infrastructure::phrase_package::PhrasePackageCodec,
    };
    use wallet_database::{
        entities::api_wallet::ApiWalletType,
        repositories::{api_wallet::wallet::ApiWalletRepo, device::DeviceRepo},
    };
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    protection_device_fixture(ctx).await;
    DeviceRepo::update_password(ctx.core_pool().unwrap(), ctx.get_sn(), Some("legacy"))
        .await
        .unwrap();
    let phrase = PhrasePackageCodec::encrypt_phrase(PASSWORD, PHRASE).await.unwrap();
    let seed = ApiWalletDomain::encrypt_seed_bundle(PASSWORD, b"synthetic api seed").await.unwrap();
    ApiWalletRepo::upsert(
        &ctx.api_wallet_pool().unwrap(),
        "fixture-uid",
        "fixture-name",
        "fixture-address",
        &phrase,
        &seed,
        ApiWalletType::SubAccount,
        None,
        "fixture-sn",
        0,
    )
    .await
    .unwrap();

    assert!(WalletApplication::validate_password(ctx, "wrong-password").await.is_err());
    assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    assert!(ctx.get_global_dirs().root_dir.join("verify").try_exists().unwrap() == false);
    let device =
        DeviceRepo::get_device_info(ctx.core_pool().unwrap(), ctx.get_sn()).await.unwrap().unwrap();
    assert_eq!(device.password.as_deref(), Some("legacy"));

    WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
    let stored: (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT phrase, seed FROM api_wallet WHERE address='fixture-address'")
            .fetch_one(ctx.api_wallet_pool().unwrap().read_ref())
            .await
            .unwrap();
    assert_eq!(stored, (phrase, seed));
}

#[tokio::test]
async fn protection_password_entry_without_provider_retains_legacy_behavior() {
    let dir = directory();
    let ctx = context_with_protection(dir.path(), DeviceProtection::default()).await;
    protection_device_fixture(ctx).await;
    let subs = derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    let before = std::fs::read(subs.join("key0.keystore")).unwrap();
    WalletApplication::prepare_device_protection_startup(ctx).await.unwrap();
    WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
    assert_eq!(std::fs::read(subs.join("key0.keystore")).unwrap(), before);
    let state: i64 =
        sqlx::query_scalar("SELECT count(*) FROM config WHERE key='device_protection.files'")
            .fetch_one(ctx.core_pool().unwrap().read_ref())
            .await
            .unwrap();
    assert_eq!(state, 0);
}

#[tokio::test]
async fn proof_reuse_status_does_not_trust_empty_inventory_or_session_token() {
    use crate::domain::api_wallet::unlock::WalletUnlockSession;
    use wallet_database::repositories::device::DeviceRepo;
    let dir = directory();
    let ctx = context_with_protection(dir.path(), DeviceProtection::default()).await;
    protection_device_fixture(ctx).await;
    DeviceRepo::update_password_proof(
        ctx.core_pool().unwrap(),
        ctx.get_sn(),
        Some("unverified-proof"),
    )
    .await
    .unwrap();
    assert!(
        WalletApplication::validate_password_with_proof_status(ctx, PASSWORD)
            .await
            .unwrap()
            .is_none()
    );
    ctx.set_wallet_unlock_session(WalletUnlockSession::new(
        "synthetic-token".into(),
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        Default::default(),
    ))
    .await
    .unwrap();
    assert!(
        WalletApplication::validate_password_with_proof_status(ctx, "synthetic-token")
            .await
            .unwrap()
            .is_none()
    );
    let device =
        DeviceRepo::get_device_info(ctx.core_pool().unwrap(), ctx.get_sn()).await.unwrap().unwrap();
    assert!(device.password_proof.as_deref() == Some("unverified-proof"));
}

#[tokio::test]
async fn proof_reuse_status_retains_legacy_authentication_and_migration() {
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    protection_device_fixture(ctx).await;
    wallet_database::repositories::device::DeviceRepo::update_password(
        ctx.core_pool().unwrap(),
        ctx.get_sn(),
        Some("legacy"),
    )
    .await
    .unwrap();
    let root = legacy_upgrade_fixture(ctx);
    register_wallet_for_password_change(ctx).await;
    assert!(
        WalletApplication::validate_password_with_proof_status(ctx, "wrong-fixture-password")
            .await
            .is_err()
    );
    assert_eq!(native.provisions.load(Ordering::SeqCst), 0);
    assert!(
        WalletApplication::validate_password_with_proof_status(ctx, PASSWORD)
            .await
            .unwrap()
            .is_none()
    );
    assert!(std::fs::read(root.join("root.keystore")).unwrap().starts_with(b"WDK\x01"));
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn proof_reuse_policy_requires_current_algorithm_parameters_and_salt_size() {
    use crate::domain::wallet::WalletDomain;
    use wallet_crypto::{KdfAlgorithm, kdf::factory::KdfFactory};
    use wallet_database::{
        entities::config::{KeystoreKdfAlgorithm, config_key::KEYSTORE_KDF_ALGORITHM},
        repositories::config::ConfigRepo,
    };
    let dir = directory();
    let ctx = context_with_protection(dir.path(), DeviceProtection::default()).await;
    for algorithm in [KdfAlgorithm::Pbkdf2, KdfAlgorithm::Scrypt, KdfAlgorithm::Argon2id] {
        ConfigRepo::upsert(
            KEYSTORE_KDF_ALGORITHM,
            &KeystoreKdfAlgorithm { keystore_kdf_algorithm: algorithm.clone() }
                .to_json_str()
                .unwrap(),
            Some(0),
            &ctx.core_pool().unwrap(),
        )
        .await
        .unwrap();
        // This helper checks policy eligibility, never authenticity. Dummy ciphertext
        // keeps this parameter-contract test fast; Manager tests authenticate real proofs.
        let params = KdfFactory::create(&algorithm, &[7u8; 32]).unwrap().params();
        let metadata = serde_json::json!({"version":3,"id":uuid::Uuid::nil(),"crypto":{"cipher":"aes-128-ctr","cipherparams":{"iv":"00000000000000000000000000000000"},"ciphertext":"","mac":"","kdf":algorithm,"kdfparams":params}});
        assert!(
            WalletDomain::password_proof_matches_current_policy(ctx, &metadata.to_string())
                .await
                .unwrap()
        );
        let mut changed = metadata.clone();
        changed["crypto"]["kdfparams"]["dklen"] = serde_json::json!(16);
        assert!(
            !WalletDomain::password_proof_matches_current_policy(ctx, &changed.to_string())
                .await
                .unwrap()
        );
        let mut changed = metadata.clone();
        changed["crypto"]["kdf"] = serde_json::json!(if algorithm == KdfAlgorithm::Scrypt {
            KdfAlgorithm::Argon2id
        } else {
            KdfAlgorithm::Scrypt
        });
        assert!(
            !WalletDomain::password_proof_matches_current_policy(ctx, &changed.to_string())
                .await
                .unwrap()
        );
        let short = KdfFactory::create(&algorithm, &[7u8; 16]).unwrap().params();
        let mut changed = metadata;
        changed["crypto"]["kdfparams"] = serde_json::to_value(short).unwrap();
        assert!(
            !WalletDomain::password_proof_matches_current_policy(ctx, &changed.to_string())
                .await
                .unwrap()
        );
    }
}
