mod support;

use serial_test::serial;

use support::{
    ScenarioRoles, WithdrawNotificationGiven, WithdrawNotificationScenario,
    WithdrawNotificationThen, WithdrawNotificationWhen, WithdrawOrderFixture,
};

#[serial]
#[tokio::test]
async fn withdraw_notification_retry_on_existing_trade_no() {
    let scenario = WithdrawNotificationScenario::new().await;
    let order = WithdrawOrderFixture::new("withdraw_notify_retry");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.withdrawal_wallet(&order).await;
    given.frontend_notification_closed().await;

    let result = when.withdraw_order_submitted(&order).await;

    then.frontend_notification_failed(result);
    then.withdraw_order_is_retryable_after_notification_failure(&order).await;

    let mut notifications = given.frontend_notification_collector().await;

    when.withdraw_order_retried(&order).await;

    then.frontend_received_withdraw_order(&mut notifications, &order).await;

    let ack_result = when.tx_ack_is_sent(&order).await;

    then.tx_ack_sent(ack_result);
    then.backend_tx_ack_attempted_once(&order).await;
}

#[serial]
#[tokio::test]
async fn withdraw_tx_ack_sends_once_and_persists_fact() {
    let scenario = WithdrawNotificationScenario::new().await;
    let order = WithdrawOrderFixture::new("withdraw_ack");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.withdraw_order(&order).await;

    let result = when.tx_ack_is_sent(&order).await;

    then.tx_ack_sent(result);
    then.backend_tx_ack_attempted_once(&order).await;
    then.tx_ack_fact_is_persisted(&order).await;
    then.scanner_will_not_retry_tx_ack(&order).await;

    let repeat_result = when.tx_ack_is_sent(&order).await;

    then.tx_ack_sent(repeat_result);
    then.backend_tx_ack_remains_once_after_quiet_period(&order).await;
}

#[serial]
#[tokio::test]
async fn withdraw_tx_ack_backend_failure_keeps_fact_unset_and_retryable() {
    let scenario = WithdrawNotificationScenario::new().await;
    let order = WithdrawOrderFixture::new("withdraw_ack_fail");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.withdraw_order(&order).await;
    given.backend_next_ack_fails(503, "ack unavailable");

    let result = when.tx_ack_is_sent(&order).await;

    then.worker_left_flow_retryable(result);
    then.backend_tx_ack_attempted_once(&order).await;
    then.tx_ack_fact_is_not_persisted(&order).await;
    then.scanner_can_retry_tx_ack(&order).await;
}
