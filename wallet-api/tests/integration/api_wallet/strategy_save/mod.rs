mod support;
use serial_test::serial;
use support::{Kind, Scenario};

#[tokio::test]
#[serial(strategy_save)]
async fn strategy_save_collect_replace() {
    let s = Scenario::given(Kind::Collect).await;
    s.save(10, &["base", "polygon"]).await.unwrap();
    s.save(20, &["base", "arbitrum"]).await.unwrap();
    s.then_local_snapshot(20, &["base", "arbitrum"]).await;
    s.then_backend_calls(2);
}

#[tokio::test]
#[serial(strategy_save)]
async fn strategy_save_collect_write_failure() {
    let s = Scenario::given(Kind::Collect).await;
    s.save(10, &["base", "polygon"]).await.unwrap();
    s.given_second_chain_write_fails().await;
    assert!(s.save(20, &["base", "arbitrum"]).await.is_err());
    s.then_local_snapshot(10, &["base", "polygon"]).await;
    s.given_writes_recover().await;
    s.save(20, &["base", "arbitrum"]).await.unwrap();
    s.then_local_snapshot(20, &["base", "arbitrum"]).await;
    s.then_backend_calls(3);
}

#[tokio::test]
#[serial(strategy_save)]
async fn strategy_save_collect_backend_failure() {
    let s = Scenario::given(Kind::Collect).await;
    s.save(10, &["base", "polygon"]).await.unwrap();
    s.given_backend_fails();
    assert!(s.save(20, &["base", "arbitrum"]).await.is_err());
    s.then_local_snapshot(10, &["base", "polygon"]).await;
    s.then_backend_calls(2);
}

#[tokio::test]
#[serial(strategy_save)]
async fn strategy_save_collect_cache_failure() {
    let s = Scenario::given(Kind::Collect).await;
    s.given_second_chain_write_fails().await;
    s.given_backend_strategy(20, &["base", "arbitrum"]);
    assert!(!s.query_for_cache().await);
    s.then_no_local_strategy().await;
    s.given_writes_recover().await;
    s.given_backend_strategy(20, &["base", "arbitrum"]);
    assert!(s.query_for_cache().await);
    s.then_local_snapshot(20, &["base", "arbitrum"]).await;
    s.then_backend_calls(2);
}

#[tokio::test]
#[serial(strategy_save)]
async fn strategy_save_withdraw_replace() {
    let s = Scenario::given(Kind::Withdraw).await;
    s.save(10, &["base", "polygon"]).await.unwrap();
    s.save(20, &["base", "arbitrum"]).await.unwrap();
    s.then_local_snapshot(20, &["base", "arbitrum"]).await;
    s.then_backend_calls(2);
}

#[tokio::test]
#[serial(strategy_save)]
async fn strategy_save_withdraw_write_failure() {
    let s = Scenario::given(Kind::Withdraw).await;
    s.save(10, &["base", "polygon"]).await.unwrap();
    s.given_second_chain_write_fails().await;
    assert!(s.save(20, &["base", "arbitrum"]).await.is_err());
    s.then_local_snapshot(10, &["base", "polygon"]).await;
    s.given_writes_recover().await;
    s.save(20, &["base", "arbitrum"]).await.unwrap();
    s.then_local_snapshot(20, &["base", "arbitrum"]).await;
    s.then_backend_calls(3);
}

#[tokio::test]
#[serial(strategy_save)]
async fn strategy_save_withdraw_backend_failure() {
    let s = Scenario::given(Kind::Withdraw).await;
    s.save(10, &["base", "polygon"]).await.unwrap();
    s.given_backend_fails();
    assert!(s.save(20, &["base", "arbitrum"]).await.is_err());
    s.then_local_snapshot(10, &["base", "polygon"]).await;
    s.then_backend_calls(2);
}

#[tokio::test]
#[serial(strategy_save)]
async fn strategy_save_withdraw_cache_failure() {
    let s = Scenario::given(Kind::Withdraw).await;
    s.given_second_chain_write_fails().await;
    s.given_backend_strategy(20, &["base", "arbitrum"]);
    assert!(!s.query_for_cache().await);
    s.then_no_local_strategy().await;
    s.given_writes_recover().await;
    s.given_backend_strategy(20, &["base", "arbitrum"]);
    assert!(s.query_for_cache().await);
    s.then_local_snapshot(20, &["base", "arbitrum"]).await;
    s.then_backend_calls(2);
}
