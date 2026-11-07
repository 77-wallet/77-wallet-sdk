use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use wallet_api::messaging::notify::FrontendNotifyEvent;

use super::fixtures::{COLLECT_VALUE, CollectOrderFixture};

pub(crate) struct CollectNotificationInbox {
    pub(crate) rx: UnboundedReceiver<FrontendNotifyEvent>,
}

impl CollectNotificationInbox {
    pub(crate) async fn then_received_collect_order(&mut self, order: &CollectOrderFixture) {
        let notify = tokio::time::timeout(Duration::from_secs(1), self.rx.recv())
            .await
            .expect("timed out waiting for collect notify")
            .expect("missing collect notify event");

        let notify_json = serde_json::to_value(&notify).expect("serialize collect notify");

        assert_eq!(notify_json["event"], "COLLECT");
        assert_eq!(notify_json["data"]["uid"], order.uid);
        assert_eq!(notify_json["data"]["fromAddr"], order.from_addr);
        assert_eq!(notify_json["data"]["toAddr"], order.to_addr);
        assert_eq!(notify_json["data"]["value"], COLLECT_VALUE);
    }
}
