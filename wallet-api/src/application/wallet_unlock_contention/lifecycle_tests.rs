use super::super::*;
use std::time::Duration;
use wallet_database::repositories::config::ConfigRepo;

#[tokio::test]
async fn missing_and_pending_initialization_wait_before_provisioning() {
    for state in [None, Some("provisioning-v1")] {
        // Arrange: fresh or interrupted initialization, and an active API writer.
        let dir = directory();
        let native = Arc::new(Native::default());
        let ctx = context(dir.path(), native.clone()).await;
        if let Some(state) = state {
            ConfigRepo::upsert("device_protection.key", state, Some(1), &ctx.core_pool().unwrap())
                .await
                .unwrap();
        }
        let pool = ctx.api_wallet_pool().unwrap();
        let writer = pool.lock_write().await;

        // Act: keep the owned initialization alive across the timeout probe.
        let mut initialization = Box::pin(WalletApplication::initialize_device_protection(ctx));
        let pending = tokio::time::timeout(Duration::from_millis(100), &mut initialization).await;
        let provisions_before_release = native.provisions.load(Ordering::SeqCst);
        let state_before_release =
            ConfigRepo::find_by_key("device_protection.key", &ctx.core_pool().unwrap())
                .await
                .unwrap()
                .map(|entry| entry.value);
        drop(writer);
        tokio::time::timeout(Duration::from_secs(5), initialization).await.unwrap().unwrap();

        // Assert: no early provisioning/publication, then original guarded recovery succeeds.
        assert!(pending.is_err());
        assert_eq!(provisions_before_release, 0);
        assert_eq!(state_before_release.as_deref(), state);
        assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
        assert_eq!(
            ConfigRepo::find_by_key("device_protection.key", &ctx.core_pool().unwrap())
                .await
                .unwrap()
                .unwrap()
                .value,
            "ready-v1"
        );
    }
}

#[tokio::test]
async fn ready_key_does_not_bypass_legacy_migration_writer_gate() {
    // Arrange: real V2 files awaiting wrapping, but the device key is already READY.
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    protection_device_fixture(ctx).await;
    let subs = derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    WalletApplication::initialize_device_protection_for_legacy(ctx, PASSWORD).await.unwrap();
    let path = subs.join("key0.keystore");
    let before = std::fs::read(&path).unwrap();
    assert!(!before.starts_with(b"WDK\x01"));
    let pool = ctx.api_wallet_pool().unwrap();
    let writer = pool.lock_write().await;

    // Act: READY must not allow the file migration to pass another API writer.
    let mut migration = Box::pin(WalletApplication::migrate_wallet_file_protection(ctx, PASSWORD));
    let pending = tokio::time::timeout(Duration::from_millis(100), &mut migration).await;
    let while_blocked = std::fs::read(&path).unwrap();
    let active_before_release =
        crate::application::wallet::protection_rollout::files_active(ctx).await.unwrap();
    drop(writer);
    tokio::time::timeout(Duration::from_secs(5), migration).await.unwrap().unwrap();

    // Assert: no early file/state changes, then a complete password-readable migration.
    assert!(pending.is_err());
    assert_eq!(while_blocked, before);
    assert!(!active_before_release);
    assert!(std::fs::read(path).unwrap().starts_with(b"WDK\x01"));
    assert!(crate::application::wallet::protection_rollout::files_active(ctx).await.unwrap());
    assert_eq!(WalletApplication::load_root(ctx, WALLET, PASSWORD).await.unwrap().phrase(), PHRASE);
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ready_native_failures_propagate_without_waiting_or_reprovisioning() {
    // Arrange: actual authenticated protected wallet; unrelated API writer stays held.
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    super::prepare_protected_wallet(ctx).await;
    let root = ctx.get_global_dirs().get_root_dir(WALLET).unwrap().join("root.keystore");
    let before = std::fs::read(&root).unwrap();
    let pool = ctx.api_wallet_pool().unwrap();
    let writer = pool.lock_write().await;
    for fault in [
        ProtectionError::KeyMissing,
        ProtectionError::Locked,
        ProtectionError::KeyInvalidated,
        ProtectionError::Timeout,
        ProtectionError::PlatformFailure,
    ] {
        // Act: inject a provider error while real password authentication still runs.
        *native.protect_error.lock().unwrap() = Some(fault);
        let mut validation = Box::pin(WalletApplication::validate_password(ctx, PASSWORD));
        let result = tokio::time::timeout(Duration::from_secs(2), &mut validation).await;
        if result.is_err() {
            // RED cleanup: allow the owned worker to leave all shared fixture gates.
            drop(writer);
            tokio::time::timeout(Duration::from_secs(5), validation).await.unwrap().unwrap_err();
            panic!("READY native failure waited for an API business writer");
        }
        assert!(matches!(result.unwrap(), Err(ServiceError::DeviceProtection(
            KeystoreProtectionError::Protection(error))) if error == fault));
        assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(&root).unwrap(), before);
    }
    *native.protect_error.lock().unwrap() = None;
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        WalletApplication::validate_password(ctx, PASSWORD),
    )
    .await;
    drop(writer);
    result.unwrap().unwrap();
}
