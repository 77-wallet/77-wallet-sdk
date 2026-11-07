mod support;

use serial_test::serial;

use support::{
    BindRelationGiven, BindRelationScenario, BindRelationThen, BindRelationWhen, ScenarioRoles,
};

#[tokio::test]
#[serial(import_bind)]
async fn scan_bind_ok_calls_backend_and_persists_bind_sn_and_relation() {
    let scenario = BindRelationScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();
    let pair = given.wallet_pair().await;

    when.scan_bind_succeeds(&pair, "scan-app-id", "scan-merchant-id").await;

    then.pair_has_bind_fields(&pair, "scan-app-id", "scan-merchant-id").await;
    then.scan_bind_backend_called_once(&pair, "scan-app-id");
}

#[tokio::test]
#[serial(import_bind)]
async fn import_bind_ok_calls_appid_import_and_persists_bind_sn_and_relation() {
    let scenario = BindRelationScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();
    let pair = given.wallet_pair().await;

    when.import_bind_succeeds(&pair, "import-bind-merchant", "import-bind-app").await;

    then.pair_has_bind_fields(&pair, "import-bind-app", "import-bind-merchant").await;
    then.appid_import_backend_called_once(&pair);
}

#[tokio::test]
#[serial(import_bind)]
async fn import_bind_backend_fail_does_not_persist_relation() {
    let scenario = BindRelationScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.import_bind_backend_fails("import bind backend fail");
    let pair = given.wallet_pair().await;
    let before = given.pair_bind_snapshot(&pair).await;

    let err = when.import_bind_fails(&pair, "import-fail-merchant", "import-fail-app").await;

    then.error_contains(err, "import bind backend fail");
    then.pair_bind_snapshot_is_unchanged(&pair, before).await;
    then.appid_import_backend_attempted_once();
}

#[tokio::test]
#[serial(import_bind)]
async fn scan_bind_backend_fail_does_not_persist_bind() {
    let scenario = BindRelationScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.scan_bind_backend_fails("scan bind backend fail");
    let pair = given.wallet_pair().await;
    let before = given.pair_bind_snapshot(&pair).await;

    let err = when.scan_bind_fails(&pair, "scan-fail-app", "scan-fail-merchant").await;

    then.error_contains(err, "scan bind backend fail");
    then.pair_bind_snapshot_is_unchanged(&pair, before).await;
    then.scan_bind_backend_attempted_once();
}

#[tokio::test]
#[serial(import_bind)]
async fn import_bind_missing_wallet_returns_not_found_and_no_backend_call() {
    let scenario = BindRelationScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let recharge_uid = given.only_recharge_wallet("import-bind-only-recharge").await;
    let before = given.wallet_bind_snapshot(&recharge_uid).await;

    let err = when.import_bind_missing_withdrawal_fails(&recharge_uid).await;

    then.missing_wallet_rejection_keeps_recharge_unchanged(err, &recharge_uid, before).await;
    then.appid_import_backend_was_not_called();
}

#[tokio::test]
#[serial(import_bind)]
async fn scan_bind_remote_first_then_persist() {
    let scenario = BindRelationScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.scan_bind_backend_fails("remote bind failed first");
    let pair = given.wallet_pair().await;
    let before = given.pair_bind_snapshot(&pair).await;

    let err = when.scan_bind_fails(&pair, "orchestration-app", "orchestration-merchant").await;

    then.error_contains(err, "remote bind failed first");
    then.scan_bind_backend_called_once(&pair, "orchestration-app");
    then.pair_bind_snapshot_is_unchanged(&pair, before).await;
}
