mod support;
use support::Scenario;
use wallet_database::entities::api_wallet::ApiWalletType;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn password_rotation_rejects_stale_creation_and_preserves_persisted_credentials() {
    tokio::time::timeout(std::time::Duration::from_secs(600), async {
        // Given a real Manager with an existing wallet and the modern KDF policy.
        let mut scenario = Scenario::given().await;
        scenario.when_same_password_first_creations_overlap().await;
        // When rotation completes while either kind of creation waits before writing.
        for (kind, next) in [
            (ApiWalletType::SubAccount, "synthetic-rotation-one"),
            (ApiWalletType::Withdrawal, "synthetic-rotation-two"),
        ] {
            scenario.when_rotation_overtakes_creation(kind, next).await;
            // Then stale work fails before mutations and new credentials remain usable.
            scenario.then_new_password_creates_readable_wallet(kind).await;
        }
        scenario.when_failed_rotation_does_not_revoke_creation().await;
        scenario.when_backend_failure_releases_gate("synthetic-rotation-three").await;
        scenario.when_cancelled_creation_finishes_safely("synthetic-rotation-four").await;
    })
    .await
    .expect("credential lifecycle scenario must terminate");
}
