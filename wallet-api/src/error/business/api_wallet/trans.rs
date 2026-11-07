#[derive(Debug, thiserror::Error)]
pub enum TransError {
    // 交易摘要验证失败
    #[error("Transaction digest verification failed")]
    TransactionDigestVerificationFailed,
    // 构建提现交易失败
    #[error("Build withdraw transaction failed: {0}")]
    BuildWithdrawTransactionFailed(String),
}
