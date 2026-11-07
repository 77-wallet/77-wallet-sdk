mod support;

use serial_test::serial;

use support::{
    CollectNotificationGiven, CollectNotificationScenario, CollectNotificationThen,
    CollectNotificationWhen, CollectOrderFixture, ScenarioRoles,
};

#[serial]
#[tokio::test]
async fn collect_notification_retry_on_existing_trade_no() {
    let scenario = CollectNotificationScenario::new().await;
    let order = CollectOrderFixture::new("collect_notify_retry");
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.sub_account_wallet(&order).await;
    given.frontend_notification_closed().await;

    let result = when.collect_order_submitted(&order).await;

    then.frontend_notification_failed(result);
    then.collect_order_is_retryable(&order).await;

    let mut notifications = given.frontend_notification_collector().await;

    when.collect_order_retried(&order).await;

    then.frontend_received_collect_order(&mut notifications, &order).await;
}
