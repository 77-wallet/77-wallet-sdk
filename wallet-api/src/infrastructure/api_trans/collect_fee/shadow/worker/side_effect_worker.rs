// collect_fee/shadow/worker/side_effect_worker.rs

use std::sync::Arc;

use tracing::{error, info, warn};
use wallet_database::{ApiTransactionDbPool, repositories::api_wallet::fee::ApiFeeRepo};
use wallet_transport_backend::request::api_wallet::transaction::{
    TransAckType, TransEventAckReq, TransType,
};

use crate::{
    error::service::ServiceError, infrastructure::api_trans::collect_fee::shadow::ShadowScanner,
};

/// SideEffectCommand 命令
#[derive(Debug, Clone)]
pub enum SideEffectCommand {
    /// 发送交易 ACK
    SendOrderAck(String),
    /// 发送交易结果 ACK
    SendResultAck(String),
    /// 上传交易执行回执
    UploadTxExecReceipt(String),
}

impl SideEffectCommand {
    /// 从 SideEffectCommand 生成对应的 RunningKey
    pub fn to_running_key(
        &self,
    ) -> crate::infrastructure::api_trans::collect_fee::shadow::dispatcher::RunningKey {
        match self {
            SideEffectCommand::SendOrderAck(trade_no) => {
                crate::infrastructure::api_trans::collect_fee::shadow::dispatcher::RunningKey::SendTxAck(trade_no.clone())
            }
            SideEffectCommand::SendResultAck(trade_no) => {
                crate::infrastructure::api_trans::collect_fee::shadow::dispatcher::RunningKey::SendTxResAck(trade_no.clone())
            }
            SideEffectCommand::UploadTxExecReceipt(trade_no) => {
                crate::infrastructure::api_trans::collect_fee::shadow::dispatcher::RunningKey::UploadTxExecReceipt(trade_no.clone())
            }
        }
    }

    /// 获取交易单号
    pub fn get_trade_no(&self) -> &str {
        match self {
            SideEffectCommand::SendOrderAck(trade_no) => trade_no,
            SideEffectCommand::SendResultAck(trade_no) => trade_no,
            SideEffectCommand::UploadTxExecReceipt(trade_no) => trade_no,
        }
    }
}

/// SideEffectWorker
///
/// 负责处理副作用操作：
/// - 发送交易 ACK
/// - 发送交易结果 ACK
/// - 上传交易执行回执
#[derive(Clone)]
pub struct SideEffectWorker {
    ctx: &'static crate::context::Context,
    /// ShadowScanner 引用，用于直接调用 try_advance
    scanner: Arc<ShadowScanner>,
}

impl SideEffectWorker {
    pub fn new(ctx: &'static crate::context::Context, scanner: Arc<ShadowScanner>) -> Self {
        Self { ctx, scanner }
    }

    /// 处理命令
    pub async fn handle(&self, command: SideEffectCommand) {
        let trade_no = command.get_trade_no().to_string();
        let trade_no_clone = trade_no.clone();
        let self_clone = self.clone();

        match tokio::time::timeout(std::time::Duration::from_secs(30), async move {
            match command {
                SideEffectCommand::SendOrderAck(trade_no) => {
                    self_clone.send_order_ack(&trade_no).await
                }
                SideEffectCommand::SendResultAck(trade_no) => {
                    self_clone.send_tx_res_ack(&trade_no).await
                }
                SideEffectCommand::UploadTxExecReceipt(trade_no) => {
                    self_clone.upload_tx_exec_receipt(&trade_no).await
                }
            }
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                error!(trade_no = %trade_no_clone, %error, "SideEffectWorker command failed");
            }
            Err(_) => {
                error!(trade_no = %trade_no_clone, "SideEffectWorker timeout after 30 seconds");
            }
        }
    }

    /// 发送交易 ACK
    ///
    /// SideEffect: send_tx_ack
    /// Requires:
    /// - raw_tx IS NOT NULL
    /// - tx_ack_sent_at IS NULL
    async fn send_order_ack(&self, trade_no: &str) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, "Sending order ACK");
        let pool = self.ctx.api_transaction_pool()?;

        // 强制读取事实，确保副作用基于最新 DB 状态（防止幻读）
        let fee = ApiFeeRepo::get_api_fee_by_trade_no(&pool, trade_no).await?;

        if fee.tx_ack_sent_at.is_some() {
            warn!(trade_no = %trade_no, "Tx ACK skipped: already sent");
            return Ok(());
        }

        // 标记交易 ACK 尝试
        info!(trade_no = %trade_no, "Marking tx ACK as attempted");
        ApiFeeRepo::mark_tx_ack_attempted(&pool, trade_no).await?;
        info!(trade_no = %trade_no, "Tx ACK marked as attempted successfully");

        // 发送交易 ACK 逻辑
        let backend = self.ctx.get_global_backend_api();
        let trans_event_req =
            TransEventAckReq::new(&fee.trade_no, TransType::ColFee, TransAckType::Tx);

        match backend.trans_event_ack(&trans_event_req).await {
            Ok(_) => {
                info!(trade_no = %trade_no, "Tx ACK sent successfully");
                // 成功路径：标记交易 ACK 已发送
                if let Err(e) = ApiFeeRepo::set_tx_ack_sent(&pool, trade_no).await {
                    error!(trade_no = %trade_no, error = %e, "Failed to mark tx ACK sent");
                } else {
                    // 直接调用 try_advance 进行点对点唤醒
                    self.scanner.try_advance(&trade_no).await;
                }
            }
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, "Failed to send tx ACK");
                // 失败路径：只保留 attempted 状态，让 Scanner 重试
            }
        }

        info!(trade_no = %trade_no, "Tx ACK processing completed");

        Ok(())
    }

    /// 发送交易结果 ACK
    ///
    /// SideEffect: send_tx_res_ack
    /// Requires:
    /// - tx_hash IS NOT NULL
    /// - tx_res_ack_sent_at IS NULL
    async fn send_tx_res_ack(&self, trade_no: &str) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, "Sending tx res ACK");
        let pool = self.ctx.api_transaction_pool()?;

        // 强制读取事实，确保副作用基于最新 DB 状态（防止幻读）
        let fee = ApiFeeRepo::get_api_fee_by_trade_no(&pool, trade_no).await?;

        // 检查是否允许发送结果 ACK
        // - tx_hash 必须已存在
        // - 尚未发送过结果 ACK
        if fee.tx_hash.is_none() {
            warn!(trade_no = %trade_no, "Tx res ACK skipped: tx_hash not exists");
            return Ok(());
        }

        // ✅ 强顺序屏障：TX_RES ACK 只能在已收到并持久化 AWM_ORDER_TRANS_RES 后发送
        if fee.tx_res_received_at.is_none() {
            warn!(trade_no = %trade_no, "Tx res ACK skipped: tx_res not received");
            return Ok(());
        }

        if fee.tx_res_ack_sent_at.is_some() {
            if fee.finished_at.is_none() && fee.transaction_time.is_some() {
                // 兼容历史半完成事实：tx_res_ack 已写但 finished 未写（例如 kill -9）
                warn!(
                    trade_no = %trade_no,
                    "Tx res ACK already sent but fee not finished; repairing finished_at"
                );
                match ApiFeeRepo::mark_chain_finished(&pool, trade_no).await {
                    Ok(_) => self.scanner.try_advance(&trade_no).await,
                    Err(e) => error!(
                        trade_no = %trade_no,
                        error = %e,
                        "Failed to repair fee finished_at"
                    ),
                }
            } else if fee.transaction_time.is_none() {
                warn!(
                    trade_no = %trade_no,
                    "Tx res ACK already sent but transaction_time is NULL; skip repairing finished_at"
                );
            }

            warn!(trade_no = %trade_no, "Tx res ACK skipped: already sent");
            return Ok(());
        }

        if fee.transaction_time.is_none() {
            warn!(
                trade_no = %trade_no,
                "Transaction time is NULL; cannot send tx res ACK"
            );
            return Ok(());
        }

        // 标记交易结果 ACK 尝试
        info!(trade_no = %trade_no, "Marking tx res ACK as attempted");
        if let Err(e) = ApiFeeRepo::mark_tx_res_ack_attempted(&pool, trade_no).await {
            error!(trade_no = %trade_no, error = %e, "Failed to mark tx res ACK attempted");
            return Ok(());
        }
        info!(trade_no = %trade_no, "Tx res ACK marked as attempted successfully");

        // 发送交易结果 ACK 逻辑
        let backend = self.ctx.get_global_backend_api();
        let trans_event_req =
            TransEventAckReq::new(&fee.trade_no, TransType::ColFee, TransAckType::TxRes);

        match backend.trans_event_ack(&trans_event_req).await {
            Ok(_) => {
                info!(trade_no = %trade_no, "Tx res ACK sent successfully");
                // 成功路径：标记交易结果 ACK 已发送并标记链上终态（原子操作）
                if let Err(e) =
                    ApiFeeRepo::set_tx_res_ack_sent_and_mark_chain_finished(&pool, trade_no).await
                {
                    error!(trade_no = %trade_no, error = %e, "Failed to mark tx res ACK sent and chain finished");
                } else {
                    // 直接调用 try_advance 进行点对点唤醒
                    self.scanner.try_advance(&trade_no).await;
                }
            }
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, "Failed to send tx res ACK");
                // 失败路径：只保留 attempted 状态，让 Scanner 重试
            }
        }

        info!(trade_no = %trade_no, "Tx res ACK processing completed");

        Ok(())
    }

    /// 上传交易执行回执
    ///
    /// SideEffect: upload_tx_exec_receipt
    /// Requires:
    /// - transaction_time IS NOT NULL
    /// - tx_exec_receipt_uploaded_at IS NULL
    async fn upload_tx_exec_receipt(&self, trade_no: &str) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, "Uploading tx exec receipt");
        let pool = self.ctx.api_transaction_pool()?;

        // 强制读取事实，确保副作用基于最新 DB 状态（防止幻读）
        let fee = ApiFeeRepo::get_api_fee_by_trade_no(&pool, trade_no).await?;

        if fee.tx_exec_receipt_uploaded_at.is_some() {
            warn!(trade_no = %trade_no, "Tx exec receipt upload skipped: already uploaded");
            return Ok(());
        }

        // 构建交易执行回执上传请求
        let upload_payload = match Self::build_tx_exec_receipt_payload(&fee, trade_no).await {
            Some(payload) => payload,
            None => {
                info!(
                    trade_no = %trade_no,
                    last_broadcast_at_present = %fee.last_broadcast_at.is_some(),
                    transaction_time_present = %fee.transaction_time.is_some(),
                    err_code_present = %fee.err_code.is_some(),
                    "Tx exec receipt still pending, skip upload"
                );
                return Ok(());
            }
        };
        info!(trade_no = %trade_no, "Built tx exec receipt upload payload");

        let tx_hash_missing =
            fee.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
        if upload_payload.is_success() && tx_hash_missing {
            error!(
                trade_no = %trade_no,
                source = "side_effect_worker",
                block_reason = "blocked_by_missing_tx_hash",
                last_broadcast_at_present = %fee.last_broadcast_at.is_some(),
                transaction_time_present = %fee.transaction_time.is_some(),
                tx_hash_is_none = %fee.tx_hash.is_none(),
                tx_hash_is_empty = %fee.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(false),
                "Skip UploadTxExecReceipt: blocked_by_missing_tx_hash (success payload requires non-empty tx_hash)"
            );
            return Ok(());
        }

        // 标记交易执行回执上传尝试
        info!(trade_no = %trade_no, "Marking tx exec receipt as attempted");
        ApiFeeRepo::mark_tx_exec_receipt_attempted(&pool, trade_no).await?;
        info!(trade_no = %trade_no, "Tx exec receipt marked as attempted successfully");

        // 获取backend_api
        let backend = self.ctx.get_global_backend_api();

        // 上传交易执行回执
        match backend.upload_tx_exec_receipt(&upload_payload).await {
            Ok(_) => {
                info!(trade_no = %trade_no, "Tx exec receipt uploaded successfully");
                // 成功路径：标记交易执行回执已上传
                if let Err(e) = ApiFeeRepo::mark_tx_exec_receipt_uploaded(&pool, trade_no).await {
                    error!(trade_no = %trade_no, error = %e, "Failed to mark tx exec receipt uploaded");
                } else {
                    // 标记交易终态：所有必要的副作用已完成
                    // 仅在“无成功证据且存在失败证据”时收口，避免链上已成功时误收口失败终态。
                    if upload_payload.is_fail()
                        && fee.transaction_time.is_none()
                        && fee.err_code.is_some()
                    {
                        info!(trade_no = %trade_no, source = "side_effect_worker", "Marking fee as finished");
                        if let Err(e) = ApiFeeRepo::mark_chain_finished(&pool, trade_no).await {
                            error!(trade_no = %trade_no, error = %e, "Failed to mark fee as finished");
                        } else {
                            info!(trade_no = %trade_no, source = "side_effect_worker", "Fee marked as finished successfully");
                        }
                    }
                    // 直接调用 try_advance 进行点对点唤醒
                    self.scanner.try_advance(&trade_no).await;
                }
            }
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, "Failed to upload tx exec receipt");
                // 失败路径：只保留 attempted 状态，让 Scanner 重试
            }
        }

        info!(trade_no = %trade_no, "Tx exec receipt processing completed");

        Ok(())
    }

    /// 构建交易执行回执上传请求
    async fn build_tx_exec_receipt_payload(
        fee: &wallet_database::entities::api_fee::ApiFeeEntity,
        trade_no: &str,
    ) -> Option<wallet_transport_backend::request::api_wallet::transaction::TxExecReceiptUploadReq>
    {
        if fee.transaction_time.is_none() && fee.err_code.is_none() {
            return None;
        }

        // 构建状态
        let upload_status = if fee.transaction_time.is_some() {
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Success
        } else if fee.err_code.is_some() {
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Fail
        } else {
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Fail
        };

        let tx_hash_missing =
            fee.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
        if fee.transaction_time.is_some() && tx_hash_missing {
            error!(
                trade_no = %trade_no,
                source = "side_effect_worker",
                transaction_time_present = %fee.transaction_time.is_some(),
                tx_hash_is_none = %fee.tx_hash.is_none(),
                tx_hash_is_empty = %fee.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(false),
                err_code_present = %fee.err_code.is_some(),
                "Inconsistent fee execution facts: execution evidence exists but tx_hash is missing"
            );
        }

        // 构建备注
        let remark = if matches!(
            upload_status,
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Success
        ) || fee.err_msg.as_deref().unwrap_or("").is_empty()
        {
            ""
        } else {
            fee.err_msg.as_deref().unwrap_or("")
        };

        // 构建请求
        let payload =
            wallet_transport_backend::request::api_wallet::transaction::TxExecReceiptUploadReq::new(
                Some(&fee.from_addr),
                Some(&fee.to_addr),
                trade_no,
                wallet_transport_backend::request::api_wallet::transaction::TransType::ColFee,
                fee.tx_hash.as_deref(),
                upload_status,
                remark,
            );

        Some(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::SideEffectWorker;
    use chrono::Utc;
    use wallet_database::entities::{
        api_fee::{ApiFeeEntity, ApiFeeStatus, ErrCode},
        asset_token_key::AssetTokenKey,
    };

    fn base_fee() -> ApiFeeEntity {
        ApiFeeEntity {
            id: 1,
            name: "n".to_string(),
            uid: "u".to_string(),
            from_addr: "from".to_string(),
            to_addr: "to".to_string(),
            value: "0".to_string(),
            validate: "v".to_string(),
            chain_code: "eth".to_string(),
            token_addr: AssetTokenKey::Native,
            symbol: "s".to_string(),
            trade_no: "F_SIDE_EFFECT_TEST".to_string(),
            trade_type: 3,
            status: ApiFeeStatus::Init,
            nonce: 0,
            tx_hash: Some("h".to_string()),
            raw_tx: Some("{}".to_string()),
            resource_consume: "0".to_string(),
            transaction_fee: "0".to_string(),
            transaction_time: None,
            block_height: Some("0".to_string()),
            notes: Some("".to_string()),
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: Some("".to_string()),
            tx_ack_sent_at: Some(Utc::now()),
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_exec_receipt_uploaded_at: None,
            tx_res_ack_sent_at: None,
            tx_res_received_at: None,
            finished_at: None,
            created_at: Utc::now(),
            updated_at: Some(Utc::now()),
        }
    }

    #[tokio::test]
    async fn build_tx_exec_receipt_payload_marks_confirmed_success() {
        let mut fee = base_fee();
        fee.transaction_time = Some(Utc::now());
        fee.last_broadcast_at = Some(Utc::now());

        let payload = SideEffectWorker::build_tx_exec_receipt_payload(&fee, &fee.trade_no)
            .await
            .expect("confirmed success should build payload");

        assert!(payload.is_success());
    }

    #[tokio::test]
    async fn build_tx_exec_receipt_payload_marks_failure_fact() {
        let mut fee = base_fee();
        fee.err_code = Some(ErrCode::UnknownError);

        let payload = SideEffectWorker::build_tx_exec_receipt_payload(&fee, &fee.trade_no)
            .await
            .expect("failure fact should build payload");

        assert!(payload.is_fail());
    }

    #[tokio::test]
    async fn build_tx_exec_receipt_payload_rejects_pending_facts() {
        let mut fee = base_fee();
        fee.transaction_time = None;
        fee.err_code = None;
        fee.last_broadcast_at = Some(Utc::now());

        let payload = SideEffectWorker::build_tx_exec_receipt_payload(&fee, &fee.trade_no).await;

        assert!(payload.is_none(), "pending facts should be skipped");
    }
}
