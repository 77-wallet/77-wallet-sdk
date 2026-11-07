use crate::harness::next_unique_id;

pub(crate) struct WithdrawResourceGateFixture {
    pub(crate) trade_no: String,
    pub(crate) resource_trade_no: String,
}

impl WithdrawResourceGateFixture {
    pub(crate) fn result_ack_payload_case(prefix: &str) -> Self {
        let id = next_unique_id();
        Self { trade_no: format!("W_ORIGIN_ACK_{id}"), resource_trade_no: format!("{prefix}_{id}") }
    }

    pub(crate) fn origin_gate_case(prefix: &str) -> Self {
        let trade_no = format!("{prefix}_{}", next_unique_id());
        Self { resource_trade_no: format!("DL_W_{trade_no}"), trade_no }
    }

    pub(crate) fn original_order_result_case(prefix: &str) -> Self {
        let trade_no = format!("{prefix}_{}", next_unique_id());
        Self { resource_trade_no: trade_no.clone(), trade_no }
    }
}
