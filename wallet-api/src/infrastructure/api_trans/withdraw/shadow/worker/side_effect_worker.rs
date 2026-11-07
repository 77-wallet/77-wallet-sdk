// withdraw/shadow/worker/side_effect_worker.rs
use std::sync::Arc;

use tracing::{error, info, warn};
use wallet_database::{
    ApiTransactionDbPool, ApiWalletDbPool,
    entities::{
        api_resource_delegation::ApiResourceDelegationResultStatus,
        api_resource_gate::ApiResourceGateResult, api_trade_type::ApiTradeType,
    },
    repositories::api_wallet::{
        resource_delegation::ApiResourceDelegationRepo, withdraw::ApiWithdrawRepo,
    },
};
use wallet_transport_backend::request::api_wallet::transaction::{
    TransAckType, TransEventAckReq, TransStatus, TransType, TxExecReceiptUploadReq,
};

use crate::{
    error::service::ServiceError,
    infrastructure::api_trans::{
        resource_ack_type::{
            is_original_order_resource_result_fact, merchant_original_resource_result_ack_type,
            merchant_original_resource_result_trans_type, platform_resource_result_ack_type,
            platform_resource_task_trans_type,
        },
        resource_chain_facts::{
            resource_chain_failed, resource_chain_success, resource_failure_receipt_uploadable,
            resource_success_receipt_uploadable,
        },
        withdraw::shadow::ShadowScanner,
    },
};

enum ResourceGateReleaseOutcome {
    Success(ApiResourceGateResult),
    FailureBypass(ApiResourceGateResult),
}

/// SideEffectWorker
///
/// 负责处理外部依赖的副作用操作：
/// - 发送交易 ACK
/// - 发送交易结果 ACK
/// - 上传交易执行回执
///
/// SideEffectWorker design invariant:
///
/// - 所有副作用操作必须基于已确认的链事实
/// - 所有副作用操作必须有并发安全保障
/// - 所有副作用操作必须是幂等的
/// - 禁止修改链事实
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
    pub async fn handle(&self, command: super::SideEffectCommand) -> Result<(), ServiceError> {
        // 提取 trade_no 用于日志
        let trade_no = match &command {
            super::SideEffectCommand::SendTxAck(trade_no) => trade_no,
            super::SideEffectCommand::SendTxResAck(trade_no) => trade_no,
            super::SideEffectCommand::UploadTxExecReceipt(trade_no) => trade_no,
            super::SideEffectCommand::SendResourceResultAck(trade_no) => trade_no,
            super::SideEffectCommand::SendResourceTaskAck(trade_no) => trade_no,
            super::SideEffectCommand::UploadResourceTxExecReceipt(trade_no) => trade_no,
        };

        let trade_no_clone = trade_no.to_string();
        let self_clone = self.clone();

        match tokio::time::timeout(std::time::Duration::from_secs(30), async move {
            match command {
                super::SideEffectCommand::SendTxAck(trade_no) => {
                    self_clone.process_send_tx_ack(trade_no).await
                }
                super::SideEffectCommand::SendTxResAck(trade_no) => {
                    self_clone.process_send_tx_res_ack(trade_no).await
                }
                super::SideEffectCommand::UploadTxExecReceipt(trade_no) => {
                    self_clone.process_upload_tx_exec_receipt(trade_no).await
                }
                super::SideEffectCommand::SendResourceResultAck(trade_no) => {
                    self_clone.process_resource_result_ack(trade_no).await
                }
                super::SideEffectCommand::SendResourceTaskAck(trade_no) => {
                    self_clone.process_resource_task_ack(trade_no).await
                }
                super::SideEffectCommand::UploadResourceTxExecReceipt(trade_no) => {
                    self_clone.process_resource_tx_exec_receipt(trade_no).await
                }
            }
        })
        .await
        {
            Ok(result) => result,
            Err(_) => {
                error!(trade_no = %trade_no_clone, source = "side_effect_worker", "SideEffectWorker timeout after 30 seconds");
                Err(ServiceError::Timeout)
            }
        }
    }

    /// 发送交易 ACK
    async fn process_send_tx_ack(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "side_effect_worker", "Processing SendTxAck command");

        // 强制读取事实，确保副作用基于最新 DB 状态（防止幻读）
        let withdraw = match ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &self.ctx.api_transaction_pool()?,
            &trade_no,
            wallet_database::entities::api_trade_type::ApiTradeType::Withdraw,
        )
        .await
        {
            Ok(withdraw) => withdraw,
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, source = "side_effect_worker", "Failed to get API withdraw by trade no");
                return Err(ServiceError::Database(e.into()));
            }
        };

        if withdraw.tx_ack_sent_at.is_some() {
            info!(trade_no = %trade_no, source = "side_effect_worker", "Tx ACK skipped: already sent");
            return Ok(());
        }

        let Some(_tx_ack_guard) =
            crate::infrastructure::api_trans::withdraw::tx_ack_gate::try_acquire_tx_ack_gate(
                &trade_no,
            )
        else {
            info!(trade_no = %trade_no, source = "side_effect_worker", "Tx ACK skipped: already in flight");
            return Ok(());
        };

        // 发送交易 ACK 逻辑
        let backend = self.ctx.get_global_backend_api();
        use wallet_transport_backend::request::api_wallet::transaction::{
            TransAckType, TransEventAckReq, TransType,
        };
        let trans_event_req =
            TransEventAckReq::new(&withdraw.trade_no, TransType::Wd, TransAckType::Tx);

        match backend.trans_event_ack(&trans_event_req).await {
            Ok(_) => {
                info!(trade_no = %trade_no, source = "side_effect_worker", "Tx ACK sent successfully");
                // 成功路径：标记交易 ACK 已发送
                let rows_affected =
                    ApiWithdrawRepo::mark_tx_ack_sent(&self.ctx.api_transaction_pool()?, &trade_no)
                        .await
                        .map_err(|e| ServiceError::Database(e.into()))?;

                // 显式处理幂等情况：ACK 已被其他并发执行
                if rows_affected == 0 {
                    info!(trade_no = %trade_no, source = "side_effect_worker", "mark_tx_ack_sent skipped: ACK already sent (idempotent hit)");
                } else {
                    self.scanner.trigger_fee_estimate_fast_path_after_tx_ack(&trade_no);
                }
            }
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, source = "side_effect_worker", "Failed to send tx ACK");
                // 失败路径：让 Scanner 重试
            }
        }

        Ok(())
    }

    /// 发送交易结果 ACK
    async fn process_send_tx_res_ack(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "side_effect_worker", "Processing SendTxResAck command");

        // 强制读取事实，确保副作用基于最新 DB 状态（防止幻读）
        let withdraw = match ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &self.ctx.api_transaction_pool()?,
            &trade_no,
            wallet_database::entities::api_trade_type::ApiTradeType::Withdraw,
        )
        .await
        {
            Ok(withdraw) => withdraw,
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, source = "side_effect_worker", "Failed to get API withdraw by trade no");
                return Err(ServiceError::Database(e.into()));
            }
        };

        // 检查是否允许发送结果 ACK
        // - tx_hash 必须已存在
        // - 尚未发送过结果 ACK
        if withdraw.tx_hash.is_none() {
            info!(trade_no = %trade_no, source = "side_effect_worker", "Tx res ACK skipped: tx_hash not exists");
            return Ok(());
        }

        // ✅ 强顺序屏障：TX_RES ACK 只能在已收到并持久化 AWM_ORDER_TRANS_RES 后发送
        if withdraw.tx_res_received_at.is_none() {
            info!(
                trade_no = %trade_no,
                tx_hash_present = %withdraw.tx_hash.is_some(),
                tx_hash_empty = %withdraw.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(false),
                tx_res_received_at = %withdraw.tx_res_received_at.is_some(),
                transaction_time_present = %withdraw.transaction_time.is_some(),
                source = "side_effect_worker",
                "Tx res ACK skipped: tx_res not received"
            );
            return Ok(());
        }

        if withdraw.tx_res_ack_sent_at.is_some() {
            if withdraw.finished_at.is_none() && withdraw.transaction_time.is_some() {
                // 兼容历史半完成事实：tx_res_ack 已写但 finished 未写（例如 kill -9）
                info!(
                    trade_no = %trade_no,
                    source = "side_effect_worker",
                    "Tx res ACK already sent but withdraw not finished; repairing finished_at"
                );
                ApiWithdrawRepo::mark_chain_finished(&self.ctx.api_transaction_pool()?, &trade_no)
                    .await
                    .map_err(|e| ServiceError::Database(e.into()))?;
                self.scanner.try_advance(&trade_no).await;
            } else if withdraw.transaction_time.is_none() {
                info!(
                    trade_no = %trade_no,
                    source = "side_effect_worker",
                    "Tx res ACK already sent but transaction_time is NULL; skip repairing finished_at"
                );
            }

            info!(
                trade_no = %trade_no,
                source = "side_effect_worker",
                "Tx res ACK skipped: already sent"
            );
            return Ok(());
        }

        if withdraw.transaction_time.is_none() {
            info!(
                trade_no = %trade_no,
                tx_hash_present = %withdraw.tx_hash.is_some(),
                tx_hash_empty = %withdraw.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(false),
                tx_res_received_at = %withdraw.tx_res_received_at.is_some(),
                transaction_time_present = %withdraw.transaction_time.is_some(),
                source = "side_effect_worker",
                "Transaction time is NULL; cannot send tx res ACK"
            );
            return Ok(());
        }

        // 发送交易结果 ACK 逻辑
        let backend = self.ctx.get_global_backend_api();
        use wallet_transport_backend::request::api_wallet::transaction::{
            TransAckType, TransEventAckReq, TransType,
        };
        let trans_event_req =
            TransEventAckReq::new(&withdraw.trade_no, TransType::Wd, TransAckType::TxRes);

        match backend.trans_event_ack(&trans_event_req).await {
            Ok(_) => {
                info!(trade_no = %trade_no, source = "side_effect_worker", "Tx res ACK sent successfully");
                // 成功路径：标记交易结果 ACK 已发送
                let rows_affected = ApiWithdrawRepo::mark_tx_res_ack_sent_and_chain_finished(
                    &self.ctx.api_transaction_pool()?,
                    &trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;

                // 显式处理幂等情况：ACK 已被其他并发执行
                if rows_affected == 0 {
                    info!(trade_no = %trade_no, source = "side_effect_worker", "mark_tx_res_ack_sent skipped: ACK already sent (idempotent hit)");
                } else {
                    // 直接调用 try_advance 进行点对点唤醒
                    self.scanner.try_advance(&trade_no).await;
                }
            }
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, source = "side_effect_worker", "Failed to send tx res ACK");
                // 失败路径：让 Scanner 重试
            }
        }

        Ok(())
    }

    async fn process_resource_result_ack(
        &self,
        resource_trade_no: String,
    ) -> Result<(), ServiceError> {
        let resource_task = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if resource_task.finished_at.is_none() {
            return Ok(());
        }
        if resource_task.result_payload.is_none() {
            warn!(
                resource_trade_no = %resource_trade_no,
                "Resource result ACK skipped because backend result payload is missing"
            );
            return Ok(());
        }

        if resource_task.result_ack_sent_at.is_some() {
            return Ok(());
        }

        // Platform resource tasks (CD/CR) and merchant original-order resource
        // projections (C/W) use the same resource result table, but backend ACK
        // semantics are different. Keep the branch explicit at the side-effect
        // boundary so CD... cannot accidentally be ACKed as TX_RSC_RES.
        let (trans_type, ack_type) = if is_original_order_resource_result_fact(&resource_task) {
            (
                merchant_original_resource_result_trans_type(&resource_task),
                merchant_original_resource_result_ack_type(),
            )
        } else {
            (platform_resource_task_trans_type(&resource_task), platform_resource_result_ack_type())
        };
        let attempted = ApiResourceDelegationRepo::mark_result_ack_attempted(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if attempted == 0 {
            return Ok(());
        }
        let backend = self.ctx.get_global_backend_api();
        if let Err(e) = backend
            .trans_event_ack(&TransEventAckReq::new(&resource_trade_no, trans_type, ack_type))
            .await
        {
            return Err(e.into());
        }

        self.mark_resource_result_ack_and_project_withdraw_gate(&resource_task).await?;
        Ok(())
    }

    async fn process_resource_task_ack(
        &self,
        resource_trade_no: String,
    ) -> Result<(), ServiceError> {
        let resource_task = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if resource_task.task_ack_sent_at.is_some() {
            return Ok(());
        }

        let backend = self.ctx.get_global_backend_api();
        backend
            .trans_event_ack(&TransEventAckReq::new(
                &resource_trade_no,
                platform_resource_task_trans_type(&resource_task),
                TransAckType::Tx,
            ))
            .await?;

        let affected = ApiResourceDelegationRepo::mark_task_ack_sent(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if affected == 0 {
            warn!(resource_trade_no = %resource_trade_no, "Resource task ACK marked 0 rows");
        }
        Ok(())
    }

    async fn process_resource_tx_exec_receipt(
        &self,
        resource_trade_no: String,
    ) -> Result<(), ServiceError> {
        let resource_task = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if resource_task.tx_exec_receipt_uploaded_at.is_some() {
            return Ok(());
        }

        let payload = Self::build_resource_tx_exec_receipt_payload(&resource_task)?;
        let tx_hash_missing =
            resource_task.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
        if payload.is_success() && tx_hash_missing {
            return Err(ServiceError::Parameter(
                "resource delegation success receipt requires non-empty tx_hash".to_string(),
            ));
        }

        let backend = self.ctx.get_global_backend_api();
        backend.upload_tx_exec_receipt(&payload).await?;
        let affected = ApiResourceDelegationRepo::mark_tx_exec_receipt_uploaded(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if affected == 0 {
            warn!(resource_trade_no = %resource_trade_no, "Resource tx exec receipt marked 0 rows");
        }

        self.project_resource_task_outcome_to_withdraw_gate(
            &resource_task,
            ResourceGateReleaseOutcome::FailureBypass(
                ApiResourceGateResult::ResourceDelegationFailedBypass,
            ),
        )
        .await?;
        Ok(())
    }

    async fn mark_resource_result_ack_and_project_withdraw_gate(
        &self,
        resource_task: &wallet_database::entities::api_resource_delegation::ApiResourceDelegationEntity,
    ) -> Result<(), ServiceError> {
        let release = Self::withdraw_gate_release_from_resource_result(resource_task);
        let ack_rows = ApiResourceDelegationRepo::mark_result_ack_sent(
            &self.ctx.api_resource_pool()?,
            &resource_task.resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if ack_rows > 0 {
            if let Some((origin_trade_no, release_result)) = release.as_ref() {
                let release_rows = ApiWithdrawRepo::mark_resource_released(
                    &self.ctx.api_transaction_pool()?,
                    origin_trade_no,
                    *release_result,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                info!(
                    resource_trade_no = %resource_task.resource_trade_no,
                    origin_trade_no = %origin_trade_no,
                    release_result = ?release_result,
                    ack_rows,
                    release_rows,
                    source = "side_effect_worker",
                    "Withdraw resource result ACK projected to origin gate"
                );
            }
        }

        if ack_rows == 0 {
            warn!(
                resource_trade_no = %resource_task.resource_trade_no,
                "Resource result ACK marked 0 rows"
            );
        } else {
            if let Some((origin_trade_no, release_result)) = release {
                if release_result == ApiResourceGateResult::ResourceDelegationSuccess {
                    self.scanner.try_advance(&origin_trade_no).await;
                } else {
                    self.scanner
                        .trigger_build_fast_path_after_resource_gate_release(&origin_trade_no);
                }
            }
        }

        Ok(())
    }

    fn withdraw_gate_release_from_resource_result(
        resource_task: &wallet_database::entities::api_resource_delegation::ApiResourceDelegationEntity,
    ) -> Option<(String, ApiResourceGateResult)> {
        if resource_task.origin_trade_type != Some(ApiTradeType::Withdraw as i64) {
            return None;
        }

        let origin_trade_no = resource_task.origin_trade_no.clone()?;
        let release_result = if is_original_order_resource_result_fact(resource_task) {
            match resource_task.result_status {
                Some(ApiResourceDelegationResultStatus::Success) => {
                    ApiResourceGateResult::ResourceDelegationSuccess
                }
                Some(ApiResourceDelegationResultStatus::Fail) => {
                    ApiResourceGateResult::ResourceDelegationFailedBypass
                }
                None => return None,
            }
        } else if resource_chain_success(resource_task) && !resource_chain_failed(resource_task) {
            ApiResourceGateResult::ResourceDelegationSuccess
        } else if resource_chain_failed(resource_task) {
            ApiResourceGateResult::ResourceDelegationFailedBypass
        } else {
            return None;
        };

        Some((origin_trade_no, release_result))
    }

    async fn project_resource_task_outcome_to_withdraw_gate(
        &self,
        resource_task: &wallet_database::entities::api_resource_delegation::ApiResourceDelegationEntity,
        outcome: ResourceGateReleaseOutcome,
    ) -> Result<(), ServiceError> {
        let release_result = match outcome {
            ResourceGateReleaseOutcome::Success(release_result) => {
                let success = if is_original_order_resource_result_fact(resource_task) {
                    resource_task.result_status == Some(ApiResourceDelegationResultStatus::Success)
                } else {
                    resource_chain_success(resource_task) && !resource_chain_failed(resource_task)
                };
                if !success {
                    return Ok(());
                }
                release_result
            }
            ResourceGateReleaseOutcome::FailureBypass(release_result) => {
                let is_failure = if is_original_order_resource_result_fact(resource_task) {
                    resource_task.result_status == Some(ApiResourceDelegationResultStatus::Fail)
                } else {
                    resource_chain_failed(resource_task)
                };
                if !is_failure {
                    return Ok(());
                }
                release_result
            }
        };

        if resource_task.origin_trade_type != Some(ApiTradeType::Withdraw as i64) {
            return Ok(());
        }
        let Some(origin_trade_no) = resource_task.origin_trade_no.as_deref() else {
            return Ok(());
        };

        let withdraw = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &self.ctx.api_transaction_pool()?,
            origin_trade_no,
            ApiTradeType::Withdraw,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if withdraw.resource_gate_released_at.is_some() {
            self.scanner.trigger_build_fast_path_after_resource_gate_release(origin_trade_no);
            return Ok(());
        }

        let release_rows = if release_result == ApiResourceGateResult::ResourceDelegationSuccess {
            ApiWithdrawRepo::mark_resource_delegation_confirmed(
                &self.ctx.api_transaction_pool()?,
                origin_trade_no,
            )
            .await
        } else {
            ApiWithdrawRepo::mark_resource_released(
                &self.ctx.api_transaction_pool()?,
                origin_trade_no,
                release_result,
            )
            .await
        }
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(
            resource_trade_no = %resource_task.resource_trade_no,
            origin_trade_no = %origin_trade_no,
            release_result = ?release_result,
            release_rows,
            source = "side_effect_worker",
            "Withdraw resource gate projected from resource task outcome"
        );
        if release_result == ApiResourceGateResult::ResourceDelegationSuccess {
            self.scanner.try_advance(origin_trade_no).await;
        } else {
            self.scanner.trigger_build_fast_path_after_resource_gate_release(origin_trade_no);
        }
        Ok(())
    }

    fn build_resource_tx_exec_receipt_payload(
        resource_task: &wallet_database::entities::api_resource_delegation::ApiResourceDelegationEntity,
    ) -> Result<TxExecReceiptUploadReq, ServiceError> {
        let trans_type = platform_resource_task_trans_type(resource_task);
        let status = if resource_success_receipt_uploadable(resource_task) {
            TransStatus::Success
        } else if resource_failure_receipt_uploadable(resource_task) {
            TransStatus::Fail
        } else {
            return Err(ServiceError::Parameter(
                "resource delegation receipt upload requires confirmed success or failure chain facts"
                    .to_string(),
            ));
        };
        let remark = if matches!(status, TransStatus::Success) {
            ""
        } else {
            resource_task.err_msg.as_deref().unwrap_or("")
        };
        let mut payload = TxExecReceiptUploadReq::new(
            Some(&resource_task.owner_address),
            Some(&resource_task.receiver_address),
            &resource_task.resource_trade_no,
            trans_type,
            resource_task.tx_hash.as_deref(),
            status,
            remark,
        );
        if let Some(err_code) = resource_task.err_code.as_deref().filter(|s| !s.trim().is_empty()) {
            payload = payload.with_error_code(err_code);
        }
        Ok(payload)
    }

    /// 上传交易执行回执
    async fn process_upload_tx_exec_receipt(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "side_effect_worker", "Processing UploadTxExecReceipt command");

        // 强制读取事实，确保副作用基于最新 DB 状态（防止幻读）
        let withdraw = match ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &self.ctx.api_transaction_pool()?,
            &trade_no,
            wallet_database::entities::api_trade_type::ApiTradeType::Withdraw,
        )
        .await
        {
            Ok(withdraw) => withdraw,
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, source = "side_effect_worker", "Failed to get API withdraw by trade no");
                return Err(ServiceError::Database(e.into()));
            }
        };

        // 幂等保护：检查是否已上传执行回执
        if withdraw.tx_exec_receipt_uploaded_at.is_some() {
            info!(trade_no = %trade_no, source = "side_effect_worker", "TxExecReceipt already uploaded, skipping");
            return Ok(());
        }

        // 构建交易执行回执上传请求
        let upload_payload = self.build_tx_exec_receipt_payload(&withdraw, &trade_no).await?;
        info!(trade_no = %trade_no, source = "side_effect_worker", "Built tx exec receipt upload payload");

        let tx_hash_missing =
            withdraw.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
        if upload_payload.is_success() && tx_hash_missing {
            error!(
                trade_no = %trade_no,
                source = "side_effect_worker",
                block_reason = "blocked_by_missing_tx_hash",
                chain_success_at_present = %withdraw.chain_success_at.is_some(),
                chain_failed_at_present = %withdraw.chain_failed_at.is_some(),
                transaction_time_present = %withdraw.transaction_time.is_some(),
                last_broadcast_at_present = %withdraw.last_broadcast_at.is_some(),
                tx_hash_is_none = %withdraw.tx_hash.is_none(),
                tx_hash_is_empty = %withdraw.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(false),
                "Skip UploadTxExecReceipt: blocked_by_missing_tx_hash (success payload requires non-empty tx_hash)"
            );
            return Err(ServiceError::Parameter(
                "success tx_exec_receipt requires non-empty tx_hash".to_string(),
            ));
        }

        // 上传交易执行回执
        let backend = self.ctx.get_global_backend_api();
        match backend.upload_tx_exec_receipt(&upload_payload).await {
            Ok(_) => {
                info!(trade_no = %trade_no, source = "side_effect_worker", "Tx exec receipt uploaded successfully");
                // 成功路径：标记交易执行回执已上传
                if let Err(e) = ApiWithdrawRepo::mark_tx_exec_receipt_uploaded(
                    &self.ctx.api_transaction_pool()?,
                    &trade_no,
                )
                .await
                {
                    error!(trade_no = %trade_no, error = %e, source = "side_effect_worker", "Failed to mark tx exec receipt uploaded");
                } else {
                    // 标记交易终态：所有必要的副作用已完成
                    // 仅在“无成功证据且存在失败证据”时收口，避免链上已成功时误收口失败终态。
                    if upload_payload.is_fail()
                        && withdraw.transaction_time.is_none()
                        && withdraw.chain_success_at.is_none()
                        && (withdraw.chain_failed_at.is_some() || withdraw.err_code.is_some())
                    {
                        info!(trade_no = %trade_no, source = "side_effect_worker", "Marking withdraw as finished");
                        if let Err(e) = ApiWithdrawRepo::mark_chain_finished(
                            &self.ctx.api_transaction_pool()?,
                            &trade_no,
                        )
                        .await
                        {
                            error!(trade_no = %trade_no, error = %e, source = "side_effect_worker", "Failed to mark withdraw as finished");
                        } else {
                            info!(trade_no = %trade_no, source = "side_effect_worker", "Withdraw marked as finished successfully");
                        }
                    }
                    // 直接调用 try_advance 进行点对点唤醒
                    self.scanner.try_advance(&trade_no).await;
                }
            }
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, source = "side_effect_worker", "Failed to upload tx exec receipt");
                // 失败路径：让 Scanner 重试
            }
        }

        Ok(())
    }

    /// 构建交易执行回执上传请求
    async fn build_tx_exec_receipt_payload(
        &self,
        withdraw: &wallet_database::entities::api_withdraw::ApiWithdrawEntity,
        trade_no: &str,
    ) -> Result<
        wallet_transport_backend::request::api_wallet::transaction::TxExecReceiptUploadReq,
        ServiceError,
    > {
        use wallet_transport_backend::request::api_wallet::transaction::{
            TransType, TxExecReceiptUploadReq,
        };

        let upload_status = tx_exec_receipt_upload_status(withdraw);

        let tx_hash_missing =
            withdraw.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
        let has_success_execution_evidence = (withdraw.chain_success_at.is_some()
            || withdraw.transaction_time.is_some())
            && withdraw.chain_failed_at.is_none();
        if has_success_execution_evidence && tx_hash_missing {
            error!(
                trade_no = %trade_no,
                source = "side_effect_worker",
                chain_success_at_present = %withdraw.chain_success_at.is_some(),
                chain_failed_at_present = %withdraw.chain_failed_at.is_some(),
                transaction_time_present = %withdraw.transaction_time.is_some(),
                last_broadcast_at_present = %withdraw.last_broadcast_at.is_some(),
                tx_hash_is_none = %withdraw.tx_hash.is_none(),
                tx_hash_is_empty = %withdraw.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(false),
                err_code_present = %withdraw.err_code.is_some(),
                "Inconsistent withdraw execution facts: execution evidence exists but tx_hash is missing"
            );
        }

        // 构建备注
        let remark = if matches!(
            upload_status,
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Success
        ) {
            ""
        } else {
            withdraw.err_msg.as_deref().unwrap_or("")
        };

        // 构建请求
        let mut payload = TxExecReceiptUploadReq::new(
            Some(&withdraw.from_addr),
            Some(&withdraw.to_addr),
            trade_no,
            TransType::Wd,
            withdraw.tx_hash.as_deref(),
            upload_status,
            remark,
        );
        if let Some(err_code) = withdraw.err_code {
            payload = payload.with_error_code(&err_code.to_string());
        }

        Ok(payload)
    }
}

fn tx_exec_receipt_upload_status(
    withdraw: &wallet_database::entities::api_withdraw::ApiWithdrawEntity,
) -> wallet_transport_backend::request::api_wallet::transaction::TransStatus {
    if withdraw.chain_success_at.is_some() || withdraw.transaction_time.is_some() {
        wallet_transport_backend::request::api_wallet::transaction::TransStatus::Success
    } else if withdraw.chain_failed_at.is_some() || withdraw.err_code.is_some() {
        wallet_transport_backend::request::api_wallet::transaction::TransStatus::Fail
    } else {
        wallet_transport_backend::request::api_wallet::transaction::TransStatus::Fail
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::api_wallet_backend::ApiWalletBackend;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::{fs, path::PathBuf, sync::Arc};
    use tokio::sync::{OnceCell, mpsc};
    use wallet_database::{
        SqliteContext,
        entities::{
            api_resource_delegation::NewApiResourceDelegation,
            api_trade_type::ApiTradeType,
            api_withdraw::{ApiWithdrawEntity, ApiWithdrawStatus, ErrCode, WithdrawFailureStage},
            asset_token_key::AssetTokenKey,
        },
        repositories::api_wallet::{
            resource_delegation::ApiResourceDelegationRepo, withdraw::ApiWithdrawRepo,
        },
    };
    use wallet_transport_backend::{
        request::{
            DeviceDeleteReq, KeysInitReq,
            api_wallet::{
                address::ExpandAddressCompleteReq,
                swap::{ApiInitSwapReq, ApiInitSwapResponse},
                wallet::{
                    AppIdImportRechargeWalletReq, AppIdImportReq, AppIdUidUsageReq, BindAppIdReq,
                },
            },
        },
        response_vo::api_wallet::wallet::{
            AppIdUidUsageRes as ApiWalletAppIdUidUsageRes, KeysUidCheckRes, QueryUidBindInfoRes,
            QueryWalletActivationInfoResp, UidStatus,
        },
    };

    #[derive(Default)]
    struct NoopApiWalletBackend;

    #[async_trait]
    impl ApiWalletBackend for NoopApiWalletBackend {
        async fn wallet_bind_appid(
            &self,
            _: BindAppIdReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }
        async fn init_api_wallet(
            &self,
            _: AppIdImportReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }
        async fn old_keys_init(
            &self,
            _: KeysInitReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }
        async fn appid_import(
            &self,
            _: AppIdImportReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }
        async fn appid_import_recharge_wallet(
            &self,
            _: AppIdImportRechargeWalletReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }
        async fn keys_uid_check(
            &self,
            _: &str,
        ) -> Result<KeysUidCheckRes, crate::error::service::ServiceError> {
            Ok(KeysUidCheckRes { uid: String::new(), status: UidStatus::NormalWallet })
        }
        async fn query_uid_bind_info(
            &self,
            _: &str,
        ) -> Result<QueryUidBindInfoRes, crate::error::service::ServiceError> {
            Ok(QueryUidBindInfoRes {
                app_id: String::new(),
                org_id: String::new(),
                bind_status: false,
                sn: String::new(),
            })
        }
        async fn query_wallet_activation_info(
            &self,
            _: &str,
        ) -> Result<QueryWalletActivationInfoResp, crate::error::service::ServiceError> {
            Ok(QueryWalletActivationInfoResp(vec![]))
        }
        async fn appid_uid_usage(
            &self,
            _: AppIdUidUsageReq,
        ) -> Result<ApiWalletAppIdUidUsageRes, crate::error::service::ServiceError> {
            Ok(ApiWalletAppIdUidUsageRes { used: false })
        }
        async fn expand_address_complete(
            &self,
            _: ExpandAddressCompleteReq,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }
        async fn appid_withdrawal_wallet_change(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(), crate::error::service::ServiceError> {
            Ok(())
        }
        async fn init_swap(
            &self,
            _: &ApiInitSwapReq,
        ) -> Result<ApiInitSwapResponse, crate::error::service::ServiceError> {
            Ok(ApiInitSwapResponse { success: true, code: None, msg: None, data: None })
        }
        async fn device_delete(
            &self,
            _: &DeviceDeleteReq,
        ) -> Result<Option<()>, crate::error::service::ServiceError> {
            Ok(None)
        }
    }

    async fn test_ctx() -> &'static crate::context::Context {
        static CTX: tokio::sync::OnceCell<&'static crate::context::Context> =
            tokio::sync::OnceCell::const_new();
        *CTX.get_or_init(|| async {
            let config = crate::config::Config::new(
                r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
aggregate_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
"#,
            )
            .expect("parse test config");

            let mut dir = PathBuf::from(std::env::temp_dir());
            dir.push("wallet-api-shadow-withdraw-side-effect-tests");
            fs::create_dir_all(&dir).expect("ensure temp dir");
            let dirs = crate::dirs::Dirs::new(&dir.to_string_lossy()).expect("create dirs");
            crate::context::init_context_with_api_wallet_backend(
                "wallet_api_shadow_withdraw_side_effect_sn",
                "unittest",
                dirs,
                None,
                config,
                Arc::new(NoopApiWalletBackend::default()),
            )
            .await
            .expect("init test context")
        })
        .await
    }

    fn base_withdraw(trade_no: &str) -> ApiWithdrawEntity {
        ApiWithdrawEntity {
            id: 1,
            name: "n".to_string(),
            uid: "u".to_string(),
            from_addr: "from".to_string(),
            to_addr: "to".to_string(),
            value: "0".to_string(),
            validate: "v".to_string(),
            chain_code: "tron".to_string(),
            token_addr: AssetTokenKey::Native,
            symbol: "s".to_string(),
            trade_no: trade_no.to_string(),
            trade_type: ApiTradeType::Withdraw,
            init_status: ApiWithdrawStatus::Init,
            status: ApiWithdrawStatus::Init,
            nonce: 0,
            tx_hash: Some("0xhash".to_string()),
            raw_tx: Some("{}".to_string()),
            resource_consume: "0".to_string(),
            transaction_fee: "0".to_string(),
            estimated_transaction_fee: None,
            estimated_resource_consume: None,
            fee_estimated_at: None,
            transaction_time: None,
            block_height: None,
            notes: None,
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: None,
            resource_check_at: None,
            resource_gate_released_at: None,
            resource_gate_result: None,
            resource_block_reason: None,
            resource_dependency_trade_no: None,
            resource_dependency_type: None,
            tx_ack_sent_at: Some(Utc::now()),
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_res_ack_sent_at: None,
            tx_res_received_at: None,
            tx_exec_receipt_uploaded_at: None,
            finished_at: None,
            audit_passed_at: Some(Utc::now()),
            audit_rejected_at: None,
            audit_reason: None,
            chain_success_at: None,
            chain_failed_at: None,
            failure_stage: Some(WithdrawFailureStage::Unknown),
            created_at: Utc::now(),
            updated_at: Some(Utc::now()),
            out_order_id: None,
            client_id: None,
            create_time: None,
        }
    }

    #[test]
    fn tx_exec_receipt_status_is_success_only_for_confirmed_success() {
        let mut withdraw = base_withdraw("W1");
        withdraw.transaction_time = Some(Utc::now());

        assert_eq!(
            tx_exec_receipt_upload_status(&withdraw),
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Success
        );
    }

    #[test]
    fn tx_exec_receipt_status_is_not_success_for_broadcast_only_pending() {
        let mut withdraw = base_withdraw("W2");
        withdraw.last_broadcast_at = Some(Utc::now());

        assert_eq!(
            tx_exec_receipt_upload_status(&withdraw),
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Fail
        );
    }

    #[test]
    fn tx_exec_receipt_status_is_fail_for_failure_facts() {
        let mut withdraw = base_withdraw("W3");
        withdraw.err_code = Some(wallet_database::entities::api_withdraw::ErrCode::UnknownError);

        assert_eq!(
            tx_exec_receipt_upload_status(&withdraw),
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Fail
        );
    }

    #[tokio::test]
    async fn withdraw_tx_exec_receipt_payload_includes_failure_error_code() {
        let (intent_tx, _intent_rx) = mpsc::channel(1);
        let scanner = Arc::new(ShadowScanner::new(
            test_ctx().await,
            crate::infrastructure::api_trans::withdraw::shadow::scanner::ScannerConfig::default(),
            intent_tx,
            None,
        ));
        let worker = SideEffectWorker::new(test_ctx().await, scanner);
        let mut withdraw = base_withdraw("W_ERR_CODE");
        withdraw.tx_hash = None;
        withdraw.raw_tx = None;
        withdraw.err_code = Some(ErrCode::BalanceInsufficient);
        withdraw.err_msg = Some("withdraw amount is insufficient".to_string());
        withdraw.failure_stage = Some(WithdrawFailureStage::Build);

        let payload =
            worker.build_tx_exec_receipt_payload(&withdraw, &withdraw.trade_no).await.unwrap();
        let payload = serde_json::to_value(payload).expect("serialize payload");

        assert_eq!(payload["tradeNo"], "W_ERR_CODE");
        assert_eq!(payload["status"], "FAIL");
        assert_eq!(payload["errorCode"], "ERR_6001");
        assert_eq!(payload["remark"], "withdraw amount is insufficient");
    }

    #[tokio::test]
    async fn original_order_resource_success_releases_gate_after_ack_projection()
    -> anyhow::Result<()> {
        let ctx = test_ctx().await;
        let tx_pool = ctx.api_transaction_pool()?;
        let resource_pool = ctx.api_resource_pool()?;
        let trade_no =
            format!("W_rsc_ack_release_{}", Utc::now().timestamp_nanos_opt().unwrap_or_default());
        let (intent_tx, _intent_rx) = mpsc::channel(8);
        let scanner = Arc::new(ShadowScanner::new(
            ctx,
            crate::infrastructure::api_trans::withdraw::shadow::scanner::ScannerConfig::default(),
            intent_tx,
            None,
        ));
        let worker = SideEffectWorker::new(ctx, scanner);

        ApiWithdrawRepo::upsert_api_withdraw(
            &tx_pool,
            "uid_1",
            "withdraw",
            "from_addr",
            "to_addr",
            "1",
            "digest",
            "tron",
            AssetTokenKey::Native,
            "TRX",
            &trade_no,
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            ApiWithdrawStatus::Init,
            ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await?;
        ApiWithdrawRepo::mark_resource_blocked(
            &tx_pool,
            &trade_no,
            wallet_database::entities::api_resource_gate::ApiResourceBlockReason::NeedPlatformDelegate,
            None,
            Some(
                wallet_database::entities::api_resource_gate::ApiResourceDependencyType::PlatformDelegate,
            ),
        )
        .await?;

        let result_payload = format!(r#"{{"tradeNo":"{trade_no}","status":true}}"#);
        ApiResourceDelegationRepo::upsert_original_order_result_fact(
            &resource_pool,
            NewApiResourceDelegation::platform_delegate(
                "uid_1",
                &trade_no,
                &trade_no,
                ApiTradeType::Withdraw as i64,
                "",
                "",
                "0",
            ),
            ApiResourceDelegationResultStatus::Success,
            None,
            Some(&result_payload),
        )
        .await?;
        let resource_task =
            ApiResourceDelegationRepo::get_by_resource_trade_no(&resource_pool, &trade_no).await?;
        worker.mark_resource_result_ack_and_project_withdraw_gate(&resource_task).await?;

        let withdraw = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &tx_pool,
            &trade_no,
            ApiTradeType::Withdraw,
        )
        .await?;
        assert!(withdraw.resource_gate_released_at.is_some());
        assert_eq!(
            withdraw.resource_gate_result,
            Some(ApiResourceGateResult::ResourceDelegationSuccess)
        );

        Ok(())
    }
}
