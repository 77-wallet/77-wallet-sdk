mod support;

use serial_test::serial;

use support::{
    CollectFeeCheckGiven, CollectFeeCheckScenario, CollectFeeCheckThen, CollectFeeCheckWhen,
    ScenarioRoles,
};

#[serial]
#[tokio::test]
async fn collect_sol_native_fee_check_fails_on_uninitialized_recipient() {
    let scenario = CollectFeeCheckScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let _guard = given.sol_recipient_adapter(true, 27_309_206);
    let collect = given
        .sol_collect_order(
            "T_collect_sol_rent_fail",
            "3m2vk1NSfKJK444bCLFCtigFyeHP4cHgvLrtjCJr7nrW",
        )
        .await;

    let err = when.sol_fee_check_fails(&collect).await;

    then.error_mentions_uninitialized_recipient(err);
    then.collect_status_is_init(&collect).await;
}

#[serial]
#[tokio::test]
async fn collect_sol_native_fee_check_allows_initialized_recipient() {
    let scenario = CollectFeeCheckScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let _guard = given.sol_recipient_adapter(false, 27_309_206);
    let collect = given
        .sol_collect_order("T_collect_sol_rent_ok", "3m2vk1NSfKJK444bCLFCtigFyeHP4cHgvLrtjCJr7nrW")
        .await;

    let pass = when.sol_fee_check_runs(&collect).await;

    then.fee_check_passed(pass);
    then.collect_status_is_init(&collect).await;
}

#[serial]
#[tokio::test]
async fn collect_eth_native_fee_check_with_partial_oracle_fallback() {
    let scenario = CollectFeeCheckScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let _guard = given.eth_fee_adapter(100_000_000_000_000_000u128, 0.00000368);
    let collect = given
        .eth_collect_order(
            "T_collect_eth_partial_oracle",
            "0x477000C778C66FaAA36596Fb846Ce34C89bc652D",
            "0xFCa230313618af2a33fa00455D8A5d1466C91332",
            "0.000015",
        )
        .await;

    let pass = when.eth_fee_check_runs(&collect).await;

    then.fee_check_passed(pass);
    then.collect_status_is_init(&collect).await;
}

#[serial]
#[tokio::test]
async fn collect_eth_native_fee_check_fails_on_insufficient_balance() {
    let scenario = CollectFeeCheckScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let _guard = given.eth_fee_adapter(10_000_000_000_000u128, 0.00000368);
    let collect = given
        .eth_collect_order(
            "T_collect_eth_insufficient",
            "0x477000C778C66FaAA36596Fb846Ce34C89bc652D",
            "0xFCa230313618af2a33fa00455D8A5d1466C91332",
            "0.000015",
        )
        .await;

    let pass = when.eth_fee_check_runs(&collect).await;

    then.fee_check_failed(pass, "insufficient ETH balance should fail the fee check");
    then.collect_status_is_init(&collect).await;
}

#[serial]
#[tokio::test]
async fn collect_build_fee_estimation_shortage_reopens_fee_cycle() {
    let scenario = CollectFeeCheckScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let _guard = given.sol_fee_shortage_adapter(false, 0);
    let collect = given
        .sol_collect_order("T_collect_fee_shortage", "3m2vk1NSfKJK444bCLFCtigFyeHP4cHgvLrtjCJr7nrW")
        .await;

    let pass = when.sol_fee_check_runs(&collect).await;
    then.fee_check_failed(
        pass,
        "fee estimate shortage must reopen the service-fee cycle instead of erroring",
    );

    let affected = when.build_attempt_is_invalidated(&collect).await;

    then.one_row_was_affected(affected);
    then.fee_cycle_is_reopened(&collect).await;
}
