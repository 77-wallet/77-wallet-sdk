use crate::harness::next_unique_id;

pub(crate) const WITHDRAW_VALUE: &str = "56.78";
pub(crate) const WITHDRAW_VALIDATE: &str = "digest";
pub(crate) const WITHDRAW_CHAIN: &str = "sol";
pub(crate) const WITHDRAW_SYMBOL: &str = "USDC";

pub(crate) struct WithdrawOrderFixture {
    pub(crate) uid: String,
    pub(crate) trade_no: String,
    pub(crate) from_addr: String,
    pub(crate) to_addr: String,
}

impl WithdrawOrderFixture {
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
