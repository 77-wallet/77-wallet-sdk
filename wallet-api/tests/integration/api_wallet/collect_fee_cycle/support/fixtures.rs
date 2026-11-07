use crate::harness::next_unique_id;

pub(crate) struct CollectFeeCycleFixture {
    pub(crate) trade_no: String,
    pub(crate) from_addr: &'static str,
    pub(crate) to_addr: &'static str,
    pub(crate) token_addr: Option<String>,
    pub(crate) symbol: &'static str,
}

impl CollectFeeCycleFixture {
    pub(crate) fn stale_uploaded_fee() -> Self {
        Self {
            trade_no: format!("T_collect_scanner_stale_{}", next_unique_id()),
            from_addr: "from-scan",
            to_addr: "to-scan",
            token_addr: Some("token".to_string()),
            symbol: "USDC",
        }
    }

    pub(crate) fn waiting_service_fee() -> Self {
        Self {
            trade_no: format!("T_collect_wait_fee_{}", next_unique_id()),
            from_addr: "from-wait",
            to_addr: "to-wait",
            token_addr: Some("token".to_string()),
            symbol: "USDC",
        }
    }

    pub(crate) fn reopened_without_fee_upload() -> Self {
        Self {
            trade_no: format!("T_collect_reopen_build_{}", next_unique_id()),
            from_addr: "from-reopen",
            to_addr: "to-reopen",
            token_addr: Some("token".to_string()),
            symbol: "USDC",
        }
    }

    pub(crate) fn completed_fee_result() -> Self {
        Self {
            trade_no: format!("T_collect_fee_ack_{}", next_unique_id()),
            from_addr: "from-sol",
            to_addr: "to-fee-ack",
            token_addr: None,
            symbol: "SOL",
        }
    }
}
