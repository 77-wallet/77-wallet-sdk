use chrono::Utc;
use wallet_database::entities::api_collect::ApiCollectEntity;

use crate::harness::next_unique_id;

use super::payload::base_collect_for_receipt;

pub(crate) struct CollectReceiptFixture {
    pub(crate) trade_no: String,
    pub(crate) from_addr: String,
    pub(crate) initial_to_addr: String,
    pub(crate) receipt_to_addr: String,
    pub(crate) tx_hash: String,
}

impl CollectReceiptFixture {
    pub(crate) fn new(prefix: &str) -> Self {
        let id = next_unique_id();
        Self {
            trade_no: format!("T_{prefix}_{id}"),
            from_addr: format!("from-{prefix}-{id}"),
            initial_to_addr: format!("old-to-{prefix}-{id}"),
            receipt_to_addr: format!("receipt-to-{prefix}-{id}"),
            tx_hash: format!("hash-{prefix}-{id}"),
        }
    }

    pub(crate) fn receipt_entity(&self) -> ApiCollectEntity {
        ApiCollectEntity {
            trade_no: self.trade_no.clone(),
            tx_hash: Some(self.tx_hash.clone()),
            to_addr: self.receipt_to_addr.clone(),
            from_addr: self.from_addr.clone(),
            last_broadcast_at: Some(Utc::now()),
            ..base_collect_for_receipt()
        }
    }
}
