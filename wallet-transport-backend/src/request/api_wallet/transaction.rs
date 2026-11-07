#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreTxRecordsReq {}

impl RestoreTxRecordsReq {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransEventAckReq {
    pub trade_no: String,
    #[serde(rename = "type")]
    typ: TransType,
    pub ack_type: TransAckType,
}

impl TransEventAckReq {
    pub fn new(trade_no: &str, typ: TransType, ack_type: TransAckType) -> Self {
        Self { trade_no: trade_no.to_string(), typ: typ, ack_type }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxExecReceiptUploadReq {
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    trade_no: String,
    #[serde(rename = "type")]
    typ: TransType,
    hash: String,
    status: TransStatus,
    remark: String,
    error_code: Option<String>,
}

impl TxExecReceiptUploadReq {
    pub fn is_success(&self) -> bool {
        self.status == TransStatus::Success
    }

    pub fn is_fail(&self) -> bool {
        !self.is_success()
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TransType {
    /// Collection
    Col,
    /// Withdraw
    Wd,
    /// Fee
    #[serde(rename = "COL_FEE")]
    ColFee,
    /// Platform resource stake/unstake task
    #[serde(rename = "PLT_RSC_STK")]
    PltRscStk,
    /// Collection resource delegation task
    #[serde(rename = "COL_RSC_DL")]
    ColRscDl,
    /// Collection resource reclaim task
    #[serde(rename = "COL_RSC_RC")]
    ColRscRc,
    /// Withdraw resource delegation task
    #[serde(rename = "WD_RSC_DL")]
    WdRscDl,
    /// Withdraw resource reclaim task
    #[serde(rename = "WD_RSC_RC")]
    WdRscRc,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TransAckType {
    Tx,
    #[serde(rename = "TX_RES")]
    TxRes,
    #[serde(rename = "CMD_ADDRESS_EXPAND")]
    CmdAddressExpand,
    #[serde(rename = "CMD_PLT_UID_UNBIND")]
    CmdPltUidUnbind,
    #[serde(rename = "CMD_WALLET_ACTIVE")]
    CmdWalletActive,
    #[serde(rename = "TX_FEE_RES")]
    TxFeeRes,
    #[serde(rename = "TX_RSC_RES")]
    TxRscRes,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum TransStatus {
    /// Success
    Success,
    /// Fail
    Fail,
}

impl TxExecReceiptUploadReq {
    pub fn new(
        from: Option<&str>,
        to: Option<&str>,
        trade_no: &str,
        typ: TransType,
        hash: Option<&str>,
        status: TransStatus,
        remark: &str,
    ) -> Self {
        Self {
            from: from.map(|s| s.to_string()),
            to: to.map(|s| s.to_string()),
            trade_no: trade_no.to_string(),
            typ,
            hash: hash.unwrap_or_default().to_string(),
            status,
            remark: remark.to_string(),
            error_code: None,
        }
    }

    pub fn with_error_code(mut self, error_code: &str) -> Self {
        self.error_code = Some(error_code.to_string());
        self
    }
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceFeeUploadReq {
    trade_no: String,
    from: String,
    to: String,
    amount: f64,
    chain_code: String,
    #[serde(rename = "tokenCode")]
    symbol: String,
    #[serde(rename = "contractAddress")]
    token_address: String,
}

impl ServiceFeeUploadReq {
    pub fn new(
        trade_no: &str,
        chain_code: &str,
        symbol: &str,
        token_address: &str,
        from: &str,
        to: &str,
        amount: f64,
    ) -> Self {
        Self {
            trade_no: trade_no.to_string(),
            chain_code: chain_code.to_string(),
            symbol: symbol.to_string(),
            token_address: token_address.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            amount,
        }
    }
}
