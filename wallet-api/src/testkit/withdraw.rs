//! Test-only entrypoints for withdraw worker and scanner steps.
//!
//! Integration tests use these wrappers when the behavior under test lives
//! behind crate-private workflow types.

use std::sync::Arc;

use crate::{
    context::Context,
    error::service::ServiceError,
    infrastructure::api_trans::withdraw::{
        ShadowWithdrawCommand, ShadowWithdrawWorker, WithdrawChainIntent, WithdrawIntent,
        WithdrawShadowScanner, WithdrawShadowScannerConfig, WithdrawShadowSideEffectCommand,
        WithdrawShadowSideEffectWorker, WithdrawSideEffectIntent,
    },
};

pub async fn send_resource_result_ack_via_worker(
    ctx: &'static Context,
    resource_trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner = Arc::new(WithdrawShadowScanner::new(
        ctx,
        WithdrawShadowScannerConfig {
            scan_interval: std::time::Duration::from_secs(60),
            max_items_per_scan: 8,
            max_fee_estimate_per_scan: 4,
            max_resource_gate_per_scan: 4,
        },
        intent_tx,
        None,
    ));
    let worker = WithdrawShadowSideEffectWorker::new(ctx, scanner);
    worker
        .handle(WithdrawShadowSideEffectCommand::SendResourceResultAck(
            resource_trade_no.to_string(),
        ))
        .await
}

pub async fn send_tx_ack_via_worker(
    ctx: &'static Context,
    trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner = Arc::new(WithdrawShadowScanner::new(
        ctx,
        WithdrawShadowScannerConfig {
            scan_interval: std::time::Duration::from_secs(60),
            max_items_per_scan: 8,
            max_fee_estimate_per_scan: 4,
            max_resource_gate_per_scan: 4,
        },
        intent_tx,
        None,
    ));
    let worker = WithdrawShadowSideEffectWorker::new(ctx, scanner);
    worker.handle(WithdrawShadowSideEffectCommand::SendTxAck(trade_no.to_string())).await
}

pub async fn upload_resource_tx_exec_receipt_via_worker(
    ctx: &'static Context,
    resource_trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner = Arc::new(WithdrawShadowScanner::new(
        ctx,
        WithdrawShadowScannerConfig {
            scan_interval: std::time::Duration::from_secs(60),
            max_items_per_scan: 8,
            max_fee_estimate_per_scan: 4,
            max_resource_gate_per_scan: 4,
        },
        intent_tx,
        None,
    ));
    let worker = WithdrawShadowSideEffectWorker::new(ctx, scanner);
    worker
        .handle(WithdrawShadowSideEffectCommand::UploadResourceTxExecReceipt(
            resource_trade_no.to_string(),
        ))
        .await
}

pub async fn eval_withdraw_resource_gate_via_worker(
    ctx: &'static Context,
    trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner = Arc::new(WithdrawShadowScanner::new(
        ctx,
        WithdrawShadowScannerConfig {
            scan_interval: std::time::Duration::from_secs(60),
            max_items_per_scan: 8,
            max_fee_estimate_per_scan: 4,
            max_resource_gate_per_scan: 4,
        },
        intent_tx,
        None,
    ));
    let worker = ShadowWithdrawWorker::new(ctx, scanner);
    worker.handle(ShadowWithdrawCommand::EvalResourceGate(trade_no.to_string())).await
}

pub async fn scan_withdraw_intent_labels_once(
    ctx: &'static Context,
) -> Result<Vec<String>, ServiceError> {
    let intents = scan_withdraw_intent_summaries_once(ctx).await?;

    Ok(intents.into_iter().map(|(label, _)| label).collect())
}

pub async fn scan_withdraw_intent_labels_for_trade_once(
    ctx: &'static Context,
    trade_no: &str,
) -> Result<Vec<String>, ServiceError> {
    let intents = scan_withdraw_intent_summaries_once(ctx).await?;

    Ok(intents
        .into_iter()
        .filter(|(_, intent_trade_no)| intent_trade_no == trade_no)
        .map(|(label, _)| label)
        .collect())
}

async fn scan_withdraw_intent_summaries_once(
    ctx: &'static Context,
) -> Result<Vec<(String, String)>, ServiceError> {
    let (intent_tx, mut intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner = WithdrawShadowScanner::new(
        ctx,
        WithdrawShadowScannerConfig {
            scan_interval: std::time::Duration::from_secs(60),
            max_items_per_scan: 8,
            max_fee_estimate_per_scan: 4,
            max_resource_gate_per_scan: 4,
        },
        intent_tx,
        None,
    );

    scanner.scan_round().await;

    let mut intents = Vec::new();
    while let Ok(intent) = intent_rx.try_recv() {
        intents.push(withdraw_intent_summary(intent));
    }

    Ok(intents)
}

fn withdraw_intent_summary(intent: WithdrawIntent) -> (String, String) {
    match intent {
        WithdrawIntent::Chain(WithdrawChainIntent::EstimateFee(trade_no)) => {
            ("EstimateFee".to_string(), trade_no)
        }
        WithdrawIntent::Chain(WithdrawChainIntent::EvalResourceGate(trade_no)) => {
            ("EvalResourceGate".to_string(), trade_no)
        }
        WithdrawIntent::Chain(WithdrawChainIntent::ApplyPlatformDelegate(trade_no)) => {
            ("ApplyPlatformDelegate".to_string(), trade_no)
        }
        WithdrawIntent::Chain(WithdrawChainIntent::ExecuteResourceDelegation(trade_no)) => {
            ("ExecuteResourceDelegation".to_string(), trade_no)
        }
        WithdrawIntent::Chain(WithdrawChainIntent::BuildTx(trade_no)) => {
            ("BuildTx".to_string(), trade_no)
        }
        WithdrawIntent::Chain(WithdrawChainIntent::BroadcastTx(trade_no)) => {
            ("BroadcastTx".to_string(), trade_no)
        }
        WithdrawIntent::Chain(WithdrawChainIntent::RecoverTx(trade_no)) => {
            ("RecoverTx".to_string(), trade_no)
        }
        WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxAck(trade_no)) => {
            ("SendTxAck".to_string(), trade_no)
        }
        WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendResourceResultAck(trade_no)) => {
            ("SendResourceResultAck".to_string(), trade_no)
        }
        WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendResourceTaskAck(trade_no)) => {
            ("SendResourceTaskAck".to_string(), trade_no)
        }
        WithdrawIntent::SideEffect(WithdrawSideEffectIntent::UploadResourceTxExecReceipt(
            trade_no,
        )) => ("UploadResourceTxExecReceipt".to_string(), trade_no),
        WithdrawIntent::SideEffect(WithdrawSideEffectIntent::SendTxResAck(trade_no)) => {
            ("SendTxResAck".to_string(), trade_no)
        }
        WithdrawIntent::SideEffect(WithdrawSideEffectIntent::UploadTxExecReceipt(trade_no)) => {
            ("UploadTxExecReceipt".to_string(), trade_no)
        }
    }
}

/// Observe the candidate used by withdrawal BuildTx, without reserving or signing.
pub async fn observe_withdraw_nonce(
    ctx: &'static Context,
    address: &str,
    chain: &str,
) -> Result<u64, ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner = Arc::new(WithdrawShadowScanner::new(
        ctx,
        WithdrawShadowScannerConfig {
            scan_interval: std::time::Duration::from_secs(60),
            max_items_per_scan: 8,
            max_fee_estimate_per_scan: 4,
            max_resource_gate_per_scan: 4,
        },
        intent_tx,
        None,
    ));
    ShadowWithdrawWorker::new(ctx, scanner)
        .observe_withdraw_nonce(address, chain)
        .await?
        .map(|(nonce, _)| nonce)
        .ok_or_else(|| ServiceError::Parameter("withdrawal nonce blocked".into()))
}

pub async fn observe_pending_self_transfers(ctx: &'static Context) -> Result<(), ServiceError> {
    crate::application::api_wallet_transfer::ApiTransferApplication::new(ctx)
        .observe_pending_self_transfers()
        .await
}

pub async fn run_self_transfer_observer(
    ctx: &'static Context,
    shutdown: tokio::sync::broadcast::Receiver<()>,
    period: std::time::Duration,
) {
    crate::application::api_wallet_transfer::ApiTransferApplication::new(ctx)
        .run_self_transfer_observer(shutdown, period)
        .await;
}

/// Exercise the unchanged shared broadcast contract without an application owner.
pub async fn broadcast_transfer_for_test(
    ctx: &'static Context,
    chain_code: &str,
    raw: crate::domain::api_wallet::RawTx,
    expected_hash: &str,
) -> Result<Option<crate::domain::chain::TransferResp>, ServiceError> {
    crate::domain::api_wallet::trans::ApiTransDomain::broadcast_transfer(
        ctx,
        chain_code,
        raw,
        Some(expected_hash),
    )
    .await
}

/// Run the real backend withdrawal Broadcast step against injected chain adapters.
pub async fn broadcast_withdraw_via_worker(
    ctx: &'static Context,
    trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner = Arc::new(WithdrawShadowScanner::new(
        ctx,
        WithdrawShadowScannerConfig::default(),
        intent_tx,
        None,
    ));
    ShadowWithdrawWorker::new(ctx, scanner)
        .handle(ShadowWithdrawCommand::Broadcast(trade_no.to_owned()))
        .await
}

/// Run the existing withdrawal failure receipt worker against an injected backend.
pub async fn upload_withdraw_tx_exec_receipt_via_worker(
    ctx: &'static Context,
    trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner = Arc::new(WithdrawShadowScanner::new(
        ctx,
        WithdrawShadowScannerConfig::default(),
        intent_tx,
        None,
    ));
    WithdrawShadowSideEffectWorker::new(ctx, scanner)
        .handle(WithdrawShadowSideEffectCommand::UploadTxExecReceipt(trade_no.to_owned()))
        .await
}

/// Run BuildTx with fixture keys and an injected chain adapter.
pub async fn build_withdraw_via_worker(
    ctx: &'static Context,
    trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner = Arc::new(WithdrawShadowScanner::new(
        ctx,
        WithdrawShadowScannerConfig::default(),
        intent_tx,
        None,
    ));
    ShadowWithdrawWorker::new(ctx, scanner)
        .process_build_tx_with_key(trade_no.to_owned(), Some("mock-private-key".to_string().into()))
        .await
}
