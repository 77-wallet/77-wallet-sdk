mod support;

use serial_test::serial;

use support::{
    RechargeWalletFixture, ScenarioRoles, WithdrawalImportFixture, WithdrawalImportGiven,
    WithdrawalImportScenario, WithdrawalImportThen, WithdrawalImportWhen,
};

#[tokio::test]
#[serial(import_bind)]
async fn import_withdrawal_wallet_ok_requires_binding_address() {
    let scenario = WithdrawalImportScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_accepts_withdrawal_import(false);
    let recharge: RechargeWalletFixture = given.recharge_wallet("uid-recharge", 3).await;
    let import = WithdrawalImportFixture::new("salt-waw", "withdraw-wallet", &recharge.address);

    let withdrawal_uid = when.withdrawal_wallet_is_imported(&import).await;

    then.wallets_are_bound_with_backend_fields(&withdrawal_uid, &recharge).await;
    then.standard_import_backend_calls_were_sent();
}

#[tokio::test]
#[serial(import_bind)]
async fn import_withdrawal_wallet_recovers_incomplete_subaccount_then_completes() {
    let scenario = WithdrawalImportScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_accepts_withdrawal_import(true);
    let recharge = given.recharge_wallet("uid-recharge-partial", 1).await;
    let import = WithdrawalImportFixture::new(
        "salt-waw-partial",
        "withdraw-wallet-partial",
        &recharge.address,
    );

    let withdrawal_uid = when.withdrawal_wallet_is_imported(&import).await;

    then.recharge_and_withdrawal_completed_and_bound(&withdrawal_uid, &recharge).await;
}

#[tokio::test]
#[serial(import_bind)]
async fn import_withdrawal_wallet_reimport_keeps_completion_and_account_count_stable() {
    let scenario = WithdrawalImportScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_accepts_withdrawal_reimport();
    let recharge = given.recharge_wallet("uid-recharge-reimport", 3).await;
    let import = WithdrawalImportFixture::new(
        "salt-waw-reimport",
        "withdraw-wallet-reimport",
        &recharge.address,
    );

    let first_uid = when.withdrawal_wallet_is_imported(&import).await;
    given.wallet_pair_belongs_to_other_device(&recharge.address, &first_uid).await;
    let second_uid = when.withdrawal_wallet_is_imported(&import).await;

    then.reimport_keeps_completion_and_uid_stable(&first_uid, &second_uid).await;
    then.wallets_are_bound_with_backend_fields(&second_uid, &recharge).await;
}

#[tokio::test]
#[serial(import_bind)]
async fn import_withdrawal_wallet_with_concurrent_asset_reads_succeeds() {
    let scenario = WithdrawalImportScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_accepts_withdrawal_import(false);
    let _network = given.backend_network_is_slow();
    let recharge = given.recharge_wallet("uid-recharge-concurrent", 3).await;
    let import = WithdrawalImportFixture::new(
        "salt-waw-concurrent",
        "withdraw-wallet-concurrent",
        &recharge.address,
    );

    let reads = when.asset_reads_start(&recharge.address);
    let withdrawal_uid = when.withdrawal_wallet_is_imported(&import).await;

    then.asset_reads_saw_successes(reads).await;
    then.wallets_are_bound_with_backend_fields(&withdrawal_uid, &recharge).await;
}

#[tokio::test]
#[serial(import_bind)]
async fn import_withdrawal_wallet_uid_usage_false_rejected_without_persist() {
    let scenario = WithdrawalImportScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_rejects_uid_usage();
    let recharge = given.recharge_wallet("uid-recharge-usage-check", 3).await;
    let import = WithdrawalImportFixture::new(
        "salt-waw-usage-false",
        "withdraw-wallet-usage-false",
        &recharge.address,
    );

    let err = when.withdrawal_wallet_import_fails(&import).await;

    then.uid_usage_rejection_did_not_persist(err, &import).await;
    then.uid_usage_rejection_backend_calls_were_sent();
}
