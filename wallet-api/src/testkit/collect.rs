//! Test-only entrypoints for collect shadow workflow steps.
//!
//! Integration tests use these wrappers when the behavior under test lives
//! behind crate-private collect worker, scanner, or dispatcher types.

use std::sync::Arc;

use wallet_database::{
    ApiTransactionDbPool,
    entities::{api_collect::ApiCollectEntity, api_trade_type::ApiTradeType},
    repositories::api_wallet::{
        collect::ApiCollectRepo, resource_delegation::ApiResourceDelegationRepo,
    },
};
use wallet_transport_backend::request::api_wallet::transaction::{
    TransStatus, TransType, TxExecReceiptUploadReq,
};

use crate::{
    context::Context,
    error::service::ServiceError,
    infrastructure::api_trans::{
        AddressLockManager,
        collect::shadow::{
            ChainIntent, CollectIntent, DispatcherConfig, ScannerConfig, ShadowAdvancer,
            ShadowCollectCommand, ShadowCollectWorker, ShadowDispatcher, ShadowScanner,
            SideEffectCommand, SideEffectIntent, SideEffectWorker,
        },
    },
};

/// Test-facing wrapper around the collect shadow worker's fee check.
///
/// Keeping this in `testkit` lets integration tests exercise the real
/// workflow without exposing helper methods from the business worker itself.
pub async fn shadow_collect_check_fee(
    worker: &ShadowCollectWorker,
    req: &ApiCollectEntity,
) -> Result<bool, ServiceError> {
    worker.check_fee(req).await
}

pub fn build_collect_tx_exec_receipt_payload(
    req: &ApiCollectEntity,
    trade_no: &str,
) -> TxExecReceiptUploadReq {
    let upload_status = if req.transaction_time.is_some() || req.last_broadcast_at.is_some() {
        TransStatus::Success
    } else {
        TransStatus::Fail
    };

    let remark = if matches!(upload_status, TransStatus::Success)
        || req.err_msg.as_deref().unwrap_or("").is_empty()
    {
        ""
    } else {
        req.err_msg.as_deref().unwrap_or("")
    };

    TxExecReceiptUploadReq::new(
        Some(&req.from_addr),
        Some(&req.to_addr),
        trade_no,
        TransType::Col,
        req.tx_hash.as_deref(),
        upload_status,
        remark,
    )
}

pub async fn upload_collect_tx_exec_receipt_via_worker(
    ctx: &'static Context,
    trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let (diagnose_tx, _diagnose_rx) = tokio::sync::mpsc::channel(8);
    let advancer = Arc::new(ShadowAdvancer::new(ctx, intent_tx, Some(diagnose_tx)));
    let worker = SideEffectWorker::new(ctx, advancer);
    worker.handle(SideEffectCommand::UploadTxExecReceipt(trade_no.to_string())).await
}

pub async fn upload_collect_service_fee_via_worker(
    ctx: &'static Context,
    trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let (diagnose_tx, _diagnose_rx) = tokio::sync::mpsc::channel(8);
    let advancer = Arc::new(ShadowAdvancer::new(ctx, intent_tx, Some(diagnose_tx)));
    let worker = SideEffectWorker::new(ctx, advancer);
    worker.handle(SideEffectCommand::UploadServiceFee(trade_no.to_string())).await
}

pub async fn send_resource_result_ack_via_worker(
    ctx: &'static Context,
    resource_trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let (diagnose_tx, _diagnose_rx) = tokio::sync::mpsc::channel(8);
    let advancer = Arc::new(ShadowAdvancer::new(ctx, intent_tx, Some(diagnose_tx)));
    let worker = SideEffectWorker::new(ctx, advancer);
    worker.handle(SideEffectCommand::SendResourceResultAck(resource_trade_no.to_string())).await
}

pub async fn upload_resource_tx_exec_receipt_via_worker(
    ctx: &'static Context,
    resource_trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let (diagnose_tx, _diagnose_rx) = tokio::sync::mpsc::channel(8);
    let advancer = Arc::new(ShadowAdvancer::new(ctx, intent_tx, Some(diagnose_tx)));
    let worker = SideEffectWorker::new(ctx, advancer);
    worker
        .handle(SideEffectCommand::UploadResourceTxExecReceipt(resource_trade_no.to_string()))
        .await
}

pub async fn eval_collect_resource_gate_via_worker(
    ctx: &'static Context,
    trade_no: &str,
) -> Result<(), ServiceError> {
    let (intent_tx, _intent_rx) = tokio::sync::mpsc::channel(8);
    let (diagnose_tx, _diagnose_rx) = tokio::sync::mpsc::channel(8);
    let advancer = Arc::new(ShadowAdvancer::new(ctx, intent_tx, Some(diagnose_tx)));
    let worker = ShadowCollectWorker::new(ctx, Arc::new(AddressLockManager::new()), advancer);
    worker.handle(ShadowCollectCommand::EvalResourceGate(trade_no.to_string())).await
}

pub async fn upload_collect_tx_exec_receipt_via_backend(
    ctx: &'static Context,
    req: &ApiCollectEntity,
    trade_no: &str,
) -> Result<(), ServiceError> {
    let payload = build_collect_tx_exec_receipt_payload(req, trade_no);
    let backend_api = ctx.get_global_backend_api();
    backend_api.upload_tx_exec_receipt(&payload).await?;
    Ok(())
}

pub async fn scan_and_dispatch_collect_tx_exec_receipt_once(
    ctx: &'static Context,
    collect_pool: ApiTransactionDbPool,
) -> Result<Option<String>, ServiceError> {
    let (intent_tx, mut intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner_intent_tx = intent_tx.clone();
    let (diagnose_tx, _diagnose_rx) = tokio::sync::mpsc::channel(8);
    let advancer = Arc::new(ShadowAdvancer::new(ctx, intent_tx.clone(), Some(diagnose_tx)));
    let side_effect_worker = Arc::new(SideEffectWorker::new(ctx, advancer.clone()));
    let shadow_worker =
        Arc::new(ShadowCollectWorker::new(ctx, Arc::new(AddressLockManager::new()), advancer));
    let dispatcher = ShadowDispatcher::new(
        collect_pool.clone(),
        DispatcherConfig::default(),
        shadow_worker,
        side_effect_worker,
        intent_tx,
    );
    let scanner = ShadowScanner::new(
        ctx,
        ScannerConfig { scan_interval: std::time::Duration::from_secs(60), max_items_per_scan: 8 },
        scanner_intent_tx,
        None,
    );

    scanner.scan_round().await;

    let mut matched: Option<(
        crate::infrastructure::api_trans::collect::shadow::CollectIntent,
        String,
    )> = None;
    while let Ok(intent) = intent_rx.try_recv() {
        let trade_no = match &intent {
            crate::infrastructure::api_trans::collect::shadow::CollectIntent::SideEffect(
                crate::infrastructure::api_trans::collect::shadow::SideEffectIntent::UploadTxExecReceipt(
                    trade_no,
                ),
            ) => trade_no.clone(),
            _ => continue,
        };
        matched = Some((intent, trade_no));
        break;
    }

    let Some((intent, trade_no)) = matched else {
        return Ok(None);
    };

    dispatcher
        .handle_intent(intent)
        .await
        .map_err(|e| ServiceError::Parameter(format!("dispatcher test helper failed: {e}")))?;

    for _ in 0..40 {
        let rec = ApiCollectRepo::get_api_collect_by_trade_no(&collect_pool, &trade_no)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
        if rec.tx_exec_receipt_uploaded_at.is_some() {
            return Ok(Some(trade_no));
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    Ok(Some(trade_no))
}

pub async fn scan_collect_intent_labels_once(
    ctx: &'static Context,
) -> Result<Vec<String>, ServiceError> {
    let (intent_tx, mut intent_rx) = tokio::sync::mpsc::channel(8);
    let scanner = ShadowScanner::new(
        ctx,
        ScannerConfig { scan_interval: std::time::Duration::from_secs(60), max_items_per_scan: 8 },
        intent_tx,
        None,
    );

    scanner.scan_round().await;

    let mut labels = Vec::new();
    while let Ok(intent) = intent_rx.try_recv() {
        let label = match intent {
            CollectIntent::Chain(ChainIntent::ExpireUnexecuted(_)) => {
                "ExpireUnexecuted".to_string()
            }
            CollectIntent::Chain(ChainIntent::EvalResourceGate(_)) => {
                "EvalResourceGate".to_string()
            }
            CollectIntent::Chain(ChainIntent::BuildTx(_)) => "BuildTx".to_string(),
            CollectIntent::Chain(ChainIntent::BroadcastTx(_)) => "BroadcastTx".to_string(),
            CollectIntent::Chain(ChainIntent::RecoverTx(_)) => "RecoverTx".to_string(),
            CollectIntent::Chain(ChainIntent::ExecuteLocalResourceDelegation(_)) => {
                "ExecuteLocalResourceDelegation".to_string()
            }
            CollectIntent::Chain(ChainIntent::ExecuteAccountActivation(_)) => {
                "ExecuteAccountActivation".to_string()
            }
            CollectIntent::SideEffect(SideEffectIntent::SendOrderAck(_)) => {
                "SendOrderAck".to_string()
            }
            CollectIntent::SideEffect(SideEffectIntent::SendResultAck(_)) => {
                "SendResultAck".to_string()
            }
            CollectIntent::SideEffect(SideEffectIntent::SendTxFeeResAck(_)) => {
                "SendTxFeeResAck".to_string()
            }
            CollectIntent::SideEffect(SideEffectIntent::SendResourceResultAck(_)) => {
                "SendResourceResultAck".to_string()
            }
            CollectIntent::SideEffect(SideEffectIntent::SendResourceTaskAck(_)) => {
                "SendResourceTaskAck".to_string()
            }
            CollectIntent::SideEffect(SideEffectIntent::UploadResourceTxExecReceipt(_)) => {
                "UploadResourceTxExecReceipt".to_string()
            }
            CollectIntent::SideEffect(SideEffectIntent::UploadServiceFee(_)) => {
                "UploadServiceFee".to_string()
            }
            CollectIntent::SideEffect(SideEffectIntent::UploadTxExecReceipt(_)) => {
                "UploadTxExecReceipt".to_string()
            }
        };
        labels.push(label);
    }

    Ok(labels)
}

pub async fn scan_collect_resource_receipt_intent_labels_once(
    ctx: &'static Context,
) -> Result<Vec<String>, ServiceError> {
    let records = ApiResourceDelegationRepo::scan_need_tx_exec_receipt_upload_for_origin_type(
        &ctx.api_resource_pool()?,
        ApiTradeType::Collect as i64,
        8,
    )
    .await
    .map_err(|e| ServiceError::Database(e.into()))?;

    Ok(records.into_iter().map(|_| "UploadResourceTxExecReceipt".to_string()).collect())
}

/// Runs the real collect build lifecycle with a fake adapter and synthetic key.
pub async fn build_collect_candidate_for_test(
    ctx: &'static Context,
    trade_no: &str,
) -> Result<(), ServiceError> {
    let (tx, _) = tokio::sync::mpsc::channel(32);
    let advancer = Arc::new(ShadowAdvancer::new(ctx, tx, None));
    let worker = ShadowCollectWorker::new(ctx, Arc::new(AddressLockManager::new()), advancer);
    let result =
        worker.build_collect_candidate(trade_no, Some("mock-private-key".to_string().into())).await;
    if result.is_err() {
        ApiCollectRepo::clear_building_at(&ctx.api_transaction_pool()?, trade_no).await?;
    }
    result
}
