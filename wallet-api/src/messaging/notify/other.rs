use wallet_database::entities::bill::BillKind;

// biz_type = ERR
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrFront {
    pub event: String,
    pub message: String,
}

// biz_type = DEBUG
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugFront {
    pub message: serde_json::Value,
}

// biz_type = ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionErrorFrontend {
    pub message: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChainChangeFrontend {
    /// 是否有新启用的链
    pub has_new_chain: bool,
    /// 链数据
    pub chains: Vec<wallet_transport_backend::response_vo::chain::ChainUrlInfo>,
}

// 执行交易的过程给前端发送交易的类型
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionProcessFrontend {
    // 交易的类型,对比账单的交易类型
    pub bill_kind: BillKind,
    // 第几笔交易,默认不需要在批量执行的时候需要
    pub tx_num: Option<i64>,
    pub process: Process,
}
impl TransactionProcessFrontend {
    pub fn new(bill_kind: BillKind, process: Process) -> Self {
        Self { bill_kind, tx_num: None, process }
    }
    pub fn new_with_num(bill_kind: BillKind, tx_num: i64, process: Process) -> Self {
        Self { bill_kind, tx_num: Some(tx_num), process }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum Process {
    Building,
    Broadcast,
}
