use std::time::{Duration, Instant};

use tokio::sync::mpsc::unbounded_channel;
use wallet_api::{
    error::service::ServiceError,
    messaging::notify::FrontendNotifyEvent,
    testkit::withdraw::{
        scan_withdraw_intent_labels_for_trade_once,
        send_tx_ack_via_worker as send_withdraw_tx_ack_via_worker,
    },
};
use wallet_database::{
    ApiTransactionDbPool,
    entities::{
        api_wallet::ApiWalletType,
        api_withdraw::{ApiWithdrawEntity, ApiWithdrawStatus},
    },
};

use crate::harness::{
    AssertRole, CountRole, GivenRole, LoadRole, SeedRole, ThenRole, WhenRole, WorkerTestEnv,
    ensure_worker_env,
};

use super::{
    assertions::{
        then_frontend_notification_failed, then_tx_ack_sent, then_worker_left_flow_retryable,
    },
    db::{insert_withdraw_order, load_withdraw, open_transaction_pool, seed_wallet},
    fixtures::{
        WITHDRAW_CHAIN, WITHDRAW_SYMBOL, WITHDRAW_VALIDATE, WITHDRAW_VALUE, WithdrawOrderFixture,
    },
    inbox::WithdrawNotificationInbox,
    recorder::count_withdraw_tx_ack_requests,
};

pub(crate) struct WithdrawNotificationScenario {
    env: &'static WorkerTestEnv,
    tx_pool: ApiTransactionDbPool,
}

impl WithdrawNotificationScenario {
    pub(crate) async fn new() -> Self {
        let env = ensure_worker_env().await;
        env.recorder.reset();

        let tx_pool = open_transaction_pool(&env.db_dir).await;

        Self { env, tx_pool }
    }

    fn seed(&self) -> SeedRole<'_, Self> {
        SeedRole::new(self)
    }

    fn load(&self) -> LoadRole<'_, Self> {
        LoadRole::new(self)
    }

    fn count(&self) -> CountRole<'_, Self> {
        CountRole::new(self)
    }

    fn assert(&self) -> AssertRole<'_, Self> {
        AssertRole::new(self)
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait WithdrawNotificationGiven {
    async fn withdrawal_wallet(&self, order: &WithdrawOrderFixture);

    async fn withdraw_order(&self, order: &WithdrawOrderFixture);

    async fn frontend_notification_closed(&self);

    async fn frontend_notification_collector(&self) -> WithdrawNotificationInbox;

    fn backend_next_ack_fails(&self, status: i64, body: &str);
}

#[async_trait::async_trait(?Send)]
impl WithdrawNotificationGiven for GivenRole<'_, WithdrawNotificationScenario> {
    async fn withdrawal_wallet(&self, order: &WithdrawOrderFixture) {
        self.scenario().seed().withdrawal_wallet(order).await;
    }

    async fn withdraw_order(&self, order: &WithdrawOrderFixture) {
        self.scenario().seed().withdraw_order(order).await;
    }

    async fn frontend_notification_closed(&self) {
        self.scenario().seed().frontend_notification_closed().await;
    }

    async fn frontend_notification_collector(&self) -> WithdrawNotificationInbox {
        self.scenario().seed().frontend_notification_collector().await
    }

    fn backend_next_ack_fails(&self, status: i64, body: &str) {
        self.scenario().seed().backend_next_ack_fails(status, body);
    }
}

#[async_trait::async_trait(?Send)]
trait WithdrawNotificationSeed {
    async fn withdrawal_wallet(&self, order: &WithdrawOrderFixture);

    async fn withdraw_order(&self, order: &WithdrawOrderFixture);

    async fn frontend_notification_closed(&self);

    async fn frontend_notification_collector(&self) -> WithdrawNotificationInbox;

    fn backend_next_ack_fails(&self, status: i64, body: &str);
}

#[async_trait::async_trait(?Send)]
impl WithdrawNotificationSeed for SeedRole<'_, WithdrawNotificationScenario> {
    async fn withdrawal_wallet(&self, order: &WithdrawOrderFixture) {
        seed_wallet(
            &self.scenario().env.db_dir,
            &order.uid,
            "withdraw-notify-wallet",
            ApiWalletType::Withdrawal,
        )
        .await;
    }

    async fn withdraw_order(&self, order: &WithdrawOrderFixture) {
        insert_withdraw_order(&self.scenario().tx_pool, order).await;
    }

    async fn frontend_notification_closed(&self) {
        let (tx, rx) = unbounded_channel::<FrontendNotifyEvent>();
        drop(rx);

        self.scenario()
            .env
            ._manager
            .set_frontend_notify_sender(tx)
            .await
            .expect("install closed frontend sender");
    }

    async fn frontend_notification_collector(&self) -> WithdrawNotificationInbox {
        let (tx, rx) = unbounded_channel::<FrontendNotifyEvent>();

        self.scenario()
            .env
            ._manager
            .set_frontend_notify_sender(tx)
            .await
            .expect("install working frontend sender");

        WithdrawNotificationInbox { rx }
    }

    fn backend_next_ack_fails(&self, status: i64, body: &str) {
        self.scenario().env.recorder.fail_next_api_backend_call(status, body);
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait WithdrawNotificationWhen {
    async fn withdraw_order_submitted(
        &self,
        order: &WithdrawOrderFixture,
    ) -> Result<(), ServiceError>;

    async fn withdraw_order_retried(&self, order: &WithdrawOrderFixture);

    async fn tx_ack_is_sent(&self, order: &WithdrawOrderFixture) -> Result<(), ServiceError>;
}

#[async_trait::async_trait(?Send)]
impl WithdrawNotificationWhen for WhenRole<'_, WithdrawNotificationScenario> {
    async fn withdraw_order_submitted(
        &self,
        order: &WithdrawOrderFixture,
    ) -> Result<(), ServiceError> {
        self.scenario()
            .env
            ._manager
            .api_withdrawal_order(
                &order.from_addr,
                &order.to_addr,
                WITHDRAW_VALUE,
                WITHDRAW_VALIDATE,
                WITHDRAW_CHAIN,
                None,
                WITHDRAW_SYMBOL,
                &order.trade_no,
                1,
                &order.uid,
            )
            .await
    }

    async fn withdraw_order_retried(&self, order: &WithdrawOrderFixture) {
        self.withdraw_order_submitted(order)
            .await
            .expect("retrying the same withdraw order should resend frontend notify");
    }

    async fn tx_ack_is_sent(&self, order: &WithdrawOrderFixture) -> Result<(), ServiceError> {
        send_withdraw_tx_ack_via_worker(self.scenario().env.ctx(), &order.trade_no).await
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait WithdrawNotificationThen {
    fn frontend_notification_failed(&self, result: Result<(), ServiceError>);

    fn tx_ack_sent(&self, result: Result<(), ServiceError>);

    fn worker_left_flow_retryable(&self, result: Result<(), ServiceError>);

    async fn withdraw_order_is_retryable_after_notification_failure(
        &self,
        order: &WithdrawOrderFixture,
    );

    async fn frontend_received_withdraw_order(
        &self,
        notifications: &mut WithdrawNotificationInbox,
        order: &WithdrawOrderFixture,
    );

    async fn backend_tx_ack_attempted_once(&self, order: &WithdrawOrderFixture);

    async fn backend_tx_ack_remains_once_after_quiet_period(&self, order: &WithdrawOrderFixture);

    async fn tx_ack_fact_is_persisted(&self, order: &WithdrawOrderFixture);

    async fn tx_ack_fact_is_not_persisted(&self, order: &WithdrawOrderFixture);

    async fn scanner_will_not_retry_tx_ack(&self, order: &WithdrawOrderFixture);

    async fn scanner_can_retry_tx_ack(&self, order: &WithdrawOrderFixture);
}

#[async_trait::async_trait(?Send)]
impl WithdrawNotificationThen for ThenRole<'_, WithdrawNotificationScenario> {
    fn frontend_notification_failed(&self, result: Result<(), ServiceError>) {
        then_frontend_notification_failed(result);
    }

    fn tx_ack_sent(&self, result: Result<(), ServiceError>) {
        then_tx_ack_sent(result);
    }

    fn worker_left_flow_retryable(&self, result: Result<(), ServiceError>) {
        then_worker_left_flow_retryable(result);
    }

    async fn withdraw_order_is_retryable_after_notification_failure(
        &self,
        order: &WithdrawOrderFixture,
    ) {
        let persisted = self.scenario().load().withdraw(&order.trade_no).await;
        self.scenario().assert().withdraw_order_is_retryable_after_notification_failure(&persisted);
    }

    async fn frontend_received_withdraw_order(
        &self,
        notifications: &mut WithdrawNotificationInbox,
        order: &WithdrawOrderFixture,
    ) {
        notifications.then_received_withdraw_order(order).await;
    }

    async fn backend_tx_ack_attempted_once(&self, order: &WithdrawOrderFixture) {
        let count = self.scenario().count().wait_for_tx_ack_requests(&order.trade_no).await;
        self.scenario().assert().backend_tx_ack_attempted_once(count);
    }

    async fn backend_tx_ack_remains_once_after_quiet_period(&self, order: &WithdrawOrderFixture) {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let count = self.scenario().count().tx_ack_requests(&order.trade_no);
        self.scenario().assert().backend_tx_ack_remains_once(count);
    }

    async fn tx_ack_fact_is_persisted(&self, order: &WithdrawOrderFixture) {
        let persisted = self.scenario().load().withdraw(&order.trade_no).await;
        self.scenario().assert().tx_ack_fact_is_persisted(&persisted);
    }

    async fn tx_ack_fact_is_not_persisted(&self, order: &WithdrawOrderFixture) {
        let persisted = self.scenario().load().withdraw(&order.trade_no).await;
        self.scenario().assert().tx_ack_fact_is_not_persisted(&persisted);
    }

    async fn scanner_will_not_retry_tx_ack(&self, order: &WithdrawOrderFixture) {
        let labels = self.scenario().load().withdraw_intent_labels(&order.trade_no).await;
        self.scenario().assert().scanner_will_not_retry_tx_ack(&labels);
    }

    async fn scanner_can_retry_tx_ack(&self, order: &WithdrawOrderFixture) {
        let labels = self.scenario().load().withdraw_intent_labels(&order.trade_no).await;
        self.scenario().assert().scanner_can_retry_tx_ack(&labels);
    }
}

#[async_trait::async_trait(?Send)]
trait WithdrawNotificationLoad {
    async fn withdraw(&self, trade_no: &str) -> ApiWithdrawEntity;

    async fn withdraw_intent_labels(&self, trade_no: &str) -> Vec<String>;
}

#[async_trait::async_trait(?Send)]
impl WithdrawNotificationLoad for LoadRole<'_, WithdrawNotificationScenario> {
    async fn withdraw(&self, trade_no: &str) -> ApiWithdrawEntity {
        load_withdraw(&self.scenario().tx_pool, trade_no).await
    }

    async fn withdraw_intent_labels(&self, trade_no: &str) -> Vec<String> {
        scan_withdraw_intent_labels_for_trade_once(self.scenario().env.ctx(), trade_no)
            .await
            .expect("scan withdraw intents")
    }
}

#[async_trait::async_trait(?Send)]
trait WithdrawNotificationCount {
    async fn wait_for_tx_ack_requests(&self, trade_no: &str) -> usize;

    fn tx_ack_requests(&self, trade_no: &str) -> usize;
}

#[async_trait::async_trait(?Send)]
impl WithdrawNotificationCount for CountRole<'_, WithdrawNotificationScenario> {
    async fn wait_for_tx_ack_requests(&self, trade_no: &str) -> usize {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let count = self.tx_ack_requests(trade_no);
            if count > 0 || Instant::now() >= deadline {
                return count;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn tx_ack_requests(&self, trade_no: &str) -> usize {
        let requests = self.scenario().env.recorder.snapshot();
        count_withdraw_tx_ack_requests(&requests, trade_no)
    }
}

trait WithdrawNotificationAssert {
    fn withdraw_order_is_retryable_after_notification_failure(&self, persisted: &ApiWithdrawEntity);

    fn backend_tx_ack_attempted_once(&self, count: usize);

    fn backend_tx_ack_remains_once(&self, count: usize);

    fn tx_ack_fact_is_persisted(&self, persisted: &ApiWithdrawEntity);

    fn tx_ack_fact_is_not_persisted(&self, persisted: &ApiWithdrawEntity);

    fn scanner_will_not_retry_tx_ack(&self, labels: &[String]);

    fn scanner_can_retry_tx_ack(&self, labels: &[String]);
}

impl WithdrawNotificationAssert for AssertRole<'_, WithdrawNotificationScenario> {
    fn withdraw_order_is_retryable_after_notification_failure(
        &self,
        persisted: &ApiWithdrawEntity,
    ) {
        assert_eq!(persisted.init_status, ApiWithdrawStatus::AuditPass);
        assert_eq!(persisted.status, ApiWithdrawStatus::InitOrder);
    }

    fn backend_tx_ack_attempted_once(&self, count: usize) {
        assert_eq!(count, 1, "withdraw order should emit 1 TX ack request");
    }

    fn backend_tx_ack_remains_once(&self, count: usize) {
        assert_eq!(count, 1, "withdraw order should not emit a second TX ack request");
    }

    fn tx_ack_fact_is_persisted(&self, persisted: &ApiWithdrawEntity) {
        assert!(
            persisted.tx_ack_sent_at.is_some(),
            "successful tx ack should persist tx_ack_sent_at"
        );
    }

    fn tx_ack_fact_is_not_persisted(&self, persisted: &ApiWithdrawEntity) {
        assert!(
            persisted.tx_ack_sent_at.is_none(),
            "failed backend ACK must not persist tx_ack_sent_at"
        );
    }

    fn scanner_will_not_retry_tx_ack(&self, labels: &[String]) {
        assert!(
            labels.iter().all(|label| label != "SendTxAck"),
            "withdraw with tx_ack_sent_at must not re-enter SendTxAck; labels: {labels:?}"
        );
    }

    fn scanner_can_retry_tx_ack(&self, labels: &[String]) {
        assert!(
            labels.iter().any(|label| label == "SendTxAck"),
            "withdraw with failed TX ACK should stay retryable; labels: {labels:?}"
        );
    }
}
