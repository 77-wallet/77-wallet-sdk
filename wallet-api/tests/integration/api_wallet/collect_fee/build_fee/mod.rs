mod support;

use serial_test::serial;

use support::{
    CollectBuildFeeGiven, CollectBuildFeeScenario, CollectBuildFeeThen, CollectBuildFeeWhen,
    ScenarioRoles,
};

#[serial]
#[tokio::test]
async fn collect_build_fee_failure_reopens_fee_cycle_on_first_insufficient_balance() {
    let scenario = CollectBuildFeeScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let _guard = given.low_balance_sol_adapter();
    let collect = given
        .sol_collect_order(
            "T_collect_fee_reopen_initial",
            "3m2vk1NSfKJK444bCLFCtigFyeHP4cHgvLrtjCJr7nrW",
        )
        .await;

    let pass = when.fee_check_runs(&collect).await;
    then.fee_check_failed(
        pass,
        "low balance should still fail the build fee check on the first attempt",
    );

    let affected = when.build_attempt_is_invalidated(&collect).await;

    then.one_row_was_affected(affected);
    then.fee_cycle_is_reopened(&collect).await;
}

#[serial]
#[tokio::test]
async fn collect_build_fee_failure_after_completed_cycle_records_terminal_failure() {
    let scenario = CollectBuildFeeScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let _guard = given.low_balance_sol_adapter();
    let collect = given
        .sol_collect_order(
            "T_collect_fee_reopen_rebuild",
            "3m2vk1NSfKJK444bCLFCtigFyeHP4cHgvLrtjCJr7nrW",
        )
        .await;
    given.completed_fee_cycle_facts(&collect).await;

    let collect = given.collect_reloaded(&collect).await;
    let pass = when.fee_check_runs(&collect).await;
    then.fee_check_failed(
        pass,
        "low balance should still fail the build fee check when fee facts already exist",
    );

    let affected = when.build_attempt_is_invalidated(&collect).await;

    then.fee_cycle_is_not_reopened(affected);
    then.completed_fee_cycle_facts_are_preserved(&collect).await;
}
