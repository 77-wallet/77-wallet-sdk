use crate::{
    batch_transfer::BatchTransferSummary,
    response_vo::standard_wallet::diagnostics::{ConsoleTaskQueueSummary, TradeDiagnostics},
};

use super::client::ClientId;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountAddressRow {
    pub wallet_address: String,
    pub account_id: u32,
    pub name: String,
    pub chain_code: String,
    pub address: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiWalletRow {
    pub role: String,
    pub name: String,
    pub address: String,
    pub uid: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BalanceAssetRow {
    pub wallet_address: String,
    pub account_id: Option<u32>,
    pub chain_code: String,
    pub symbol: String,
    pub name: String,
    pub amount: f64,
    pub currency: String,
    pub fiat_value: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WithdrawOrderRow {
    pub trade_no: String,
    pub out_order_id: Option<String>,
    pub client_id: Option<String>,
    pub chain_code: String,
    pub symbol: String,
    pub value: String,
    pub from_addr: String,
    pub to_addr: String,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClientRuntimeInfo {
    pub device_sn: String,
    pub device_type: String,
    pub device_app_id: Option<String>,
    pub package_id: Option<String>,
    pub app_version: String,
}

#[derive(Debug)]
pub enum ConsoleEvent {
    ClientStarted {
        client_id: ClientId,
    },
    ClientFailed {
        client_id: ClientId,
        error: String,
    },
    Log {
        client_id: Option<ClientId>,
        message: String,
    },
    Notify {
        client_id: ClientId,
        payload: String,
    },
    ImportFinished {
        client_id: ClientId,
        messages: Vec<String>,
    },
    WalletsLoaded {
        client_id: ClientId,
        wallets: Vec<ApiWalletRow>,
    },
    RuntimeInfoLoaded {
        client_id: ClientId,
        info: ClientRuntimeInfo,
    },
    BindFinished {
        client_id: ClientId,
    },
    AccountAddressesLoaded {
        client_id: ClientId,
        wallet_address: String,
        rows: Vec<AccountAddressRow>,
    },
    BalanceAssetsLoaded {
        client_id: ClientId,
        wallet_address: String,
        rows: Vec<BalanceAssetRow>,
    },
    WithdrawOrdersLoaded {
        client_id: ClientId,
        rows: Vec<WithdrawOrderRow>,
    },
    WithdrawReviewFinished {
        client_id: ClientId,
    },
    ChainNodeSet {
        client_id: ClientId,
        chain_code: String,
        rpc_url: String,
    },
    ChainNodeSetFailed {
        client_id: ClientId,
        error: String,
    },
    TaskQueueStatusLoaded {
        client_id: ClientId,
        summary: ConsoleTaskQueueSummary,
    },
    TradeDiagnosticsLoaded {
        client_id: ClientId,
        diagnostics: TradeDiagnostics,
    },
    LoadedTargets {
        client_id: ClientId,
        targets: Vec<String>,
    },
    TransferFinished {
        client_id: ClientId,
        summary: BatchTransferSummary,
    },
}
