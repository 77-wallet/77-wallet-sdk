use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use dashmap::DashSet;
use tokio::sync::{Mutex as AsyncMutex, Semaphore, broadcast, mpsc};
use tracing::{error, info, warn};
use wallet_chain_interact::{
    BillResourceConsume,
    tron::operations::{RawTransactionParams, TronTxOperation, stake::UnDelegateArgs},
};
use wallet_database::{
    ApiResourceDbPool, ApiTransactionDbPool,
    entities::{
        api_resource_delegation::{
            ApiResourceDelegationEntity, ApiResourceDelegationOperationType,
            ApiResourceDelegationResultStatus, ApiResourceDelegationSource,
            NewApiResourceDelegation,
        },
        api_resource_type::ApiResourceType,
        api_trade_type::ApiTradeType,
    },
    repositories::api_wallet::{
        collect::ApiCollectRepo, resource_delegation::ApiResourceDelegationRepo,
    },
};
use wallet_utils::RetryableError as _;

use crate::{
    domain::{
        api_wallet::{adapter::tx::RawTx, trans::ApiTransDomain},
        chain::adapter::ChainAdapterFactory,
    },
    error::{service::ServiceError, system::SystemError},
    infrastructure::{
        api_trans::{
            resource_amount::parse_resource_delegation_native_trx_units,
            resource_delegation::{
                execute_resource_delegation, mark_recovered_resource_delegation_failure,
                record_resource_uncertain_and_maybe_rebroadcast,
                resource_delegation_is_built_unbroadcast, resource_delegation_retry_wait_secs,
                resource_recovery_error_is_uncertain,
            },
            shadow_rpc_policy,
        },
        runtime::time::new_production_interval,
    },
};

#[derive(Debug, Clone)]
pub enum LocalResourceReclaimIntent {
    ExecuteLocalUndelegation(String),
    RecoverLocalUndelegation(String),
}

impl LocalResourceReclaimIntent {
    fn resource_trade_no(&self) -> &str {
        match self {
            Self::ExecuteLocalUndelegation(resource_trade_no)
            | Self::RecoverLocalUndelegation(resource_trade_no) => resource_trade_no,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalResourceReclaimDispatcherConfig {
    pub concurrency: usize,
}

impl Default for LocalResourceReclaimDispatcherConfig {
    fn default() -> Self {
        let concurrency = shadow_rpc_policy::read_usize_env(
            "LOCAL_RESOURCE_RECLAIM_DISPATCHER_CONCURRENCY",
            2,
            1,
            16,
        );
        Self { concurrency }
    }
}

static LOCAL_RESOURCE_RECLAIM_INTENT_TX: OnceLock<
    Mutex<Option<mpsc::Sender<LocalResourceReclaimIntent>>>,
> = OnceLock::new();

fn local_resource_reclaim_intent_tx()
-> &'static Mutex<Option<mpsc::Sender<LocalResourceReclaimIntent>>> {
    LOCAL_RESOURCE_RECLAIM_INTENT_TX.get_or_init(|| Mutex::new(None))
}

pub(crate) fn trigger_local_undelegation_fast_path(resource_trade_no: &str) -> bool {
    let intent_tx = local_resource_reclaim_intent_tx()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let Some(intent_tx) = intent_tx else {
        return false;
    };

    match intent_tx.try_send(LocalResourceReclaimIntent::ExecuteLocalUndelegation(
        resource_trade_no.to_string(),
    )) {
        Ok(()) => {
            info!(resource_trade_no, "Local undelegation fast path enqueued");
            true
        }
        Err(e) => {
            warn!(resource_trade_no, error = %e, "Local undelegation fast path unavailable; scanner will retry");
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalResourceReclaimScannerConfig {
    pub scan_interval: Duration,
    pub max_items_per_scan: usize,
}

impl Default for LocalResourceReclaimScannerConfig {
    fn default() -> Self {
        let scan_interval_secs = shadow_rpc_policy::read_u64_env(
            "LOCAL_RESOURCE_RECLAIM_SHADOW_SCAN_INTERVAL_SECS",
            30,
            10,
            120,
        );
        let max_items_per_scan = shadow_rpc_policy::read_u64_env(
            "LOCAL_RESOURCE_RECLAIM_SHADOW_MAX_ITEMS_PER_SCAN",
            20,
            1,
            200,
        ) as usize;

        Self { scan_interval: Duration::from_secs(scan_interval_secs), max_items_per_scan }
    }
}

#[derive(Debug, Clone)]
pub struct LocalResourceReclaimScanner {
    api_resource_pool: ApiResourceDbPool,
    api_transaction_pool: Option<ApiTransactionDbPool>,
    config: LocalResourceReclaimScannerConfig,
    scan_guard: Arc<AsyncMutex<()>>,
}

impl LocalResourceReclaimScanner {
    pub fn new(api_resource_pool: ApiResourceDbPool) -> Self {
        Self::with_config(api_resource_pool, LocalResourceReclaimScannerConfig::default())
    }

    pub fn with_config(
        api_resource_pool: ApiResourceDbPool,
        config: LocalResourceReclaimScannerConfig,
    ) -> Self {
        Self {
            api_resource_pool,
            api_transaction_pool: None,
            config,
            scan_guard: Arc::new(AsyncMutex::new(())),
        }
    }

    pub fn with_transaction_pool_and_config(
        api_resource_pool: ApiResourceDbPool,
        api_transaction_pool: ApiTransactionDbPool,
        config: LocalResourceReclaimScannerConfig,
    ) -> Self {
        Self {
            api_resource_pool,
            api_transaction_pool: Some(api_transaction_pool),
            config,
            scan_guard: Arc::new(AsyncMutex::new(())),
        }
    }

    pub async fn scan_round(&self) -> Vec<LocalResourceReclaimIntent> {
        let _scan_guard = self.scan_guard.lock().await;
        let mut intents = Vec::new();
        let mut execute_trade_nos = HashSet::new();

        match self.reconcile_missing_local_undelegations().await {
            Ok(resource_trade_nos) => {
                for resource_trade_no in resource_trade_nos {
                    execute_trade_nos.insert(resource_trade_no.clone());
                    intents.push(LocalResourceReclaimIntent::ExecuteLocalUndelegation(
                        resource_trade_no,
                    ));
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to reconcile missing local undelegation records");
            }
        }

        match ApiResourceDelegationRepo::scan_can_execute_for_origin_type_source_and_operation(
            &self.api_resource_pool,
            ApiTradeType::Collect as i64,
            ApiResourceDelegationSource::Local,
            ApiResourceDelegationOperationType::Undelegate,
            self.config.max_items_per_scan,
        )
        .await
        {
            Ok(records) => {
                for record in records {
                    if !self.origin_collect_is_terminal(&record).await {
                        continue;
                    }
                    if execute_trade_nos.insert(record.resource_trade_no.clone()) {
                        intents.push(LocalResourceReclaimIntent::ExecuteLocalUndelegation(
                            record.resource_trade_no,
                        ));
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to scan executable local undelegation records");
            }
        }

        match ApiResourceDelegationRepo::scan_unreconciled_terminal_broadcasts_for_recovery(
            &self.api_resource_pool,
            ApiTradeType::Collect as i64,
            ApiResourceDelegationOperationType::Undelegate,
            self.config.max_items_per_scan,
        )
        .await
        {
            Ok(records) => {
                for record in records {
                    if !self.origin_collect_is_terminal(&record).await {
                        continue;
                    }
                    match ApiResourceDelegationRepo::reopen_terminal_broadcast_for_reconciliation(
                        &self.api_resource_pool,
                        &record.resource_trade_no,
                    )
                    .await
                    {
                        Ok(1) => info!(
                            resource_trade_no = record.resource_trade_no,
                            "Reopened historical terminal local undelegation for chain reconciliation"
                        ),
                        Ok(_) => {}
                        Err(e) => error!(
                            resource_trade_no = record.resource_trade_no,
                            error = %e,
                            "Failed to reopen historical transient local undelegation failure"
                        ),
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to scan historical terminal local undelegations for reconciliation");
            }
        }

        match ApiResourceDelegationRepo::scan_can_recover_local_undelegation_for_origin_type(
            &self.api_resource_pool,
            ApiTradeType::Collect as i64,
            self.config.max_items_per_scan,
        )
        .await
        {
            Ok(records) => {
                for record in records {
                    if !self.origin_collect_is_terminal(&record).await {
                        continue;
                    }
                    intents.push(LocalResourceReclaimIntent::RecoverLocalUndelegation(
                        record.resource_trade_no,
                    ));
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to scan recoverable local undelegation records");
            }
        }

        intents
    }

    async fn origin_collect_is_terminal(&self, delegation: &ApiResourceDelegationEntity) -> bool {
        let Some(api_transaction_pool) = &self.api_transaction_pool else {
            return true;
        };
        let Some(origin_trade_no) = delegation.origin_trade_no.as_deref() else {
            return false;
        };

        match ApiCollectRepo::find_api_collect_by_trade_no(api_transaction_pool, origin_trade_no)
            .await
        {
            Ok(Some(origin_collect)) if origin_collect.finished_at.is_some() => true,
            Ok(_) => {
                info!(
                    origin_trade_no,
                    resource_trade_no = %delegation.resource_trade_no,
                    source = "local_resource_reclaim_shadow",
                    "Skipping local undelegation before collect terminal fact"
                );
                false
            }
            Err(e) => {
                error!(
                    origin_trade_no,
                    resource_trade_no = %delegation.resource_trade_no,
                    error = %e,
                    "Failed to verify collect terminal fact before local undelegation"
                );
                false
            }
        }
    }

    async fn reconcile_missing_local_undelegations(
        &self,
    ) -> Result<Vec<String>, wallet_database::Error> {
        let Some(api_transaction_pool) = &self.api_transaction_pool else {
            return Ok(Vec::new());
        };
        let mut after_id = None;
        let mut resource_trade_nos = Vec::new();
        loop {
            let delegates = ApiResourceDelegationRepo::
                scan_local_delegates_missing_undelegation_page_for_origin_type(
                &self.api_resource_pool,
                ApiTradeType::Collect as i64,
                after_id,
                self.config.max_items_per_scan,
            )
            .await?;
            if delegates.is_empty() {
                break;
            }
            let page_len = delegates.len();
            after_id = delegates.last().map(|delegate| delegate.id);

            for delegate in delegates {
                let Some(origin_trade_no) = delegate.origin_trade_no.clone() else {
                    continue;
                };
                let Some(origin_collect) = ApiCollectRepo::find_api_collect_by_trade_no(
                    api_transaction_pool,
                    &origin_trade_no,
                )
                .await?
                else {
                    continue;
                };
                if origin_collect.finished_at.is_none() {
                    info!(
                        origin_trade_no = %origin_trade_no,
                        source = "local_resource_reclaim_shadow",
                        "Skipping missing local undelegation reconciliation before collect terminal fact"
                    );
                    continue;
                }
                let resource_trade_no = collect_local_undelegate_trade_no(&origin_trade_no);
                ApiResourceDelegationRepo::upsert(
                    &self.api_resource_pool,
                    NewApiResourceDelegation::local_undelegate(
                        delegate.uid,
                        resource_trade_no.clone(),
                        origin_trade_no.clone(),
                        ApiTradeType::Collect as i64,
                        delegate.owner_address,
                        delegate.receiver_address,
                        delegate.native_amount,
                        delegate.amount,
                    ),
                )
                .await?;
                resource_trade_nos.push(resource_trade_no.clone());
                info!(
                    resource_trade_no = %resource_trade_no,
                    origin_trade_no = %origin_trade_no,
                    source = "local_resource_reclaim_shadow",
                    "Reconciled missing local undelegation after local delegation success"
                );
            }

            if page_len < self.config.max_items_per_scan {
                break;
            }
        }

        Ok(resource_trade_nos)
    }
}

fn collect_local_undelegate_trade_no(origin_trade_no: &str) -> String {
    format!("rsc_local_undelegate_{}", origin_trade_no)
}

pub struct LocalResourceReclaimScannerActor {
    scanner: Arc<LocalResourceReclaimScanner>,
    intent_tx: mpsc::Sender<LocalResourceReclaimIntent>,
    shutdown_rx: broadcast::Receiver<()>,
}

impl LocalResourceReclaimScannerActor {
    pub fn new(
        scanner: Arc<LocalResourceReclaimScanner>,
        intent_tx: mpsc::Sender<LocalResourceReclaimIntent>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self { scanner, intent_tx, shutdown_rx }
    }

    pub async fn run(mut self) {
        crate::infrastructure::system_ready::wait_system_ready().await;
        info!("Local resource reclaim shadow scanner actor running");

        let mut interval = new_production_interval(self.scanner.config.scan_interval);

        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    info!("Received shutdown signal for local resource reclaim scanner actor");
                    break;
                }
                _ = interval.tick() => {
                    for intent in self.scanner.scan_round().await {
                        if let Err(e) = self.intent_tx.send(intent).await {
                            error!(error = %e, "Failed to enqueue local resource reclaim intent");
                            break;
                        }
                    }
                }
            }
        }

        info!("Local resource reclaim shadow scanner actor stopped");
    }
}

#[derive(Clone)]
pub struct LocalResourceReclaimWorker {
    ctx: &'static crate::context::Context,
}

impl LocalResourceReclaimWorker {
    const LOCAL_UNDELEGATION_POST_BROADCAST_CONFIRM_DELAYS_SECS: [u64; 3] = [3, 8, 15];

    pub fn new(ctx: &'static crate::context::Context) -> Self {
        Self { ctx }
    }
    pub async fn handle(&self, intent: LocalResourceReclaimIntent) -> Result<(), ServiceError> {
        match intent {
            LocalResourceReclaimIntent::ExecuteLocalUndelegation(resource_trade_no) => {
                let result =
                    self.process_local_undelegation_execute(resource_trade_no.clone()).await;
                self.handle_local_undelegation_execute_failure_if_needed(&resource_trade_no, result)
                    .await
            }
            LocalResourceReclaimIntent::RecoverLocalUndelegation(resource_trade_no) => {
                self.process_local_undelegation_recover(resource_trade_no).await
            }
        }
    }

    fn origin_trade_no<'a>(delegation: &'a ApiResourceDelegationEntity) -> &'a str {
        delegation.origin_trade_no.as_deref().unwrap_or("<missing>")
    }

    async fn handle_local_undelegation_execute_failure_if_needed(
        &self,
        resource_trade_no: &str,
        result: Result<(), ServiceError>,
    ) -> Result<(), ServiceError> {
        let Err(err) = result else {
            return Ok(());
        };
        if Self::is_local_undelegation_already_reclaimed_error(&err) {
            return self.mark_local_undelegation_already_reclaimed(resource_trade_no, &err).await;
        }
        self.schedule_local_undelegation_rebuild_retry(resource_trade_no, &err).await
    }

    fn is_local_undelegation_already_reclaimed_error(err: &ServiceError) -> bool {
        err.to_string().contains("delegated Resource does not exist")
    }

    fn schedule_local_undelegation_post_broadcast_confirms(
        &self,
        resource_trade_no: &str,
        tx_hash: &str,
    ) {
        for delay_secs in Self::LOCAL_UNDELEGATION_POST_BROADCAST_CONFIRM_DELAYS_SECS {
            let worker = self.clone();
            let resource_trade_no = resource_trade_no.to_string();
            let tx_hash = tx_hash.to_string();
            info!(
                resource_trade_no = %resource_trade_no,
                tx_hash = %tx_hash,
                delay_secs = delay_secs,
                source = "local_resource_reclaim_shadow",
                "Scheduled local undelegation post-broadcast confirmation"
            );
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                info!(
                    resource_trade_no = %resource_trade_no,
                    tx_hash = %tx_hash,
                    delay_secs = delay_secs,
                    source = "local_resource_reclaim_shadow",
                    "Triggering local undelegation post-broadcast confirmation"
                );
                if let Err(err) = worker
                    .handle(LocalResourceReclaimIntent::RecoverLocalUndelegation(
                        resource_trade_no.clone(),
                    ))
                    .await
                {
                    warn!(
                        resource_trade_no = %resource_trade_no,
                        tx_hash = %tx_hash,
                        delay_secs = delay_secs,
                        error = %err,
                        source = "local_resource_reclaim_shadow",
                        "Local undelegation post-broadcast confirmation failed"
                    );
                }
            });
        }
    }

    async fn mark_local_undelegation_already_reclaimed(
        &self,
        resource_trade_no: &str,
        err: &ServiceError,
    ) -> Result<(), ServiceError> {
        Self::mark_local_undelegation_already_reclaimed_in_pool(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
            err,
        )
        .await
    }

    async fn mark_local_undelegation_already_reclaimed_in_pool(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
        err: &ServiceError,
    ) -> Result<(), ServiceError> {
        let payload = format!("local_undelegation_already_reclaimed:{err}");
        let affected = ApiResourceDelegationRepo::mark_result_received(
            pool,
            resource_trade_no,
            ApiResourceDelegationResultStatus::Success,
            None,
            None,
            None,
            Some(&payload),
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        info!(
            resource_trade_no = %resource_trade_no,
            affected = affected,
            error = %err,
            source = "local_resource_reclaim_shadow",
            "Local undelegation recorded as already reclaimed"
        );
        Ok(())
    }

    async fn process_local_undelegation_execute(
        &self,
        resource_trade_no: String,
    ) -> Result<(), ServiceError> {
        info!(
            resource_trade_no = %resource_trade_no,
            source = "local_resource_reclaim_shadow",
            "Processing local undelegation execution"
        );

        let existing = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        let built_unbroadcast = resource_delegation_is_built_unbroadcast(&existing);
        if existing.last_broadcast_at.is_some() {
            return Ok(());
        }
        let affected = if built_unbroadcast {
            1
        } else {
            ApiResourceDelegationRepo::claim_build_slot(
                &self.ctx.api_resource_pool()?,
                &resource_trade_no,
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?
        };
        if affected == 0 {
            info!(
                resource_trade_no = %resource_trade_no,
                source = "local_resource_reclaim_shadow",
                "Local undelegation execution was already claimed or completed"
            );
            return Ok(());
        }

        let delegation = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if delegation.source != ApiResourceDelegationSource::Local
            || delegation.operation_type != ApiResourceDelegationOperationType::Undelegate
        {
            return Err(ServiceError::Parameter(format!(
                "local resource reclaim requires source=Local + Undelegate, got source={:?} operation={:?}",
                delegation.source, delegation.operation_type
            )));
        }

        let tx_hash = execute_resource_delegation(
            self.ctx,
            &self.ctx.api_resource_pool()?,
            &delegation,
            "local_resource_delegation_undelegate",
        )
        .await?;
        self.schedule_local_undelegation_post_broadcast_confirms(&resource_trade_no, &tx_hash);

        Ok(())
    }

    async fn process_local_undelegation_recover(
        &self,
        resource_trade_no: String,
    ) -> Result<(), ServiceError> {
        info!(
            resource_trade_no = %resource_trade_no,
            source = "local_resource_reclaim_shadow",
            "Processing local undelegation recover"
        );

        let delegation = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            &resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

        if delegation.source != ApiResourceDelegationSource::Local
            || delegation.operation_type != ApiResourceDelegationOperationType::Undelegate
        {
            return Ok(());
        }

        if delegation.finished_at.is_some() || delegation.err_code.is_some() {
            info!(
                resource_trade_no = %resource_trade_no,
                origin_trade_no = %Self::origin_trade_no(&delegation),
                finished_at = ?delegation.finished_at,
                err_code = ?delegation.err_code,
                source = "local_resource_reclaim_shadow",
                "Local undelegation already reached a terminal fact, skipping recover"
            );
            return Ok(());
        }

        if resource_delegation_is_built_unbroadcast(&delegation) {
            let tx_hash = execute_resource_delegation(
                self.ctx,
                &self.ctx.api_resource_pool()?,
                &delegation,
                "local_resource_delegation_undelegate",
            )
            .await?;
            self.schedule_local_undelegation_post_broadcast_confirms(&resource_trade_no, &tx_hash);
            return Ok(());
        }

        let tx_hash =
            delegation.tx_hash.as_deref().filter(|s| !s.trim().is_empty()).ok_or_else(|| {
                ServiceError::Parameter("local undelegation recover requires tx_hash".to_string())
            })?;

        match ApiTransDomain::process_recovered_tx(
            self.ctx,
            &delegation.chain_code,
            &delegation.owner_address,
            tx_hash,
            0,
            "0",
        )
        .await
        {
            Ok(Some(resp)) => {
                let payload = format!("local_undelegation_recovered:{}", resp.tx_hash);
                let transaction_time = resp
                    .transaction_time_ms
                    .and_then(|millis| {
                        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis as i64)
                    })
                    .map(|time| time.to_rfc3339());
                ApiResourceDelegationRepo::mark_recovered_chain_success(
                    &self.ctx.api_resource_pool()?,
                    &resource_trade_no,
                    tx_hash,
                    transaction_time.as_deref(),
                    &payload,
                )
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
                info!(
                    resource_trade_no = %resource_trade_no,
                    origin_trade_no = %Self::origin_trade_no(&delegation),
                    tx_hash = %tx_hash,
                    source = "local_resource_reclaim_shadow",
                    "Local undelegation recovered as success"
                );
                Ok(())
            }
            Ok(None) => {
                let rebroadcast = record_resource_uncertain_and_maybe_rebroadcast(
                    &self.ctx.api_resource_pool()?,
                    &resource_trade_no,
                )
                .await?;
                if rebroadcast {
                    return Ok(());
                }
                Ok(())
            }
            Err(err) if resource_recovery_error_is_uncertain(&err) => {
                record_resource_uncertain_and_maybe_rebroadcast(
                    &self.ctx.api_resource_pool()?,
                    &resource_trade_no,
                )
                .await?;
                Ok(())
            }
            Err(err) if Self::is_transaction_expired_error(&err) => {
                self.schedule_local_undelegation_rebuild_retry(&resource_trade_no, &err).await
            }
            Err(err) => {
                let recovery_context = if delegation.result_payload.as_deref()
                    == Some("historical_chain_reconciliation_pending")
                {
                    "historical_chain_reconciliation_confirmed_failure:local_undelegation"
                } else {
                    "local_undelegation_recovered_failure"
                };
                mark_recovered_resource_delegation_failure(
                    &self.ctx.api_resource_pool()?,
                    &delegation,
                    tx_hash,
                    &err,
                    recovery_context,
                )
                .await?;
                Ok(())
            }
        }
    }

    fn is_transaction_expired_error(err: &ServiceError) -> bool {
        err.to_string().contains("Transaction expired")
    }

    async fn schedule_local_undelegation_rebuild_retry(
        &self,
        resource_trade_no: &str,
        err: &ServiceError,
    ) -> Result<(), ServiceError> {
        let task = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        ApiResourceDelegationRepo::reset_for_rebuild(
            &self.ctx.api_resource_pool()?,
            resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        info!(
            resource_trade_no = %resource_trade_no,
            origin_trade_no = %Self::origin_trade_no(&task),
            error = %err,
            source = "local_resource_reclaim_shadow",
            "Local undelegation reset for rebuild"
        );
        Ok(())
    }
}

pub struct LocalResourceReclaimDispatcherActor {
    worker: LocalResourceReclaimWorker,
    intent_rx: mpsc::Receiver<LocalResourceReclaimIntent>,
    shutdown_rx: broadcast::Receiver<()>,
    running: Arc<DashSet<String>>,
    semaphore: Arc<Semaphore>,
}

impl LocalResourceReclaimDispatcherActor {
    pub fn new(
        worker: LocalResourceReclaimWorker,
        intent_rx: mpsc::Receiver<LocalResourceReclaimIntent>,
        shutdown_rx: broadcast::Receiver<()>,
        config: LocalResourceReclaimDispatcherConfig,
    ) -> Self {
        Self {
            worker,
            intent_rx,
            shutdown_rx,
            running: Arc::new(DashSet::new()),
            semaphore: Arc::new(Semaphore::new(config.concurrency)),
        }
    }

    pub async fn run(mut self) {
        crate::infrastructure::system_ready::wait_system_ready().await;
        info!("Local resource reclaim shadow dispatcher actor running");

        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    info!("Received shutdown signal for local resource reclaim dispatcher actor");
                    break;
                }
                Some(intent) = self.intent_rx.recv() => {
                    let resource_trade_no = intent.resource_trade_no().to_string();
                    if !self.running.insert(resource_trade_no.clone()) {
                        info!(resource_trade_no, "Local resource reclaim task already running; skipping duplicate intent");
                        continue;
                    }

                    let worker = self.worker.clone();
                    let running = self.running.clone();
                    let semaphore = self.semaphore.clone();
                    tokio::spawn(async move {
                        let permit = semaphore.acquire_owned().await;
                        let Ok(_permit) = permit else {
                            running.remove(&resource_trade_no);
                            return;
                        };

                        if let Err(e) = worker.handle(intent).await {
                            error!(resource_trade_no, error = %e, "Failed to handle local resource reclaim intent");
                        }
                        running.remove(&resource_trade_no);
                    });
                }
                else => {
                    info!("Local resource reclaim intent channel closed");
                    break;
                }
            }
        }

        info!("Local resource reclaim shadow dispatcher actor stopped");
    }
}

#[derive(Debug)]
pub struct LocalResourceReclaimShadowActorSystem {
    shutdown_tx: broadcast::Sender<()>,
    intent_tx: mpsc::Sender<LocalResourceReclaimIntent>,
    scanner_handle: Option<tokio::task::JoinHandle<()>>,
    dispatcher_handle: Option<tokio::task::JoinHandle<()>>,
}

impl LocalResourceReclaimShadowActorSystem {
    pub fn new(ctx: &'static crate::context::Context) -> Result<Self, ServiceError> {
        let api_resource_pool = ctx.api_resource_pool()?;
        let api_transaction_pool = ctx.api_transaction_pool()?;
        let (shutdown_tx, shutdown_rx1) = broadcast::channel(1);
        let shutdown_rx2 = shutdown_tx.subscribe();
        let (intent_tx, intent_rx) = mpsc::channel(100);
        *local_resource_reclaim_intent_tx()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(intent_tx.clone());

        let scanner = Arc::new(LocalResourceReclaimScanner::with_transaction_pool_and_config(
            api_resource_pool.clone(),
            api_transaction_pool,
            LocalResourceReclaimScannerConfig::default(),
        ));
        let worker = LocalResourceReclaimWorker::new(ctx);
        let dispatcher_config = LocalResourceReclaimDispatcherConfig::default();

        info!(
            scan_interval_secs = scanner.config.scan_interval.as_secs(),
            max_items_per_scan = scanner.config.max_items_per_scan,
            dispatcher_concurrency = dispatcher_config.concurrency,
            "Local resource reclaim shadow runtime config"
        );

        let scanner_clone = scanner.clone();
        let intent_tx_clone = intent_tx.clone();
        tokio::spawn(async move {
            for intent in scanner_clone.scan_round().await {
                if let Err(e) = intent_tx_clone.send(intent).await {
                    error!(error = %e, "Failed to enqueue warm local resource reclaim intent");
                    break;
                }
            }
            info!("Warm local resource reclaim shadow scan completed");
        });

        let scanner_actor =
            LocalResourceReclaimScannerActor::new(scanner.clone(), intent_tx.clone(), shutdown_rx1);
        let scanner_handle = Some(tokio::spawn(async move {
            scanner_actor.run().await;
        }));

        let dispatcher_actor = LocalResourceReclaimDispatcherActor::new(
            worker,
            intent_rx,
            shutdown_rx2,
            dispatcher_config,
        );
        let dispatcher_handle = Some(tokio::spawn(async move {
            dispatcher_actor.run().await;
        }));

        Ok(Self { shutdown_tx, intent_tx, scanner_handle, dispatcher_handle })
    }

    pub async fn stop(&mut self) {
        info!("Stopping local resource reclaim shadow system");

        let _ = self.shutdown_tx.send(());

        {
            let mut registered_tx = local_resource_reclaim_intent_tx()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if registered_tx.as_ref().is_some_and(|tx| tx.same_channel(&self.intent_tx)) {
                *registered_tx = None;
            }
        }

        if let Some(handle) = self.scanner_handle.take() {
            if let Err(err) = handle.await {
                warn!(error = %err, "local resource reclaim scanner join failed during stop");
            }
        }

        if let Some(handle) = self.dispatcher_handle.take() {
            if let Err(err) = handle.await {
                warn!(error = %err, "local resource reclaim dispatcher join failed during stop");
            }
        }

        info!("Local resource reclaim shadow system stopped");
    }
}

pub(crate) async fn init(
    ctx: &'static crate::context::Context,
) -> Result<LocalResourceReclaimShadowActorSystem, ServiceError> {
    LocalResourceReclaimShadowActorSystem::new(ctx)
}

pub async fn scan_and_process_once(
    ctx: &'static crate::context::Context,
) -> Result<(), ServiceError> {
    let api_resource_pool = ctx.api_resource_pool()?;
    let scanner = LocalResourceReclaimScanner::with_transaction_pool_and_config(
        api_resource_pool.clone(),
        ctx.api_transaction_pool()?,
        LocalResourceReclaimScannerConfig::default(),
    );
    let worker = LocalResourceReclaimWorker::new(ctx);

    for intent in scanner.scan_round().await {
        worker.handle(intent).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_reclaim_execute_and_recover_share_running_key() {
        let execute = LocalResourceReclaimIntent::ExecuteLocalUndelegation(
            "rsc_local_undelegate_C_SAME".to_string(),
        );
        let recover = LocalResourceReclaimIntent::RecoverLocalUndelegation(
            "rsc_local_undelegate_C_SAME".to_string(),
        );
        let other = LocalResourceReclaimIntent::ExecuteLocalUndelegation(
            "rsc_local_undelegate_C_OTHER".to_string(),
        );

        assert_eq!(execute.resource_trade_no(), recover.resource_trade_no());
        assert_ne!(execute.resource_trade_no(), other.resource_trade_no());
    }
    use wallet_database::{
        SqliteContext, entities::api_resource_delegation::NewApiResourceDelegation,
    };

    async fn test_ctx() -> &'static crate::context::Context {
        crate::testkit::context::api_trans_test_ctx().await
    }

    async fn transaction_pool_with_collect(
        db_root: &str,
        trade_no: &str,
        finished: bool,
    ) -> ApiTransactionDbPool {
        let pool = SqliteContext::new(db_root, Some("api_transaction.db"))
            .await
            .expect("init api_transaction.db")
            .into_transaction_db_pool()
            .expect("transaction pool");
        sqlx::query(
            r#"
            INSERT INTO api_collect (
                uid, name, from_addr, to_addr, value, validate, chain_code,
                trade_no, trade_type, status, finished_at, created_at
            ) VALUES (?, 'api_wallet', 'from', 'to', '1', '0', 'tron', ?, 2, ?, ?,
                strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            "#,
        )
        .bind("uid")
        .bind(trade_no)
        .bind(if finished { 9_i64 } else { 0_i64 })
        .bind(finished.then(|| "2026-08-01T10:00:00Z".to_string()))
        .execute(pool.write_ref())
        .await
        .expect("insert collect fact");
        pool
    }

    #[tokio::test]
    async fn scanner_owns_only_local_undelegation_execute_and_recover() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");

        ApiResourceDelegationRepo::upsert(
            &pool,
            NewApiResourceDelegation::local_undelegate(
                "uid",
                "rsc_local_undelegate_execute",
                "C_EXECUTE",
                ApiTradeType::Collect as i64,
                "owner",
                "receiver",
                "5",
                "1000",
            ),
        )
        .await
        .expect("insert executable local undelegate");

        ApiResourceDelegationRepo::upsert(
            &pool,
            NewApiResourceDelegation::local_undelegate(
                "uid",
                "rsc_local_undelegate_recover",
                "C_RECOVER",
                ApiTradeType::Collect as i64,
                "owner",
                "receiver",
                "5",
                "1000",
            ),
        )
        .await
        .expect("insert recoverable local undelegate");
        ApiResourceDelegationRepo::claim_build_slot(&pool, "rsc_local_undelegate_recover")
            .await
            .expect("claim build slot");
        ApiResourceDelegationRepo::mark_broadcast_success(
            &pool,
            "rsc_local_undelegate_recover",
            "tx_hash_recover",
        )
        .await
        .expect("mark broadcast");
        let scanner = LocalResourceReclaimScanner::with_config(
            pool,
            LocalResourceReclaimScannerConfig {
                scan_interval: Duration::from_secs(60),
                max_items_per_scan: 8,
            },
        );

        let intents = scanner.scan_round().await;
        assert!(intents.iter().any(|intent| matches!(
            intent,
            LocalResourceReclaimIntent::ExecuteLocalUndelegation(trade_no)
                if trade_no == "rsc_local_undelegate_execute"
        )));
        assert!(intents.iter().any(|intent| matches!(
            intent,
            LocalResourceReclaimIntent::RecoverLocalUndelegation(trade_no)
                if trade_no == "rsc_local_undelegate_recover"
        )));
    }

    #[tokio::test]
    async fn scanner_reopens_historical_terminal_undelegations_for_chain_reconciliation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");

        for (trade_no, err_code, err_msg) in [
            ("rsc_local_undelegate_timeout", "ERR_6005", "rpc timeout"),
            ("rsc_local_undelegate_chain_failed", "ERR_6008", "chain execution failed"),
        ] {
            ApiResourceDelegationRepo::upsert(
                &pool,
                NewApiResourceDelegation::local_undelegate(
                    "uid",
                    trade_no,
                    &format!("C_{trade_no}"),
                    ApiTradeType::Collect as i64,
                    "owner",
                    "receiver",
                    "5",
                    "1000",
                ),
            )
            .await
            .expect("insert local undelegate");
            ApiResourceDelegationRepo::claim_build_slot(&pool, trade_no)
                .await
                .expect("claim build slot");
            let tx_hash = format!("tx_hash_{trade_no}");
            ApiResourceDelegationRepo::mark_built(&pool, trade_no, &tx_hash, "signed_raw")
                .await
                .expect("mark built");
            ApiResourceDelegationRepo::mark_broadcasted(&pool, trade_no, &tx_hash)
                .await
                .expect("mark broadcasted");
            ApiResourceDelegationRepo::mark_recovered_chain_failure(
                &pool, trade_no, &tx_hash, None, err_code, err_msg, err_msg,
            )
            .await
            .expect("mark historical failure");
        }

        let scanner = LocalResourceReclaimScanner::with_config(
            pool.clone(),
            LocalResourceReclaimScannerConfig {
                scan_interval: Duration::from_secs(60),
                max_items_per_scan: 8,
            },
        );
        let intents = scanner.scan_round().await;

        assert!(intents.iter().any(|intent| matches!(
            intent,
            LocalResourceReclaimIntent::RecoverLocalUndelegation(trade_no)
                if trade_no == "rsc_local_undelegate_timeout"
        )));
        assert!(intents.iter().any(|intent| matches!(
            intent,
            LocalResourceReclaimIntent::RecoverLocalUndelegation(trade_no)
                if trade_no == "rsc_local_undelegate_chain_failed"
        )));
        let reopened = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &pool,
            "rsc_local_undelegate_timeout",
        )
        .await
        .expect("load reopened task");
        assert!(reopened.finished_at.is_none());
        assert!(reopened.chain_failed_at.is_none());
        assert_eq!(reopened.tx_hash.as_deref(), Some("tx_hash_rsc_local_undelegate_timeout"));
    }

    #[tokio::test]
    async fn scanner_serializes_concurrent_scan_rounds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");
        let scanner = LocalResourceReclaimScanner::with_config(
            pool,
            LocalResourceReclaimScannerConfig {
                scan_interval: Duration::from_secs(60),
                max_items_per_scan: 8,
            },
        );
        let held_round = scanner.scan_guard.lock().await;
        let waiting_scanner = scanner.clone();
        let waiting_round = tokio::spawn(async move { waiting_scanner.scan_round().await });

        tokio::task::yield_now().await;
        assert!(!waiting_round.is_finished());

        drop(held_round);
        tokio::time::timeout(Duration::from_secs(1), waiting_round)
            .await
            .expect("waiting scan should run after the active round exits")
            .expect("waiting scan task should not panic");
    }

    #[tokio::test]
    async fn local_undelegation_rebuild_resets_execution_facts() {
        let ctx = test_ctx().await;
        let pool = ctx.api_resource_pool().expect("resource pool");
        let worker = LocalResourceReclaimWorker::new(ctx);

        ApiResourceDelegationRepo::upsert(
            &pool,
            NewApiResourceDelegation::local_undelegate(
                "uid",
                "rsc_local_undelegate_retry",
                "C_LOCAL_UNDELEGATE_RETRY",
                ApiTradeType::Collect as i64,
                "withdraw_owner",
                "receiver",
                "5",
                "1000",
            ),
        )
        .await
        .expect("insert local undelegate");
        ApiResourceDelegationRepo::mark_broadcast_success(
            &pool,
            "rsc_local_undelegate_retry",
            "tx_hash_retry",
        )
        .await
        .expect("mark broadcast");

        worker
            .schedule_local_undelegation_rebuild_retry(
                "rsc_local_undelegate_retry",
                &ServiceError::System(SystemError::Internal("recover failed".to_string())),
            )
            .await
            .expect("schedule retry");

        let persisted = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &pool,
            "rsc_local_undelegate_retry",
        )
        .await
        .expect("load task");
        assert_eq!(persisted.tx_hash, None);
    }

    #[tokio::test]
    async fn local_undelegation_missing_relationship_is_idempotent_success() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");

        ApiResourceDelegationRepo::upsert(
            &pool,
            NewApiResourceDelegation::local_undelegate(
                "uid",
                "rsc_local_undelegate_terminal_validation",
                "C_LOCAL_UNDELEGATE_TERMINAL",
                ApiTradeType::Collect as i64,
                "owner",
                "receiver",
                "5",
                "1000",
            ),
        )
        .await
        .expect("insert local undelegate");

        LocalResourceReclaimWorker::mark_local_undelegation_already_reclaimed_in_pool(
            &pool,
            "rsc_local_undelegate_terminal_validation",
            &ServiceError::ChainInteract(wallet_chain_interact::Error::RpcNode(
                "contract validation error: delegated Resource does not exist".to_string(),
            )),
        )
        .await
        .expect("record idempotent success");

        let got = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &pool,
            "rsc_local_undelegate_terminal_validation",
        )
        .await
        .expect("load local undelegate");
        assert_eq!(got.err_code, None);
        assert_eq!(got.err_msg, None);
        assert_eq!(got.result_status, Some(ApiResourceDelegationResultStatus::Success));
        assert!(got.finished_at.is_some());
        assert!(
            got.result_payload
                .as_deref()
                .unwrap_or_default()
                .contains("local_undelegation_already_reclaimed")
        );

        let scanner = LocalResourceReclaimScanner::with_config(
            pool,
            LocalResourceReclaimScannerConfig {
                scan_interval: Duration::from_secs(60),
                max_items_per_scan: 8,
            },
        );
        let intents = scanner.scan_round().await;
        assert!(!intents.iter().any(|intent| matches!(
            intent,
            LocalResourceReclaimIntent::ExecuteLocalUndelegation(trade_no)
                if trade_no == "rsc_local_undelegate_terminal_validation"
        )));
    }

    #[test]
    fn local_undelegation_insufficient_balance_remains_retryable() {
        let err = ServiceError::ChainInteract(wallet_chain_interact::Error::RpcNode(
            "contract validation error: insufficient delegateFrozenBalance".to_string(),
        ));

        assert!(!LocalResourceReclaimWorker::is_local_undelegation_already_reclaimed_error(&err));
    }

    #[test]
    fn local_undelegation_post_broadcast_confirm_delays_cover_short_window() {
        assert_eq!(
            LocalResourceReclaimWorker::LOCAL_UNDELEGATION_POST_BROADCAST_CONFIRM_DELAYS_SECS,
            [3, 8, 15]
        );
    }

    #[tokio::test]
    async fn local_undelegation_fast_path_enqueues_exact_task() {
        let (intent_tx, mut intent_rx) = mpsc::channel(1);
        let previous_tx = {
            let mut registered_tx = local_resource_reclaim_intent_tx()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            registered_tx.replace(intent_tx)
        };

        assert!(trigger_local_undelegation_fast_path("rsc_local_undelegate_C_FAST_PATH"));
        assert!(matches!(
            intent_rx.try_recv(),
            Ok(LocalResourceReclaimIntent::ExecuteLocalUndelegation(trade_no))
                if trade_no == "rsc_local_undelegate_C_FAST_PATH"
        ));

        *local_resource_reclaim_intent_tx()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = previous_tx;
    }

    #[test]
    fn local_undelegation_treats_transaction_expired_as_rebuildable() {
        let expired = ServiceError::System(SystemError::Internal(
            "Chain interact error: `rpc error Transaction expired`".to_string(),
        ));
        let other = ServiceError::System(SystemError::Internal(
            "Chain interact error: `broadcasted tx failed`".to_string(),
        ));

        assert!(LocalResourceReclaimWorker::is_transaction_expired_error(&expired));
        assert!(!LocalResourceReclaimWorker::is_transaction_expired_error(&other));
    }

    #[tokio::test]
    async fn scanner_reopens_execute_immediately_after_rebuild_reset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");

        ApiResourceDelegationRepo::upsert(
            &pool,
            NewApiResourceDelegation::local_undelegate(
                "uid",
                "rsc_local_undelegate_future_retry",
                "C_FUTURE_RETRY",
                ApiTradeType::Collect as i64,
                "owner",
                "receiver",
                "5",
                "1000",
            ),
        )
        .await
        .expect("insert local undelegate");

        ApiResourceDelegationRepo::reset_for_rebuild(&pool, "rsc_local_undelegate_future_retry")
            .await
            .expect("reset for rebuild");

        let scanner = LocalResourceReclaimScanner::with_config(
            pool.clone(),
            LocalResourceReclaimScannerConfig {
                scan_interval: Duration::from_secs(60),
                max_items_per_scan: 8,
            },
        );
        let intents = scanner.scan_round().await;
        assert!(intents.iter().any(|intent| matches!(
            intent,
            LocalResourceReclaimIntent::ExecuteLocalUndelegation(trade_no)
                if trade_no == "rsc_local_undelegate_future_retry"
        )));
    }

    #[tokio::test]
    async fn scanner_reconciles_missing_local_undelegation_after_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");
        let transaction_pool = transaction_pool_with_collect(&db_root, "C_RESTART", true).await;

        ApiResourceDelegationRepo::upsert(
            &pool,
            NewApiResourceDelegation::local_delegate(
                "uid",
                "rsc_local_delegate_C_RESTART",
                "C_RESTART",
                ApiTradeType::Collect as i64,
                "owner",
                "receiver",
                "5",
                "1000",
            ),
        )
        .await
        .expect("insert local delegate");
        ApiResourceDelegationRepo::mark_result_received(
            &pool,
            "rsc_local_delegate_C_RESTART",
            ApiResourceDelegationResultStatus::Success,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("mark delegate success");

        let scanner = LocalResourceReclaimScanner::with_transaction_pool_and_config(
            pool.clone(),
            transaction_pool,
            LocalResourceReclaimScannerConfig {
                scan_interval: Duration::from_secs(60),
                max_items_per_scan: 8,
            },
        );
        let intents = scanner.scan_round().await;

        assert!(intents.iter().any(|intent| matches!(
            intent,
            LocalResourceReclaimIntent::ExecuteLocalUndelegation(trade_no)
                if trade_no == "rsc_local_undelegate_C_RESTART"
        )));
        let undelegate = ApiResourceDelegationRepo::get_by_resource_trade_no(
            &pool,
            "rsc_local_undelegate_C_RESTART",
        )
        .await
        .expect("load reconciled undelegate");
        assert_eq!(undelegate.origin_trade_no.as_deref(), Some("C_RESTART"));
        assert_eq!(undelegate.source, ApiResourceDelegationSource::Local);
        assert_eq!(undelegate.operation_type, ApiResourceDelegationOperationType::Undelegate);
    }

    #[tokio::test]
    async fn scanner_blocks_existing_undelegation_until_collect_is_terminal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");
        let transaction_pool =
            transaction_pool_with_collect(&db_root, "C_WAIT_RECLAIM", false).await;

        for resource_trade_no in ["rsc_wait_execute", "rsc_wait_recover"] {
            ApiResourceDelegationRepo::upsert(
                &pool,
                NewApiResourceDelegation::local_undelegate(
                    "uid",
                    resource_trade_no,
                    "C_WAIT_RECLAIM",
                    ApiTradeType::Collect as i64,
                    "owner",
                    "receiver",
                    "5",
                    "1000",
                ),
            )
            .await
            .expect("insert local undelegate");
        }
        ApiResourceDelegationRepo::claim_build_slot(&pool, "rsc_wait_recover")
            .await
            .expect("claim recovery build");
        ApiResourceDelegationRepo::mark_built(
            &pool,
            "rsc_wait_recover",
            "tx_hash_wait_recover",
            "signed_raw",
        )
        .await
        .expect("mark recovery built");
        ApiResourceDelegationRepo::mark_broadcasted(
            &pool,
            "rsc_wait_recover",
            "tx_hash_wait_recover",
        )
        .await
        .expect("mark recovery broadcasted");

        let scanner = LocalResourceReclaimScanner::with_transaction_pool_and_config(
            pool,
            transaction_pool.clone(),
            LocalResourceReclaimScannerConfig {
                scan_interval: Duration::from_secs(60),
                max_items_per_scan: 8,
            },
        );
        assert!(scanner.scan_round().await.is_empty());

        sqlx::query(
            "UPDATE api_collect SET finished_at = '2026-08-01T10:00:00Z' WHERE trade_no = ?",
        )
        .bind("C_WAIT_RECLAIM")
        .execute(transaction_pool.write_ref())
        .await
        .expect("finish collect");

        let intents = scanner.scan_round().await;
        assert!(intents.iter().any(|intent| matches!(
            intent,
            LocalResourceReclaimIntent::ExecuteLocalUndelegation(trade_no)
                if trade_no == "rsc_wait_execute"
        )));
        assert!(intents.iter().any(|intent| matches!(
            intent,
            LocalResourceReclaimIntent::RecoverLocalUndelegation(trade_no)
                if trade_no == "rsc_wait_recover"
        )));
    }

    #[tokio::test]
    async fn scanner_reconciles_terminal_origin_beyond_unfinished_first_page() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");
        let transaction_pool =
            transaction_pool_with_collect(&db_root, "C_UNFINISHED_PAGE_0", false).await;

        for (trade_no, finished) in [("C_UNFINISHED_PAGE_1", false), ("C_TERMINAL_PAGE_2", true)] {
            sqlx::query(
                r#"
                INSERT INTO api_collect (
                    uid, name, from_addr, to_addr, value, validate, chain_code,
                    trade_no, trade_type, status, finished_at, created_at
                ) VALUES (?, 'api_wallet', 'from', 'to', '1', '0', 'tron', ?, 2, ?, ?,
                    strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                "#,
            )
            .bind("uid")
            .bind(trade_no)
            .bind(if finished { 9_i64 } else { 0_i64 })
            .bind(finished.then(|| "2026-08-01T10:00:00Z".to_string()))
            .execute(transaction_pool.write_ref())
            .await
            .expect("insert collect fact");
        }

        for origin_trade_no in ["C_UNFINISHED_PAGE_0", "C_UNFINISHED_PAGE_1", "C_TERMINAL_PAGE_2"] {
            let resource_trade_no = format!("rsc_local_delegate_{origin_trade_no}");
            ApiResourceDelegationRepo::upsert(
                &pool,
                NewApiResourceDelegation::local_delegate(
                    "uid",
                    resource_trade_no.clone(),
                    origin_trade_no,
                    ApiTradeType::Collect as i64,
                    "owner",
                    "receiver",
                    "5",
                    "1000",
                ),
            )
            .await
            .expect("insert local delegate");
            ApiResourceDelegationRepo::mark_result_received(
                &pool,
                &resource_trade_no,
                ApiResourceDelegationResultStatus::Success,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("mark delegate success");
        }

        let scanner = LocalResourceReclaimScanner::with_transaction_pool_and_config(
            pool.clone(),
            transaction_pool,
            LocalResourceReclaimScannerConfig {
                scan_interval: Duration::from_secs(60),
                max_items_per_scan: 2,
            },
        );
        let intents = scanner.scan_round().await;

        assert!(intents.iter().any(|intent| matches!(
            intent,
            LocalResourceReclaimIntent::ExecuteLocalUndelegation(trade_no)
                if trade_no == "rsc_local_undelegate_C_TERMINAL_PAGE_2"
        )));
        for origin_trade_no in ["C_UNFINISHED_PAGE_0", "C_UNFINISHED_PAGE_1"] {
            assert!(
                ApiResourceDelegationRepo::find_by_resource_trade_no(
                    &pool,
                    &format!("rsc_local_undelegate_{origin_trade_no}"),
                )
                .await
                .expect("query undelegation")
                .is_none()
            );
        }
    }

    #[tokio::test]
    async fn scanner_does_not_reconcile_before_origin_collect_finishes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");
        let transaction_pool =
            transaction_pool_with_collect(&db_root, "C_NOT_FINISHED", false).await;

        ApiResourceDelegationRepo::upsert(
            &pool,
            NewApiResourceDelegation::local_delegate(
                "uid",
                "rsc_local_delegate_C_NOT_FINISHED",
                "C_NOT_FINISHED",
                ApiTradeType::Collect as i64,
                "owner",
                "receiver",
                "5",
                "1000",
            ),
        )
        .await
        .expect("insert local delegate");
        ApiResourceDelegationRepo::mark_result_received(
            &pool,
            "rsc_local_delegate_C_NOT_FINISHED",
            ApiResourceDelegationResultStatus::Success,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("mark delegate success");

        let scanner = LocalResourceReclaimScanner::with_transaction_pool_and_config(
            pool.clone(),
            transaction_pool,
            LocalResourceReclaimScannerConfig {
                scan_interval: Duration::from_secs(60),
                max_items_per_scan: 8,
            },
        );

        let intents = scanner.scan_round().await;

        assert!(intents.is_empty());
        assert!(
            ApiResourceDelegationRepo::find_by_resource_trade_no(
                &pool,
                "rsc_local_undelegate_C_NOT_FINISHED",
            )
            .await
            .expect("query undelegation")
            .is_none()
        );
    }

    #[tokio::test]
    async fn local_undelegation_reconciliation_bypasses_full_execute_page() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await
            .expect("init api_resource.db")
            .into_api_resource_db_pool()
            .expect("resource pool");
        let transaction_pool = transaction_pool_with_collect(&db_root, "C_NOT_STARVED", true).await;

        for index in 0..20 {
            ApiResourceDelegationRepo::upsert(
                &pool,
                NewApiResourceDelegation::local_undelegate(
                    "uid",
                    format!("rsc_local_undelegate_old_{index:02}"),
                    format!("C_OLD_{index:02}"),
                    ApiTradeType::Collect as i64,
                    "owner",
                    "receiver",
                    "5",
                    "1000",
                ),
            )
            .await
            .expect("insert old local undelegate");
        }

        ApiResourceDelegationRepo::upsert(
            &pool,
            NewApiResourceDelegation::local_delegate(
                "uid",
                "rsc_local_delegate_C_NOT_STARVED",
                "C_NOT_STARVED",
                ApiTradeType::Collect as i64,
                "owner",
                "receiver",
                "5",
                "1000",
            ),
        )
        .await
        .expect("insert local delegate");
        ApiResourceDelegationRepo::mark_result_received(
            &pool,
            "rsc_local_delegate_C_NOT_STARVED",
            ApiResourceDelegationResultStatus::Success,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("mark delegate success");

        let scanner = LocalResourceReclaimScanner::with_transaction_pool_and_config(
            pool,
            transaction_pool,
            LocalResourceReclaimScannerConfig {
                scan_interval: Duration::from_secs(60),
                max_items_per_scan: 20,
            },
        );
        let intents = scanner.scan_round().await;

        assert!(intents.iter().any(|intent| matches!(
            intent,
            LocalResourceReclaimIntent::ExecuteLocalUndelegation(trade_no)
                if trade_no == "rsc_local_undelegate_C_NOT_STARVED"
        )));
    }
}
