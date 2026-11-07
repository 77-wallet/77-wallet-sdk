use std::sync::Arc;

// collect/shadow/worker/side_effect_worker.rs
//
// SideEffect Worker 负责处理所有外部依赖的副作用操作
// - 发送结果确认 (SendResultAck)
// - 上传服务费记录 (UploadServiceFee)
// - 上传交易执行回执 (UploadTxExecReceipt)
// - 其他未来可能的副作用操作
//
// SideEffect Worker 约束：
// - 只处理外部依赖的操作，不处理链相关操作
// - 所有操作必须有幂等性保护
// - 统一执行流程：事实读取 → 幂等判断 → 执行 + 写结果事实
// - 可随时被 kill-9，不影响系统一致性
//
// Invariants:
// 1. *_uploaded_at.is_some() => *_attempted_at.is_some()
// 2. SideEffectWorker never writes business status
// 3. Failure can never overwrite success
use alloy::primitives::U256;
use rust_decimal::{Decimal, prelude::ToPrimitive as _};
use tracing::{error, info, warn};
use wallet_database::{
    ApiTransactionDbPool, ApiWalletDbPool,
    entities::{
        api_coin::ApiCoinEntity,
        api_resource_delegation::{
            ApiResourceDelegationOperationType, ApiResourceDelegationResultStatus,
            ApiResourceDelegationSource, NewApiResourceDelegation,
        },
        api_resource_gate::{
            ApiResourceBlockReason, ApiResourceDependencyType, ApiResourceGateResult,
        },
        api_trade_type::ApiTradeType,
        asset_token_key::AssetTokenKey,
    },
    repositories::api_wallet::{
        collect::ApiCollectRepo, resource_delegation::ApiResourceDelegationRepo,
    },
};
use wallet_transport_backend::request::api_wallet::transaction::{
    ServiceFeeUploadReq, TransAckType, TransEventAckReq, TransStatus, TransType,
    TxExecReceiptUploadReq,
};
use wallet_types::chain::chain::ChainCode;
use wallet_utils::{conversion, unit};

use crate::{
    domain::{
        api_wallet::{chain::ApiChainTransDomain, coin::ApiCoinDomain, trans::shared_mechanism},
        chain::adapter::sol_tx::SYSTEM_ACCOUNT_RENT,
    },
    error::service::ServiceError,
    infrastructure::api_trans::{
        collect::shadow::ShadowAdvancer,
        resource_ack_type::{
            is_original_order_resource_result_fact, merchant_original_resource_result_ack_type,
            merchant_original_resource_result_trans_type, platform_resource_result_ack_type,
            platform_resource_task_trans_type,
        },
        resource_chain_facts::{
            resource_chain_failed, resource_chain_success, resource_failure_receipt_uploadable,
            resource_success_receipt_uploadable,
        },
        resource_reclaim::local_shadow::trigger_local_undelegation_fast_path,
    },
    request::api_wallet::trans::ApiBaseTransferReq,
};

/// SideEffect Worker Command 结构
/// 只表达："对某个 trade_no 执行某个确定的副作用动作"
#[derive(Debug)]
pub enum SideEffectCommand {
    /// 发送订单确认
    SendOrderAck(String),
    /// 发送结果确认
    SendResultAck(String),
    /// 上传服务费记录
    UploadServiceFee(String),
    /// 上传交易执行回执
    UploadTxExecReceipt(String),
    /// 发送手续费结果确认
    SendTxFeeResAck(String),
    /// 发送资源任务结果 ACK
    SendResourceResultAck(String),
    /// 发送资源任务 ACK
    SendResourceTaskAck(String),
    /// 上传资源任务交易执行回执
    UploadResourceTxExecReceipt(String),
}

/// Projection of a resource-task terminal outcome back into the origin collect
/// resource gate.
///
/// These variants are about "what collect gate result should be written", not
/// about "which side-effect command is currently running".
enum ResourceGateReleaseOutcome {
    Success(ApiResourceGateResult),
    FailureBypass(ApiResourceGateResult),
}

fn collect_local_undelegate_trade_no(origin_trade_no: &str) -> String {
    format!("rsc_local_undelegate_{}", origin_trade_no)
}

impl SideEffectCommand {
    /// 从 SideEffectCommand 生成对应的 RunningKey
    pub fn to_running_key(
        &self,
    ) -> crate::infrastructure::api_trans::collect::shadow::dispatcher::RunningKey {
        match self {
            SideEffectCommand::SendOrderAck(trade_no) => {
                crate::infrastructure::api_trans::collect::shadow::dispatcher::RunningKey::SendOrderAck(
                    trade_no.clone(),
                )
            }
            SideEffectCommand::SendResultAck(trade_no) => {
                crate::infrastructure::api_trans::collect::shadow::dispatcher::RunningKey::SendResultAck(
                    trade_no.clone(),
                )
            }
            SideEffectCommand::UploadServiceFee(trade_no) => {
                crate::infrastructure::api_trans::collect::shadow::dispatcher::RunningKey::UploadServiceFee(
                    trade_no.clone(),
                )
            }
            SideEffectCommand::UploadTxExecReceipt(trade_no) => {
                crate::infrastructure::api_trans::collect::shadow::dispatcher::RunningKey::UploadTxExecReceipt(
                    trade_no.clone(),
                )
            }
            SideEffectCommand::SendTxFeeResAck(trade_no) => {
                crate::infrastructure::api_trans::collect::shadow::dispatcher::RunningKey::SendTxFeeResAck(
                    trade_no.clone(),
                )
            }
            SideEffectCommand::SendResourceResultAck(trade_no)
            | SideEffectCommand::SendResourceTaskAck(trade_no)
            | SideEffectCommand::UploadResourceTxExecReceipt(trade_no) => {
                crate::infrastructure::api_trans::collect::shadow::dispatcher::RunningKey::UploadTxExecReceipt(
                    trade_no.clone(),
                )
            }
        }
    }
}

/// SideEffect Worker
/// 处理所有外部依赖的副作用操作
#[derive(Clone)]
pub struct SideEffectWorker {
    ctx: &'static crate::context::Context,
    /// ShadowAdvancer 引用，用于统一推进执行
    advancer: Arc<ShadowAdvancer>,
    fee_reservations:
        crate::infrastructure::api_trans::resource_reservation::CollectFeeReservationManager,
}

fn matches_unrecoverable_business_error(err: &ServiceError) -> bool {
    use crate::error::business::{
        BusinessError,
        api_wallet::{ApiWalletError, account::AccountError, wallet::WalletError},
    };
    matches!(
        err,
        ServiceError::Business(BusinessError::ApiWallet(api_err))
            if matches!(
                api_err,
                ApiWalletError::Account(AccountError::NotFound)
                    | ApiWalletError::Wallet(WalletError::NotFound)
                    | ApiWalletError::Wallet(WalletError::SubAccountWalletNotBoundWithdrawalWalletAddress)
                    | ApiWalletError::ChainConfigNotFound(_)
            )
    )
}

impl SideEffectWorker {
    /// 创建新的 SideEffect Worker
    pub fn new(ctx: &'static crate::context::Context, advancer: Arc<ShadowAdvancer>) -> Self {
        Self::new_with_fee_reservations(ctx, advancer, Default::default())
    }

    pub(crate) fn new_with_fee_reservations(
        ctx: &'static crate::context::Context,
        advancer: Arc<ShadowAdvancer>,
        fee_reservations: crate::infrastructure::api_trans::resource_reservation::CollectFeeReservationManager,
    ) -> Self {
        Self { ctx, advancer, fee_reservations }
    }
    /// 从数据库中获取归集交易信息
    async fn get_collect_entity(
        &self,
        trade_no: &str,
    ) -> Result<wallet_database::entities::api_collect::ApiCollectEntity, ServiceError> {
        let entity = wallet_database::repositories::api_wallet::collect::ApiCollectRepo::get_api_collect_by_trade_no(&self.ctx.api_transaction_pool()?, trade_no)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
        Ok(entity)
    }

    /// 解析出款地址
    async fn resolve_withdraw_from_addr(
        &self,
        req: &wallet_database::entities::api_collect::ApiCollectEntity,
    ) -> Result<String, ServiceError> {
        info!(trade_no = %req.trade_no, source = "side_effect_worker", "Resolving withdrawal address");

        // 1. 根据from_addr + chain_code查询account
        let account = match wallet_database::repositories::api_wallet::account::ApiAccountRepo::find_one_by_address_chain_code(
            &req.from_addr,
            &req.chain_code,
            &self.ctx.api_wallet_pool()?,
        )
        .await?
        {
            Some(account) => account,
            None => {
                error!(trade_no = %req.trade_no, from_addr = %req.from_addr, chain_code = %req.chain_code, source = "side_effect_worker", "Account not found");
                return Err(ServiceError::Business(
                    crate::error::business::BusinessError::ApiWallet(
                        crate::error::business::api_wallet::ApiWalletError::Account(
                            crate::error::business::api_wallet::account::AccountError::NotFound,
                        ),
                    ),
                ));
            }
        };

        // 2. 根据account.wallet_address查询wallet
        let wallet =
            match wallet_database::repositories::api_wallet::wallet::ApiWalletRepo::find_by_address(
                &self.ctx.api_wallet_pool()?.clone(),
                &account.wallet_address,
            )
            .await?
            {
                Some(wallet) => wallet,
                None => {
                    error!(trade_no = %req.trade_no, wallet_address = %account.wallet_address, source = "side_effect_worker", "Wallet not found");
                    return Err(ServiceError::Business(
                        crate::error::business::BusinessError::ApiWallet(
                            crate::error::business::api_wallet::ApiWalletError::Wallet(
                                crate::error::business::api_wallet::wallet::WalletError::NotFound
                                    .into(),
                            ),
                        ),
                    ));
                }
            };
        let Some(bind_address) = wallet.binding_address else {
            error!(trade_no = %req.trade_no, wallet_address = %account.wallet_address, source = "side_effect_worker", "Wallet not bound to address");
            return Err(ServiceError::Business(
                crate::error::business::BusinessError::ApiWallet(
                    crate::error::business::api_wallet::ApiWalletError::Wallet(
                        crate::error::business::api_wallet::wallet::WalletError::SubAccountWalletNotBoundWithdrawalWalletAddress
                            .into(),
                    ),
                ),
            ));
        };

        let Some(withdraw_wallet) =
            wallet_database::repositories::api_wallet::wallet::ApiWalletRepo::find_by_address(
                &self.ctx.api_wallet_pool()?.clone(),
                &bind_address,
            )
            .await?
        else {
            error!(trade_no = %req.trade_no, bind_address = %bind_address, source = "side_effect_worker", "Withdrawal wallet not found");
            return Err(ServiceError::Business(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::ApiWalletError::Wallet(
                    crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
                ),
            )));
        };

        // 3. 查询用户提币策略
        let ctx = self.ctx;
        let strategy =
            crate::domain::api_wallet::strategy::StrategyDomain::query_withdraw_strategy_with_ctx(
                ctx,
                &withdraw_wallet.uid,
            )
            .await?;
        info!(trade_no = %req.trade_no, source = "side_effect_worker", "Retrieved withdrawal strategy successfully");

        // 4. 根据chain_code查询链配置
        let chain_config = match strategy
            .chain_configs
            .into_iter()
            .find(|config| config.chain_code == req.chain_code)
        {
            Some(config) => config,
            None => {
                error!(trade_no = %req.trade_no, chain_code = %req.chain_code, source = "side_effect_worker", "Chain config not found");
                return Err(ServiceError::Business(
                    crate::error::business::BusinessError::ApiWallet(
                        crate::error::business::api_wallet::ApiWalletError::ChainConfigNotFound(
                            req.chain_code.clone(),
                        ),
                    ),
                ));
            }
        };

        // 5. 根据risk_addr决定normal/risk地址
        // risk_addr: 1 正常地址，2 风险地址
        let exec_to_addr = chain_config.normal_address.address;

        info!(trade_no = %req.trade_no, exec_to_addr = %exec_to_addr, source = "side_effect_worker", "Resolved withdrawal address successfully");
        Ok(exec_to_addr)
    }

    /// 处理单个副作用命令
    pub async fn handle(&self, cmd: SideEffectCommand) -> Result<(), ServiceError> {
        // 提取 trade_no 用于日志
        let trade_no = match &cmd {
            SideEffectCommand::SendOrderAck(trade_no) => trade_no,
            SideEffectCommand::SendResultAck(trade_no) => trade_no,
            SideEffectCommand::UploadServiceFee(trade_no) => trade_no,
            SideEffectCommand::UploadTxExecReceipt(trade_no) => trade_no,
            SideEffectCommand::SendTxFeeResAck(trade_no) => trade_no,
            SideEffectCommand::SendResourceResultAck(trade_no) => trade_no,
            SideEffectCommand::SendResourceTaskAck(trade_no) => trade_no,
            SideEffectCommand::UploadResourceTxExecReceipt(trade_no) => trade_no,
        };

        let trade_no_clone = trade_no.to_string();
        let trade_no_for_async = trade_no_clone.clone();
        let self_clone = self.clone();

        match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            async move {
                info!(trade_no = %trade_no_for_async, command = ?cmd, source = "side_effect_worker", "Received side effect command");

                // 幂等保护：检查原 collect 是否已终态。资源 ACK 的 trade_no
                // 是资源任务号，不对应 api_collect，查询失败时允许继续。
                // finished_at 一旦存在，世界已经结束，后面发生的一切都只是日志
                if let Ok(collect) = self_clone.get_collect_entity(&trade_no_for_async).await {
                    if collect.finished_at.is_some() {
                        info!(trade_no = %trade_no_for_async, command = ?cmd, source = "side_effect_worker", "Collect already finished, skipping side effect");
                        return Ok(());
                    }
                }

                match cmd {
                    SideEffectCommand::SendOrderAck(trade_no) => self_clone.process_order_ack(trade_no).await,
                    SideEffectCommand::SendResultAck(trade_no) => self_clone.process_result_ack(trade_no).await,
                    SideEffectCommand::UploadServiceFee(trade_no) => {
                        self_clone.process_upload_service_fee(trade_no).await
                    }
                    SideEffectCommand::UploadTxExecReceipt(trade_no) => {
                        self_clone.process_tx_exec_receipt(trade_no).await
                    }
                    SideEffectCommand::SendTxFeeResAck(trade_no) => {
                        self_clone.process_tx_fee_res_ack(trade_no).await
                    }
                    SideEffectCommand::SendResourceResultAck(trade_no) => {
                        self_clone.process_resource_result_ack(trade_no).await
                    }
                    SideEffectCommand::SendResourceTaskAck(trade_no) => {
                        self_clone.process_resource_task_ack(trade_no).await
                    }
                    SideEffectCommand::UploadResourceTxExecReceipt(trade_no) => {
                        self_clone.process_resource_tx_exec_receipt(trade_no).await
                    }
                }
            }
        ).await {
            Ok(result) => result,
            Err(_) => {
                error!(trade_no = %trade_no_clone, source = "side_effect_worker", "SideEffectWorker timeout after 30 seconds");
                Err(ServiceError::Timeout)
            }
        }
    }

    /// 处理发送订单确认
    async fn process_order_ack(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "side_effect_worker", "Processing Order ACK command");

        // 获取交易信息
        let req = self.get_collect_entity(&trade_no).await?;

        // 幂等保护：检查是否已发送订单 ACK
        if req.order_ack_sent_at.is_some() {
            info!(trade_no = %trade_no, source = "side_effect_worker", "Order ACK already sent, skipping");
            return Ok(());
        }

        // 标记订单 ACK 尝试
        info!(trade_no = %trade_no, source = "side_effect_worker", "Marking order ACK as attempted");
        wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_order_ack_attempted(
            &self.ctx.api_transaction_pool()?,
            &trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(trade_no = %trade_no, source = "side_effect_worker", "Order ACK marked as attempted successfully");

        // 获取backend_api
        let backend_api = self.ctx.get_global_backend_api();

        // 发送Order ACK
        match backend_api
            .trans_event_ack(
                &wallet_transport_backend::request::api_wallet::transaction::TransEventAckReq::new(
                    &trade_no,
                    wallet_transport_backend::request::api_wallet::transaction::TransType::Col,
                    wallet_transport_backend::request::api_wallet::transaction::TransAckType::Tx,
                ),
            )
            .await
        {
            Ok(_) => {
                info!(trade_no = %trade_no, "Order ACK sent successfully");
                // 成功路径：标记订单 ACK 已发送
                wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_order_ack_sent(
                        &self.ctx.api_transaction_pool()?,
                        &trade_no,
                    ).await
                    .map_err(|e| {
                        error!(trade_no = %trade_no, error = %e, "Failed to mark order ACK sent");
                        ServiceError::Database(e.into())
                    })?;

                if shared_mechanism::is_tron_chain(&req.chain_code) {
                    self.advancer.trigger_resource_gate_fast_path_after_order_ack(&trade_no);
                } else {
                    self.advancer.trigger_build_fast_path_after_order_ack(&trade_no);
                }
            }
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, "Failed to send Order ACK");
                // 失败路径：只保留 attempted 状态，让 Scanner 重试
                return Err(e.into());
            }
        }

        Ok(())
    }

    /// 处理发送结果确认
    async fn process_result_ack(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "side_effect_worker", "Processing Result ACK command");

        // 获取交易信息
        let req = self.get_collect_entity(&trade_no).await?;

        // 幂等保护：检查是否已发送结果确认
        if req.result_ack_sent_at.is_some() {
            if req.finished_at.is_none() && req.transaction_time.is_some() {
                // 兼容历史半完成事实：result_ack 已写但 finished 未写（例如 kill -9）
                info!(
                    trade_no = %trade_no,
                    source = "side_effect_worker",
                    "Result ACK already sent but collect not finished; repairing finished_at"
                );
                wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_chain_finished(
                        &self.ctx.api_transaction_pool()?,
                        &trade_no,
                    )
                    .await
                    .map_err(|e| ServiceError::Database(e.into()))?;
                self.ensure_local_undelegation_after_collect_finished(&trade_no).await?;
                self.advancer.try_advance(&trade_no).await;
            } else if req.transaction_time.is_none() {
                warn!(
                    trade_no = %trade_no,
                    source = "side_effect_worker",
                    "Result ACK already sent but transaction_time is NULL; skip repairing finished_at"
                );
            }

            info!(
                trade_no = %trade_no,
                source = "side_effect_worker",
                "Result ACK already sent, skipping"
            );
            if req.finished_at.is_some() {
                self.ensure_local_undelegation_after_collect_finished(&trade_no).await?;
            }
            return Ok(());
        }

        // ✅ 强顺序屏障：TX_RES ACK 只能在已收到并持久化 AWM_ORDER_TRANS_RES 后发送
        if req.tx_res_received_at.is_none() {
            warn!(
                trade_no = %trade_no,
                tx_hash_present = %req.tx_hash.is_some(),
                tx_hash_empty = %req.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(false),
                tx_res_received_at = %req.tx_res_received_at.is_some(),
                transaction_time_present = %req.transaction_time.is_some(),
                source = "side_effect_worker",
                "Result ACK skipped: tx_res not received"
            );
            return Ok(());
        }

        if req.transaction_time.is_none() {
            warn!(
                trade_no = %trade_no,
                tx_hash_present = %req.tx_hash.is_some(),
                tx_hash_empty = %req.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(false),
                tx_res_received_at = %req.tx_res_received_at.is_some(),
                transaction_time_present = %req.transaction_time.is_some(),
                source = "side_effect_worker",
                "Transaction time is NULL; cannot send result ACK"
            );
            return Ok(());
        }

        // 标记结果确认尝试
        info!(trade_no = %trade_no, source = "side_effect_worker", "Marking result ACK as attempted");
        wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_result_ack_attempted(
            &self.ctx.api_transaction_pool()?,
            &trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(trade_no = %trade_no, source = "side_effect_worker", "Result ACK marked as attempted successfully");

        // 获取backend_api
        let backend_api = self.ctx.get_global_backend_api();

        // 发送TxRes ACK
        match backend_api
            .trans_event_ack(
                &wallet_transport_backend::request::api_wallet::transaction::TransEventAckReq::new(
                    &trade_no,
                    wallet_transport_backend::request::api_wallet::transaction::TransType::Col,
                    wallet_transport_backend::request::api_wallet::transaction::TransAckType::TxRes,
                ),
            )
            .await
        {
            Ok(_) => {
                info!(trade_no = %trade_no, "TxRes ACK sent successfully");
                wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_result_ack_confirmed(
                        &self.ctx.api_transaction_pool()?,
                        &trade_no,
                    ).await
                    .map_err(|e| {
                        error!(trade_no = %trade_no, error = %e, "Failed to mark result ACK confirmed");
                        ServiceError::Database(e.into())
                    })?;
                self.ensure_local_undelegation_after_collect_finished(&trade_no).await?;
                wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_chain_finished(
                        &self.ctx.api_transaction_pool()?,
                        &trade_no,
                    ).await
                    .map_err(|e| {
                        error!(trade_no = %trade_no, error = %e, "Failed to mark collect finished after result ACK");
                        ServiceError::Database(e.into())
                    })?;

                // 直接调用 try_advance 进行点对点唤醒
                self.advancer.try_advance(&trade_no).await;
            }
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, "Failed to send TxRes ACK");
                // 失败路径：只保留 attempted 状态，让 Scanner 重试
                return Err(e.into());
            }
        }

        Ok(())
    }

    /// 处理发送手续费结果确认 ACK
    async fn process_tx_fee_res_ack(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "side_effect_worker", "Processing Tx Fee Res ACK command");

        // 获取交易信息
        let req = self.get_collect_entity(&trade_no).await?;

        // 幂等保护：检查是否已发送手续费结果确认 ACK
        if req.tx_fee_res_ack_sent_at.is_some() {
            info!(trade_no = %trade_no, source = "side_effect_worker", "Tx Fee Res ACK already sent, skipping");
            return Ok(());
        }

        // 获取backend_api
        let backend_api = self.ctx.get_global_backend_api();

        // 发送手续费结果确认 ACK
        match backend_api
            .trans_event_ack(
                &wallet_transport_backend::request::api_wallet::transaction::TransEventAckReq::new(
                    &trade_no,
                    wallet_transport_backend::request::api_wallet::transaction::TransType::Col,
                    wallet_transport_backend::request::api_wallet::transaction::TransAckType::TxFeeRes,
                ),
            )
            .await
        {
            Ok(_) => {
                info!(trade_no = %trade_no, "Tx Fee Res ACK sent successfully");
                // 成功路径：标记手续费结果确认 ACK 已发送
                let affected = wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_tx_fee_res_ack_sent(
                        &self.ctx.api_transaction_pool()?,
                        &trade_no,
                    ).await
                    .map_err(|e| {
                        error!(trade_no = %trade_no, error = %e, "Failed to mark Tx Fee Res ACK sent");
                        ServiceError::Database(e.into())
                    })?;
                if affected == 0 {
                    warn!(trade_no = %trade_no, "Tx Fee Res ACK marked 0 rows (trade_no missing or already marked)");
                }

                self.fee_reservations.release(&trade_no);

                let fresh = self.get_collect_entity(&trade_no).await?;
                self.advancer.trigger_fast_path_after_tx_fee_res_ack(
                    &trade_no,
                    fresh.raw_tx.is_some(),
                );
            }
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, "Failed to send Tx Fee Res ACK");
                // 失败路径：让 Scanner 重试
                return Err(e.into());
            }
        }

        Ok(())
    }

    async fn process_resource_result_ack(
        &self,
        resource_trade_no: String,
    ) -> Result<(), ServiceError> {
        info!(resource_trade_no = %resource_trade_no, source = "side_effect_worker", "Processing resource result ACK command");

        let resource_task = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if resource_task.result_ack_sent_at.is_some() {
            info!(resource_trade_no = %resource_trade_no, source = "side_effect_worker", "Resource result ACK already sent, skipping");
            return Ok(());
        }

        if resource_task.finished_at.is_none() {
            warn!(resource_trade_no = %resource_trade_no, source = "side_effect_worker", "Resource result ACK skipped because result has not been received");
            return Ok(());
        }
        if resource_task.result_payload.is_none() {
            warn!(
                resource_trade_no = %resource_trade_no,
                source = "side_effect_worker",
                "Resource result ACK skipped because backend result payload is missing"
            );
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
        let backend_api = self.ctx.get_global_backend_api();
        match backend_api
            .trans_event_ack(&TransEventAckReq::new(&resource_trade_no, trans_type, ack_type))
            .await
        {
            Ok(_) => {
                self.mark_resource_result_ack_and_project_collect_gate(&resource_task).await?;
                info!(resource_trade_no = %resource_trade_no, "Resource result ACK sent successfully");
            }
            Err(e) => {
                error!(resource_trade_no = %resource_trade_no, error = %e, "Failed to send resource result ACK");
                return Err(e.into());
            }
        }

        Ok(())
    }

    async fn mark_resource_result_ack_and_project_collect_gate(
        &self,
        resource_task: &wallet_database::entities::api_resource_delegation::ApiResourceDelegationEntity,
    ) -> Result<(), ServiceError> {
        let release = Self::collect_gate_release_from_resource_result(resource_task);
        let ack_rows = ApiResourceDelegationRepo::mark_result_ack_sent(
            &self.ctx.api_resource_pool()?,
            &resource_task.resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if ack_rows > 0 {
            if let Some((origin_trade_no, release_result)) = release.as_ref() {
                let release_rows = ApiCollectRepo::mark_resource_released(
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
                    "Collect resource gate projected from resource result ACK"
                );
            }
        }

        if ack_rows == 0 {
            warn!(
                resource_trade_no = %resource_task.resource_trade_no,
                "Resource result ACK marked 0 rows"
            );
        } else if let Some((origin_trade_no, _)) = release {
            self.advancer.trigger_build_fast_path_after_resource_gate_release(&origin_trade_no);
        }

        Ok(())
    }

    fn collect_gate_release_from_resource_result(
        resource_task: &wallet_database::entities::api_resource_delegation::ApiResourceDelegationEntity,
    ) -> Option<(String, ApiResourceGateResult)> {
        if resource_task.origin_trade_type != Some(ApiTradeType::Collect as i64) {
            return None;
        }

        let origin_trade_no = resource_task.origin_trade_no.clone()?;
        let release_result = if is_original_order_resource_result_fact(resource_task) {
            match resource_task.result_status {
                Some(ApiResourceDelegationResultStatus::Success) => {
                    ApiResourceGateResult::PlatformDelegateSuccess
                }
                None => return None,
                Some(ApiResourceDelegationResultStatus::Fail) => return None,
            }
        } else if resource_chain_success(resource_task) && !resource_chain_failed(resource_task) {
            ApiResourceGateResult::PlatformDelegateSuccess
        } else {
            return None;
        };

        Some((origin_trade_no, release_result))
    }

    /// Project a resource delegation terminal outcome back into the origin
    /// collect gate.
    ///
    /// Important boundary:
    /// - result ACK releases success and failure facts that already arrived
    ///   through backend result notification
    /// - receipt upload remains a stable failure fallback when no backend
    ///   result notification is available
    ///
    /// So `UploadResourceTxExecReceipt` is only a fallback closure hook;
    /// uploading a success receipt does not mean "failed_bypass".
    async fn project_resource_task_outcome_to_collect_gate(
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

        if resource_task.origin_trade_type != Some(ApiTradeType::Collect as i64) {
            info!(
                resource_trade_no = %resource_task.resource_trade_no,
                origin_trade_type = ?resource_task.origin_trade_type,
                source = "side_effect_worker",
                "Skip collect resource gate release: origin is not collect"
            );
            return Ok(());
        }

        let Some(origin_trade_no) = resource_task.origin_trade_no.as_deref() else {
            info!(
                resource_trade_no = %resource_task.resource_trade_no,
                source = "side_effect_worker",
                "Skip collect resource gate release: origin trade no missing"
            );
            return Ok(());
        };

        let collect = ApiCollectRepo::get_api_collect_by_trade_no(
            &self.ctx.api_transaction_pool()?,
            origin_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if collect.resource_gate_released_at.is_some() {
            info!(
                resource_trade_no = %resource_task.resource_trade_no,
                origin_trade_no = %origin_trade_no,
                source = "side_effect_worker",
                "Collect resource gate already released after resource success"
            );
            self.advancer.trigger_build_fast_path_after_resource_gate_release(origin_trade_no);
            return Ok(());
        }

        let affected = ApiCollectRepo::mark_resource_released(
            &self.ctx.api_transaction_pool()?,
            origin_trade_no,
            release_result,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if affected == 0 {
            warn!(
                resource_trade_no = %resource_task.resource_trade_no,
                origin_trade_no = %origin_trade_no,
                source = "side_effect_worker",
                "Collect resource gate release marked 0 rows after resource success"
            );
        } else {
            info!(
                resource_trade_no = %resource_task.resource_trade_no,
                origin_trade_no = %origin_trade_no,
                source = "side_effect_worker",
                "Collect resource gate released after resource success"
            );
        }

        self.advancer.trigger_build_fast_path_after_resource_gate_release(origin_trade_no);
        Ok(())
    }

    async fn ensure_local_undelegation_after_collect_finished(
        &self,
        origin_trade_no: &str,
    ) -> Result<(), ServiceError> {
        let delegations = ApiResourceDelegationRepo::list_by_origin_trade_no(
            &self.ctx.api_resource_pool()?,
            origin_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if delegations.iter().any(|item| {
            item.source == ApiResourceDelegationSource::Local
                && item.operation_type == ApiResourceDelegationOperationType::Undelegate
        }) {
            return Ok(());
        }

        let Some(local_delegate) = delegations.iter().find(|item| {
            item.source == ApiResourceDelegationSource::Local
                && item.operation_type == ApiResourceDelegationOperationType::Delegate
                && item.result_status == Some(ApiResourceDelegationResultStatus::Success)
        }) else {
            return Ok(());
        };

        let resource_trade_no = collect_local_undelegate_trade_no(origin_trade_no);
        ApiResourceDelegationRepo::upsert(
            &self.ctx.api_resource_pool()?,
            NewApiResourceDelegation::local_undelegate(
                local_delegate.uid.clone(),
                resource_trade_no.clone(),
                origin_trade_no.to_string(),
                ApiTradeType::Collect as i64,
                local_delegate.owner_address.clone(),
                local_delegate.receiver_address.clone(),
                local_delegate.native_amount.clone(),
                local_delegate.amount.clone(),
            ),
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        trigger_local_undelegation_fast_path(&resource_trade_no);

        Ok(())
    }

    async fn process_resource_task_ack(
        &self,
        resource_trade_no: String,
    ) -> Result<(), ServiceError> {
        info!(resource_trade_no = %resource_trade_no, source = "side_effect_worker", "Processing resource task ACK command");

        let resource_task = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if resource_task.task_ack_sent_at.is_some() {
            info!(resource_trade_no = %resource_trade_no, source = "side_effect_worker", "Resource task ACK already sent, skipping");
            return Ok(());
        }

        let trans_type = platform_resource_task_trans_type(&resource_task);
        let attempted = ApiResourceDelegationRepo::mark_task_ack_attempted(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if attempted == 0 {
            return Ok(());
        }

        let backend_api = self.ctx.get_global_backend_api();
        match backend_api
            .trans_event_ack(&TransEventAckReq::new(
                &resource_trade_no,
                trans_type,
                TransAckType::Tx,
            ))
            .await
        {
            Ok(_) => {
                let affected = ApiResourceDelegationRepo::mark_task_ack_sent(
                    &self.ctx.api_resource_pool()?,
                    &resource_trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                if affected == 0 {
                    warn!(resource_trade_no = %resource_trade_no, "Resource task ACK marked 0 rows");
                }
                info!(resource_trade_no = %resource_trade_no, "Resource task ACK sent successfully");
            }
            Err(e) => {
                error!(resource_trade_no = %resource_trade_no, error = %e, "Failed to send resource task ACK");
                return Err(e.into());
            }
        }

        Ok(())
    }

    async fn process_resource_tx_exec_receipt(
        &self,
        resource_trade_no: String,
    ) -> Result<(), ServiceError> {
        info!(resource_trade_no = %resource_trade_no, source = "side_effect_worker", "Processing resource tx exec receipt command");

        let resource_task = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if resource_task.tx_exec_receipt_uploaded_at.is_some() {
            info!(resource_trade_no = %resource_trade_no, source = "side_effect_worker", "Resource tx exec receipt already uploaded, skipping");
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

        let backend_api = self.ctx.get_global_backend_api();
        backend_api.upload_tx_exec_receipt(&payload).await?;

        let affected = ApiResourceDelegationRepo::mark_tx_exec_receipt_uploaded(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if affected == 0 {
            warn!(resource_trade_no = %resource_trade_no, "Resource tx exec receipt marked 0 rows");
        } else {
            info!(resource_trade_no = %resource_trade_no, "Resource tx exec receipt uploaded and marked");
        }

        if resource_task.source == ApiResourceDelegationSource::Platform {
            self.revisit_collect_resource_gate_after_platform_failure(&resource_task).await?;
        }

        Ok(())
    }

    async fn revisit_collect_resource_gate_after_platform_failure(
        &self,
        resource_task: &wallet_database::entities::api_resource_delegation::ApiResourceDelegationEntity,
    ) -> Result<(), ServiceError> {
        let is_failure = resource_chain_failed(resource_task);
        if !is_failure || resource_task.origin_trade_type != Some(ApiTradeType::Collect as i64) {
            return Ok(());
        }

        let Some(origin_trade_no) = resource_task.origin_trade_no.as_deref() else {
            return Ok(());
        };

        let rows = ApiCollectRepo::mark_resource_blocked(
            &self.ctx.api_transaction_pool()?,
            origin_trade_no,
            ApiResourceBlockReason::NeedLocalDelegate,
            None,
            Some(ApiResourceDependencyType::LocalDelegate),
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(
            resource_trade_no = %resource_task.resource_trade_no,
            origin_trade_no = %origin_trade_no,
            rows = %rows,
            source = "side_effect_worker",
            "Switch collect resource gate to local delegation after platform failure receipt"
        );
        self.advancer.try_advance(origin_trade_no).await;
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

    /// 处理上传服务费记录
    async fn process_upload_service_fee(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "side_effect_worker", "Processing UploadServiceFee command");

        // 获取交易信息
        let req = self.get_collect_entity(&trade_no).await?;
        let ctx = self.ctx;

        // 幂等保护：检查服务费是否已上传
        // invariant: uploaded_at.is_some() => attempted_at.is_some()
        if req.service_fee_uploaded_at.is_some() {
            info!(trade_no = %trade_no, source = "side_effect_worker", "Service fee already uploaded, skipping");
            return Ok(());
        }

        // 解析出款地址
        let exec_from_addr = match self.resolve_withdraw_from_addr(&req).await {
            Ok(addr) => addr,
            Err(e) => {
                if matches_unrecoverable_business_error(&e) {
                    error!(
                        trade_no = %trade_no,
                        error = %e,
                        source = "side_effect_worker",
                        "resolve_withdraw_from_addr failed with unrecoverable error, marking collect as failed"
                    );
                    let pool = self.ctx.api_transaction_pool()?;
                    let err_msg = format!("ServiceFeeUpload failed: {}", e);
                    #[allow(deprecated)]
                    let cleared = ApiCollectRepo::update_api_collect_status_and_err(
                        &pool,
                        &trade_no,
                        wallet_database::entities::api_collect::ApiCollectStatus::Failure,
                        wallet_database::entities::api_collect::ErrCode::TransactionOnChainException,
                        &err_msg,
                    )
                    .await?;
                    if cleared == 0 {
                        warn!(trade_no = %trade_no, source = "side_effect_worker", "failed to update err_code for collect");
                    }
                    ApiCollectRepo::clear_need_service_fee(&pool, &trade_no).await?;
                }
                return Err(e);
            }
        };
        info!(trade_no = %trade_no, exec_from_addr = %exec_from_addr, source = "side_effect_worker", "Resolved withdrawal address");

        // 查询主币信息
        let main_coin = ApiChainTransDomain::main_coin(ctx, &req.chain_code).await?;
        info!(trade_no = %trade_no, source = "side_effect_worker", "Retrieved main coin information");

        // 解析链代码
        let chain_code: ChainCode = req.chain_code.as_str().try_into()?;
        let (fee_symbol, fee_token_key, fee_decimals) = self
            .resolve_fee_estimation_coin_info(&req.chain_code, &req.token_addr, &main_coin)
            .await?;

        let mut estimated_fee_str: Option<String> = None;
        let mut reestimated_due_to_non_positive_stored_fee = false;

        // 使用 transaction_fee 事实；若现有事实非正数，则视为不可靠并强制重估
        let fee = if req.transaction_fee.is_empty()
            || crate::domain::chain::is_evm_chain(chain_code)
        {
            let fee_str = self
                .estimate_fee(
                    &req.from_addr,
                    &req.to_addr,
                    &req.value,
                    chain_code,
                    &fee_symbol,
                    &main_coin.symbol,
                    fee_token_key.clone(),
                    fee_decimals,
                )
                .await?;
            estimated_fee_str = Some(fee_str.clone());
            let fee = conversion::decimal_from_str(&fee_str)?;
            info!(trade_no = %trade_no, fee = %fee_str, source = "side_effect_worker", "Estimated fresh fee for service fee upload");
            fee
        } else {
            let stored_fee = conversion::decimal_from_str(&req.transaction_fee)?;
            if stored_fee <= rust_decimal::Decimal::ZERO {
                reestimated_due_to_non_positive_stored_fee = true;
                warn!(
                    trade_no = %trade_no,
                    transaction_fee = %req.transaction_fee,
                    reason_code = "non_positive_transaction_fee_fact",
                    source = "side_effect_worker",
                    "Non-positive transaction_fee fact detected, re-estimating before service fee upload"
                );
                let fee_str = self
                    .estimate_fee(
                        &req.from_addr,
                        &req.to_addr,
                        &req.value,
                        chain_code,
                        &fee_symbol,
                        &main_coin.symbol,
                        fee_token_key.clone(),
                        fee_decimals,
                    )
                    .await?;
                estimated_fee_str = Some(fee_str.clone());
                let fee = conversion::decimal_from_str(&fee_str)?;
                info!(
                    trade_no = %trade_no,
                    fee = %fee_str,
                    source = "side_effect_worker",
                    "Re-estimated fee successfully after non-positive transaction_fee fact"
                );
                fee
            } else {
                info!(trade_no = %trade_no, fee = %req.transaction_fee, source = "side_effect_worker", "Using existing transaction_fee fact");
                stored_fee
            }
        };

        // 计算需要补充的手续费
        // Solana token collect 需要同时覆盖 sender rent reserve。
        let sender_rent_reserve =
            Self::sol_token_collect_sender_rent_reserve(&req.chain_code, &req.token_addr)?;
        let mut fee_to_upload = if crate::domain::chain::is_evm_chain(chain_code) {
            conversion::decimal_from_str(&super::evm_funding_budget(&fee.to_string())?)?
        } else {
            fee
        };
        if sender_rent_reserve > Decimal::ZERO {
            let current_balance = self
                .query_balance(
                    &req.from_addr,
                    &req.chain_code,
                    AssetTokenKey::Native,
                    main_coin.decimals,
                )
                .await?;
            let total_need = fee + sender_rent_reserve;
            fee_to_upload =
                Self::sol_token_collect_service_fee_upload_shortfall(total_need, current_balance);
            info!(
                trade_no = %trade_no,
                sender_rent_reserve = %sender_rent_reserve,
                current_balance = %current_balance,
                total_need = %total_need,
                fee_to_upload = %fee_to_upload,
                source = "side_effect_worker",
                "Solana token collect service fee upload uses the missing shortfall against current balance"
            );
        }
        let fee_to_upload = fee_to_upload.to_f64().unwrap_or(0.0);

        info!(
            trade_no = %trade_no,
            source = "side_effect_worker",
            transaction_fee = %req.transaction_fee,
            estimated_fee = ?estimated_fee_str,
            fee_to_upload,
            reestimated_due_to_non_positive_stored_fee,
            "Computed service fee upload amount"
        );

        if fee_to_upload <= 0.0 {
            warn!(
                trade_no = %trade_no,
                transaction_fee = %req.transaction_fee,
                estimated_fee = ?estimated_fee_str,
                fee_to_upload,
                reestimated_due_to_non_positive_stored_fee,
                reason_code = "non_positive_service_fee",
                source = "side_effect_worker",
                "Computed non-positive service fee; skipping upload and clearing need_service_fee for local self-heal"
            );

            let cleared_rows = wallet_database::repositories::api_wallet::collect::ApiCollectRepo::clear_need_service_fee(
                &self.ctx.api_transaction_pool()?,
                &trade_no,
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;

            if cleared_rows == 0 {
                warn!(
                    trade_no = %trade_no,
                    rows_affected = %cleared_rows,
                    source = "side_effect_worker",
                    "clear_need_service_fee affected 0 rows during non-positive service fee self-heal"
                );
            } else {
                info!(
                    trade_no = %trade_no,
                    rows_affected = %cleared_rows,
                    source = "side_effect_worker",
                    "Cleared need_service_fee after non-positive service fee self-heal"
                );
            }

            self.advancer.try_advance(&trade_no).await;
            return Ok(());
        }

        // 标记服务费上传尝试
        info!(trade_no = %trade_no, source = "side_effect_worker", "Marking service fee as attempted");
        wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_service_fee_attempted(
            &self.ctx.api_transaction_pool()?,
            &trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(trade_no = %trade_no, source = "side_effect_worker", "Service fee marked as attempted successfully");

        // 获取backend_api
        let backend_api = self.ctx.get_global_backend_api();

        // 构建服务费上传请求
        let upload_req = ServiceFeeUploadReq::new(
            &trade_no,
            &req.chain_code,
            &main_coin.symbol,
            "",
            &exec_from_addr,
            &req.from_addr,
            fee_to_upload,
        );

        // 上传服务费记录
        info!(trade_no = %trade_no, source = "side_effect_worker", "Uploading service fee record");
        backend_api.upload_service_fee_record(&upload_req).await?;
        info!(trade_no = %trade_no, source = "side_effect_worker", "Service fee record uploaded successfully");

        // 标记服务费已上传
        info!(trade_no = %trade_no, source = "side_effect_worker", "Marking service fee as uploaded");
        wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_service_fee_uploaded(
            &self.ctx.api_transaction_pool()?,
            &trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(trade_no = %trade_no, source = "side_effect_worker", "Service fee marked as uploaded successfully");

        // Reporting the service-fee request does not mean the fee transfer has
        // arrived. Stay asleep until the backend fee order is received and its
        // TX_FEE_RES ACK fact wakes the collect flow.
        self.advancer.try_advance(&trade_no).await;

        Ok(())
    }

    /// 处理上传交易执行回执
    async fn process_tx_exec_receipt(&self, trade_no: String) -> Result<(), ServiceError> {
        info!(trade_no = %trade_no, source = "side_effect_worker", "Processing UploadTxExecReceipt command");

        // 获取交易信息
        let req = self.get_collect_entity(&trade_no).await?;

        if !Self::has_tx_exec_receipt_fact(&req) {
            info!(
                trade_no = %trade_no,
                source = "side_effect_worker",
                last_broadcast_at_present = %req.last_broadcast_at.is_some(),
                transaction_time_present = %req.transaction_time.is_some(),
                err_code_present = %req.err_code.is_some(),
                "TxExecReceipt still pending, skip upload"
            );
            return Ok(());
        }

        // 幂等保护：检查是否已上传执行回执
        if req.tx_exec_receipt_uploaded_at.is_some() {
            info!(trade_no = %trade_no, source = "side_effect_worker", "TxExecReceipt already uploaded, skipping");
            return Ok(());
        }

        // 标记执行回执上传尝试
        info!(trade_no = %trade_no, source = "side_effect_worker", "Marking TxExecReceipt as attempted");
        wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_tx_exec_receipt_attempted(
            &self.ctx.api_transaction_pool()?,
            &trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(trade_no = %trade_no, source = "side_effect_worker", "TxExecReceipt marked as attempted successfully");

        // 获取backend_api
        let backend_api = self.ctx.get_global_backend_api();

        // 构建交易执行回执上传请求
        let upload_payload = Self::build_tx_exec_receipt_payload(&req, &trade_no).await?;
        info!(
            trade_no = %trade_no,
            tx_hash = %req.tx_hash.as_deref().unwrap_or_default(),
            report_to_addr = %req.to_addr,
            upload_payload = ?upload_payload,
            source = "side_effect_worker",
            "Built TxExecReceipt upload payload"
        );

        let tx_hash_missing =
            req.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
        if upload_payload.is_success() && tx_hash_missing {
            error!(
                trade_no = %trade_no,
                source = "side_effect_worker",
                block_reason = "blocked_by_missing_tx_hash",
                last_broadcast_at_present = %req.last_broadcast_at.is_some(),
                transaction_time_present = %req.transaction_time.is_some(),
                tx_hash_is_none = %req.tx_hash.is_none(),
                tx_hash_is_empty = %req.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(false),
                need_service_fee = ?req.need_service_fee,
                "Skip UploadTxExecReceipt: blocked_by_missing_tx_hash (success payload requires non-empty tx_hash)"
            );
            return Err(ServiceError::Parameter(
                "success tx_exec_receipt requires non-empty tx_hash".to_string(),
            ));
        }

        // 上传交易执行回执
        match backend_api.upload_tx_exec_receipt(&upload_payload).await {
            Ok(_) => {
                info!(trade_no = %trade_no, "TxExecReceipt uploaded successfully");
                // 成功路径：标记执行回执已上传
                wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_tx_exec_receipt_uploaded(
                        &self.ctx.api_transaction_pool()?,
                        &trade_no,
                    ).await
                    .map_err(|e| {
                        error!(trade_no = %trade_no, error = %e, "Failed to mark TxExecReceipt uploaded");
                        ServiceError::Database(e.into())
                    })?;

                // 标记交易终态：所有必要的副作用已完成
                // 仅在“无成功证据且存在失败证据”时收口，避免链上已成功时误收口失败终态。
                if upload_payload.is_fail()
                    && req.transaction_time.is_none()
                    && req.err_code.is_some()
                {
                    info!(trade_no = %trade_no, source = "side_effect_worker", "Marking collect as finished");
                    wallet_database::repositories::api_wallet::collect::ApiCollectRepo::mark_chain_finished(
                        &self.ctx.api_transaction_pool()?,
                        &trade_no
                    ).await
                    .map_err(|e| {
                        error!(trade_no = %trade_no, error = %e, "Failed to mark collect as finished");
                        ServiceError::Database(e.into())
                    })?;
                    info!(trade_no = %trade_no, source = "side_effect_worker", "Collect marked as finished successfully");
                }
                // 直接调用 try_advance 进行点对点唤醒
                self.advancer.try_advance(&trade_no).await;
            }
            Err(e) => {
                error!(trade_no = %trade_no, error = %e, "Failed to upload TxExecReceipt");
                // 失败路径：只保留 attempted 状态，让 Scanner 重试
                return Err(e.into());
            }
        }

        Ok(())
    }

    fn has_tx_exec_receipt_fact(
        req: &wallet_database::entities::api_collect::ApiCollectEntity,
    ) -> bool {
        req.transaction_time.is_some() || req.err_code.is_some()
    }

    fn select_fee_estimation_coin_info(
        token_addr: &AssetTokenKey,
        main_coin: &ApiCoinEntity,
        token_coin: Option<&ApiCoinEntity>,
    ) -> Result<(String, AssetTokenKey, u8), ServiceError> {
        if token_addr.is_contract() {
            let coin = token_coin.ok_or_else(|| {
                ServiceError::Parameter(format!(
                    "token coin not found for service fee estimation: token_addr={}",
                    token_addr.as_db_str()
                ))
            })?;
            Ok((coin.symbol.clone(), coin.token_address.clone(), coin.decimals))
        } else {
            Ok((main_coin.symbol.clone(), AssetTokenKey::Native, main_coin.decimals))
        }
    }

    fn sol_token_collect_sender_rent_reserve(
        chain_code: &str,
        token_addr: &AssetTokenKey,
    ) -> Result<Decimal, ServiceError> {
        if shared_mechanism::is_sol_chain(chain_code) && token_addr.is_contract() {
            return Ok(conversion::decimal_from_str(&SYSTEM_ACCOUNT_RENT.to_string())?);
        }

        Ok(Decimal::ZERO)
    }

    fn sol_token_collect_service_fee_upload_shortfall(
        total_need: Decimal,
        current_balance: Decimal,
    ) -> Decimal {
        if total_need > current_balance { total_need - current_balance } else { Decimal::ZERO }
    }

    async fn query_balance(
        &self,
        owner_address: &str,
        chain_code: &str,
        token_key: AssetTokenKey,
        decimals: u8,
    ) -> Result<Decimal, ServiceError> {
        info!(
            owner_address = %owner_address,
            chain_code = %chain_code,
            token_address = %token_key.as_db_str(),
            source = "side_effect_worker",
            "Querying balance for service fee shortfall calculation"
        );

        let ctx = self.ctx;
        let adapter =
            crate::domain::api_wallet::adapter_factory::ApiChainAdapterFactory::get_transaction_adapter_with_ctx(
                ctx,
                chain_code,
            )
            .await?;
        let balance = adapter.balance_token_key(owner_address, token_key.clone()).await?;
        let amount = unit::format_to_string(balance, decimals)?;
        let amount = conversion::decimal_from_str(&amount)?;

        info!(
            owner_address = %owner_address,
            chain_code = %chain_code,
            token_address = %token_key.as_db_str(),
            balance = %amount,
            source = "side_effect_worker",
            "Querying balance completed"
        );

        Ok(amount)
    }

    async fn resolve_fee_estimation_coin_info(
        &self,
        chain_code: &str,
        token_addr: &AssetTokenKey,
        main_coin: &ApiCoinEntity,
    ) -> Result<(String, AssetTokenKey, u8), ServiceError> {
        if token_addr.is_contract() {
            let ctx = self.ctx;
            let token_coin = ApiCoinDomain::get_coin_by_token_key_exact_with_ctx(
                ctx,
                chain_code,
                token_addr.clone(),
            )
            .await?;
            Self::select_fee_estimation_coin_info(token_addr, main_coin, Some(&token_coin))
        } else {
            Self::select_fee_estimation_coin_info(token_addr, main_coin, None)
        }
    }

    /// 估算手续费
    pub(super) async fn estimate_fee(
        &self,
        from: &str,
        to: &str,
        value: &str,
        chain_code: ChainCode,
        symbol: &str,
        main_symbol: &str,
        token_key: AssetTokenKey,
        decimals: u8,
    ) -> Result<String, ServiceError> {
        info!(from=%from, to=%to, value=%value, chain_code=%chain_code.to_string(), symbol=%symbol, main_symbol=%main_symbol, token_address=%token_key.as_db_str(), source = "side_effect_worker", "Estimating transaction fee");

        let ctx = self.ctx;
        let adapter =
            crate::domain::api_wallet::adapter_factory::ApiChainAdapterFactory::get_transaction_adapter_with_ctx(
                ctx,
                &chain_code.to_string(),
            )
            .await?;
        info!(chain_code=%chain_code.to_string(), source = "side_effect_worker", "Retrieved transaction adapter");

        let mut params = ApiBaseTransferReq::new(from, to, value, &chain_code.to_string());
        params.with_token(token_key.to_chain_token_option(), decimals, symbol);
        info!(chain_code=%chain_code.to_string(), source = "side_effect_worker", "Built transfer parameters");

        if crate::domain::chain::is_evm_chain(chain_code) {
            return super::estimate_evm_collect_fee(
                adapter.as_ref(),
                chain_code,
                params,
                main_symbol,
            )
            .await;
        }

        let fee = adapter.estimate_fee_without_balance_check(params.clone(), main_symbol).await?;
        info!(chain_code=%chain_code.to_string(), source = "side_effect_worker", "Received fee estimate from adapter without local balance gate");

        let recipient_ata_rent =
            if matches!(&chain_code, ChainCode::Solana) && token_key.is_contract() {
                let ata_rent = adapter.recipient_ata_rent(&params).await?;
                if ata_rent > 0 {
                    let ata_rent = conversion::decimal_from_str(&unit::format_to_string(
                        U256::from(ata_rent),
                        wallet_chain_interact::sol::consts::SOL_DECIMAL,
                    )?)?;
                    info!(
                        recipient_ata_rent = %ata_rent,
                        source = "side_effect_worker",
                        "Resolved Solana recipient ATA rent for service fee upload"
                    );
                    ata_rent
                } else {
                    Decimal::ZERO
                }
            } else {
                Decimal::ZERO
            };

        // 解析手续费结果
        let amount = match chain_code {
            ChainCode::Tron => {
                let res: crate::response_vo::TronFeeDetails =
                    wallet_utils::serde_func::serde_from_str(&fee)?;
                res.estimate_fee.amount.to_string()
            }
            ChainCode::Bitcoin => todo!(),
            ChainCode::Solana => {
                let res: crate::response_vo::CommonFeeDetails =
                    wallet_utils::serde_func::serde_from_str(&fee)?;
                let mut amount = res.estimate_fee.amount;
                if recipient_ata_rent > Decimal::ZERO {
                    amount += recipient_ata_rent;
                }
                amount.to_string()
            }
            ChainCode::Ethereum
            | ChainCode::BnbSmartChain
            | ChainCode::Polygon
            | ChainCode::Avalanche
            | ChainCode::Fantom
            | ChainCode::Arbitrum
            | ChainCode::Base
            | ChainCode::Scroll => super::evm_propose_max_fee(&fee)?,
            ChainCode::Litecoin => todo!(),
            ChainCode::Dogcoin => todo!(),
            ChainCode::Sui => todo!(),
            ChainCode::Ton => todo!(),
            _ => {
                return Err(crate::error::service::ServiceError::Parameter(format!(
                    "unsupported collect chain: {chain_code}"
                )));
            }
        };
        info!(chain_code=%chain_code.to_string(), amount=%amount, source = "side_effect_worker", "Parsed fee estimate successfully");

        Ok(amount)
    }

    /// 构建交易执行回执上传请求
    async fn build_tx_exec_receipt_payload(
        req: &wallet_database::entities::api_collect::ApiCollectEntity,
        trade_no: &str,
    ) -> Result<
        wallet_transport_backend::request::api_wallet::transaction::TxExecReceiptUploadReq,
        ServiceError,
    > {
        if !Self::has_tx_exec_receipt_fact(req) {
            return Err(ServiceError::Parameter(
                "tx_exec_receipt upload requires confirmed success or failure facts".to_string(),
            ));
        }

        // 构建状态
        let upload_status = if req.transaction_time.is_some() {
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Success
        } else if req.err_code.is_some() {
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Fail
        } else {
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Fail
        };

        let tx_hash_missing =
            req.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
        if req.transaction_time.is_some() && tx_hash_missing {
            error!(
                trade_no = %trade_no,
                source = "side_effect_worker",
                transaction_time_present = %req.transaction_time.is_some(),
                last_broadcast_at_present = %req.last_broadcast_at.is_some(),
                tx_hash_is_none = %req.tx_hash.is_none(),
                tx_hash_is_empty = %req.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(false),
                err_code_present = %req.err_code.is_some(),
                "Inconsistent collect execution facts: execution evidence exists but tx_hash is missing"
            );
        }

        // 构建备注
        let remark = if matches!(
            upload_status,
            wallet_transport_backend::request::api_wallet::transaction::TransStatus::Success
        ) || req.err_msg.as_deref().unwrap_or("").is_empty()
        {
            ""
        } else {
            req.err_msg.as_deref().unwrap_or("")
        };

        // 构建请求
        let payload =
            wallet_transport_backend::request::api_wallet::transaction::TxExecReceiptUploadReq::new(
                Some(&req.from_addr),
                Some(&req.to_addr),
                trade_no,
                wallet_transport_backend::request::api_wallet::transaction::TransType::Col,
                req.tx_hash.as_deref(),
                upload_status,
                remark,
            );

        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::api_wallet_backend::ApiWalletBackend;
    use async_trait::async_trait;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::{
        fs,
        path::PathBuf,
        str::FromStr,
        sync::{Arc, OnceLock},
    };
    use tempfile::tempdir;
    use tokio::sync::{OnceCell, mpsc};
    use wallet_database::{
        ApiWalletDbPool, SqliteContext,
        entities::{
            api_coin::ApiCoinEntity,
            api_collect::{ApiCollectEntity, ApiCollectStatus, ErrCode},
            api_resource_delegation::{
                ApiResourceDelegationResultStatus, NewApiResourceDelegation,
            },
            api_trade_type::ApiTradeType,
            asset_token_key::AssetTokenKey,
        },
        repositories::api_wallet::{
            collect::ApiCollectRepo, resource_delegation::ApiResourceDelegationRepo,
        },
    };
    use wallet_transport_backend::{
        request::{
            DeviceDeleteReq as ApiDeviceDeleteReq, KeysInitReq,
            api_wallet::{
                address::ExpandAddressCompleteReq,
                swap::{ApiInitSwapReq, ApiInitSwapResponse},
                transaction::TransType,
                wallet::{AppIdImportReq, AppIdUidUsageReq, BindAppIdReq},
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
            _: wallet_transport_backend::request::api_wallet::wallet::AppIdImportRechargeWalletReq,
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
            _: &ApiDeviceDeleteReq,
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
            dir.push(format!("wallet-api-shadow-worker-tests-{}", std::process::id()));
            fs::create_dir_all(&dir).expect("ensure temp dir");

            let dirs = crate::dirs::Dirs::new(&dir.to_string_lossy()).expect("create dirs");

            crate::context::init_context_with_api_wallet_backend(
                "wallet_api_shadow_side_effect_test_sn",
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

    use crate::infrastructure::api_trans::{
        collect::shadow::ShadowAdvancer,
        resource_ack_type::{platform_resource_result_ack_type, platform_resource_task_trans_type},
    };

    #[tokio::test]
    async fn original_order_resource_success_releases_collect_gate_after_ack_projection()
    -> anyhow::Result<()> {
        let dir = tempdir()?;
        let dir_path = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await?
            .into_transaction_db_pool()?;
        let resource_pool = SqliteContext::new(&dir_path, Some("api_resource.db"))
            .await?
            .into_api_resource_db_pool()?;

        ApiResourceDelegationRepo::upsert_original_order_result_fact(
            &resource_pool,
            NewApiResourceDelegation::platform_delegate(
                "uid_1",
                "C_rsc_ack_release",
                "C_rsc_ack_release",
                ApiTradeType::Collect as i64,
                "",
                "",
                "0",
            ),
            ApiResourceDelegationResultStatus::Success,
            None,
            Some(r#"{"tradeNo":"C_rsc_ack_release","status":true}"#),
        )
        .await?;

        let resource_task = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &resource_pool,
            "C_rsc_ack_release",
        )
        .await?;
        assert_eq!(
            SideEffectWorker::collect_gate_release_from_resource_result(&resource_task),
            Some(
                ("C_rsc_ack_release".to_string(), ApiResourceGateResult::PlatformDelegateSuccess,)
            )
        );

        Ok(())
    }

    #[tokio::test]
    async fn original_order_resource_failure_keeps_collect_gate_blocked() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let dir_path = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&dir_path, Some("api_transaction.db"))
            .await?
            .into_transaction_db_pool()?;
        let resource_pool = SqliteContext::new(&dir_path, Some("api_resource.db"))
            .await?
            .into_api_resource_db_pool()?;

        ApiResourceDelegationRepo::upsert_original_order_result_fact(
            &resource_pool,
            NewApiResourceDelegation::platform_delegate(
                "uid_1",
                "C_rsc_ack_fail_release",
                "C_rsc_ack_fail_release",
                ApiTradeType::Collect as i64,
                "",
                "",
                "0",
            ),
            ApiResourceDelegationResultStatus::Fail,
            Some(3),
            Some(r#"{"tradeNo":"C_rsc_ack_fail_release","status":false,"errorCode":"ERR_6008"}"#),
        )
        .await?;

        let resource_task = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &resource_pool,
            "C_rsc_ack_fail_release",
        )
        .await?;
        assert_eq!(
            SideEffectWorker::collect_gate_release_from_resource_result(&resource_task),
            None
        );

        Ok(())
    }

    fn make_coin(symbol: &str, token_address: AssetTokenKey, decimals: u8) -> ApiCoinEntity {
        ApiCoinEntity {
            id: 1,
            name: symbol.to_string(),
            symbol: symbol.to_string(),
            chain_code: "eth".to_string(),
            token_address,
            price: "0".to_string(),
            protocol: None,
            decimals,
            is_default: 1,
            is_popular: 0,
            is_custom: 0,
            status: 1,
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    fn base_collect() -> ApiCollectEntity {
        ApiCollectEntity {
            id: 1,
            name: "n".to_string(),
            uid: "u".to_string(),
            from_addr: "from".to_string(),
            to_addr: "to".to_string(),
            value: "0".to_string(),
            validate: "v".to_string(),
            chain_code: "eth".to_string(),
            token_addr: AssetTokenKey::Native,
            symbol: "USDT".to_string(),
            trade_no: "C_SIDE_EFFECT_TEST".to_string(),
            trade_type: 2,
            risk_addr: 0,
            status: ApiCollectStatus::Init,
            nonce: 0,
            tx_hash: Some("h".to_string()),
            transaction_fee: "0".to_string(),
            transaction_time: None,
            block_height: Some("0".to_string()),
            notes: Some(String::new()),
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: Some(String::new()),
            resource_check_at: None,
            resource_gate_released_at: None,
            resource_gate_result: None,
            resource_block_reason: None,
            resource_dependency_trade_no: None,
            resource_dependency_type: None,
            order_ack_sent_at: Some(Utc::now()),
            raw_tx: Some("{}".to_string()),
            resource_consume: "0".to_string(),
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            result_ack_sent_at: None,
            result_ack_send_count: 0,
            tx_res_received_at: None,
            service_fee_order_received_at: None,
            service_fee_uploaded_at: None,
            need_service_fee: None,
            ever_needed_service_fee: false,
            tx_fee_res_ack_sent_at: None,
            tx_exec_receipt_uploaded_at: None,
            finished_at: None,
            created_at: Utc::now(),
            updated_at: Some(Utc::now()),
        }
    }

    #[test]
    fn select_fee_estimation_coin_info_uses_token_decimals_for_contract_token() {
        let main_coin = make_coin("ETH", AssetTokenKey::Native, 18);
        let token_coin = make_coin(
            "USDT",
            AssetTokenKey::Contract("0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string()),
            6,
        );

        let (symbol, token_key, decimals) = SideEffectWorker::select_fee_estimation_coin_info(
            &AssetTokenKey::Contract("0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string()),
            &main_coin,
            Some(&token_coin),
        )
        .expect("contract token should resolve");

        assert_eq!(symbol, "USDT");
        assert_eq!(token_key, token_coin.token_address);
        assert_eq!(decimals, 6);
    }

    #[test]
    fn select_fee_estimation_coin_info_uses_main_coin_for_native_token() {
        let main_coin = make_coin("ETH", AssetTokenKey::Native, 18);
        let token_coin = make_coin(
            "USDT",
            AssetTokenKey::Contract("0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string()),
            6,
        );

        let (symbol, token_key, decimals) = SideEffectWorker::select_fee_estimation_coin_info(
            &AssetTokenKey::Native,
            &main_coin,
            Some(&token_coin),
        )
        .expect("native token should resolve");

        assert_eq!(symbol, "ETH");
        assert_eq!(token_key, AssetTokenKey::Native);
        assert_eq!(decimals, 18);
    }

    #[test]
    fn select_fee_estimation_coin_info_errors_when_contract_token_is_missing() {
        let main_coin = make_coin("ETH", AssetTokenKey::Native, 18);

        let err = SideEffectWorker::select_fee_estimation_coin_info(
            &AssetTokenKey::Contract("0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string()),
            &main_coin,
            None,
        )
        .expect_err("missing contract token should fail");

        assert!(err.to_string().contains("token coin not found"), "unexpected error: {err}");
    }

    #[test]
    fn sol_token_collect_service_fee_upload_uses_shortfall_against_balance() {
        let fee = Decimal::from_str("0.000015").expect("fee");
        let sender_rent_reserve = SideEffectWorker::sol_token_collect_sender_rent_reserve(
            "sol",
            &AssetTokenKey::Contract("token".to_string()),
        )
        .expect("rent reserve");
        let current_balance = Decimal::from_str("0.00099088").expect("balance");
        let total_need = fee + sender_rent_reserve;
        let service_fee = SideEffectWorker::sol_token_collect_service_fee_upload_shortfall(
            total_need,
            current_balance,
        );

        assert_eq!(service_fee, Decimal::from_str("0.000015").expect("expected"));
    }

    #[test]
    fn native_sol_service_fee_upload_keeps_fee_only() {
        let fee = Decimal::from_str("0.000015").expect("fee");
        let amount =
            SideEffectWorker::sol_token_collect_sender_rent_reserve("sol", &AssetTokenKey::Native)
                .expect("rent reserve");

        assert_eq!(amount, Decimal::ZERO);
        assert_eq!(fee + amount, fee);
    }

    #[test]
    fn sol_token_collect_service_fee_upload_shortfall_is_zero_when_balance_is_sufficient() {
        let fee = Decimal::from_str("0.000015").expect("fee");
        let sender_rent_reserve = SideEffectWorker::sol_token_collect_sender_rent_reserve(
            "sol",
            &AssetTokenKey::Contract("token".to_string()),
        )
        .expect("rent reserve");
        let total_need = fee + sender_rent_reserve;
        let current_balance = Decimal::from_str("0.00100588").expect("balance");

        let service_fee = SideEffectWorker::sol_token_collect_service_fee_upload_shortfall(
            total_need,
            current_balance,
        );

        assert_eq!(service_fee, Decimal::ZERO);
    }

    #[tokio::test]
    async fn build_tx_exec_receipt_payload_marks_confirmed_success() {
        let mut c = base_collect();
        c.transaction_time = Some(Utc::now());
        c.last_broadcast_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = None;

        let payload = SideEffectWorker::build_tx_exec_receipt_payload(&c, &c.trade_no)
            .await
            .expect("confirmed success should build payload");

        assert!(payload.is_success());
    }

    #[tokio::test]
    async fn build_tx_exec_receipt_payload_marks_failure_fact() {
        let mut c = base_collect();
        c.transaction_time = None;
        c.err_code = Some(ErrCode::UnknownError);
        c.tx_exec_receipt_uploaded_at = None;

        let payload = SideEffectWorker::build_tx_exec_receipt_payload(&c, &c.trade_no)
            .await
            .expect("failure fact should build payload");

        assert!(payload.is_fail());
    }

    #[tokio::test]
    async fn build_tx_exec_receipt_payload_rejects_pending_facts() {
        let mut c = base_collect();
        c.transaction_time = None;
        c.err_code = None;
        c.last_broadcast_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = None;

        let err = SideEffectWorker::build_tx_exec_receipt_payload(&c, &c.trade_no)
            .await
            .expect_err("pending facts should be rejected");

        assert!(err.to_string().contains("confirmed success or failure facts"));
    }

    #[tokio::test]
    async fn ensure_local_undelegation_after_collect_finished_creates_one_task() {
        let ctx = test_ctx().await;
        let resource_pool = ctx.api_resource_pool().expect("resource pool");
        let (intent_tx, _intent_rx) = mpsc::channel(1);
        let worker =
            SideEffectWorker::new(ctx, Arc::new(ShadowAdvancer::new(ctx, intent_tx, None)));
        let origin_trade_no = format!("C_LOCAL_UNDELEGATE_{}", Utc::now().timestamp_micros());
        let delegate_trade_no = format!("rsc_local_delegate_{origin_trade_no}");
        let undelegate_trade_no = collect_local_undelegate_trade_no(&origin_trade_no);

        ApiResourceDelegationRepo::upsert(
            &resource_pool,
            NewApiResourceDelegation::local_delegate(
                "uid",
                delegate_trade_no.clone(),
                origin_trade_no.clone(),
                ApiTradeType::Collect as i64,
                "withdraw_owner",
                "receiver",
                "5",
                "1000",
            ),
        )
        .await
        .expect("insert local delegate");
        ApiResourceDelegationRepo::mark_result_received(
            &resource_pool,
            &delegate_trade_no,
            ApiResourceDelegationResultStatus::Success,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("mark local delegate success");

        worker
            .ensure_local_undelegation_after_collect_finished(&origin_trade_no)
            .await
            .expect("create undelegation");
        worker
            .ensure_local_undelegation_after_collect_finished(&origin_trade_no)
            .await
            .expect("create undelegation idempotent");

        let task = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &resource_pool,
            &undelegate_trade_no,
        )
        .await
        .expect("load undelegation");
        assert_eq!(task.origin_trade_no.as_deref(), Some(origin_trade_no.as_str()));
        assert_eq!(task.native_amount, "5");
        assert_eq!(task.amount, "1000");
    }
}
