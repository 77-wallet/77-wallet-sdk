use tokio::sync::mpsc::unbounded_channel;
use wallet_api::{error::service::ServiceError, messaging::notify::FrontendNotifyEvent};
use wallet_database::{
    ApiTransactionDbPool,
    entities::{
        api_collect::{ApiCollectEntity, ApiCollectStatus},
        api_wallet::ApiWalletType,
    },
};

use crate::harness::{
    AssertRole, GivenRole, LoadRole, SeedRole, ThenRole, WhenRole, WorkerTestEnv, ensure_worker_env,
};

use super::{
    assertions::then_frontend_notification_failed,
    db::{load_collect, open_transaction_pool, seed_wallet},
    fixtures::{
        COLLECT_CHAIN, COLLECT_SYMBOL, COLLECT_VALIDATE, COLLECT_VALUE, CollectOrderFixture,
    },
    inbox::CollectNotificationInbox,
};

pub(crate) struct CollectNotificationScenario {
    env: &'static WorkerTestEnv,
    tx_pool: ApiTransactionDbPool,
}

impl CollectNotificationScenario {
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

    fn assert(&self) -> AssertRole<'_, Self> {
        AssertRole::new(self)
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectNotificationGiven {
    async fn sub_account_wallet(&self, order: &CollectOrderFixture);

    async fn frontend_notification_closed(&self);

    async fn frontend_notification_collector(&self) -> CollectNotificationInbox;
}

#[async_trait::async_trait(?Send)]
impl CollectNotificationGiven for GivenRole<'_, CollectNotificationScenario> {
    async fn sub_account_wallet(&self, order: &CollectOrderFixture) {
        self.scenario().seed().sub_account_wallet(order).await;
    }

    async fn frontend_notification_closed(&self) {
        self.scenario().seed().frontend_notification_closed().await;
    }

    async fn frontend_notification_collector(&self) -> CollectNotificationInbox {
        self.scenario().seed().frontend_notification_collector().await
    }
}

#[async_trait::async_trait(?Send)]
trait CollectNotificationSeed {
    async fn sub_account_wallet(&self, order: &CollectOrderFixture);

    async fn frontend_notification_closed(&self);

    async fn frontend_notification_collector(&self) -> CollectNotificationInbox;
}

#[async_trait::async_trait(?Send)]
impl CollectNotificationSeed for SeedRole<'_, CollectNotificationScenario> {
    async fn sub_account_wallet(&self, order: &CollectOrderFixture) {
        seed_wallet(
            &self.scenario().env.db_dir,
            &order.uid,
            "collect-notify-wallet",
            ApiWalletType::SubAccount,
        )
        .await;
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

    async fn frontend_notification_collector(&self) -> CollectNotificationInbox {
        let (tx, rx) = unbounded_channel::<FrontendNotifyEvent>();

        self.scenario()
            .env
            ._manager
            .set_frontend_notify_sender(tx)
            .await
            .expect("install working frontend sender");

        CollectNotificationInbox { rx }
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectNotificationWhen {
    async fn collect_order_submitted(
        &self,
        order: &CollectOrderFixture,
    ) -> Result<(), ServiceError>;

    async fn collect_order_retried(&self, order: &CollectOrderFixture);
}

#[async_trait::async_trait(?Send)]
impl CollectNotificationWhen for WhenRole<'_, CollectNotificationScenario> {
    async fn collect_order_submitted(
        &self,
        order: &CollectOrderFixture,
    ) -> Result<(), ServiceError> {
        self.scenario()
            .env
            ._manager
            .api_collect_order(
                &order.from_addr,
                &order.to_addr,
                COLLECT_VALUE,
                COLLECT_VALIDATE,
                COLLECT_CHAIN,
                None,
                COLLECT_SYMBOL,
                &order.trade_no,
                2,
                &order.uid,
            )
            .await
    }

    async fn collect_order_retried(&self, order: &CollectOrderFixture) {
        self.collect_order_submitted(order)
            .await
            .expect("retrying the same collect order should resend frontend notify");
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectNotificationThen {
    fn frontend_notification_failed(&self, result: Result<(), ServiceError>);

    async fn collect_order_is_retryable(&self, order: &CollectOrderFixture);

    async fn frontend_received_collect_order(
        &self,
        notifications: &mut CollectNotificationInbox,
        order: &CollectOrderFixture,
    );
}

#[async_trait::async_trait(?Send)]
impl CollectNotificationThen for ThenRole<'_, CollectNotificationScenario> {
    fn frontend_notification_failed(&self, result: Result<(), ServiceError>) {
        then_frontend_notification_failed(result);
    }

    async fn collect_order_is_retryable(&self, order: &CollectOrderFixture) {
        let persisted = self.scenario().load().collect(&order.trade_no).await;
        self.scenario().assert().collect_order_is_retryable(&persisted);
    }

    async fn frontend_received_collect_order(
        &self,
        notifications: &mut CollectNotificationInbox,
        order: &CollectOrderFixture,
    ) {
        notifications.then_received_collect_order(order).await;
    }
}

#[async_trait::async_trait(?Send)]
trait CollectNotificationLoad {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity;
}

#[async_trait::async_trait(?Send)]
impl CollectNotificationLoad for LoadRole<'_, CollectNotificationScenario> {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity {
        load_collect(&self.scenario().tx_pool, trade_no).await
    }
}

trait CollectNotificationAssert {
    fn collect_order_is_retryable(&self, collect: &ApiCollectEntity);
}

impl CollectNotificationAssert for AssertRole<'_, CollectNotificationScenario> {
    fn collect_order_is_retryable(&self, collect: &ApiCollectEntity) {
        assert_eq!(collect.status, ApiCollectStatus::Init);
    }
}
