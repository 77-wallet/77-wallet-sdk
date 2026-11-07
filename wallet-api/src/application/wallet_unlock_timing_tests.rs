//! Observe actual authentication and owned-worker lifetimes without logging their inputs.
use super::*;
use std::{future::Future, io::Write, pin::Pin, time::Duration};

#[derive(Clone, Default)]
struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
    fn lines(&self) -> Vec<String> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .filter(|line| {
                line.contains("wallet_unlock_stage_ms") || line.contains("wallet_unlock_task")
            })
            .map(str::to_owned)
            .collect()
    }
    fn events(&self, phase: &str, event: &str) -> Vec<String> {
        self.lines()
            .into_iter()
            .filter(|line| {
                field(line, "phase") == Some(phase) && field(line, "event") == Some(event)
            })
            .collect()
    }
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=");
    line.split_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))
        .map(|value| value.trim_matches('"'))
}

type DiagnosticFuture = Pin<Box<dyn Future<Output = ()>>>;
type DiagnosticRun = Box<dyn FnOnce(Captured) -> DiagnosticFuture>;

fn capture(test: DiagnosticRun) -> Captured {
    let logs = Captured::default();
    let output = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .with_level(false)
        .with_writer(move || output.clone())
        .finish();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    tracing::subscriber::with_default(subscriber, || runtime.block_on(test(logs.clone())));
    logs
}

async fn protected_wallet(ctx: &'static Context) {
    protection_device_fixture(ctx).await;
    derived_password_fixture(ctx).await;
    WalletApplication::store_root(ctx, WALLET, RootData::new(PHRASE, b"synthetic seed"), PASSWORD)
        .await
        .unwrap();
    WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
}

#[test]
fn unlock_timing_reports_safe_correlated_success_and_failure_phases() {
    // Arrange/Act: actual protected-wallet authentication, including both failure boundaries.
    let logs = capture(Box::new(|logs| {
        Box::pin(async move {
            let dir = directory();
            let native = Arc::new(Native::default());
            let ctx = context(dir.path(), native.clone()).await;
            protected_wallet(ctx).await;
            logs.clear();
            WalletApplication::validate_password(ctx, PASSWORD).await.unwrap();
            assert!(matches!(
                WalletApplication::validate_password(ctx, "wrong-password").await,
                Err(ServiceError::Business(crate::error::business::BusinessError::Wallet(
                    crate::error::business::wallet::WalletError::PasswordIncorrect
                )))
            ));
            *native.protect_error.lock().unwrap() = Some(ProtectionError::KeyMissing);
            assert!(matches!(
                WalletApplication::validate_password(ctx, PASSWORD).await,
                Err(ServiceError::DeviceProtection(KeystoreProtectionError::Protection(
                    ProtectionError::KeyMissing
                )))
            ));
            *native.protect_error.lock().unwrap() = None;
            assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
        })
    }));
    // Assert: stages exit on failure too; task links distinguish nested owned initialization.
    for phase in [
        "validation_total",
        "setup_wait",
        "protection_state_read",
        "session_lookup",
        "password_check",
        "device_lookup",
        "wallet_lookup",
        "proof_decrypt",
        "file_lock_wait",
        "device_key_state_read",
        "device_key_probe",
        "native_protect",
        "native_unprotect",
    ] {
        assert!(!logs.events(phase, "begin").is_empty(), "missing begin: {phase}");
        assert!(!logs.events(phase, "end").is_empty(), "missing end: {phase}");
    }
    assert_eq!(logs.events("validation_total", "end").len(), 3);
    let parents: Vec<_> = logs
        .events("validation_total", "begin")
        .iter()
        .map(|line| field(line, "task_id").unwrap().to_owned())
        .collect();
    assert!(
        logs.lines()
            .iter()
            .any(|line| field(line, "operation") == Some("device_key_initialization")
                && field(line, "parent_task_id").is_some_and(|id| parents.iter().any(|p| p == id)))
    );
    for line in logs.lines() {
        for secret in [PASSWORD, PHRASE, WALLET, "wrong-password", "synthetic seed", "KeyMissing"] {
            assert!(!line.contains(secret), "diagnostic leaked a fixture input/error");
        }
        for part in line.split_whitespace().filter(|part| part.contains('=')) {
            let key = part.split('=').next().unwrap();
            assert!(
                [
                    "metric",
                    "phase",
                    "event",
                    "value_ms",
                    "task_id",
                    "operation",
                    "parent_task_id",
                    "dispatch_ms"
                ]
                .contains(&key),
                "unexpected diagnostic field: {key}"
            );
        }
        if field(&line, "event") == Some("end") {
            assert!(field(&line, "value_ms").unwrap().parse::<f64>().unwrap() >= 0.0);
        }
    }
}

#[test]
fn unlock_timing_measures_controlled_setup_and_file_waits() {
    let logs = capture(Box::new(|logs| {
        Box::pin(async move {
            // Arrange: a native probe holds the original SETUP/file guards.
            let dir = directory();
            let native = Arc::new(Native::default());
            let ctx = context(dir.path(), native.clone()).await;
            protected_wallet(ctx).await;
            logs.clear();
            native.pause_protect.store(true, Ordering::SeqCst);
            let first = tokio::spawn(WalletApplication::validate_password(ctx, PASSWORD));
            tokio::time::timeout(Duration::from_secs(5), native.protect_entered.notified())
                .await
                .unwrap();
            let second = tokio::spawn(WalletApplication::validate_password(ctx, PASSWORD));
            let third = tokio::spawn(WalletApplication::initialize_device_protection(ctx));
            // Act: wait for each contender's begin event before the controlled hold interval.
            let armed = tokio::time::timeout(Duration::from_millis(500), async {
                while logs.events("setup_wait", "begin").len() < 2
                    || logs.events("file_lock_wait", "begin").len() < 2
                {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
            .await;
            tokio::time::sleep(Duration::from_millis(150)).await;
            let blocked = !second.is_finished() && !third.is_finished();
            native.protect_resume.notify_one();
            for task in [first, second, third] {
                tokio::time::timeout(Duration::from_secs(5), task).await.unwrap().unwrap().unwrap();
            }
            // Assert after cleanup, including RED, so shared fixture gates are never stranded.
            assert!(armed.is_ok(), "missing contender timing begin events");
            assert!(blocked);
        })
    }));
    for phase in ["setup_wait", "file_lock_wait"] {
        assert!(
            logs.events(phase, "end").iter().any(|line| field(line, "value_ms")
                .unwrap()
                .parse::<f64>()
                .unwrap()
                >= 100.0),
            "unmeasured wait: {phase}"
        );
    }
}

#[test]
fn unlock_timing_caller_abort_does_not_end_the_owned_validation_early() {
    let logs = capture(Box::new(|logs| {
        Box::pin(async move {
            // Arrange: accepted validation paused at the provider boundary.
            let dir = directory();
            let native = Arc::new(Native::default());
            let ctx = context(dir.path(), native.clone()).await;
            protected_wallet(ctx).await;
            logs.clear();
            native.pause_protect.store(true, Ordering::SeqCst);
            let waiter = tokio::spawn(WalletApplication::validate_password(ctx, PASSWORD));
            tokio::time::timeout(Duration::from_secs(5), native.protect_entered.notified())
                .await
                .unwrap();
            // Act: abort only the public waiter, then allow the still-owned worker to finish.
            waiter.abort();
            assert!(waiter.await.unwrap_err().is_cancelled());
            let ended_early = !logs.events("validation_total", "end").is_empty();
            native.protect_resume.notify_one();
            tokio::time::timeout(
                Duration::from_secs(5),
                WalletApplication::validate_password(ctx, PASSWORD),
            )
            .await
            .unwrap()
            .unwrap();
            // Assert: probe completion and the second SETUP owner form a completion barrier.
            assert!(!ended_early);
            assert_eq!(logs.events("validation_total", "end").len(), 2);
            assert_eq!(native.provisions.load(Ordering::SeqCst), 1);
        })
    }));
    assert_eq!(logs.events("native_unprotect", "end").len(), 2);
}
