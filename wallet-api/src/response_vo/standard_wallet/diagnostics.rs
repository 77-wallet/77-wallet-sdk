#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleTaskQueueSummary {
    pub all_tasks: usize,
    pub running_tasks: usize,
    pub pending_tasks: usize,
    pub failed_task_count: usize,
    pub done_tasks: usize,
    pub bill_count: i64,
    pub expand_batches: usize,
    pub unfinished_expand_items: usize,
    pub address_query_states: usize,
    pub overall_percent: f32,
    pub failed_tasks: Vec<ConsoleFailedTaskSummary>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleFailedTaskSummary {
    pub id: String,
    pub task_name: String,
    pub status: u8,
    pub err_msg: Option<String>,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeDiagnostics {
    pub trade_no: String,
    pub records: Vec<TradeDiagnosticRecord>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeDiagnosticRecord {
    pub flow: String,
    pub status: String,
    pub chain_code: String,
    pub symbol: String,
    pub value: String,
    pub from_addr: String,
    pub to_addr: String,
    pub tx_hash: Option<String>,
    pub err_msg: Option<String>,
    pub notes: Option<String>,
}
