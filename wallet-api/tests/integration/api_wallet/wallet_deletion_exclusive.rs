//! Fresh Manager inventory is required to exercise the table-clear branch.
#[path = "wallet_deletion/support.rs"]
mod support;
use support::DeletionScenario;

#[tokio::test]
async fn wallet_deletion_exclusive_accounts_preserves_backend_retry_and_cleanup() {
    // Given all addresses belong to the pair, with another empty wallet retained.
    let mut scenario = DeletionScenario::given().await;
    scenario.given_all_addresses_belong_to_pair().await;
    scenario.given_backend_fails();
    // When the backend fails, then original facts remain available for retry.
    assert!(scenario.when_delete_subaccount().await.is_err());
    scenario.then_pair_and_accounts_present().await;
    scenario.given_backend_recovers();
    // When retried, then local data clears and bounded cleanup remains durable.
    scenario.when_delete_subaccount().await.unwrap();
    scenario.then_pair_deleted_and_other_unchanged().await;
    scenario.then_backend_calls(2);
}
