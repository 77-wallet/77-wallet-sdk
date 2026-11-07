//! READY checks must be independent of API business writers; lifecycle gates stay intact.
use super::*;
use std::time::Duration;
use wallet_database::repositories::api_wallet::{
    expand_batch::ExpandBatchRepo, expand_batch_item::ExpandBatchItemRepo,
};

#[path = "wallet_unlock_contention/lifecycle_tests.rs"]
mod lifecycle_tests;

async fn prepare_protected_wallet(ctx: &'static Context) {
    protection_device_fixture(ctx).await;
    derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
}

#[tokio::test]
async fn ready_password_entry_finishes_before_expansion_releases_api_writer() {
    // Arrange: real protected wallet; repository operation paused while owning its gate.
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    prepare_protected_wallet(ctx).await;
    let pool = ctx.api_wallet_pool().unwrap();
    ExpandBatchRepo::create_batch(&pool, "fixture", "contention", "fixture", "base", 1)
        .await
        .unwrap();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
    let writer_pool = pool.clone();
    let writer = tokio::spawn(async move {
        let result: Result<usize, wallet_database::Error> =
            ExpandBatchRepo::plan_items(&writer_pool, "contention", |_| async move {
                entered_tx.send(()).unwrap();
                resume_rx.await.unwrap();
                Ok(vec![0])
            })
            .await;
        result.unwrap()
    });
    tokio::time::timeout(Duration::from_secs(5), entered_rx).await.unwrap().unwrap();

    // Act: checks must finish before the test sends the writer's release signal.
    let mut correct = Box::pin(WalletApplication::validate_password(ctx, PASSWORD));
    let correct_result = tokio::time::timeout(Duration::from_secs(2), &mut correct).await;
    let wrong_result = if correct_result.is_ok() {
        Some(WalletApplication::validate_password(ctx, "wrong-password").await)
    } else {
        None
    };
    let writer_finished_early = writer.is_finished();
    // Cleanup even on RED: do not strand owned workers or global fixture gates.
    resume_tx.send(()).unwrap();
    assert_eq!(tokio::time::timeout(Duration::from_secs(5), writer).await.unwrap().unwrap(), 1);
    if correct_result.is_err() {
        tokio::time::timeout(Duration::from_secs(5), &mut correct).await.unwrap().unwrap();
    }

    // Assert: authentication finished during contention; the writer still commits its item.
    assert!(!writer_finished_early);
    correct_result.expect("READY password validation waited for an API business writer").unwrap();
    assert!(matches!(
        wrong_result.unwrap(),
        Err(ServiceError::Business(crate::error::business::BusinessError::Wallet(
            crate::error::business::wallet::WalletError::PasswordIncorrect
        )))
    ));
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
    assert_eq!(ExpandBatchItemRepo::count_by_batch_id(&pool, "contention").await.unwrap(), 1);
}

#[tokio::test]
async fn ready_native_probe_retains_file_gate_but_not_api_writer() {
    // Arrange: READY protection; pause only its next native usability check.
    let dir = directory();
    let native = Arc::new(Native::default());
    let ctx = context(dir.path(), native.clone()).await;
    WalletApplication::initialize_device_protection(ctx).await.unwrap();
    native.pause_protect.store(true, Ordering::SeqCst);
    let probe = tokio::spawn(WalletApplication::initialize_device_protection(ctx));
    tokio::time::timeout(Duration::from_secs(5), native.protect_entered.notified()).await.unwrap();
    let pool = ctx.api_wallet_pool().unwrap();

    // Act: other API writers can run; file mutators must stay serialized with the probe.
    let writer = tokio::time::timeout(Duration::from_millis(100), pool.lock_write()).await;
    let file_gate = tokio::time::timeout(
        Duration::from_millis(100),
        crate::application::wallet::derived_store::lock(),
    )
    .await;
    let api_available = writer.is_ok();
    let files_excluded = file_gate.is_err();
    drop(writer);
    drop(file_gate);
    native.protect_resume.notify_one();
    tokio::time::timeout(Duration::from_secs(5), probe).await.unwrap().unwrap().unwrap();

    // Assert: API business serialization relaxed, existing file lifecycle exclusion preserved.
    assert!(api_available, "READY native probe retained the API business writer gate");
    assert!(files_excluded);
    let _guard = tokio::time::timeout(
        Duration::from_secs(5),
        crate::application::wallet::derived_store::lock(),
    )
    .await
    .unwrap();
    assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
}
