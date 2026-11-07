mod support;

use serial_test::serial;

use support::{
    ScenarioRoles, SubaccountImportFixture, SubaccountImportGiven, SubaccountImportScenario,
    SubaccountImportThen, SubaccountImportWhen,
};

#[tokio::test]
#[serial(import_bind)]
async fn import_subaccount_wallet_ok_unbound() {
    let scenario = SubaccountImportScenario::new().await;
    let import = SubaccountImportFixture::new("salt-sub", "sub-wallet");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_accepts_unbound_subaccount();

    let uid = when.subaccount_wallet_is_imported(&import).await;

    then.subaccount_wallet_is_unbound_and_initialized(&uid).await;
    then.standard_import_backend_calls_were_sent();
}

#[tokio::test]
#[serial(import_bind)]
async fn import_subaccount_wallet_query_failure_does_not_persist_half_state() {
    let scenario = SubaccountImportScenario::new().await;
    let import = SubaccountImportFixture::new("salt-sub-fail", "sub-wallet-fail");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_bind_info_query_fails();

    let err = when.subaccount_wallet_import_fails(&import).await;

    then.bind_info_failure_did_not_persist(err, &import).await;
    then.bind_info_failure_backend_calls_were_sent();
}

#[tokio::test]
#[serial(import_bind)]
async fn import_subaccount_wallet_sets_progress_stage_before_completion() {
    let scenario = SubaccountImportScenario::new().await;
    let import = SubaccountImportFixture::new("salt-sub-stage", "sub-wallet-stage");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_accepts_unbound_subaccount();

    let uid = when.subaccount_wallet_is_imported(&import).await;

    then.import_returns_expected_uid_and_completes_stage(&uid, &import).await;
}

#[tokio::test]
#[serial(import_bind)]
async fn import_subaccount_wallet_uid_status_mismatch_rejected_without_persist() {
    let scenario = SubaccountImportScenario::new().await;
    let import = SubaccountImportFixture::new("salt-sub-mismatch", "sub-wallet-mismatch");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_reports_withdrawal_uid_status();

    let err = when.subaccount_wallet_import_fails(&import).await;

    then.uid_status_mismatch_did_not_persist(err, &import).await;
    then.only_uid_check_was_called();
}

#[tokio::test]
#[serial(import_bind)]
async fn reimport_subaccount_removes_stale_cross_device_withdrawal_wallet() {
    let scenario = SubaccountImportScenario::new().await;
    let import = SubaccountImportFixture::new("salt-sub-stale-binding", "sub-wallet-stale-binding");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_accepts_unbound_subaccount();
    let uid = when.subaccount_wallet_is_imported(&import).await;
    let withdrawal_address = given.bound_withdrawal_wallet(&uid, false).await;
    then.wallet_pair_is_cached(&uid, &withdrawal_address).await;

    given.backend_accepts_unbound_subaccount();
    when.subaccount_wallet_is_imported(&import).await;

    then.stale_withdrawal_wallet_is_removed(&uid, &withdrawal_address).await;
    then.wallet_list_contains_only_reimported_subaccount(&uid, &withdrawal_address).await;
}

#[tokio::test]
#[serial(import_bind)]
async fn reimport_subaccount_preserves_withdrawal_wallet_imported_on_current_device() {
    let scenario = SubaccountImportScenario::new().await;
    let import =
        SubaccountImportFixture::new("salt-sub-current-binding", "sub-wallet-current-binding");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_accepts_unbound_subaccount();
    let uid = when.subaccount_wallet_is_imported(&import).await;
    let withdrawal_address = given.bound_withdrawal_wallet(&uid, true).await;

    given.backend_accepts_unbound_subaccount();
    when.subaccount_wallet_is_imported(&import).await;

    then.wallet_relation_is_preserved(&uid, &withdrawal_address).await;
}
