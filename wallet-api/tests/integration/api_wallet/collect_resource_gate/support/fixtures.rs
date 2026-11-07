use crate::harness::next_unique_id;

pub(crate) struct CollectResourceGateFixture {
    pub(crate) trade_no: String,
    pub(crate) resource_trade_no: String,
}

impl CollectResourceGateFixture {
    pub(crate) fn resource_scan_case(prefix: &str) -> Self {
        let id = next_unique_id();
        Self { trade_no: format!("C_RSC_SCAN_{id}"), resource_trade_no: format!("{prefix}_{id}") }
    }

    pub(crate) fn origin_case(prefix: &str) -> Self {
        let trade_no = format!("{prefix}_{}", next_unique_id());
        Self { resource_trade_no: format!("rsc_delegate_{trade_no}"), trade_no }
    }

    pub(crate) fn original_order_result_case(prefix: &str) -> Self {
        let trade_no = format!("{prefix}_{}", next_unique_id());
        Self { resource_trade_no: trade_no.clone(), trade_no }
    }
}
