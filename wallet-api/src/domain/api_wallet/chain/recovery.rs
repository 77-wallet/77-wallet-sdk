//! A-scoped queue ownership and retry metadata; no other task retry policy changes.
use crate::{
    context::Context, error::service::ServiceError,
    infrastructure::task_queue::backend::BackendApiTaskData,
};
use sha2::{Digest, Sha256};
use wallet_database::{
    entities::task_queue::{KnownTaskName, TaskName, TaskQueueEntity},
    repositories::{api_wallet::chain_recovery::ChainRecoveryRepo, task_queue::TaskQueueRepo},
};
use wallet_transport_backend::{
    consts::endpoint::api_wallet::ADDRESS_INIT, request::api_wallet::address::ApiAddressInitReq,
};

/// Log only a digest even for legacy/user-supplied task or batch identifiers.
pub(crate) fn log_id(id: &str) -> String {
    format!("{:x}", Sha256::digest(id.as_bytes()))
}

/// No dynamic error messages: backend responses can echo addresses or credentials.
pub(crate) fn log_reason(error: &ServiceError) -> (&'static str, Option<i64>) {
    match error {
        ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(code, _)) => {
            ("backend", Some(*code))
        }
        ServiceError::TransportBackend(wallet_transport_backend::Error::Timeout)
        | ServiceError::Timeout => ("timeout", None),
        ServiceError::TransportBackend(wallet_transport_backend::Error::RateLimited) => {
            ("rate_limit", None)
        }
        ServiceError::TransportBackend(_) | ServiceError::Transport(_) => ("transport", None),
        ServiceError::Database(_) => ("database", None),
        ServiceError::System(crate::error::system::SystemError::Service(message))
            if message == "A registration timed out" =>
        {
            ("registration_timeout", None)
        }
        ServiceError::System(crate::error::system::SystemError::Service(message))
            if message == "A derivation deferred until wallet unlock" =>
        {
            ("wallet_locked", None)
        }
        ServiceError::System(crate::error::system::SystemError::Service(message))
            if message.contains("at capacity; retry later") =>
        {
            ("queue_capacity", None)
        }
        ServiceError::System(_) => ("system", None),
        _ => ("service", None),
    }
}

pub(crate) const OWNER: &str = wallet_database::repositories::api_wallet::chain_recovery::OWNER;
pub(crate) const MAX_ACTIVE: i64 =
    wallet_database::repositories::api_wallet::chain_recovery::MAX_ACTIVE;
pub(crate) const DUE_PAGE_SIZE: u32 = 100;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct RetryState {
    pub owner: String,
    pub attempts: u32,
    pub next_retry_at_ms: i64,
    #[serde(default)]
    pub replaced_by_children: bool,
}
impl RetryState {
    pub(crate) fn fresh() -> Self {
        Self {
            owner: OWNER.to_owned(),
            attempts: 0,
            next_retry_at_ms: 0,
            replaced_by_children: false,
        }
    }
    pub(crate) fn parse(row: &TaskQueueEntity) -> Option<Self> {
        let state: Self = serde_json::from_str(row.remark.as_deref()?).ok()?;
        (state.owner == OWNER).then_some(state)
    }
    pub(crate) fn eligible(&self, now_ms: i64) -> bool {
        !self.replaced_by_children && self.next_retry_at_ms <= now_ms
    }
}

/// Persist before dispatch. A stable backend ID does not use the MQTT-ID builder.
pub(crate) async fn enqueue_registration(
    ctx: &'static Context,
    request: ApiAddressInitReq,
) -> Result<(), ServiceError> {
    if request.address_list.0.is_empty() {
        return Ok(());
    }
    let row = registration_row(&request)?;
    let id = row.id.clone();
    let now = chrono::Utc::now();
    let pool = ctx.task_pool()?;
    if TaskQueueRepo::task_detail(&pool, &id).await?.is_none() {
        TaskQueueRepo::insert_owned_with_capacity(&pool, &row, OWNER, MAX_ACTIVE).await?;
        if TaskQueueRepo::task_detail(&pool, &id).await?.is_none() {
            return Err(crate::error::system::SystemError::Service(
                "A registration queue at capacity; retry later".to_owned(),
            )
            .into());
        }
    }
    if let Some(saved) = TaskQueueRepo::task_detail(&pool, &id).await? {
        if RetryState::parse(&saved).is_some_and(|state| state.replaced_by_children) {
            let mut left = request.clone();
            let mut right = ApiAddressInitReq::new(left.epoch);
            right.batch_id = left.batch_id.clone();
            right.address_list.0 = left.address_list.0.split_off(left.address_list.0.len() / 2);
            Box::pin(enqueue_registration(ctx, left)).await?;
            Box::pin(enqueue_registration(ctx, right)).await?;
            return Ok(());
        } else {
            ChainRecoveryRepo::rearm(
                &pool,
                &id,
                TaskName::Known(KnownTaskName::BackendApi),
                row.remark.as_deref().expect("owned row"),
                0,
            )
            .await?;
        }
    }
    let saved = TaskQueueRepo::task_detail(&pool, &id).await?;
    if let Some(saved) = saved {
        if saved.status == 2
            && !RetryState::parse(&saved).is_some_and(|state| state.replaced_by_children)
        {
            return Err(crate::error::system::SystemError::Service(
                "A registration rearm at capacity; retry later".into(),
            )
            .into());
        }
        if saved.status != 2
            && RetryState::parse(&saved).is_some_and(|state| state.eligible(now.timestamp_millis()))
        {
            ctx.api_chain_recovery.notify.notify_one();
        }
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ReconcileChain {
    pub epoch: u64,
    pub chain_code: String,
}

pub(crate) fn retry_delay_ms(attempt: u32) -> i64 {
    // A-only continuous recovery: capped exponential delay, never ten-try termination.
    2_000_i64.saturating_mul(1_i64 << attempt.saturating_sub(1).min(9)).min(600_000)
}

pub(crate) async fn defer(
    ctx: &'static Context,
    row: &TaskQueueEntity,
    error: &ServiceError,
    now_ms: i64,
) -> Result<(), ServiceError> {
    let pool = ctx.task_pool()?;
    let Some(latest) = TaskQueueRepo::task_detail(&pool, &row.id).await? else {
        return Ok(());
    };
    if latest.status == 2 {
        return Ok(());
    }
    let Some(mut state) = RetryState::parse(&latest) else {
        return Ok(());
    };
    state.attempts = state.attempts.saturating_add(1);
    state.next_retry_at_ms = now_ms.saturating_add(retry_delay_ms(state.attempts));
    let message = match error {
        ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(
            code,
            message,
        )) => format!("backend code {code}: {}", message.as_deref().unwrap_or("no message")),
        _ => error.to_string(),
    };
    let changed = TaskQueueRepo::defer_owned(
        &pool,
        &row.id,
        OWNER,
        &wallet_utils::serde_func::serde_to_string(&state)?,
        &message,
    )
    .await?;
    if changed > 0 {
        let (reason, backend_code) = log_reason(error);
        tracing::warn!(task_id_hash=%log_id(&row.id), reason, backend_code=?backend_code,
            attempt=state.attempts, next_retry_at_ms=state.next_retry_at_ms,
            "A retry persisted");
    }
    Ok(())
}

pub(crate) async fn schedule_chains(ctx: &'static Context) -> Result<(), ServiceError> {
    let api = ctx.api_wallet_pool()?;
    let epoch = crate::domain::app::config::ConfigDomain::get_keys_reset_epoch(ctx).await?;
    if epoch == 0 {
        return Ok(());
    }
    let task_pool = ctx.task_pool()?;
    let now = chrono::Utc::now();
    let state = wallet_utils::serde_func::serde_to_string(&RetryState::fresh())?;
    let mut rows = Vec::new();
    for chain in
        wallet_database::repositories::api_wallet::chain::ApiChainRepo::get_chain_list(&api).await?
    {
        let id = format!("api_chain_reconcile:{epoch}:{}", chain.chain_code);
        ChainRecoveryRepo::rearm(
            &task_pool,
            &id,
            TaskName::Unknown("ReconcileApiChainAccounts".into()),
            &state,
            3,
        )
        .await?;
        rows.push(TaskQueueEntity {
            id,
            task_name: TaskName::Unknown("ReconcileApiChainAccounts".into()),
            request_body: wallet_utils::serde_func::serde_to_string(&ReconcileChain {
                epoch,
                chain_code: chain.chain_code,
            })?,
            r#type: 1,
            status: 3,
            err_msg: None,
            remark: Some(state.clone()),
            created_at: now,
            updated_at: Some(now),
        });
    }
    TaskQueueRepo::insert_batch_task_ignore_conflict(&task_pool, &rows).await?;
    tracing::info!(epoch, chain_count = rows.len(), "A reconcile tasks persisted");
    ctx.api_chain_recovery.notify.notify_one();
    Ok(())
}

pub(crate) async fn run_reconcile(
    ctx: &'static Context,
    job: &ReconcileChain,
) -> Result<(), ServiceError> {
    if !crate::domain::app::config::ConfigDomain::check_epoch_validity(ctx, job.epoch).await? {
        return Ok(());
    }
    crate::domain::api_wallet::chain::ApiChainDomain::run_queued_chain_reconcile(
        ctx,
        &job.chain_code,
    )
    .await
}

fn registration_row(request: &ApiAddressInitReq) -> Result<TaskQueueEntity, ServiceError> {
    let data = BackendApiTaskData::new(ADDRESS_INIT, request)?;
    let body = wallet_utils::serde_func::serde_to_string(&data)?;
    let now = chrono::Utc::now();
    Ok(TaskQueueEntity {
        id: format!("api_chain_address:{:x}", Sha256::digest(body.as_bytes())),
        task_name: TaskName::Known(KnownTaskName::BackendApi),
        request_body: body,
        r#type: 1,
        status: 0,
        err_msg: None,
        remark: Some(wallet_utils::serde_func::serde_to_string(&RetryState::fresh())?),
        created_at: now,
        updated_at: Some(now),
    })
}

/// docs/reference/error_codes.md: 8050001 is the address-local "address not found" code.
/// Generic 400/422, 8020001 parameter, auth, rate and network errors are never split.
/// Only explicit address rejection is localized. Transport/service failures keep one batch.
/// Two children and replacement are committed together; maximum tree is 2*n-1 requests.
pub(crate) async fn isolate_rejected_batch(
    ctx: &'static Context,
    row: &TaskQueueEntity,
    error: &ServiceError,
) -> Result<bool, ServiceError> {
    if !address_rejection(error) {
        return Ok(false);
    }
    if row.task_name != TaskName::Known(KnownTaskName::BackendApi) {
        return Ok(false);
    }
    let data: BackendApiTaskData = wallet_utils::serde_func::serde_from_str(&row.request_body)?;
    if data.endpoint != ADDRESS_INIT {
        return Ok(false);
    }
    let mut left: ApiAddressInitReq = serde_json::from_value(data.body)
        .map_err(|e| crate::error::system::SystemError::Service(e.to_string()))?;
    if left.address_list.0.len() <= 1 {
        return Ok(false);
    }
    let mut right = ApiAddressInitReq::new(left.epoch);
    right.batch_id = left.batch_id.clone();
    right.address_list.0 = left.address_list.0.split_off(left.address_list.0.len() / 2);
    let children = [registration_row(&left)?, registration_row(&right)?];
    let mut state = RetryState::parse(row).expect("A-owned row");
    state.replaced_by_children = true;
    let pool = ctx.task_pool()?;
    let replaced = ChainRecoveryRepo::split(
        &pool,
        &row.id,
        &wallet_utils::serde_func::serde_to_string(&state)?,
        &children,
    )
    .await?;
    if replaced {
        tracing::info!(task_id_hash=%log_id(&row.id), left_count=left.address_list.0.len(),
            right_count=right.address_list.0.len(), backend_code=8050001,
            "A registration split persisted");
    }
    Ok(replaced)
}

fn address_rejection(error: &ServiceError) -> bool {
    matches!(
        error,
        ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(8050001, _))
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_metadata_does_not_echo_identifiers_or_error_messages() {
        // Arrange: a legacy ID and a backend response containing sensitive text.
        let secret = "wallet-address-and-credential";
        let error = ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(
            8050001,
            Some(secret.to_owned()),
        ));
        // Act / Assert: only fixed classification, numeric code and a digest survive.
        let digest = log_id(secret);
        assert_eq!(digest.len(), 64);
        assert!(!digest.contains(secret));
        assert_eq!(digest, log_id(secret));
        assert_eq!(log_reason(&error), ("backend", Some(8050001)));
        assert_eq!(log_reason(&ServiceError::Context(secret.into())), ("service", None));
        assert_eq!(
            log_reason(&ServiceError::System(crate::error::system::SystemError::Service(
                "A derivation deferred until wallet unlock".into()
            ))),
            ("wallet_locked", None)
        );
    }
    #[test]
    fn only_documented_address_errors_allow_localization() {
        for code in [400, 401, 403, 422, 429, 500, 503, 8020001, 8030001, 8660001] {
            assert!(
                !address_rejection(&ServiceError::TransportBackend(
                    wallet_transport_backend::Error::ApiBackend(code, Some("address error".into()))
                )),
                "{code}"
            );
        }
        assert!(address_rejection(&ServiceError::TransportBackend(
            wallet_transport_backend::Error::ApiBackend(8050001, None)
        )));
        assert!(!address_rejection(&ServiceError::TransportBackend(
            wallet_transport_backend::Error::Timeout
        )));
        assert!(!address_rejection(&ServiceError::System(
            crate::error::system::SystemError::Service("address error".into())
        )));
    }
    #[test]
    fn backoff_is_bounded_and_continues_past_ten_attempts() {
        assert_eq!(retry_delay_ms(1), 2000);
        assert_eq!(retry_delay_ms(2), 4000);
        for attempts in [10, 11, 12, u32::MAX] {
            assert_eq!(retry_delay_ms(attempts), 600_000);
        }
        let mut state = RetryState::fresh();
        state.attempts = 12;
        state.next_retry_at_ms = 123;
        assert!(!state.eligible(122));
        assert!(state.eligible(123));
        state.replaced_by_children = true;
        assert!(!state.eligible(i64::MAX));
    }
    #[tokio::test]
    async fn cancellation_drops_a_running_id_guard() {
        let tasks = std::sync::Arc::new(dashmap::DashSet::new());
        tasks.insert("a-running".to_owned());
        let guard = super::OwnedRunningGuard { tasks: tasks.clone(), id: "a-running".to_owned() };
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let job = tokio::spawn(async move {
            let _guard = guard;
            started_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        started_rx.await.unwrap();
        job.abort();
        assert!(job.await.unwrap_err().is_cancelled());
        assert!(tasks.is_empty(), "cancelled A jobs must be eligible for future wakeups");
    }
    #[tokio::test]
    async fn owned_panic_is_retryable_and_releases_only_its_outer_reservation() {
        let tasks = std::sync::Arc::new(dashmap::DashSet::new());
        tasks.insert("panicking".into());
        {
            let _reservation =
                super::OwnedRunningGuard { tasks: tasks.clone(), id: "panicking".into() };
            let error = super::owned_attempt(async {
                panic!("synthetic A execution panic");
                #[allow(unreachable_code)]
                Ok(())
            })
            .await
            .unwrap_err();
            assert!(error.to_string().contains("facts retained"));
            assert!(
                tasks.contains("panicking"),
                "reservation remains owned through post-attempt cleanup"
            );
        }
        assert!(tasks.is_empty());
    }
}

/// The only consumer of missing-chain recovery records. Context-local reservations
/// are shared by periodic wake and explicit test steps; no generic queue consumer runs them.
#[derive(Default)]
pub(crate) struct RecoveryCoordinator {
    running: std::sync::Arc<dashmap::DashSet<String>>,
    pub(crate) notify: std::sync::Arc<tokio::sync::Notify>,
    loop_handle: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}
struct OwnedRunningGuard {
    tasks: std::sync::Arc<dashmap::DashSet<String>>,
    id: String,
}
impl Drop for OwnedRunningGuard {
    fn drop(&mut self) {
        self.tasks.remove(&self.id);
    }
}
async fn owned_attempt(
    work: impl std::future::Future<Output = Result<(), ServiceError>>,
) -> Result<(), ServiceError> {
    use futures::FutureExt;
    std::panic::AssertUnwindSafe(work).catch_unwind().await.unwrap_or_else(|_| {
        Err(crate::error::system::SystemError::Service("A task panicked; facts retained".into())
            .into())
    })
}
impl RecoveryCoordinator {
    pub(crate) fn start(self: &std::sync::Arc<Self>, ctx: &'static Context) {
        let mut handle = self.loop_handle.lock().expect("chain recovery lifecycle mutex");
        if handle.as_ref().is_some_and(|h| !h.is_finished()) {
            return;
        }
        let coordinator = self.clone();
        *handle = Some(tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
            let mut jobs = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _=tick.tick()=>{},
                    _=coordinator.notify.notified()=>{},
                    _=jobs.join_next(),if !jobs.is_empty()=>{continue;}
                }
                let rows = match ctx.task_pool() {
                    Ok(pool) => {
                        ChainRecoveryRepo::due(
                            &pool,
                            chrono::Utc::now().timestamp_millis(),
                            DUE_PAGE_SIZE,
                        )
                        .await
                    }
                    Err(_error) => {
                        tracing::warn!(reason = "database", "A storage unavailable");
                        continue;
                    }
                };
                match rows {
                    Ok(rows) => {
                        for row in rows {
                            if jobs.len() >= 4 {
                                break;
                            }
                            if !coordinator.running.insert(row.id.clone()) {
                                continue;
                            }
                            let reservation = OwnedRunningGuard {
                                tasks: coordinator.running.clone(),
                                id: row.id.clone(),
                            };
                            jobs.spawn(async move {
                                let _reservation = reservation;
                                if let Err(error) = consume(ctx, &row.id).await {
                                    let (reason, backend_code) = log_reason(&error);
                                    tracing::warn!(task_id_hash=%log_id(&row.id),reason,backend_code=?backend_code,"A recovery attempt deferred");
                                }
                            });
                        }
                    }
                    Err(_error) => tracing::warn!(reason = "database", "A recovery wake deferred"),
                }
            }
        }));
    }
    #[cfg(any(test, feature = "integration-tests"))]
    pub(crate) fn stop_for_test(&self) {
        if let Some(handle) =
            self.loop_handle.lock().expect("chain recovery lifecycle mutex").take()
        {
            handle.abort();
        }
    }
    #[cfg(any(test, feature = "integration-tests"))]
    pub(crate) async fn run_saved_once_for_test(
        &self,
        ctx: &'static Context,
        id: &str,
    ) -> Result<(), ServiceError> {
        if !self.running.insert(id.into()) {
            return Ok(());
        }
        let _reservation = OwnedRunningGuard { tasks: self.running.clone(), id: id.into() };
        consume(ctx, id).await
    }
}
async fn consume(ctx: &'static Context, id: &str) -> Result<(), ServiceError> {
    let pool = ctx.task_pool()?;
    let Some(row) = TaskQueueRepo::task_detail(&pool, id).await? else {
        return Ok(());
    };
    let Some(state) = RetryState::parse(&row) else {
        return Ok(());
    };
    if row.status == 2 || !state.eligible(chrono::Utc::now().timestamp_millis()) {
        return Ok(());
    }
    let attempt = async {
        if !TaskQueueRepo::claim_owned_running(&pool, id, OWNER, MAX_ACTIVE).await? {
            return Err(crate::error::system::SystemError::Service(
                "A active queue at capacity; retry later".into(),
            )
            .into());
        }
        if row.task_name == TaskName::Known(KnownTaskName::BackendApi) {
            let data: BackendApiTaskData =
                wallet_utils::serde_func::serde_from_str(&row.request_body)?;
            if data.endpoint != ADDRESS_INIT {
                return Err(crate::error::system::SystemError::Service(
                    "Unexpected A recovery endpoint".into(),
                )
                .into());
            }
            let request: ApiAddressInitReq = serde_json::from_value(data.body)
                .map_err(|e| crate::error::system::SystemError::Service(e.to_string()))?;
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                crate::infrastructure::system_ready::wait_system_ready().await;
                crate::infrastructure::expand_init::executor::do_init_with_ctx(ctx, request).await
            })
            .await
            .map_err(|_| {
                crate::error::system::SystemError::Service("A registration timed out".into())
            })??;
        } else if row.task_name == TaskName::Unknown("ReconcileApiChainAccounts".into()) {
            let job: ReconcileChain = wallet_utils::serde_func::serde_from_str(&row.request_body)?;
            run_reconcile(ctx, &job).await?;
        } else {
            return Err(crate::error::system::SystemError::Service(
                "Unexpected A recovery record".into(),
            )
            .into());
        }
        TaskQueueRepo::task_done(&pool, id).await?;
        tracing::info!(task_id_hash=%log_id(id), "A task persisted done");
        Ok(())
    };
    if let Err(error) = owned_attempt(attempt).await {
        match isolate_rejected_batch(ctx, &row, &error).await {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(split_error) => {
                let (reason, backend_code) = log_reason(&split_error);
                tracing::warn!(task_id_hash=%log_id(&row.id),reason,backend_code=?backend_code,"A split deferred; original retained");
            }
        }
        defer(ctx, &row, &error, chrono::Utc::now().timestamp_millis()).await?;
    }
    Ok(())
}
