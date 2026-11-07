use crate::harness::next_unique_id;

pub(crate) const COLLECT_VALUE: &str = "12.34";
pub(crate) const COLLECT_VALIDATE: &str = "digest";
pub(crate) const COLLECT_CHAIN: &str = "sol";
pub(crate) const COLLECT_SYMBOL: &str = "USDC";

pub(crate) struct CollectOrderFixture {
    pub(crate) uid: String,
    pub(crate) trade_no: String,
    pub(crate) from_addr: String,
    pub(crate) to_addr: String,
}

impl CollectOrderFixture {
    pub(crate) fn new(prefix: &str) -> Self {
        let id = next_unique_id();
        Self {
            uid: format!("uid_{prefix}_{id}"),
            trade_no: format!("T_{prefix}_{id}"),
            from_addr: format!("from-{prefix}-{id}"),
            to_addr: format!("to-{prefix}-{id}"),
        }
    }
}
