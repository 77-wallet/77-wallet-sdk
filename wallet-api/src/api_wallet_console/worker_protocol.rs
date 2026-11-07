use crate::{
    batch_transfer::BatchTransferSummary,
    response_vo::standard_wallet::diagnostics::{ConsoleTaskQueueSummary, TradeDiagnostics},
};

use super::events::{
    AccountAddressRow, ApiWalletRow, BalanceAssetRow, ClientRuntimeInfo, WithdrawOrderRow,
};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum WorkerEvent {
    Started,
    Failed { error: String },
    Log { message: String },
    Notify { payload: String },
    ImportFinished { messages: Vec<String> },
    RuntimeInfoLoaded { info: ClientRuntimeInfo },
    WalletsLoaded { wallets: Vec<ApiWalletRow> },
    BindFinished,
    AccountAddressesLoaded { wallet_address: String, rows: Vec<AccountAddressRow> },
    BalanceAssetsLoaded { wallet_address: String, rows: Vec<BalanceAssetRow> },
    WithdrawOrdersLoaded { rows: Vec<WithdrawOrderRow> },
    WithdrawReviewFinished,
    ChainNodeSet { chain_code: String, rpc_url: String },
    ChainNodeSetFailed { error: String },
    TaskQueueStatusLoaded { summary: ConsoleTaskQueueSummary },
    TradeDiagnosticsLoaded { diagnostics: TradeDiagnostics },
    LoadedTargets { targets: Vec<String> },
    TransferFinished { summary: BatchTransferSummary },
}
