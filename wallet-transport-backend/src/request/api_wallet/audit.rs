#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditResultReportReq {
    pub trade_no: String,
    pub result: bool,
    pub remark: String,
}

impl AuditResultReportReq {
    pub fn new(trade_no: String, result: bool, remark: &str) -> Self {
        Self { trade_no, result, remark: remark.to_owned() }
    }
}
