#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ApiResourceOperationStatus {
    Created,
    Broadcasting,
    Success,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResourceOperationResp {
    pub trade_no: String,
    pub tx_hash: Option<String>,
    pub status: ApiResourceOperationStatus,
}

impl ApiResourceOperationResp {
    pub(crate) fn success(trade_no: String, tx_hash: String) -> Self {
        Self { trade_no, tx_hash: Some(tx_hash), status: ApiResourceOperationStatus::Success }
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiResourceOperationResp, ApiResourceOperationStatus};

    #[test]
    fn api_resource_operation_success_keeps_trade_no_and_tx_hash() {
        let resp =
            ApiResourceOperationResp::success("local-trade".to_string(), "0xhash".to_string());

        assert_eq!(resp.trade_no, "local-trade");
        assert_eq!(resp.tx_hash.as_deref(), Some("0xhash"));
        assert_eq!(resp.status, ApiResourceOperationStatus::Success);
    }
}
