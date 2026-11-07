mod support;
use serial_test::serial;
use support::Scenario;

#[tokio::test]
#[serial(expand_planning)]
async fn expand_planning_reopens_running_empty_batch_and_creates_items_once() {
    let scenario = Scenario::given(3).await;
    scenario.given_running_without_items().await;
    scenario.when_reopened_planner_runs().await.unwrap();
    scenario.then_running_with_exact_items(3).await;
    scenario.when_reopened_planner_runs().await.unwrap();
    scenario.then_running_with_exact_items(3).await;
}

#[tokio::test]
#[serial(expand_planning)]
async fn expand_planning_second_chunk_failure_rolls_back_and_retries_after_reopen() {
    let scenario = Scenario::given(151).await;
    scenario.given_second_chunk_fails().await;
    assert!(scenario.when_plan_batch_runs().await.is_err());
    scenario.then_pending_without_items_after_reopen().await;
    scenario.given_writes_recover().await;
    scenario.when_reopened_planner_runs().await.unwrap();
    scenario.then_running_with_exact_items(151).await;
}

#[tokio::test]
#[serial(expand_planning)]
async fn expand_planning_concurrent_batches_keep_disjoint_frozen_indices() {
    let scenario = Scenario::given(3).await;
    let other = scenario.given_another_batch().await;
    let (a, b) = tokio::join!(
        scenario.when_plan_batch_runs(),
        wallet_api::testkit::expand::plan_batch(scenario.ctx, &other)
    );
    a.unwrap();
    b.unwrap();
    scenario.then_two_batches_have_disjoint_indices(&other).await;
}
