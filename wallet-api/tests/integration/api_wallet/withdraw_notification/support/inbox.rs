use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use wallet_api::messaging::notify::FrontendNotifyEvent;

use super::fixtures::{WITHDRAW_VALUE, WithdrawOrderFixture};

pub(crate) struct WithdrawNotificationInbox {
    pub(crate) rx: UnboundedReceiver<FrontendNotifyEvent>,
}

impl WithdrawNotificationInbox {
    pub(crate) async fn then_received_withdraw_order(&mut self, order: &WithdrawOrderFixture) {
        let notify = tokio::time::timeout(Duration::from_secs(1), self.rx.recv())
            .await
            .expect("timed out waiting for withdraw notify")
            .expect("missing withdraw notify event");

        let notify_json = serde_json::to_value(&notify).expect("serialize withdraw notify");

        assert_eq!(notify_json["event"], "WITHDRAW");
        assert_eq!(notify_json["data"]["uid"], order.uid);
        assert_eq!(notify_json["data"]["fromAddr"], order.from_addr);
        assert_eq!(notify_json["data"]["toAddr"], order.to_addr);
        assert_eq!(notify_json["data"]["value"], WITHDRAW_VALUE);
    }
}
