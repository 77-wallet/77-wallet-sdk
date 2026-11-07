use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Duration};

use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, trace, warn};
use wallet_chain_interact::{
    BillResourceConsume,
    tron::{
        self,
        operations::{
            RawTransactionParams, TronTxOperation,
            stake::{FreezeBalanceArgs, UnFreezeBalanceArgs},
        },
    },
};
use wallet_database::{
    ApiResourceDbPool,
    entities::{
        api_resource_operation::{ApiResourceOperationEntity, ApiResourceOperationType},
        api_resource_type::ApiResourceType,
    },
    repositories::api_wallet::resource_operation::ApiResourceOperationRepo,
};
use wallet_transport_backend::request::api_wallet::transaction::{
    TransAckType, TransEventAckReq, TransStatus, TransType, TxExecReceiptUploadReq,
};
use wallet_utils::RetryableError as _;

use crate::{
    domain::{
        api_wallet::{
            adapter::tx::RawTx,
            trans::{ApiTransDomain, shared_mechanism},
        },
        chain::adapter::ChainAdapterFactory,
    },
    error::{service::ServiceError, system::SystemError},
    infrastructure::{api_trans::shadow_rpc_policy, runtime::time::new_production_interval},
};

/// 独立平台资源质押/解质押流程。
///
/// 这里只处理 `api_resource_operation` / `tradeType=4`。
/// 打能量/回收能量属于 `api_resource_delegation`，服务归集/提币 gate，
/// 不得接入本流程。
#[derive(Debug, Clone)]
pub enum ResourceOperationIntent {
    SendTaskAck(String),
    ClaimBuildSlot(String),
    BroadcastTx(String),
    RecoverTx(String),
    UploadTxExecReceipt(String),
    SendResultAck(String),
}

#[derive(Debug, Clone)]
pub struct ResourceOperationScannerConfig {
    pub scan_interval: Duration,
    pub max_items_per_scan: usize,
}

impl Default for ResourceOperationScannerConfig {
    fn default() -> Self {
        let scan_interval_secs = shadow_rpc_policy::read_u64_env(
            "RESOURCE_OPERATION_SHADOW_SCAN_INTERVAL_SECS",
            30,
            10,
            120,
        );
        let max_items_per_scan = shadow_rpc_policy::read_u64_env(
            "RESOURCE_OPERATION_SHADOW_MAX_ITEMS_PER_SCAN",
            20,
            1,
            200,
        ) as usize;

        Self { scan_interval: Duration::from_secs(scan_interval_secs), max_items_per_scan }
    }
}

#[derive(Debug, Clone)]
pub struct ResourceOperationScanner {
    api_resource_pool: ApiResourceDbPool,
    config: ResourceOperationScannerConfig,
}

impl ResourceOperationScanner {
    pub fn new(api_resource_pool: ApiResourceDbPool) -> Self {
        Self::with_config(api_resource_pool, ResourceOperationScannerConfig::default())
    }

    pub fn with_config(
        api_resource_pool: ApiResourceDbPool,
        config: ResourceOperationScannerConfig,
    ) -> Self {
        Self { api_resource_pool, config }
    }

    pub async fn try_advance(&self, resource_trade_no: &str) -> Vec<ResourceOperationIntent> {
        match ApiResourceOperationRepo::get_by_resource_trade_no(
            &self.api_resource_pool,
            resource_trade_no,
        )
        .await
        {
            Ok(record) => self.intents_for_record(&record),
            Err(e) => {
                warn!(
                    resource_trade_no = %resource_trade_no,
                    error = %e,
                    "Failed to load resource operation for targeted shadow wakeup"
                );
                Vec::new()
            }
        }
    }

    fn intents_for_record(
        &self,
        record: &ApiResourceOperationEntity,
    ) -> Vec<ResourceOperationIntent> {
        use wallet_database::entities::api_resource_operation::ApiResourceOperationTaskSource;

        if record.task_source != ApiResourceOperationTaskSource::Backend {
            return Vec::new();
        }

        let mut intents = Vec::new();
        let resource_trade_no = record.resource_trade_no.clone();

        if record.task_ack_sent_at.is_none() {
            intents.push(ResourceOperationIntent::SendTaskAck(resource_trade_no.clone()));
        }

        if record.task_ack_sent_at.is_some()
            && record.building_at.is_none()
            && record.raw_tx.is_none()
            && record.err_code.is_none()
        {
            intents.push(ResourceOperationIntent::ClaimBuildSlot(resource_trade_no.clone()));
        }

        if Self::has_text(record.raw_tx.as_deref())
            && Self::has_text(record.tx_hash.as_deref())
            && record.last_broadcast_at.is_none()
            && record.err_code.is_none()
        {
            intents.push(ResourceOperationIntent::BroadcastTx(resource_trade_no.clone()));
        }

        if Self::has_text(record.tx_hash.as_deref())
            && Self::has_text(record.raw_tx.as_deref())
            && record.last_broadcast_at.is_some()
            && record.transaction_time.is_none()
            && record.tx_exec_receipt_uploaded_at.is_none()
            && record.err_code.is_none()
        {
            intents.push(ResourceOperationIntent::RecoverTx(resource_trade_no.clone()));
        }

        if record.tx_exec_receipt_uploaded_at.is_none()
            && (record.transaction_time.is_some() || record.err_code.is_some())
        {
            intents.push(ResourceOperationIntent::UploadTxExecReceipt(resource_trade_no.clone()));
        }

        if record.result_received_at.is_some() && record.result_ack_sent_at.is_none() {
            intents.push(ResourceOperationIntent::SendResultAck(resource_trade_no));
        }

        intents
    }

    fn has_text(value: Option<&str>) -> bool {
        value.is_some_and(|value| !value.trim().is_empty())
    }

    pub async fn scan_round(&self) -> Vec<ResourceOperationIntent> {
        let mut intents = Vec::new();

        match ApiResourceOperationRepo::scan_need_task_ack(
            &self.api_resource_pool,
            self.config.max_items_per_scan,
        )
        .await
        {
            Ok(records) => {
                for record in records {
                    intents.push(ResourceOperationIntent::SendTaskAck(record.resource_trade_no));
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to scan resource operation task ACK records");
            }
        }

        match ApiResourceOperationRepo::scan_can_build(
            &self.api_resource_pool,
            self.config.max_items_per_scan,
        )
        .await
        {
            Ok(records) => {
                for record in records {
                    intents.push(ResourceOperationIntent::ClaimBuildSlot(record.resource_trade_no));
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to scan resource operation build-slot records");
            }
        }

        match ApiResourceOperationRepo::scan_can_broadcast(
            &self.api_resource_pool,
            self.config.max_items_per_scan,
        )
        .await
        {
            Ok(records) => {
                for record in records {
                    intents.push(ResourceOperationIntent::BroadcastTx(record.resource_trade_no));
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to scan resource operation broadcast records");
            }
        }

        match ApiResourceOperationRepo::scan_need_recover(
            &self.api_resource_pool,
            self.config.max_items_per_scan,
        )
        .await
        {
            Ok(records) => {
                for record in records {
                    intents.push(ResourceOperationIntent::RecoverTx(record.resource_trade_no));
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to scan resource operation recover records");
            }
        }

        match ApiResourceOperationRepo::scan_need_tx_exec_receipt_upload(
            &self.api_resource_pool,
            self.config.max_items_per_scan,
        )
        .await
        {
            Ok(records) => {
                for record in records {
                    intents.push(ResourceOperationIntent::UploadTxExecReceipt(
                        record.resource_trade_no,
                    ));
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to scan resource operation receipt upload records");
            }
        }

        match ApiResourceOperationRepo::scan_need_result_ack(
            &self.api_resource_pool,
            self.config.max_items_per_scan,
        )
        .await
        {
            Ok(records) => {
                for record in records {
                    intents.push(ResourceOperationIntent::SendResultAck(record.resource_trade_no));
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to scan resource operation result ACK records");
            }
        }

        intents
    }
}

pub struct ResourceOperationScannerActor {
    scanner: Arc<ResourceOperationScanner>,
    intent_tx: mpsc::Sender<ResourceOperationIntent>,
    shutdown_rx: broadcast::Receiver<()>,
}

impl ResourceOperationScannerActor {
    pub fn new(
        scanner: Arc<ResourceOperationScanner>,
        intent_tx: mpsc::Sender<ResourceOperationIntent>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self { scanner, intent_tx, shutdown_rx }
    }

    pub async fn run(mut self) {
        crate::infrastructure::system_ready::wait_system_ready().await;
        info!("Resource operation shadow scanner actor running");

        let mut interval = new_production_interval(self.scanner.config.scan_interval);

        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    info!("Received shutdown signal for resource operation scanner actor");
                    break;
                }
                _ = interval.tick() => {
                    for intent in self.scanner.scan_round().await {
                        if let Err(e) = self.intent_tx.send(intent).await {
                            error!(error = %e, "Failed to enqueue resource operation intent");
                            break;
                        }
                    }
                }
            }
        }

        info!("Resource operation shadow scanner actor stopped");
    }
}

#[derive(Clone)]
pub struct ResourceOperationWorker {
    ctx: &'static crate::context::Context,
}

impl ResourceOperationWorker {
    const BUILD_SLOT_STALE_SECS: i64 = 30;

    pub fn new(ctx: &'static crate::context::Context) -> Self {
        Self { ctx }
    }
    pub async fn handle(&self, intent: ResourceOperationIntent) -> Result<(), ServiceError> {
        match intent {
            ResourceOperationIntent::SendTaskAck(resource_trade_no) => {
                self.send_task_ack(resource_trade_no).await
            }
            ResourceOperationIntent::ClaimBuildSlot(resource_trade_no) => {
                let result = self.claim_build_slot(resource_trade_no.clone()).await;
                self.handle_terminal_failure_if_needed(&resource_trade_no, result).await
            }
            ResourceOperationIntent::BroadcastTx(resource_trade_no) => {
                let result = self.broadcast_tx(resource_trade_no.clone()).await;
                self.handle_terminal_failure_if_needed(&resource_trade_no, result).await
            }
            ResourceOperationIntent::RecoverTx(resource_trade_no) => {
                let result = self.recover_tx(resource_trade_no.clone()).await;
                self.handle_terminal_failure_if_needed(&resource_trade_no, result).await
            }
            ResourceOperationIntent::UploadTxExecReceipt(resource_trade_no) => {
                self.upload_tx_exec_receipt(resource_trade_no).await
            }
            ResourceOperationIntent::SendResultAck(resource_trade_no) => {
                self.send_result_ack(resource_trade_no).await
            }
        }
    }

    async fn handle_terminal_failure_if_needed(
        &self,
        resource_trade_no: &str,
        result: Result<(), ServiceError>,
    ) -> Result<(), ServiceError> {
        let Err(err) = result else {
            return Ok(());
        };

        match err.retry_policy() {
            wallet_utils::RetryPolicy::Never => {
                // 终止型错误（Never）在这里仅负责“落失败事实”，不继续向上抛错中断主循环。
                //
                // 设计意图（与 collect/withdraw side-effect 对齐）：
                // 1) 失败事实先持久化（err_code/err_msg），形成可恢复的单一事实来源；
                // 2) 后续由 scanner 基于事实推进 UploadTxExecReceipt / SendResultAck；
                // 3) worker 本轮可安全结束，避免同一轮里重复执行或状态抖动。
                //
                // 简单说：这里“吞错”不是忽略错误，而是把错误转成事实后交给调度层续跑。
                self.mark_failed(resource_trade_no, &err).await?;
                Ok(())
            }
            wallet_utils::RetryPolicy::Delay => {
                info!(
                    resource_trade_no = %resource_trade_no,
                    error = %err,
                    "Resource operation terminal step failed, will retry later"
                );
                Ok(())
            }
        }
    }

    async fn send_task_ack(&self, resource_trade_no: String) -> Result<(), ServiceError> {
        info!(resource_trade_no = %resource_trade_no, "Processing resource operation task ACK");

        let resource_task = ApiResourceOperationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if resource_task.task_ack_sent_at.is_some() {
            trace!(resource_trade_no = %resource_trade_no, "Resource operation task ACK already sent");
            return Ok(());
        }

        let backend_api = self.ctx.get_global_backend_api();
        backend_api
            .trans_event_ack(&TransEventAckReq::new(
                &resource_trade_no,
                // tradeType=4 平台资源质押/解锁任务，对应后端 ACK type=PLT_RSC_STK。
                TransType::PltRscStk,
                TransAckType::Tx,
            ))
            .await?;

        let affected = ApiResourceOperationRepo::mark_task_ack_sent(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if affected == 0 {
            warn!(resource_trade_no = %resource_trade_no, "Resource operation task ACK marked 0 rows");
        }

        Ok(())
    }

    async fn claim_build_slot(&self, resource_trade_no: String) -> Result<(), ServiceError> {
        info!(resource_trade_no = %resource_trade_no, "Claiming resource operation build slot");

        let affected = ApiResourceOperationRepo::claim_building_at(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if affected == 0 {
            let operation = ApiResourceOperationRepo::get_by_resource_trade_no(
                &self.ctx.api_resource_pool()?,
                &resource_trade_no,
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;

            if self.reclaim_stale_build_slot(&resource_trade_no, operation.building_at).await? {
                let reclaimed = ApiResourceOperationRepo::claim_building_at(
                    &self.ctx.api_resource_pool()?,
                    &resource_trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                if reclaimed == 0 {
                    trace!(
                        resource_trade_no = %resource_trade_no,
                        "Resource operation build slot still unavailable after stale reclaim"
                    );
                    return Ok(());
                }
                info!(
                    resource_trade_no = %resource_trade_no,
                    "Reclaimed stale resource operation build slot"
                );
            } else {
                trace!(resource_trade_no = %resource_trade_no, "Resource operation build slot not claimed");
                return Ok(());
            }
        }

        if let Err(err) = self.build_resource_operation(&resource_trade_no).await {
            if let Err(db_err) = ApiResourceOperationRepo::clear_building_at(
                &self.ctx.api_resource_pool()?,
                &resource_trade_no,
            )
            .await
            {
                error!(
                    resource_trade_no = %resource_trade_no,
                    error = %db_err,
                    "Failed to release resource operation build slot after build failure"
                );
            }
            return Err(err);
        }

        Ok(())
    }

    fn is_stale_build_slot(building_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
        shared_mechanism::is_stale_slot(building_at, Self::BUILD_SLOT_STALE_SECS, now)
    }

    async fn reclaim_stale_build_slot(
        &self,
        resource_trade_no: &str,
        building_at: Option<DateTime<Utc>>,
    ) -> Result<bool, ServiceError> {
        if !Self::is_stale_build_slot(building_at, Utc::now()) {
            return Ok(false);
        }

        Self::reclaim_stale_build_slot_in_pool(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
            building_at,
        )
        .await
    }

    async fn reclaim_stale_build_slot_in_pool(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
        building_at: Option<DateTime<Utc>>,
    ) -> Result<bool, ServiceError> {
        if !Self::is_stale_build_slot(building_at, Utc::now()) {
            return Ok(false);
        }

        let rows_affected = ApiResourceOperationRepo::clear_building_at(pool, resource_trade_no)
            .await
            .map_err(|e| {
                error!(
                    resource_trade_no = %resource_trade_no,
                    error = %e,
                    "Failed to clear stale resource operation build slot"
                );
                ServiceError::Database(e.into())
            })?;

        info!(
            resource_trade_no = %resource_trade_no,
            rows_affected = %rows_affected,
            building_at = ?building_at,
            "Cleared stale resource operation build slot before retrying BuildTx"
        );

        Ok(rows_affected > 0)
    }

    async fn build_resource_operation(&self, resource_trade_no: &str) -> Result<(), ServiceError> {
        let operation = ApiResourceOperationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if operation.raw_tx.is_some() {
            trace!(
                resource_trade_no = %resource_trade_no,
                "Resource operation raw_tx already exists, skipping build"
            );
            return Ok(());
        }

        let Some((tx_hash, raw_tx, transaction_fee)) =
            crate::infrastructure::chain_rpc_guard::run_guarded_with_ctx_and_log(
                self.ctx,
                &operation.chain_code,
                resource_trade_no,
                "build",
                "resource_operation_shadow",
                crate::infrastructure::chain_rpc_guard::ChainRpcFailureLog::new()
                    .trade_no(resource_trade_no)
                    .chain_code(&operation.chain_code)
                    .source("resource_operation_shadow")
                    .op("build"),
                self.build_tron_resource_raw(&operation),
            )
            .await?
        else {
            self.clear_build_slot_after_claim(resource_trade_no).await?;
            return Ok(());
        };
        let raw_tx_str = wallet_utils::serde_func::serde_to_string(&raw_tx)?;
        let affected = ApiResourceOperationRepo::update_after_build(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
            &tx_hash,
            &raw_tx_str,
            &transaction_fee,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if affected == 0 {
            trace!(
                resource_trade_no = %resource_trade_no,
                "Resource operation build fact was already committed"
            );
        } else {
            info!(
                resource_trade_no = %resource_trade_no,
                tx_hash = %tx_hash,
                transaction_fee = %transaction_fee,
                "Resource operation raw transaction built and persisted"
            );
        }

        Ok(())
    }

    async fn clear_build_slot_after_claim(
        &self,
        resource_trade_no: &str,
    ) -> Result<(), ServiceError> {
        let rows_affected = ApiResourceOperationRepo::clear_building_at(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
        )
        .await
        .map_err(|e| {
            error!(
                resource_trade_no = %resource_trade_no,
                error = %e,
                "Failed to release resource operation build slot after early exit"
            );
            ServiceError::Database(e.into())
        })?;

        info!(
            resource_trade_no = %resource_trade_no,
            rows_affected = %rows_affected,
            "Resource operation BuildTx early exit cleared build slot"
        );

        Ok(())
    }

    async fn build_tron_resource_raw(
        &self,
        operation: &ApiResourceOperationEntity,
    ) -> Result<(String, RawTx, String), ServiceError> {
        if operation.chain_code != "tron" {
            return Err(ServiceError::Parameter(format!(
                "resource operation only supports tron, got {}",
                operation.chain_code
            )));
        }

        let amount = Self::parse_trx_amount(&operation.amount)?;
        let resource = Self::tron_resource_name(operation.resource_type);
        let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(&self.ctx).await?;

        let raw = match operation.operation_type {
            ApiResourceOperationType::Stake => {
                let args =
                    FreezeBalanceArgs::new(&operation.owner_address, resource, amount, None)?;
                args.build_raw_transaction(chain.get_provider()).await?
            }
            ApiResourceOperationType::Unstake => {
                let args =
                    UnFreezeBalanceArgs::new(&operation.owner_address, resource, amount, None)?;
                args.build_raw_transaction(chain.get_provider()).await?
            }
            ApiResourceOperationType::Vote
            | ApiResourceOperationType::WithdrawReward
            | ApiResourceOperationType::WithdrawUnfreeze
            | ApiResourceOperationType::Delegate
            | ApiResourceOperationType::Undelegate
            | ApiResourceOperationType::BatchDelegate
            | ApiResourceOperationType::BatchUndelegate => {
                return Err(ServiceError::Parameter(
                    "resource operation worker does not build client governance tasks".to_string(),
                ));
            }
        };

        self.sign_tron_resource_raw(operation, raw).await
    }

    async fn sign_tron_resource_raw(
        &self,
        operation: &ApiResourceOperationEntity,
        mut raw: RawTransactionParams,
    ) -> Result<(String, RawTx, String), ServiceError> {
        let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(&self.ctx).await?;
        let provider = chain.get_provider();
        let consumer =
            provider.transfer_fee(&operation.owner_address, None, &raw.raw_data_hex, 1).await?;
        let balance = chain.balance(&operation.owner_address, None).await?;
        let stake_amount_sun = match operation.operation_type {
            ApiResourceOperationType::Stake => {
                Self::parse_trx_amount(&operation.amount)? * tron::consts::TRX_VALUE
            }
            ApiResourceOperationType::Unstake => 0,
            ApiResourceOperationType::Vote
            | ApiResourceOperationType::WithdrawReward
            | ApiResourceOperationType::WithdrawUnfreeze
            | ApiResourceOperationType::Delegate
            | ApiResourceOperationType::Undelegate
            | ApiResourceOperationType::BatchDelegate
            | ApiResourceOperationType::BatchUndelegate => 0,
        };
        let need_sun = consumer.transaction_fee_i64().saturating_add(stake_amount_sun);
        if balance.to::<i64>() < need_sun {
            return Err(ServiceError::Parameter(format!(
                "resource operation balance is insufficient: balance={}, need={}",
                balance, need_sun
            )));
        }

        let handles = self.ctx.get_handles_arc().await?;
        let private_key_manager = handles.get_global_private_key_manager();
        let private_key = private_key_manager
            .get_private_key(&operation.owner_address, &operation.chain_code)
            .await?;
        let sign = wallet_utils::sign::sign_tron(&raw.tx_id, &private_key, None)?;
        raw.signature.push(sign);

        let tx_hash = raw.tx_id.clone();
        let transaction_fee = consumer.transaction_fee();
        let raw_tx = RawTx::Tron(
            raw,
            BillResourceConsume::new_tron(consumer.act_bandwidth() as u64, 0),
            transaction_fee.clone(),
        );

        Ok((tx_hash, raw_tx, transaction_fee))
    }

    fn parse_trx_amount(amount: &str) -> Result<i64, ServiceError> {
        let parsed = amount
            .trim()
            .parse::<i64>()
            .map_err(|_| ServiceError::Parameter(format!("invalid resource amount: {amount}")))?;
        if parsed <= 0 {
            return Err(ServiceError::Parameter(format!(
                "resource amount must be positive: {amount}"
            )));
        }
        Ok(parsed)
    }

    fn tron_resource_name(resource_type: ApiResourceType) -> &'static str {
        match resource_type {
            ApiResourceType::Bandwidth => "bandwidth",
            ApiResourceType::Energy => "energy",
        }
    }

    const BROADCAST_UNCERTAIN_TIMEOUT_SECS: i64 = 5 * 60;

    fn broadcast_uncertain_elapsed_secs(
        operation: &ApiResourceOperationEntity,
        now: DateTime<Utc>,
    ) -> Option<i64> {
        operation
            .broadcast_uncertain_since_at
            .map(|since| now.signed_duration_since(since).num_seconds().max(0))
    }

    fn should_timeout_broadcast_uncertain(
        operation: &ApiResourceOperationEntity,
        now: DateTime<Utc>,
    ) -> bool {
        Self::broadcast_uncertain_elapsed_secs(operation, now)
            .map(|elapsed| elapsed >= Self::BROADCAST_UNCERTAIN_TIMEOUT_SECS)
            .unwrap_or(false)
    }

    async fn broadcast_tx(&self, resource_trade_no: String) -> Result<(), ServiceError> {
        info!(resource_trade_no = %resource_trade_no, "Processing resource operation BroadcastTx");

        let operation = ApiResourceOperationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if operation.last_broadcast_at.is_some() {
            trace!(
                resource_trade_no = %resource_trade_no,
                "Resource operation already broadcast, skipping"
            );
            return Ok(());
        }

        let raw_tx_json =
            operation.raw_tx.as_deref().filter(|s| !s.trim().is_empty()).ok_or_else(|| {
                ServiceError::Parameter("resource operation broadcast requires raw_tx".to_string())
            })?;
        let tx_hash =
            operation.tx_hash.as_deref().filter(|s| !s.trim().is_empty()).ok_or_else(|| {
                ServiceError::Parameter("resource operation broadcast requires tx_hash".to_string())
            })?;

        if shared_mechanism::should_invalidate_expired_tron_raw(
            &operation.chain_code,
            raw_tx_json,
            Utc::now().timestamp_millis(),
        ) {
            warn!(
                resource_trade_no = %resource_trade_no,
                tx_hash = %tx_hash,
                "Detected expired tron raw_tx during broadcast; invalidating stale tx facts"
            );
            let rows = ApiResourceOperationRepo::invalidate_raw_tx(
                &self.ctx.api_resource_pool()?,
                &resource_trade_no,
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
            if rows > 0 {
                info!(
                    resource_trade_no = %resource_trade_no,
                    "Invalidated expired raw_tx, will rebuild"
                );
            }
            return Ok(());
        }

        let raw_tx: RawTx = wallet_utils::serde_func::serde_from_str(raw_tx_json)?;
        let _chain_rpc_guard = crate::infrastructure::chain_rpc_guard::acquire_if_guarded_with_ctx(
            self.ctx,
            &operation.chain_code,
        )
        .await;
        let tx_resp = ApiTransDomain::broadcast_transfer(
            &self.ctx,
            &operation.chain_code,
            raw_tx,
            Some(tx_hash),
        )
        .await?;

        match tx_resp {
            Some(tx) => {
                if tx.tx_hash != tx_hash {
                    error!(
                        resource_trade_no = %resource_trade_no,
                        expected_tx_hash = %tx_hash,
                        broadcast_tx_hash = %tx.tx_hash,
                        "Resource operation tx_hash mismatch between build and broadcast"
                    );
                    return Err(ServiceError::System(SystemError::Internal(
                        "resource operation tx_hash mismatch between build and broadcast"
                            .to_string(),
                    )));
                }

                let affected = ApiResourceOperationRepo::mark_broadcast_executed(
                    &self.ctx.api_resource_pool()?,
                    &resource_trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                if affected == 0 {
                    trace!(
                        resource_trade_no = %resource_trade_no,
                        "Resource operation broadcast fact already committed"
                    );
                } else {
                    info!(
                        resource_trade_no = %resource_trade_no,
                        tx_hash = %tx_hash,
                        "Resource operation broadcast fact committed"
                    );
                }
            }
            None => {
                info!(
                    resource_trade_no = %resource_trade_no,
                    tx_hash = %tx_hash,
                    "Resource operation broadcast result uncertain"
                );

                let now = Utc::now();
                let rows_affected = ApiResourceOperationRepo::mark_broadcast_uncertain_attempt(
                    &self.ctx.api_resource_pool()?,
                    &resource_trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;

                let refreshed = ApiResourceOperationRepo::get_by_resource_trade_no(
                    &self.ctx.api_resource_pool()?,
                    &resource_trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;

                info!(
                    resource_trade_no = %refreshed.resource_trade_no,
                    tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                    rows_affected = %rows_affected,
                    retry_count = refreshed.broadcast_uncertain_retry_count,
                    uncertain_since_at = ?refreshed.broadcast_uncertain_since_at,
                    "Resource operation broadcast uncertain state recorded"
                );

                if Self::should_timeout_broadcast_uncertain(&refreshed, now) {
                    warn!(
                        resource_trade_no = %refreshed.resource_trade_no,
                        tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                        uncertain_duration_sec = %Self::broadcast_uncertain_elapsed_secs(&refreshed, now).unwrap_or_default(),
                        "Broadcast uncertain timeout reached; invalidating raw_tx for rebuild"
                    );
                    let rows = ApiResourceOperationRepo::invalidate_raw_tx(
                        &self.ctx.api_resource_pool()?,
                        &resource_trade_no,
                    )
                    .await
                    .map_err(|e| ServiceError::Database(e.into()))?;
                    if rows > 0 {
                        info!(
                            resource_trade_no = %resource_trade_no,
                            "Invalidated timed-out uncertain raw_tx, will rebuild"
                        );
                    }
                }
            }
        }

        Ok(())
    }

    async fn recover_tx(&self, resource_trade_no: String) -> Result<(), ServiceError> {
        info!(resource_trade_no = %resource_trade_no, "Processing resource operation RecoverTx");

        let operation = ApiResourceOperationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if operation.transaction_time.is_some() {
            trace!(
                resource_trade_no = %resource_trade_no,
                "Resource operation transaction_time already exists, skipping recover"
            );
            return Ok(());
        }

        let tx_hash =
            operation.tx_hash.as_deref().filter(|s| !s.trim().is_empty()).ok_or_else(|| {
                ServiceError::Parameter("resource operation recover requires tx_hash".to_string())
            })?;
        let transaction_fee = operation.transaction_fee.as_deref().unwrap_or("0");

        let _chain_rpc_guard = crate::infrastructure::chain_rpc_guard::acquire_if_guarded_with_ctx(
            self.ctx,
            &operation.chain_code,
        )
        .await;
        let tx_resp = ApiTransDomain::process_recovered_tx(
            &self.ctx,
            &operation.chain_code,
            &operation.owner_address,
            tx_hash,
            0,
            transaction_fee,
        )
        .await?;

        let Some(tx_resp) = tx_resp else {
            info!(
                resource_trade_no = %resource_trade_no,
                tx_hash = %tx_hash,
                "Resource operation recover result uncertain"
            );

            let now = Utc::now();
            let rows_affected = ApiResourceOperationRepo::mark_broadcast_uncertain_attempt(
                &self.ctx.api_resource_pool()?,
                &resource_trade_no,
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;

            let refreshed = ApiResourceOperationRepo::get_by_resource_trade_no(
                &self.ctx.api_resource_pool()?,
                &resource_trade_no,
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;

            info!(
                resource_trade_no = %refreshed.resource_trade_no,
                tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                rows_affected = %rows_affected,
                retry_count = refreshed.broadcast_uncertain_retry_count,
                uncertain_since_at = ?refreshed.broadcast_uncertain_since_at,
                "Resource operation recover uncertain state recorded"
            );

            if Self::should_timeout_broadcast_uncertain(&refreshed, now) {
                warn!(
                    resource_trade_no = %refreshed.resource_trade_no,
                    tx_hash = %refreshed.tx_hash.as_deref().unwrap_or_default(),
                    uncertain_duration_sec = %Self::broadcast_uncertain_elapsed_secs(&refreshed, now).unwrap_or_default(),
                    "Recover uncertain timeout reached; invalidating raw_tx for rebuild"
                );
                let rows = ApiResourceOperationRepo::invalidate_raw_tx(
                    &self.ctx.api_resource_pool()?,
                    &resource_trade_no,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                if rows > 0 {
                    info!(
                        resource_trade_no = %resource_trade_no,
                        "Invalidated timed-out uncertain raw_tx, will rebuild"
                    );
                }
            }

            return Ok(());
        };

        if tx_resp.tx_hash != tx_hash {
            error!(
                resource_trade_no = %resource_trade_no,
                expected_tx_hash = %tx_hash,
                recovered_tx_hash = %tx_resp.tx_hash,
                "Resource operation tx_hash mismatch between build and recover"
            );
            return Err(ServiceError::System(SystemError::Internal(
                "resource operation tx_hash mismatch between build and recover".to_string(),
            )));
        }

        let transaction_time_ms = tx_resp.transaction_time_ms.ok_or_else(|| {
            ServiceError::System(SystemError::Internal(
                "resource operation recover returned final result but missing transaction_time_ms"
                    .to_string(),
            ))
        })?;
        let transaction_time =
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(transaction_time_ms as i64)
                .ok_or_else(|| {
                    ServiceError::System(SystemError::Internal(
                        "invalid resource operation transaction_time_ms from chain".to_string(),
                    ))
                })?
                .to_rfc3339();

        let affected = ApiResourceOperationRepo::confirm_transaction_time_if_absent(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
            &transaction_time,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if affected == 0 {
            trace!(
                resource_trade_no = %resource_trade_no,
                "Resource operation transaction_time already committed"
            );
        } else {
            info!(
                resource_trade_no = %resource_trade_no,
                tx_hash = %tx_hash,
                transaction_time = %transaction_time,
                "Resource operation chain confirmation fact committed"
            );
        }

        Ok(())
    }

    async fn upload_tx_exec_receipt(&self, resource_trade_no: String) -> Result<(), ServiceError> {
        info!(resource_trade_no = %resource_trade_no, "Processing resource operation UploadTxExecReceipt");

        let operation = ApiResourceOperationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if operation.tx_exec_receipt_uploaded_at.is_some() {
            trace!(
                resource_trade_no = %resource_trade_no,
                "Resource operation tx exec receipt already uploaded"
            );
            return Ok(());
        }

        let payload = Self::build_tx_exec_receipt_payload(&operation)?;
        let tx_hash_missing =
            operation.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
        if payload.is_success() && tx_hash_missing {
            return Err(ServiceError::Parameter(
                "resource operation success receipt requires non-empty tx_hash".to_string(),
            ));
        }

        let backend_api = self.ctx.get_global_backend_api();
        backend_api.upload_tx_exec_receipt(&payload).await?;

        let affected = ApiResourceOperationRepo::mark_tx_exec_receipt_uploaded(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if affected == 0 {
            trace!(
                resource_trade_no = %resource_trade_no,
                "Resource operation tx exec receipt upload fact already committed"
            );
        } else {
            info!(
                resource_trade_no = %resource_trade_no,
                "Resource operation tx exec receipt uploaded and marked"
            );
        }

        Ok(())
    }

    async fn send_result_ack(&self, resource_trade_no: String) -> Result<(), ServiceError> {
        info!(resource_trade_no = %resource_trade_no, "Processing resource operation result ACK");

        let operation = ApiResourceOperationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if operation.result_ack_sent_at.is_some() {
            trace!(resource_trade_no = %resource_trade_no, "Resource operation result ACK already sent");
            return Ok(());
        }

        if operation.result_received_at.is_none() {
            warn!(resource_trade_no = %resource_trade_no, "Resource operation result ACK skipped because result has not been received");
            return Ok(());
        }

        let backend_api = self.ctx.get_global_backend_api();
        backend_api
            .trans_event_ack(&TransEventAckReq::new(
                &resource_trade_no,
                TransType::PltRscStk,
                TransAckType::TxRes,
            ))
            .await?;

        let affected = ApiResourceOperationRepo::mark_result_ack_sent(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        if affected == 0 {
            warn!(resource_trade_no = %resource_trade_no, "Resource operation result ACK marked 0 rows");
        } else {
            info!(resource_trade_no = %resource_trade_no, "Resource operation result ACK sent and marked");
        }

        Ok(())
    }

    async fn mark_failed(
        &self,
        resource_trade_no: &str,
        err: &ServiceError,
    ) -> Result<(), ServiceError> {
        if err.is_transient_db_pool_timeout() {
            warn!(
                resource_trade_no = %resource_trade_no,
                error = %err,
                "Skip marking resource operation failed: local DB pool timeout is retryable infrastructure noise"
            );
            return Ok(());
        }

        let (err_code, err_msg) = Self::failure_fact_from_error(err);
        let affected = ApiResourceOperationRepo::mark_failed_if_unfinished(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
            &err_code,
            &err_msg,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if affected == 0 {
            trace!(
                resource_trade_no = %resource_trade_no,
                err_code = %err_code,
                "Resource operation failure fact already committed or no longer eligible"
            );
        } else {
            info!(
                resource_trade_no = %resource_trade_no,
                err_code = %err_code,
                "Resource operation failure fact committed"
            );
        }

        Ok(())
    }

    fn failure_fact_from_error(err: &ServiceError) -> (String, String) {
        // 对齐归集/提币：失败码先只区分网络异常和 SDK 内部错误，
        // 细分原因保留在 err_msg，后续统一收集后再交给产品归纳。
        let err_code = if err.is_network_error() { "ERR_6005" } else { "ERR_6008" };
        (err_code.to_string(), err.to_string())
    }

    fn build_tx_exec_receipt_payload(
        operation: &ApiResourceOperationEntity,
    ) -> Result<TxExecReceiptUploadReq, ServiceError> {
        if operation.transaction_time.is_none() && operation.err_code.is_none() {
            return Err(ServiceError::Parameter(
                "resource operation receipt upload requires confirmed success or failure facts"
                    .to_string(),
            ));
        }

        let status = if operation.transaction_time.is_some() {
            TransStatus::Success
        } else {
            TransStatus::Fail
        };
        let remark = if matches!(status, TransStatus::Success) {
            ""
        } else {
            operation.err_msg.as_deref().unwrap_or("")
        };

        let mut payload = TxExecReceiptUploadReq::new(
            Some(&operation.owner_address),
            operation.receiver_address.as_deref(),
            &operation.resource_trade_no,
            TransType::PltRscStk,
            operation.tx_hash.as_deref(),
            status,
            remark,
        );
        if let Some(err_code) = operation.err_code.as_deref().filter(|s| !s.trim().is_empty()) {
            payload = payload.with_error_code(err_code);
        }

        Ok(payload)
    }
}

pub struct ResourceOperationDispatcherActor {
    worker: ResourceOperationWorker,
    intent_rx: mpsc::Receiver<ResourceOperationIntent>,
    shutdown_rx: broadcast::Receiver<()>,
}

impl ResourceOperationDispatcherActor {
    pub fn new(
        worker: ResourceOperationWorker,
        intent_rx: mpsc::Receiver<ResourceOperationIntent>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self { worker, intent_rx, shutdown_rx }
    }

    pub async fn run(mut self) {
        crate::infrastructure::system_ready::wait_system_ready().await;
        info!("Resource operation shadow dispatcher actor running");

        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    info!("Received shutdown signal for resource operation dispatcher actor");
                    break;
                }
                Some(intent) = self.intent_rx.recv() => {
                    if let Err(e) = self.worker.handle(intent).await {
                        error!(error = %e, "Failed to handle resource operation intent");
                    }
                }
                else => {
                    info!("Resource operation intent channel closed");
                    break;
                }
            }
        }

        info!("Resource operation shadow dispatcher actor stopped");
    }
}

#[derive(Debug)]
pub struct ResourceOperationShadowActorSystem {
    shutdown_tx: broadcast::Sender<()>,
    scanner: Arc<ResourceOperationScanner>,
    intent_tx: mpsc::Sender<ResourceOperationIntent>,
    scanner_handle: Option<tokio::task::JoinHandle<()>>,
    dispatcher_handle: Option<tokio::task::JoinHandle<()>>,
}

impl ResourceOperationShadowActorSystem {
    pub fn new(ctx: &'static crate::context::Context) -> Result<Self, ServiceError> {
        let api_resource_pool = ctx.api_resource_pool()?;
        let (shutdown_tx, shutdown_rx1) = broadcast::channel(1);
        let shutdown_rx2 = shutdown_tx.subscribe();
        let (intent_tx, intent_rx) = mpsc::channel(100);

        let scanner = Arc::new(ResourceOperationScanner::new(api_resource_pool.clone()));
        let worker = ResourceOperationWorker::new(ctx);

        info!(
            scan_interval_secs = scanner.config.scan_interval.as_secs(),
            max_items_per_scan = scanner.config.max_items_per_scan,
            "Resource operation shadow runtime config"
        );

        let scanner_clone = scanner.clone();
        let warm_intent_tx = intent_tx.clone();
        let trigger_intent_tx = intent_tx.clone();
        tokio::spawn(async move {
            for intent in scanner_clone.scan_round().await {
                if let Err(e) = warm_intent_tx.send(intent).await {
                    error!(error = %e, "Failed to enqueue warm resource operation intent");
                    break;
                }
            }
            info!("Warm resource operation shadow scan completed");
        });

        let scanner_actor =
            ResourceOperationScannerActor::new(scanner.clone(), intent_tx, shutdown_rx1);
        let scanner_handle = Some(tokio::spawn(async move {
            scanner_actor.run().await;
        }));

        let dispatcher_actor =
            ResourceOperationDispatcherActor::new(worker, intent_rx, shutdown_rx2);
        let dispatcher_handle = Some(tokio::spawn(async move {
            dispatcher_actor.run().await;
        }));

        Ok(Self {
            shutdown_tx,
            scanner,
            intent_tx: trigger_intent_tx,
            scanner_handle,
            dispatcher_handle,
        })
    }

    pub async fn trigger_resource_operation(
        &self,
        resource_trade_no: &str,
    ) -> Result<(), ServiceError> {
        for intent in self.scanner.try_advance(resource_trade_no).await {
            if let Err(e) = self.intent_tx.send(intent).await {
                return Err(ServiceError::System(SystemError::ChannelSendFailed(e.to_string())));
            }
        }

        Ok(())
    }

    pub async fn stop(&mut self) {
        info!("Stopping resource operation shadow system");

        let _ = self.shutdown_tx.send(());

        if let Some(handle) = self.scanner_handle.take() {
            if let Err(err) = handle.await {
                warn!(error = %err, "resource operation scanner join failed during stop");
            }
        }

        if let Some(handle) = self.dispatcher_handle.take() {
            if let Err(err) = handle.await {
                warn!(error = %err, "resource operation dispatcher join failed during stop");
            }
        }

        info!("Resource operation shadow system stopped");
    }
}

pub(crate) async fn init(
    ctx: &'static crate::context::Context,
) -> Result<ResourceOperationShadowActorSystem, ServiceError> {
    ResourceOperationShadowActorSystem::new(ctx)
}

pub async fn scan_and_process_once(
    ctx: &'static crate::context::Context,
) -> Result<(), ServiceError> {
    let api_resource_pool = ctx.api_resource_pool()?;
    let scanner = ResourceOperationScanner::new(api_resource_pool.clone());
    let worker = ResourceOperationWorker::new(ctx);

    for intent in scanner.scan_round().await {
        worker.handle(intent).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use wallet_database::{
        SqliteContext, entities::api_resource_operation::NewApiResourceOperation,
        repositories::api_wallet::resource_operation::ApiResourceOperationRepo,
    };

    async fn test_ctx() -> &'static crate::context::Context {
        crate::testkit::context::api_trans_test_ctx().await
    }

    #[tokio::test]
    async fn scanner_owns_resource_operation_ack_build_broadcast_recover_and_receipt_intents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");

        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_need_ack", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_can_build", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_can_build").await.unwrap();
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_can_broadcast", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_can_broadcast").await.unwrap();
        ApiResourceOperationRepo::claim_building_at(&pool, "op_can_broadcast").await.unwrap();
        ApiResourceOperationRepo::update_after_build(
            &pool,
            "op_can_broadcast",
            "0xhash_1",
            "{\"Tron\":[{\"tx_id\":\"0xhash_1\",\"raw_data_hex\":\"00\",\"signature\":[]},{\"netUsed\":0,\"energyUsed\":0},\"0\"]}",
            "0",
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_need_recover", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_need_recover").await.unwrap();
        ApiResourceOperationRepo::claim_building_at(&pool, "op_need_recover").await.unwrap();
        ApiResourceOperationRepo::update_after_build(
            &pool,
            "op_need_recover",
            "0xhash_2",
            "{\"Tron\":[{\"tx_id\":\"0xhash_2\",\"raw_data_hex\":\"00\",\"signature\":[]},{\"netUsed\":0,\"energyUsed\":0},\"0\"]}",
            "0",
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_broadcast_executed(&pool, "op_need_recover").await.unwrap();
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_need_receipt", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_need_receipt").await.unwrap();
        ApiResourceOperationRepo::claim_building_at(&pool, "op_need_receipt").await.unwrap();
        ApiResourceOperationRepo::update_after_build(
            &pool,
            "op_need_receipt",
            "0xhash_3",
            "{\"Tron\":[{\"tx_id\":\"0xhash_3\",\"raw_data_hex\":\"00\",\"signature\":[]},{\"netUsed\":0,\"energyUsed\":0},\"0\"]}",
            "0",
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_broadcast_executed(&pool, "op_need_receipt").await.unwrap();
        ApiResourceOperationRepo::confirm_transaction_time_if_absent(
            &pool,
            "op_need_receipt",
            "2026-05-04T00:00:00Z",
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_need_result_ack", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_result_received(
            &pool,
            "op_need_result_ack",
            "success",
            Some(0),
            None,
            None,
            Some("{\"status\":true}"),
        )
        .await
        .unwrap();

        let scanner = ResourceOperationScanner::new(pool.clone());
        let intents = scanner.scan_round().await;

        assert!(intents.iter().any(|intent| {
            matches!(intent, ResourceOperationIntent::SendTaskAck(trade_no) if trade_no == "op_need_ack")
        }));
        assert!(intents.iter().any(|intent| {
            matches!(intent, ResourceOperationIntent::ClaimBuildSlot(trade_no) if trade_no == "op_can_build")
        }));
        assert!(intents.iter().any(|intent| {
            matches!(intent, ResourceOperationIntent::BroadcastTx(trade_no) if trade_no == "op_can_broadcast")
        }));
        assert!(intents.iter().any(|intent| {
            matches!(intent, ResourceOperationIntent::RecoverTx(trade_no) if trade_no == "op_need_recover")
        }));
        assert!(intents.iter().any(|intent| {
            matches!(intent, ResourceOperationIntent::UploadTxExecReceipt(trade_no) if trade_no == "op_need_receipt")
        }));
        assert!(intents.iter().any(|intent| {
            matches!(intent, ResourceOperationIntent::SendResultAck(trade_no) if trade_no == "op_need_result_ack")
        }));
    }

    #[tokio::test]
    async fn resource_operation_reclaim_stale_build_slot_only_without_raw_tx() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");

        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_stale_build_slot", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_stale_build_slot").await.unwrap();
        assert_eq!(
            ApiResourceOperationRepo::claim_building_at(&pool, "op_stale_build_slot")
                .await
                .unwrap(),
            1
        );

        let stale_building_at = Utc::now() - chrono::Duration::seconds(60);
        sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET building_at = ?
            WHERE resource_trade_no = ?
            "#,
        )
        .bind(stale_building_at)
        .bind("op_stale_build_slot")
        .execute(pool.write_ref())
        .await
        .expect("age build slot");

        let reclaimed = ResourceOperationWorker::reclaim_stale_build_slot_in_pool(
            &pool,
            "op_stale_build_slot",
            Some(stale_building_at),
        )
        .await
        .expect("reclaim stale build slot");
        assert!(reclaimed);
        assert_eq!(
            ApiResourceOperationRepo::claim_building_at(&pool, "op_stale_build_slot")
                .await
                .unwrap(),
            1
        );

        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_built_slot", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_built_slot").await.unwrap();
        ApiResourceOperationRepo::claim_building_at(&pool, "op_built_slot").await.unwrap();
        ApiResourceOperationRepo::update_after_build(
            &pool,
            "op_built_slot",
            "0xhash_built",
            "{\"Tron\":[{\"tx_id\":\"0xhash_built\",\"raw_data_hex\":\"00\",\"signature\":[]},{\"netUsed\":0,\"energyUsed\":0},\"0\"]}",
            "0",
        )
        .await
        .unwrap();
        sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET building_at = ?
            WHERE resource_trade_no = ?
            "#,
        )
        .bind(stale_building_at)
        .bind("op_built_slot")
        .execute(pool.write_ref())
        .await
        .expect("age built slot");

        let built_reclaimed = ResourceOperationWorker::reclaim_stale_build_slot_in_pool(
            &pool,
            "op_built_slot",
            Some(stale_building_at),
        )
        .await
        .expect("attempt built slot reclaim");
        assert!(!built_reclaimed);

        let built = ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_built_slot")
            .await
            .unwrap();
        assert_eq!(built.raw_tx.as_deref().map(str::is_empty), Some(false));
        assert_eq!(built.tx_hash.as_deref(), Some("0xhash_built"));
        assert!(built.building_at.is_some());
    }

    #[test]
    fn resource_operation_amount_requires_positive_integer_trx() {
        assert_eq!(ResourceOperationWorker::parse_trx_amount("1000").unwrap(), 1000);
        assert!(ResourceOperationWorker::parse_trx_amount("0").is_err());
        assert!(ResourceOperationWorker::parse_trx_amount("-1").is_err());
        assert!(ResourceOperationWorker::parse_trx_amount("1.5").is_err());
    }

    #[test]
    fn resource_operation_resource_type_maps_to_tron_names() {
        assert_eq!(ResourceOperationWorker::tron_resource_name(ApiResourceType::Energy), "energy");
        assert_eq!(
            ResourceOperationWorker::tron_resource_name(ApiResourceType::Bandwidth),
            "bandwidth"
        );
    }

    #[test]
    fn resource_operation_receipt_payload_marks_confirmed_success() {
        let operation = ApiResourceOperationEntity {
            id: 1,
            uid: "uid_1".to_string(),
            task_source: wallet_database::entities::api_resource_operation::ApiResourceOperationTaskSource::Backend,
            operation_type: wallet_database::entities::api_resource_operation::ApiResourceOperationType::Stake,
            resource_trade_no: "op_payload".to_string(),
            chain_code: "tron".to_string(),
            owner_address: "owner".to_string(),
            receiver_address: Some("receiver".to_string()),
            resource_type: ApiResourceType::Energy,
            amount: "1".to_string(),
            status: wallet_database::entities::api_resource_operation::ApiResourceOperationStatus::Pending,
            task_ack_sent_at: None,
            building_at: None,
            raw_tx: Some("{}".to_string()),
            tx_hash: Some("0xhash".to_string()),
            transaction_fee: Some("0".to_string()),
            last_broadcast_at: None,
            transaction_time: Some(chrono::Utc::now()),
            tx_status: None,
            tx_exec_receipt_uploaded_at: None,
            result_status: None,
            result_received_at: None,
            result_ack_sent_at: None,
            result_payload: None,
            fail_type: None,
            err_code: None,
            err_msg: None,
            recover_status: None,
            next_retry_at: None,
            retry_count: 0,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            created_at: chrono::Utc::now(),
            updated_at: None,
        };

        let payload = ResourceOperationWorker::build_tx_exec_receipt_payload(&operation).unwrap();
        assert!(payload.is_success());
    }

    #[test]
    fn resource_operation_receipt_payload_marks_failure_with_code() {
        let operation = ApiResourceOperationEntity {
            id: 1,
            uid: "uid_1".to_string(),
            task_source: wallet_database::entities::api_resource_operation::ApiResourceOperationTaskSource::Backend,
            operation_type: wallet_database::entities::api_resource_operation::ApiResourceOperationType::Stake,
            resource_trade_no: "op_failed_payload".to_string(),
            chain_code: "tron".to_string(),
            owner_address: "owner".to_string(),
            receiver_address: None,
            resource_type: ApiResourceType::Energy,
            amount: "1".to_string(),
            status: wallet_database::entities::api_resource_operation::ApiResourceOperationStatus::Pending,
            task_ack_sent_at: None,
            building_at: None,
            raw_tx: None,
            tx_hash: None,
            transaction_fee: None,
            last_broadcast_at: None,
            transaction_time: None,
            tx_status: Some("fail".to_string()),
            tx_exec_receipt_uploaded_at: None,
            result_status: None,
            result_received_at: None,
            result_ack_sent_at: None,
            result_payload: None,
            fail_type: None,
            err_code: Some("ERR_6008".to_string()),
            err_msg: Some("invalid resource amount".to_string()),
            recover_status: None,
            next_retry_at: None,
            retry_count: 0,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            created_at: chrono::Utc::now(),
            updated_at: None,
        };

        let payload = ResourceOperationWorker::build_tx_exec_receipt_payload(&operation).unwrap();
        assert!(payload.is_fail());
        let payload = serde_json::to_value(payload).unwrap();
        assert_eq!(payload["errorCode"], "ERR_6008");
        assert_eq!(payload["remark"], "invalid resource amount");
        assert_eq!(payload["hash"], "");
    }

    #[test]
    fn resource_operation_failure_fact_maps_service_error() {
        let (code, msg) = ResourceOperationWorker::failure_fact_from_error(
            &ServiceError::Parameter("invalid resource amount".to_string()),
        );

        assert_eq!(code, "ERR_6008");
        assert!(msg.contains("invalid resource amount"));
    }

    #[tokio::test]
    async fn terminal_failure_fact_is_scannable_for_receipt_upload() {
        let ctx = test_ctx().await;
        let pool = ctx.api_resource_pool().expect("resource pool");

        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_terminal_failed", "owner", "1"),
        )
        .await
        .unwrap();

        let worker = ResourceOperationWorker::new(ctx);
        worker
            .handle_terminal_failure_if_needed(
                "op_terminal_failed",
                Err(ServiceError::Parameter("invalid resource amount".to_string())),
            )
            .await
            .expect("terminal failure should be absorbed after persisting failure fact");

        let scanner = ResourceOperationScanner::new(pool.clone());
        let intents = scanner.scan_round().await;
        assert!(intents.iter().any(|intent| {
            matches!(
                intent,
                ResourceOperationIntent::UploadTxExecReceipt(trade_no)
                    if trade_no == "op_terminal_failed"
            )
        }));
    }

    #[tokio::test]
    async fn resource_operation_targeted_wakeup_emits_result_ack() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");

        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_target_result_ack", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_result_received(
            &pool,
            "op_target_result_ack",
            "success",
            Some(0),
            None,
            None,
            Some("{\"status\":true}"),
        )
        .await
        .unwrap();

        let scanner = ResourceOperationScanner::new(pool.clone());
        let intents = scanner.try_advance("op_target_result_ack").await;

        assert!(intents.iter().any(|intent| {
            matches!(
                intent,
                ResourceOperationIntent::SendResultAck(trade_no)
                    if trade_no == "op_target_result_ack"
            )
        }));
    }
}
