mod support;
use serial_test::serial;
use support::Scenario;

#[tokio::test]
#[serial(import_bind)]
async fn account_deletion_tron_uses_core_permissions_and_preserves_other_accounts() {
    // Given three logical withdrawal accounts, with another self signer retained.
    let scenario = Scenario::given().await;
    // When removing the unconfigured third logical account through the App SDK entry.
    let result = scenario.when_delete(2).await;
    // Then removal succeeds, including TRON permissions, without deleting other accounts.
    scenario.then_target_absent().await;
    assert!(result.is_ok(), "normal deletion returned failure after deleting locally: {result:?}");
    scenario.then_other_accounts_unchanged().await;
    scenario.then_permission_membership_updated().await;
}

#[tokio::test]
#[serial(import_bind)]
async fn account_deletion_strategy_account_rejected_without_local_changes() {
    let scenario = Scenario::given().await;
    let result = scenario.when_delete(0).await;
    assert!(result.unwrap_err().to_string().contains("configured with a withdrawal strategy"));
    scenario.then_all_accounts_present().await;
    scenario.then_permission_membership_unchanged().await;
}

#[tokio::test]
#[serial(import_bind)]
async fn account_deletion_tron_without_multisig_permissions_succeeds() {
    // Given an ordinary account that has never participated in multisig.
    let scenario = Scenario::given().await;
    scenario.given_no_permissions().await;
    let result = scenario.when_delete(2).await;
    scenario.then_target_absent().await;
    assert!(result.is_ok(), "ordinary account deletion failed after local deletion: {result:?}");
    scenario.then_other_accounts_unchanged().await;
}
