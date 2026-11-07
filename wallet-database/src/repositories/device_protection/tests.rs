use super::*;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nonce =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/migration-state-tests")
            .join(format!("{}-{nonce}-{unique}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    async fn open(&self) -> crate::CoreDbPool {
        crate::SqliteContext::new(self.0.to_str().unwrap(), Some("data.db"))
            .await
            .unwrap()
            .into_core_db_pool()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn close(pool: crate::CoreDbPool) {
    pool.read_ref().close().await;
    pool.write_ref().close().await;
}

#[tokio::test]
async fn fresh_state_uses_file_protection_key() {
    let fixture = Fixture::new();
    let pool = fixture.open().await;
    let scope = "0123456789abcdef";

    assert_eq!(MigrationRepo::begin(scope, &pool).await.unwrap(), MigrationState::Pending);

    let concise = crate::repositories::config::ConfigRepo::find_by_key(
        "device_protection.file.v1:0123456789abcdef",
        &pool,
    )
    .await
    .unwrap();
    let legacy = crate::repositories::config::ConfigRepo::find_by_key(
        "device_protection.migration.v1:file-v1:0123456789abcdef",
        &pool,
    )
    .await
    .unwrap();
    assert_eq!(concise.unwrap().value, "migrating-v1");
    assert!(legacy.is_none());
    close(pool).await;
}

#[tokio::test]
async fn completed_state_survives_reopen_and_cannot_be_restarted() {
    let fixture = Fixture::new();
    let pool = fixture.open().await;
    assert_eq!(MigrationRepo::get("fixture-wallet", &pool).await.unwrap(), None);
    assert_eq!(
        MigrationRepo::begin("fixture-wallet", &pool).await.unwrap(),
        MigrationState::Pending
    );
    assert_eq!(
        MigrationRepo::complete("fixture-wallet", &pool).await.unwrap(),
        MigrationState::DeviceRequired
    );
    close(pool).await;
    let pool = fixture.open().await;
    assert_eq!(
        MigrationRepo::get("fixture-wallet", &pool).await.unwrap(),
        Some(MigrationState::DeviceRequired)
    );
    let (begin, complete) = tokio::join!(
        MigrationRepo::begin("fixture-wallet", &pool),
        MigrationRepo::complete("fixture-wallet", &pool)
    );
    assert_eq!(begin.unwrap(), MigrationState::DeviceRequired);
    assert_eq!(complete.unwrap(), MigrationState::DeviceRequired);
    assert_eq!(
        MigrationRepo::get("fixture-wallet", &pool).await.unwrap(),
        Some(MigrationState::DeviceRequired)
    );
    close(pool).await;
}

#[tokio::test]
async fn completion_without_begin_does_not_create_state() {
    let fixture = Fixture::new();
    let pool = fixture.open().await;
    assert!(matches!(
        MigrationRepo::complete("fixture-wallet", &pool).await,
        Err(MigrationError::NotStarted)
    ));
    assert_eq!(MigrationRepo::get("fixture-wallet", &pool).await.unwrap(), None);
    assert!(matches!(MigrationRepo::begin("", &pool).await, Err(MigrationError::InvalidScope)));
    close(pool).await;
}

#[tokio::test]
async fn unknown_state_is_preserved_and_never_reset() {
    let fixture = Fixture::new();
    let pool = fixture.open().await;
    let key = state_key("fixture-wallet").unwrap();
    crate::repositories::config::ConfigRepo::upsert(&key, "unknown-version", Some(1), &pool)
        .await
        .unwrap();
    assert!(matches!(
        MigrationRepo::get("fixture-wallet", &pool).await,
        Err(MigrationError::InvalidState)
    ));
    assert!(matches!(
        MigrationRepo::begin("fixture-wallet", &pool).await,
        Err(MigrationError::InvalidState)
    ));
    assert!(matches!(
        MigrationRepo::complete("fixture-wallet", &pool).await,
        Err(MigrationError::InvalidState)
    ));
    let stored =
        crate::repositories::config::ConfigRepo::find_by_key(&key, &pool).await.unwrap().unwrap();
    assert_eq!(stored.value, "unknown-version");
    close(pool).await;
}

#[tokio::test]
async fn completion_racing_with_first_begin_never_reports_pending_success() {
    let fixture = Fixture::new();
    let pool = fixture.open().await;
    for index in 0..32 {
        let scope = format!("fixture-race-{index}");
        let (completed, begun) = tokio::join!(
            MigrationRepo::complete(&scope, &pool),
            MigrationRepo::begin(&scope, &pool),
        );
        begun.unwrap();
        match completed {
            Ok(state) => {
                assert_eq!(state, MigrationState::DeviceRequired);
                assert_eq!(
                    MigrationRepo::get(&scope, &pool).await.unwrap(),
                    Some(MigrationState::DeviceRequired)
                );
            }
            Err(MigrationError::NotStarted) => {}
            Err(other) => panic!("unexpected migration result: {other}"),
        }
    }
    close(pool).await;
}

#[tokio::test]
async fn authorized_recreation_survives_reopen_and_completion_revokes_it() {
    let fixture = Fixture::new();
    let pool = fixture.open().await;
    MigrationRepo::begin("deleted-account", &pool).await.unwrap();
    MigrationRepo::complete("deleted-account", &pool).await.unwrap();
    MigrationRepo::authorize_recreation("deleted-account", &pool).await.unwrap();
    close(pool).await;
    let pool = fixture.open().await;
    assert_eq!(
        MigrationRepo::get("deleted-account", &pool).await.unwrap(),
        Some(MigrationState::RecreationAllowed)
    );
    assert_eq!(
        MigrationRepo::begin("deleted-account", &pool).await.unwrap(),
        MigrationState::RecreationAllowed
    );
    MigrationRepo::complete("deleted-account", &pool).await.unwrap();
    assert_eq!(
        MigrationRepo::get("deleted-account", &pool).await.unwrap(),
        Some(MigrationState::DeviceRequired)
    );
    close(pool).await;
}

#[tokio::test]
async fn first_creation_authorization_survives_reopen_and_cannot_overwrite_history() {
    let fixture = Fixture::new();
    let pool = fixture.open().await;
    assert_eq!(
        MigrationRepo::authorize_creation("new-account", &pool).await.unwrap(),
        MigrationState::CreationAuthorized
    );
    close(pool).await;
    let pool = fixture.open().await;
    assert_eq!(
        MigrationRepo::get("new-account", &pool).await.unwrap(),
        Some(MigrationState::CreationAuthorized)
    );
    MigrationRepo::complete("new-account", &pool).await.unwrap();
    assert!(MigrationRepo::authorize_creation("new-account", &pool).await.is_err());
    assert_eq!(
        MigrationRepo::get("new-account", &pool).await.unwrap(),
        Some(MigrationState::DeviceRequired)
    );
    close(pool).await;
}

#[tokio::test]
async fn recreation_authorization_preserves_unknown_state() {
    let fixture = Fixture::new();
    let pool = fixture.open().await;
    let key = state_key("unknown-account").unwrap();
    crate::repositories::config::ConfigRepo::upsert(&key, "future-state", Some(1), &pool)
        .await
        .unwrap();
    assert!(MigrationRepo::authorize_recreation("unknown-account", &pool).await.is_err());
    let stored =
        crate::repositories::config::ConfigRepo::find_by_key(&key, &pool).await.unwrap().unwrap();
    assert_eq!(stored.value, "future-state");
    assert!(MigrationRepo::authorize_recreation("", &pool).await.is_err());
    assert_eq!(MigrationRepo::get("missing-account", &pool).await.unwrap(), None);
    MigrationRepo::authorize_recreation("missing-account", &pool).await.unwrap();
    assert_eq!(
        MigrationRepo::get("missing-account", &pool).await.unwrap(),
        Some(MigrationState::RecreationAllowed)
    );
    close(pool).await;
}
