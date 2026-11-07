use crate::{context::Context, error::service::ServiceError};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::{
    collections::HashSet,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Chain RPC guard: shared concurrency limiter + per-endpoint circuit breaker.
///
/// Design goals:
/// - Multi-chain friendly: do NOT hardcode "TRON-only" semantics into module names or call-sites.
/// - Opt-in by RPC endpoint host allowlist, so production mainnet is not impacted unless configured.
/// - Best-effort: guard failures MUST NOT block business flows.
///
/// Configuration (env):
/// - `CHAIN_RPC_GUARD_HOSTS`: comma-separated hosts to guard (default: Nile public HTTP hosts)
/// - `CHAIN_RPC_GUARD_MAX_CONCURRENCY`: guarded-endpoint global concurrency (default: 6)
/// - `CHAIN_RPC_GUARD_ENDPOINT_MAX_CONCURRENCY`: per-endpoint concurrency
///   (default: global concurrency, never exceeds the global limit)
/// - `CHAIN_RPC_GUARD_ACQUIRE_TIMEOUT_MS`: max wait for a guarded permit (default: 30000)
/// - `CHAIN_RPC_GUARD_RPC_TIMEOUT_MS`: max guarded RPC await time (default: 60000)
const DEFAULT_GUARDED_HOSTS: &str = "api.nileex.io,nile.trongrid.io";
const DEFAULT_MAX_CONCURRENCY: usize = 6;
const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(60);
const CHAIN_HOST_CACHE_TTL: Duration = Duration::from_secs(60);
const TRANSIENT_503_REPORT_WINDOW_MS: u64 = 60_000;
const HTTP_STATUS_SERVICE_UNAVAILABLE: u16 = 503;
const DEFAULT_FAILURE_WINDOW: Duration = Duration::from_secs(10);
const DEFAULT_FAILURE_THRESHOLD: u32 = 5;
const DEFAULT_BREAKER_OPEN: Duration = Duration::from_secs(30);
const DEFAULT_503_FAILURE_THRESHOLD: u32 = 2;
const DEFAULT_503_BREAKER_OPEN: Duration = Duration::from_secs(60);
const DEFAULT_503_MAX_OPEN: Duration = Duration::from_secs(300);

static GUARDED_HOSTS: Lazy<HashSet<String>> = Lazy::new(|| {
    let raw =
        std::env::var("CHAIN_RPC_GUARD_HOSTS").unwrap_or_else(|_| DEFAULT_GUARDED_HOSTS.into());
    raw.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string()).collect()
});

fn max_concurrency() -> usize {
    std::env::var("CHAIN_RPC_GUARD_MAX_CONCURRENCY")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MAX_CONCURRENCY)
}

fn endpoint_max_concurrency_from(raw: Option<&str>, global_max: usize) -> usize {
    raw.and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(global_max)
        .min(global_max)
}

fn endpoint_max_concurrency() -> usize {
    endpoint_max_concurrency_from(
        std::env::var("CHAIN_RPC_GUARD_ENDPOINT_MAX_CONCURRENCY").ok().as_deref(),
        max_concurrency(),
    )
}

fn acquire_timeout() -> Duration {
    std::env::var("CHAIN_RPC_GUARD_ACQUIRE_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_ACQUIRE_TIMEOUT)
}

fn rpc_timeout() -> Duration {
    std::env::var("CHAIN_RPC_GUARD_RPC_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_RPC_TIMEOUT)
}

fn failure_window() -> Duration {
    DEFAULT_FAILURE_WINDOW
}

fn failure_threshold(error_code: Option<u16>) -> u32 {
    if error_code == Some(HTTP_STATUS_SERVICE_UNAVAILABLE) {
        DEFAULT_503_FAILURE_THRESHOLD
    } else {
        DEFAULT_FAILURE_THRESHOLD
    }
}

fn breaker_open_duration(error_code: Option<u16>, open_count: u32) -> Duration {
    if error_code != Some(HTTP_STATUS_SERVICE_UNAVAILABLE) {
        return DEFAULT_BREAKER_OPEN;
    }

    let multiplier = 1_u32 << open_count.saturating_sub(1).min(31);
    DEFAULT_503_BREAKER_OPEN.saturating_mul(multiplier).min(DEFAULT_503_MAX_OPEN)
}

static CHAIN_RPC_SEM: Lazy<Arc<Semaphore>> =
    Lazy::new(|| Arc::new(Semaphore::new(max_concurrency())));
static CHAIN_RPC_ENDPOINT_SEMS: Lazy<DashMap<String, Arc<Semaphore>>> = Lazy::new(DashMap::new);
static TRANSIENT_503_COUNT: AtomicU64 = AtomicU64::new(0);
static LAST_TRANSIENT_503_REPORT_MS: AtomicU64 = AtomicU64::new(0);

pub fn shared_chain_rpc_semaphore() -> Arc<Semaphore> {
    CHAIN_RPC_SEM.clone()
}

fn endpoint_semaphore(endpoint: &GuardedRpcEndpoint) -> Arc<Semaphore> {
    CHAIN_RPC_ENDPOINT_SEMS
        .entry(endpoint.key())
        .or_insert_with(|| Arc::new(Semaphore::new(endpoint_max_concurrency())))
        .clone()
}

pub struct ChainRpcGuardPermit {
    host: String,
    _global_permit: OwnedSemaphorePermit,
    _endpoint_permit: OwnedSemaphorePermit,
    acquired_at: std::time::Instant,
}

impl ChainRpcGuardPermit {
    fn new(
        host: String,
        global_permit: OwnedSemaphorePermit,
        endpoint_permit: OwnedSemaphorePermit,
    ) -> Self {
        Self {
            host,
            _global_permit: global_permit,
            _endpoint_permit: endpoint_permit,
            acquired_at: std::time::Instant::now(),
        }
    }
}

impl Drop for ChainRpcGuardPermit {
    fn drop(&mut self) {
        tracing::debug!(
            host = %self.host,
            held = ?self.acquired_at.elapsed(),
            "released chain rpc guard permit"
        );
    }
}

pub enum ChainRpcGuardAcquire {
    NotGuarded,
    Acquired { host: String, permit: ChainRpcGuardPermit, waited: Duration },
    TimedOut { host: String, timeout: Duration },
    Closed { host: String },
}

pub enum ChainRpcGuardAttempt {
    Proceed(Option<ChainRpcGuardPermit>),
    Skip,
}

pub enum ChainRpcBreakerAttempt {
    Proceed(ChainRpcHalfOpenProbe),
    Skip,
}

pub struct ChainRpcHalfOpenProbe {
    breaker: Arc<RpcCircuitBreaker>,
    owns_probe: bool,
}

impl Drop for ChainRpcHalfOpenProbe {
    fn drop(&mut self) {
        if self.owns_probe {
            self.breaker.release_half_open_probe();
        }
    }
}

#[derive(Default)]
pub struct RpcCircuitBreaker {
    open_until_ms: AtomicU64,
    consecutive_failures: AtomicU32,
    last_failure_ms: AtomicU64,
    last_reported_open_until_ms: AtomicU64,
    last_error_code: AtomicU32,
    open_count: AtomicU32,
    half_open_probe: AtomicBool,
}

enum RpcCircuitBreakerAdmission {
    Closed,
    HalfOpen,
    Open(Duration),
}

impl RpcCircuitBreaker {
    fn now_ms() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::from_secs(0)).as_millis()
            as u64
    }

    pub fn is_open(&self) -> bool {
        let now = Self::now_ms();
        now < self.open_until_ms.load(Ordering::Relaxed)
    }

    fn try_admit(&self) -> RpcCircuitBreakerAdmission {
        let now = Self::now_ms();
        let until = self.open_until_ms.load(Ordering::Acquire);
        if now < until {
            return RpcCircuitBreakerAdmission::Open(Duration::from_millis(until - now));
        }

        if until != 0 {
            return match self.half_open_probe.compare_exchange(
                false,
                true,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => RpcCircuitBreakerAdmission::HalfOpen,
                Err(_) => RpcCircuitBreakerAdmission::Open(Duration::from_millis(0)),
            };
        }

        RpcCircuitBreakerAdmission::Closed
    }

    fn release_half_open_probe(&self) {
        self.half_open_probe.store(false, Ordering::Release);
    }

    pub fn remaining(&self) -> Option<Duration> {
        let now = Self::now_ms();
        let until = self.open_until_ms.load(Ordering::Relaxed);
        if now >= until { None } else { Some(Duration::from_millis(until - now)) }
    }

    pub fn last_error_code(&self) -> Option<u16> {
        let code = self.last_error_code.load(Ordering::Relaxed);
        if code == 0 { None } else { Some(code as u16) }
    }

    pub fn record_success(&self) {
        // A request admitted before the breaker opened may finish later. Its
        // success must not close the breaker while the endpoint is still in
        // the cooldown window.
        if self.is_open() {
            return;
        }
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.last_failure_ms.store(0, Ordering::Relaxed);
        self.last_error_code.store(0, Ordering::Relaxed);
        self.open_count.store(0, Ordering::Relaxed);
        self.half_open_probe.store(false, Ordering::Release);
        // 成功后立即清空 open_until，避免熔断窗口继续阻塞后续请求。
        self.open_until_ms.store(0, Ordering::Relaxed);
        let last_reported = self.last_reported_open_until_ms.swap(0, Ordering::Relaxed);
        if last_reported != 0 {
            tracing::info!("chain rpc circuit breaker closed");
        }
    }

    pub fn record_failure(&self, error_code: Option<u16>) {
        // Do not let failures from already-admitted in-flight requests keep
        // moving the cooldown window. A half-open probe reaches this method
        // only after open_until has elapsed, so its failure is still counted.
        if self.is_open() {
            return;
        }
        self.half_open_probe.store(false, Ordering::Release);
        if let Some(code) = error_code {
            self.last_error_code.store(code as u32, Ordering::Relaxed);
        }
        let now = Self::now_ms();
        let last = self.last_failure_ms.swap(now, Ordering::Relaxed);

        let mut streak =
            if last == 0 || now.saturating_sub(last) > failure_window().as_millis() as u64 {
                1
            } else {
                self.consecutive_failures.load(Ordering::Relaxed).saturating_add(1)
            };
        if streak > 1000 {
            streak = 1000;
        }
        self.consecutive_failures.store(streak, Ordering::Relaxed);

        let threshold = failure_threshold(error_code);
        if streak >= threshold {
            let open_count = self.open_count.fetch_add(1, Ordering::Relaxed).saturating_add(1);
            let open_for = breaker_open_duration(error_code, open_count);
            let new_until = now.saturating_add(open_for.as_millis() as u64);
            self.open_until_ms.store(new_until, Ordering::Release);

            let prev_reported = self.last_reported_open_until_ms.swap(new_until, Ordering::Relaxed);
            if prev_reported == 0 || prev_reported < now {
                tracing::warn!(
                    error_code = ?error_code,
                    streak = streak,
                    threshold = threshold,
                    open_count = open_count,
                    open_for = ?open_for,
                    "chain rpc circuit breaker opened"
                );
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GuardedRpcEndpoint {
    network: String,
    chain_code: String,
    host: String,
}

impl GuardedRpcEndpoint {
    fn key(&self) -> String {
        format!("{}:{}:{}", self.network, self.chain_code, self.host)
    }
}

static BREAKERS: Lazy<DashMap<String, Arc<RpcCircuitBreaker>>> = Lazy::new(DashMap::new);

fn breaker_for_endpoint(endpoint: &GuardedRpcEndpoint) -> Arc<RpcCircuitBreaker> {
    BREAKERS.entry(endpoint.key()).or_insert_with(|| Arc::new(RpcCircuitBreaker::default())).clone()
}

fn breaker_for_host(host: &str) -> Arc<RpcCircuitBreaker> {
    breaker_for_endpoint(&GuardedRpcEndpoint {
        network: "unknown".to_string(),
        chain_code: "unknown".to_string(),
        host: host.to_string(),
    })
}

#[derive(Clone)]
struct ChainHostCacheEntry {
    endpoint: Option<GuardedRpcEndpoint>,
    fetched_at_ms: u64,
}

static CHAIN_HOST_CACHE: Lazy<DashMap<String, ChainHostCacheEntry>> = Lazy::new(DashMap::new);

fn host_from_rpc_url(rpc_url: &str) -> Option<String> {
    // Best-effort parsing:
    // - strip scheme if any
    // - take authority until first '/'
    // - strip port if present
    let without_scheme = rpc_url
        .strip_prefix("https://")
        .or_else(|| rpc_url.strip_prefix("http://"))
        .unwrap_or(rpc_url);
    let authority = without_scheme.split('/').next().unwrap_or(without_scheme);
    let host = authority.split('@').last().unwrap_or(authority);
    let host = host.strip_prefix('[').unwrap_or(host);
    let host = host.split(']').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.trim();
    if host.is_empty() { None } else { Some(host.to_string()) }
}

fn is_guarded_host(host: &str) -> bool {
    GUARDED_HOSTS.contains(host)
}

async fn fetch_chain_rpc_endpoint(chain_code: &str, ctx: &Context) -> Option<GuardedRpcEndpoint> {
    let core_pool = ctx.core_pool().ok()?;
    let api_pool = ctx.api_wallet_pool().ok()?;

    let ensurer = crate::infrastructure::chain_node::chain_node_ensurer::ChainNodeEnsurer::new(
        core_pool, api_pool,
    );
    let chain_with_node = ensurer.ensure_and_get_api_chain_with_node(chain_code).await.ok()?;
    Some(GuardedRpcEndpoint {
        network: chain_with_node.get_network().to_ascii_lowercase(),
        chain_code: chain_code.to_string(),
        host: host_from_rpc_url(&chain_with_node.rpc_url)?,
    })
}

async fn guarded_endpoint_for_chain_code(
    chain_code: &str,
    ctx: &Context,
) -> Option<GuardedRpcEndpoint> {
    let now = RpcCircuitBreaker::now_ms();
    if let Some(entry) = CHAIN_HOST_CACHE.get(chain_code) {
        if now.saturating_sub(entry.fetched_at_ms) <= CHAIN_HOST_CACHE_TTL.as_millis() as u64 {
            return entry.endpoint.clone().filter(|endpoint| is_guarded_host(&endpoint.host));
        }
    }

    let endpoint = fetch_chain_rpc_endpoint(chain_code, ctx).await;
    CHAIN_HOST_CACHE.insert(
        chain_code.to_string(),
        ChainHostCacheEntry { endpoint: endpoint.clone(), fetched_at_ms: now },
    );
    endpoint.filter(|endpoint| is_guarded_host(&endpoint.host))
}

pub async fn breaker_open_for_chain_code_with_ctx(
    ctx: &Context,
    chain_code: &str,
) -> Option<(String, Duration)> {
    breaker_open_detail_for_chain_code_with_ctx(ctx, chain_code)
        .await
        .map(|detail| (detail.host, detail.remaining))
}

pub struct ChainRpcBreakerOpenDetail {
    pub network: String,
    pub chain_code: String,
    pub host: String,
    pub remaining: Duration,
    pub last_error_code: Option<u16>,
}

pub async fn breaker_open_detail_for_chain_code_with_ctx(
    ctx: &Context,
    chain_code: &str,
) -> Option<ChainRpcBreakerOpenDetail> {
    let endpoint = guarded_endpoint_for_chain_code(chain_code, ctx).await?;
    let breaker = breaker_for_endpoint(&endpoint);
    if breaker.is_open() {
        let remaining = breaker.remaining().unwrap_or(Duration::from_secs(0));
        Some(ChainRpcBreakerOpenDetail {
            network: endpoint.network,
            chain_code: endpoint.chain_code,
            host: endpoint.host,
            remaining,
            last_error_code: breaker.last_error_code(),
        })
    } else {
        None
    }
}

pub async fn acquire_if_guarded_with_ctx(
    ctx: &Context,
    chain_code: &str,
) -> Option<ChainRpcGuardPermit> {
    let endpoint = guarded_endpoint_for_chain_code(chain_code, ctx).await?;
    let endpoint_permit = match endpoint_semaphore(&endpoint).acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            tracing::error!(host = %endpoint.host, "chain rpc endpoint semaphore closed; skip guard acquire");
            return None;
        }
    };
    let global_permit = match shared_chain_rpc_semaphore().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            tracing::error!(host = %endpoint.host, "chain rpc semaphore closed; skip guard acquire");
            return None;
        }
    };
    Some(ChainRpcGuardPermit::new(endpoint.host, global_permit, endpoint_permit))
}

async fn acquire_guarded_host_permit(
    host: String,
    endpoint_sem: Arc<Semaphore>,
    global_sem: Arc<Semaphore>,
    timeout: Duration,
) -> ChainRpcGuardAcquire {
    let started_at = std::time::Instant::now();
    tracing::debug!(host = %host, timeout = ?timeout, "waiting for chain rpc guard permit");
    let permits = tokio::time::timeout(timeout, async move {
        let endpoint_permit = endpoint_sem.acquire_owned().await.map_err(|_| ())?;
        let global_permit = global_sem.acquire_owned().await.map_err(|_| ())?;
        Ok::<_, ()>((global_permit, endpoint_permit))
    })
    .await;
    match permits {
        Ok(Ok((global_permit, endpoint_permit))) => {
            let waited = started_at.elapsed();
            ChainRpcGuardAcquire::Acquired {
                host: host.clone(),
                permit: ChainRpcGuardPermit::new(host, global_permit, endpoint_permit),
                waited,
            }
        }
        Ok(Err(())) => {
            tracing::error!(host = %host, "chain rpc semaphore closed; skip guard acquire");
            ChainRpcGuardAcquire::Closed { host }
        }
        Err(_) => {
            tracing::warn!(
                observability_event = "chain_rpc_guard_acquire_timeout",
                reason = "guard_wait_timeout",
                host = %host,
                timeout = ?timeout,
                waited = ?started_at.elapsed(),
                "chain rpc guard permit wait timed out"
            );
            ChainRpcGuardAcquire::TimedOut { host, timeout }
        }
    }
}

pub async fn acquire_if_guarded_with_ctx_timeout(
    ctx: &Context,
    chain_code: &str,
) -> ChainRpcGuardAcquire {
    let Some(endpoint) = guarded_endpoint_for_chain_code(chain_code, ctx).await else {
        return ChainRpcGuardAcquire::NotGuarded;
    };
    let host = endpoint.host.clone();
    acquire_guarded_host_permit(
        host,
        endpoint_semaphore(&endpoint),
        shared_chain_rpc_semaphore(),
        acquire_timeout(),
    )
    .await
}

pub async fn acquire_for_attempt_with_ctx(
    ctx: &Context,
    chain_code: &str,
    trade_no: &str,
    action: &str,
    source: &str,
) -> ChainRpcGuardAttempt {
    match acquire_if_guarded_with_ctx_timeout(ctx, chain_code).await {
        ChainRpcGuardAcquire::Acquired { host, permit, waited } => {
            tracing::info!(
                trade_no = %trade_no,
                chain_code = %chain_code,
                action = %action,
                host = %host,
                waited = ?waited,
                source = %source,
                "Acquired chain rpc guard permit"
            );
            ChainRpcGuardAttempt::Proceed(Some(permit))
        }
        ChainRpcGuardAcquire::TimedOut { host, timeout } => {
            tracing::warn!(
                observability_event = "chain_rpc_guard_acquire_timeout",
                reason = "guard_wait_timeout",
                trade_no = %trade_no,
                chain_code = %chain_code,
                action = %action,
                host = %host,
                timeout = ?timeout,
                source = %source,
                "Skip chain action: chain rpc guard permit wait timed out; scanner will retry"
            );
            ChainRpcGuardAttempt::Skip
        }
        ChainRpcGuardAcquire::Closed { host } => {
            tracing::warn!(
                observability_event = "chain_rpc_call_timeout",
                reason = "rpc_execution_timeout",
                trade_no = %trade_no,
                chain_code = %chain_code,
                action = %action,
                host = %host,
                source = %source,
                "Chain rpc guard closed; continue without permit"
            );
            ChainRpcGuardAttempt::Proceed(None)
        }
        ChainRpcGuardAcquire::NotGuarded => ChainRpcGuardAttempt::Proceed(None),
    }
}

pub async fn admit_breaker_for_attempt_with_ctx(
    ctx: &Context,
    chain_code: &str,
    trade_no: &str,
    action: &str,
    source: &str,
) -> ChainRpcBreakerAttempt {
    let Some(endpoint) = guarded_endpoint_for_chain_code(chain_code, ctx).await else {
        return ChainRpcBreakerAttempt::Proceed(ChainRpcHalfOpenProbe {
            breaker: Arc::new(RpcCircuitBreaker::default()),
            owns_probe: false,
        });
    };
    let breaker = breaker_for_endpoint(&endpoint);
    match breaker.try_admit() {
        RpcCircuitBreakerAdmission::Open(remaining) => {
            tracing::warn!(
                reason = "rpc_circuit_breaker_open",
                trade_no = %trade_no,
                chain_code = %chain_code,
                network = %endpoint.network,
                action = %action,
                host = %endpoint.host,
                remaining = ?remaining,
                last_error_code = ?breaker.last_error_code(),
                source = %source,
                "Skip chain rpc: circuit breaker open"
            );
            ChainRpcBreakerAttempt::Skip
        }
        RpcCircuitBreakerAdmission::HalfOpen => {
            tracing::info!(
                reason = "rpc_circuit_breaker_half_open_probe",
                trade_no = %trade_no,
                chain_code = %chain_code,
                network = %endpoint.network,
                action = %action,
                host = %endpoint.host,
                source = %source,
                "Admit one chain rpc half-open probe"
            );
            ChainRpcBreakerAttempt::Proceed(ChainRpcHalfOpenProbe { breaker, owns_probe: true })
        }
        RpcCircuitBreakerAdmission::Closed => {
            ChainRpcBreakerAttempt::Proceed(ChainRpcHalfOpenProbe { breaker, owns_probe: false })
        }
    }
}

pub async fn run_with_rpc_timeout<F, T>(
    future: F,
    chain_code: &str,
    trade_no: &str,
    action: &str,
    source: &str,
) -> Result<T, tokio::time::error::Elapsed>
where
    F: Future<Output = T>,
{
    run_with_rpc_timeout_duration(future, rpc_timeout(), chain_code, trade_no, action, source).await
}

pub async fn run_guarded_with_ctx<T, Fut>(
    ctx: &Context,
    chain_code: &str,
    trade_no: &str,
    action: &str,
    source: &str,
    future: Fut,
) -> Result<Option<T>, ServiceError>
where
    Fut: Future<Output = Result<T, ServiceError>>,
{
    run_guarded_with_ctx_and_log(
        ctx,
        chain_code,
        trade_no,
        action,
        source,
        ChainRpcFailureLog::new()
            .trade_no(trade_no)
            .chain_code(chain_code)
            .source(source)
            .op(action),
        future,
    )
    .await
}

pub async fn run_guarded_with_ctx_and_log<T, Fut>(
    ctx: &Context,
    chain_code: &str,
    trade_no: &str,
    action: &str,
    source: &str,
    failure_log: ChainRpcFailureLog<'_>,
    future: Fut,
) -> Result<Option<T>, ServiceError>
where
    Fut: Future<Output = Result<T, ServiceError>>,
{
    match run_guarded_with_ctx_and_log_classified(
        ctx,
        chain_code,
        trade_no,
        action,
        source,
        failure_log,
        future,
    )
    .await?
    {
        GuardedCallOutcome::Executed(value) => Ok(Some(value)),
        GuardedCallOutcome::SkippedBeforeExecution | GuardedCallOutcome::ExecutionTimedOut => {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardedCallOutcome<T> {
    Executed(T),
    SkippedBeforeExecution,
    ExecutionTimedOut,
}

pub async fn run_guarded_with_ctx_and_log_classified<T, Fut>(
    ctx: &Context,
    chain_code: &str,
    trade_no: &str,
    action: &str,
    source: &str,
    failure_log: ChainRpcFailureLog<'_>,
    future: Fut,
) -> Result<GuardedCallOutcome<T>, ServiceError>
where
    Fut: Future<Output = Result<T, ServiceError>>,
{
    let _guard = match acquire_for_attempt_with_ctx(ctx, chain_code, trade_no, action, source).await
    {
        ChainRpcGuardAttempt::Proceed(permit) => permit,
        ChainRpcGuardAttempt::Skip => return Ok(GuardedCallOutcome::SkippedBeforeExecution),
    };

    let _breaker_probe =
        match admit_breaker_for_attempt_with_ctx(ctx, chain_code, trade_no, action, source).await {
            ChainRpcBreakerAttempt::Proceed(probe) => probe,
            ChainRpcBreakerAttempt::Skip => {
                return Ok(GuardedCallOutcome::SkippedBeforeExecution);
            }
        };

    let result = match run_with_rpc_timeout(future, chain_code, trade_no, action, source).await {
        Ok(result) => result,
        Err(_) => {
            failure_log.timeout_skip("rpc_execution_timeout");
            return Ok(GuardedCallOutcome::ExecutionTimedOut);
        }
    };

    match result {
        Ok(value) => {
            record_success_for_chain_code_with_ctx(ctx, chain_code).await;
            Ok(GuardedCallOutcome::Executed(value))
        }
        Err(err) => {
            let rpc_error = ChainRpcErrorContext::from_display(&err);
            failure_log.warn(&rpc_error, "guarded chain rpc call failed");
            rpc_error.record_transient_failure_for_chain_code_with_ctx(ctx, chain_code).await;
            if let Some(endpoint) = guarded_endpoint_for_chain_code(chain_code, ctx).await {
                breaker_for_endpoint(&endpoint).release_half_open_probe();
            }
            Err(err)
        }
    }
}

pub async fn run_guarded_raw_with_ctx_and_log<T, Fut>(
    ctx: &Context,
    chain_code: &str,
    trade_no: &str,
    action: &str,
    source: &str,
    failure_log: ChainRpcFailureLog<'_>,
    future: Fut,
) -> Result<Option<T>, ServiceError>
where
    Fut: Future<Output = T>,
{
    let _guard = match acquire_for_attempt_with_ctx(ctx, chain_code, trade_no, action, source).await
    {
        ChainRpcGuardAttempt::Proceed(permit) => permit,
        ChainRpcGuardAttempt::Skip => return Ok(None),
    };

    // Breaker admission must happen after semaphore waiting. Otherwise a burst
    // admitted while closed can keep hitting the endpoint after queued requests
    // ahead of it open the breaker.
    let _breaker_probe =
        match admit_breaker_for_attempt_with_ctx(ctx, chain_code, trade_no, action, source).await {
            ChainRpcBreakerAttempt::Proceed(probe) => probe,
            ChainRpcBreakerAttempt::Skip => return Ok(None),
        };

    match run_with_rpc_timeout(future, chain_code, trade_no, action, source).await {
        Ok(value) => Ok(Some(value)),
        Err(_) => {
            failure_log.timeout_skip("rpc_execution_timeout");
            Ok(None)
        }
    }
}

async fn run_with_rpc_timeout_duration<F, T>(
    future: F,
    timeout: Duration,
    chain_code: &str,
    trade_no: &str,
    action: &str,
    source: &str,
) -> Result<T, tokio::time::error::Elapsed>
where
    F: Future<Output = T>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => Ok(result),
        Err(err) => {
            tracing::warn!(
                trade_no = %trade_no,
                chain_code = %chain_code,
                action = %action,
                timeout = ?timeout,
                source = %source,
                "chain rpc call timed out"
            );
            Err(err)
        }
    }
}

pub async fn record_success_for_chain_code_with_ctx(ctx: &Context, chain_code: &str) {
    let Some(endpoint) = guarded_endpoint_for_chain_code(chain_code, ctx).await else { return };
    breaker_for_endpoint(&endpoint).record_success();
}

fn maybe_report_transient_503_count(now_ms: u64) {
    let last = LAST_TRANSIENT_503_REPORT_MS.load(Ordering::Relaxed);
    if last == 0 {
        let _ = LAST_TRANSIENT_503_REPORT_MS.compare_exchange(
            0,
            now_ms,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        return;
    }

    if now_ms.saturating_sub(last) < TRANSIENT_503_REPORT_WINDOW_MS {
        return;
    }

    if LAST_TRANSIENT_503_REPORT_MS
        .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        let count = TRANSIENT_503_COUNT.swap(0, Ordering::Relaxed);
        tracing::info!(count = count, "chain rpc code=503 count in last minute");
    }
}

pub fn record_transient_failure_from_error<E: std::fmt::Display>(err: &E) {
    let s = err.to_string();
    record_transient_failure_from_message(&s);
}

pub fn record_transient_failure_from_message(msg: &str) {
    let classification = classify_chain_rpc_error_message(msg);
    record_transient_failure_from_classification(&classification);
}

pub fn record_transient_failure_from_classification(classification: &ChainRpcErrorClassification) {
    if classification.is_service_unavailable {
        TRANSIENT_503_COUNT.fetch_add(1, Ordering::Relaxed);
        maybe_report_transient_503_count(RpcCircuitBreaker::now_ms());
    }

    if !classification.is_transient {
        return;
    }
    let Some(host) = classification.guarded_host.as_deref() else { return };
    breaker_for_host(host).record_failure(classification.error_code);
}

async fn record_transient_failure_for_chain_code_with_ctx(
    ctx: &Context,
    chain_code: &str,
    classification: &ChainRpcErrorClassification,
) {
    if !classification.is_transient {
        return;
    }

    if classification.is_service_unavailable {
        TRANSIENT_503_COUNT.fetch_add(1, Ordering::Relaxed);
        maybe_report_transient_503_count(RpcCircuitBreaker::now_ms());
    }

    if let Some(endpoint) = guarded_endpoint_for_chain_code(chain_code, ctx).await {
        breaker_for_endpoint(&endpoint).record_failure(classification.error_code);
    } else {
        if let Some(host) = classification.guarded_host.as_deref() {
            breaker_for_host(host).record_failure(classification.error_code);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainRpcErrorContext {
    message: String,
    classification: ChainRpcErrorClassification,
}

impl ChainRpcErrorContext {
    pub fn from_display<E: std::fmt::Display>(err: &E) -> Self {
        Self::from_message(err.to_string())
    }

    pub fn from_message(message: impl Into<String>) -> Self {
        let message = message.into();
        let classification = classify_chain_rpc_error_message(&message);
        Self { message, classification }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn classification(&self) -> &ChainRpcErrorClassification {
        &self.classification
    }

    pub fn error_code(&self) -> Option<u16> {
        self.classification.error_code
    }

    pub fn guarded_host(&self) -> Option<&str> {
        self.classification.guarded_host.as_deref()
    }

    pub fn transient_reason(&self) -> Option<&'static str> {
        self.classification.transient_reason
    }

    pub fn is_service_unavailable(&self) -> bool {
        self.classification.is_service_unavailable
    }

    pub fn is_transient(&self) -> bool {
        self.classification.is_transient
    }

    pub fn record_transient_failure(&self) {
        record_transient_failure_from_classification(&self.classification);
    }

    pub async fn record_transient_failure_for_chain_code_with_ctx(
        &self,
        ctx: &Context,
        chain_code: &str,
    ) {
        record_transient_failure_for_chain_code_with_ctx(ctx, chain_code, &self.classification)
            .await;
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ChainRpcFailureLog<'a> {
    chain_code: Option<&'a str>,
    rpc: Option<&'a str>,
    trade_no: Option<&'a str>,
    tx_hash: Option<&'a str>,
    source: Option<&'a str>,
    op: Option<&'a str>,
    visibility_kind: Option<&'a str>,
    address: Option<&'a str>,
    symbol: Option<&'a str>,
    token: Option<&'a str>,
    exec_to_addr: Option<&'a str>,
    attempt: Option<u32>,
    delay_ms: Option<u64>,
}

impl<'a> ChainRpcFailureLog<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn chain_code(mut self, chain_code: &'a str) -> Self {
        self.chain_code = Some(chain_code);
        self
    }

    pub fn rpc(mut self, rpc: &'a str) -> Self {
        self.rpc = Some(rpc);
        self
    }

    pub fn trade_no(mut self, trade_no: &'a str) -> Self {
        self.trade_no = Some(trade_no);
        self
    }

    pub fn tx_hash(mut self, tx_hash: &'a str) -> Self {
        self.tx_hash = Some(tx_hash);
        self
    }

    pub fn source(mut self, source: &'a str) -> Self {
        self.source = Some(source);
        self
    }

    pub fn op(mut self, op: &'a str) -> Self {
        self.op = Some(op);
        self
    }

    pub fn visibility_kind(mut self, visibility_kind: &'a str) -> Self {
        self.visibility_kind = Some(visibility_kind);
        self
    }

    pub fn address(mut self, address: &'a str) -> Self {
        self.address = Some(address);
        self
    }

    pub fn symbol(mut self, symbol: &'a str) -> Self {
        self.symbol = Some(symbol);
        self
    }

    pub fn token(mut self, token: &'a str) -> Self {
        self.token = Some(token);
        self
    }

    pub fn exec_to_addr(mut self, exec_to_addr: &'a str) -> Self {
        self.exec_to_addr = Some(exec_to_addr);
        self
    }

    pub fn attempt(mut self, attempt: u32) -> Self {
        self.attempt = Some(attempt);
        self
    }

    pub fn delay_ms(mut self, delay_ms: u64) -> Self {
        self.delay_ms = Some(delay_ms);
        self
    }

    pub fn warn(self, error: &ChainRpcErrorContext, reason: &str) {
        tracing::warn!(
            chain_code = self.chain_code.unwrap_or(""),
            rpc = self.rpc.unwrap_or(""),
            trade_no = self.trade_no.unwrap_or(""),
            tx_hash = self.tx_hash.unwrap_or(""),
            source = self.source.unwrap_or(""),
            op = self.op.unwrap_or(""),
            visibility_kind = self.visibility_kind.unwrap_or(""),
            address = self.address.unwrap_or(""),
            symbol = self.symbol.unwrap_or(""),
            token = self.token.unwrap_or(""),
            exec_to_addr = self.exec_to_addr.unwrap_or(""),
            attempt = ?self.attempt,
            delay_ms = ?self.delay_ms,
            error_code = ?error.error_code(),
            guarded_host = error.guarded_host().unwrap_or(""),
            transient_reason = error.transient_reason().unwrap_or(""),
            error = %error.message(),
            reason = %reason,
            "chain rpc failure"
        );
    }

    pub fn error(self, error: &ChainRpcErrorContext, reason: &str) {
        tracing::error!(
            chain_code = self.chain_code.unwrap_or(""),
            rpc = self.rpc.unwrap_or(""),
            trade_no = self.trade_no.unwrap_or(""),
            tx_hash = self.tx_hash.unwrap_or(""),
            source = self.source.unwrap_or(""),
            op = self.op.unwrap_or(""),
            visibility_kind = self.visibility_kind.unwrap_or(""),
            address = self.address.unwrap_or(""),
            symbol = self.symbol.unwrap_or(""),
            token = self.token.unwrap_or(""),
            exec_to_addr = self.exec_to_addr.unwrap_or(""),
            attempt = ?self.attempt,
            delay_ms = ?self.delay_ms,
            error_code = ?error.error_code(),
            guarded_host = error.guarded_host().unwrap_or(""),
            transient_reason = error.transient_reason().unwrap_or(""),
            error = %error.message(),
            reason = %reason,
            "chain rpc failure"
        );
    }

    pub fn timeout_skip(self, reason: &str) {
        tracing::warn!(
            observability_event = "chain_rpc_skipped",
            chain_code = self.chain_code.unwrap_or(""),
            rpc = self.rpc.unwrap_or(""),
            trade_no = self.trade_no.unwrap_or(""),
            tx_hash = self.tx_hash.unwrap_or(""),
            source = self.source.unwrap_or(""),
            op = self.op.unwrap_or(""),
            visibility_kind = self.visibility_kind.unwrap_or(""),
            address = self.address.unwrap_or(""),
            symbol = self.symbol.unwrap_or(""),
            token = self.token.unwrap_or(""),
            exec_to_addr = self.exec_to_addr.unwrap_or(""),
            attempt = ?self.attempt,
            delay_ms = ?self.delay_ms,
            reason = %reason,
            "chain rpc skipped"
        );
    }
}

pub fn warn_failure_and_record<E: std::fmt::Display>(
    error: &E,
    log: ChainRpcFailureLog<'_>,
    reason: &str,
) -> ChainRpcErrorContext {
    let rpc_error = ChainRpcErrorContext::from_display(error);
    log.warn(&rpc_error, reason);
    rpc_error.record_transient_failure();
    rpc_error
}

pub fn error_failure_and_record<E: std::fmt::Display>(
    error: &E,
    log: ChainRpcFailureLog<'_>,
    reason: &str,
) -> ChainRpcErrorContext {
    let rpc_error = ChainRpcErrorContext::from_display(error);
    log.error(&rpc_error, reason);
    rpc_error.record_transient_failure();
    rpc_error
}

pub async fn warn_failure_and_record_with_ctx<E: std::fmt::Display>(
    ctx: &Context,
    chain_code: &str,
    error: &E,
    log: ChainRpcFailureLog<'_>,
    reason: &str,
) -> ChainRpcErrorContext {
    let rpc_error = ChainRpcErrorContext::from_display(error);
    log.warn(&rpc_error, reason);
    rpc_error.record_transient_failure_for_chain_code_with_ctx(ctx, chain_code).await;
    rpc_error
}

pub async fn error_failure_and_record_with_ctx<E: std::fmt::Display>(
    ctx: &Context,
    chain_code: &str,
    error: &E,
    log: ChainRpcFailureLog<'_>,
    reason: &str,
) -> ChainRpcErrorContext {
    let rpc_error = ChainRpcErrorContext::from_display(error);
    log.error(&rpc_error, reason);
    rpc_error.record_transient_failure_for_chain_code_with_ctx(ctx, chain_code).await;
    rpc_error
}

pub fn guarded_chain_rpc_host_from_message(msg: &str) -> Option<String> {
    GUARDED_HOSTS.iter().find(|h| msg.contains(h.as_str())).cloned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainRpcErrorClassification {
    pub error_code: Option<u16>,
    pub guarded_host: Option<String>,
    pub transient_reason: Option<&'static str>,
    pub is_service_unavailable: bool,
    pub is_html_error_page: bool,
    pub is_empty_response: bool,
    pub is_transient: bool,
}

pub fn classify_chain_rpc_error_message(msg: &str) -> ChainRpcErrorClassification {
    let error_code = chain_rpc_error_code(msg);
    let is_service_unavailable = error_code == Some(HTTP_STATUS_SERVICE_UNAVAILABLE);
    let is_html_error_page = is_chain_rpc_html_error_page(msg);
    let is_empty_response = is_empty_chain_rpc_response_message(msg);
    let transient_reason = if is_service_unavailable {
        Some("http_503")
    } else if is_html_error_page {
        Some("html_error_page")
    } else if is_empty_response {
        Some("empty_response")
    } else {
        None
    };
    ChainRpcErrorClassification {
        error_code,
        guarded_host: guarded_chain_rpc_host_from_message(msg),
        transient_reason,
        is_service_unavailable,
        is_html_error_page,
        is_empty_response,
        is_transient: is_service_unavailable || is_html_error_page || is_empty_response,
    }
}

pub fn chain_rpc_error_code(msg: &str) -> Option<u16> {
    for marker in ["code=", "code:"] {
        let Some((_, tail)) = msg.split_once(marker) else {
            continue;
        };
        let digits: String =
            tail.trim_start().chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if let Ok(code) = digits.parse::<u16>() {
            return Some(code);
        }
    }
    None
}

pub fn is_empty_chain_rpc_response_message(msg: &str) -> bool {
    msg.contains("missing field `Error`")
        || msg.contains("value = {}")
        || msg.contains("all response:{}")
}

pub fn is_service_unavailable_error(msg: &str) -> bool {
    classify_chain_rpc_error_message(msg).is_service_unavailable
}

pub fn is_transient_chain_rpc_error_message(msg: &str) -> bool {
    // Transient patterns seen under pressure:
    // - HTTP 503 + HTML error pages
    // - Empty JSON / partial bodies causing deserialization errors
    classify_chain_rpc_error_message(msg).is_transient
}

pub fn is_chain_rpc_html_error_page(msg: &str) -> bool {
    msg.contains("<!DOCTYPE html>")
}

#[cfg(test)]
mod tests {
    use super::{
        ChainRpcBreakerOpenDetail, ChainRpcErrorContext, ChainRpcGuardAcquire, GuardedRpcEndpoint,
        RpcCircuitBreaker, RpcCircuitBreakerAdmission, acquire_guarded_host_permit,
        chain_rpc_error_code, classify_chain_rpc_error_message, endpoint_max_concurrency_from,
        guarded_chain_rpc_host_from_message, is_chain_rpc_html_error_page,
        is_empty_chain_rpc_response_message, is_service_unavailable_error,
        is_transient_chain_rpc_error_message, run_with_rpc_timeout_duration,
    };
    use std::{
        sync::{Arc, atomic::Ordering},
        time::Duration,
    };
    use tokio::sync::Semaphore;

    #[test]
    fn endpoint_max_concurrency_defaults_to_global_and_never_exceeds_it() {
        assert_eq!(endpoint_max_concurrency_from(None, 6), 6);
        assert_eq!(endpoint_max_concurrency_from(Some("4"), 6), 4);
        assert_eq!(endpoint_max_concurrency_from(Some("8"), 6), 6);
        assert_eq!(endpoint_max_concurrency_from(Some("0"), 6), 6);
        assert_eq!(endpoint_max_concurrency_from(Some("invalid"), 6), 6);
    }

    #[test]
    fn transient_chain_rpc_error_message_detects_503_and_html_payloads() {
        let nile_503 = "Node response error: code=503, rpc=https://nile.trongrid.io/wallet/triggerconstantcontract";
        assert!(is_transient_chain_rpc_error_message(nile_503));
        assert!(is_service_unavailable_error(nile_503));
        assert_eq!(
            chain_rpc_error_code(
                "Node response error: code=503, rpc=https://nile.trongrid.io/wallet/triggerconstantcontract"
            ),
            Some(503)
        );
        assert_eq!(
            chain_rpc_error_code(
                "NodeResponseError { code: 503, rpc: \"https://nile.trongrid.io/wallet/triggerconstantcontract\" }"
            ),
            Some(503)
        );
        assert_eq!(
            chain_rpc_error_code("Node response error: code=-32000, rpc=https://example"),
            None
        );
        assert_eq!(
            guarded_chain_rpc_host_from_message(nile_503).as_deref(),
            Some("nile.trongrid.io")
        );
        let classification = classify_chain_rpc_error_message(nile_503);
        assert_eq!(classification.error_code, Some(503));
        assert_eq!(classification.guarded_host.as_deref(), Some("nile.trongrid.io"));
        assert_eq!(classification.transient_reason, Some("http_503"));
        assert!(classification.is_service_unavailable);
        assert!(classification.is_transient);
        assert!(is_transient_chain_rpc_error_message(
            "all response:<!DOCTYPE html><html><body>nginx</body></html>"
        ));
        assert!(is_chain_rpc_html_error_page(
            "all response:<!DOCTYPE html><html><body>nginx</body></html>"
        ));
        assert!(is_empty_chain_rpc_response_message("all response:{}"));
        assert!(!is_transient_chain_rpc_error_message("rpc error: invalid address"));
    }

    #[test]
    fn chain_rpc_error_context_exposes_shared_fields() {
        let err = "Node response error: code=503, rpc=https://nile.trongrid.io/wallet/getaccount";
        let ctx = ChainRpcErrorContext::from_display(&err);

        assert_eq!(ctx.message(), err);
        assert_eq!(ctx.error_code(), Some(503));
        assert_eq!(ctx.guarded_host(), Some("nile.trongrid.io"));
        assert_eq!(ctx.transient_reason(), Some("http_503"));
        assert!(ctx.is_service_unavailable());
        assert!(ctx.is_transient());
        assert_eq!(ctx.classification().error_code, Some(503));
    }

    #[test]
    fn rpc_circuit_breaker_opens_quickly_for_503() {
        let breaker = RpcCircuitBreaker::default();
        breaker.record_failure(Some(503));
        assert!(!breaker.is_open());
        breaker.record_failure(Some(503));
        assert!(breaker.is_open());
        assert!(breaker.remaining().is_some());
        assert_eq!(breaker.last_error_code(), Some(503));

        breaker.record_success();
        assert!(breaker.is_open());

        breaker
            .open_until_ms
            .store(RpcCircuitBreaker::now_ms().saturating_sub(1), Ordering::Release);
        breaker.record_success();
        assert!(!breaker.is_open());
        assert!(breaker.remaining().is_none());
        assert_eq!(breaker.last_error_code(), None);
    }

    #[test]
    fn rpc_circuit_breaker_keeps_normal_failure_threshold() {
        let breaker = RpcCircuitBreaker::default();
        for _ in 0..4 {
            breaker.record_failure(Some(500));
            assert!(!breaker.is_open());
        }
        breaker.record_failure(Some(500));
        assert!(breaker.is_open());
    }

    #[test]
    fn rpc_circuit_breaker_half_open_allows_one_probe() {
        let breaker = RpcCircuitBreaker::default();
        breaker.record_failure(Some(503));
        breaker.record_failure(Some(503));
        breaker
            .open_until_ms
            .store(RpcCircuitBreaker::now_ms().saturating_sub(1), Ordering::Release);

        assert!(matches!(breaker.try_admit(), RpcCircuitBreakerAdmission::HalfOpen));
        assert!(matches!(
            breaker.try_admit(),
            RpcCircuitBreakerAdmission::Open(remaining) if remaining.is_zero()
        ));

        breaker.record_success();
        assert!(matches!(breaker.try_admit(), RpcCircuitBreakerAdmission::Closed));
    }

    #[test]
    fn rpc_circuit_breaker_extends_503_window_after_failed_probe() {
        let breaker = RpcCircuitBreaker::default();
        breaker.record_failure(Some(503));
        breaker.record_failure(Some(503));
        breaker
            .open_until_ms
            .store(RpcCircuitBreaker::now_ms().saturating_sub(1), Ordering::Release);
        assert!(matches!(breaker.try_admit(), RpcCircuitBreakerAdmission::HalfOpen));

        breaker.record_failure(Some(503));
        assert!(breaker.remaining().is_some_and(|remaining| remaining >= Duration::from_secs(100)));
    }

    #[test]
    fn rpc_circuit_breaker_ignores_stale_outcomes_during_cooldown() {
        let breaker = RpcCircuitBreaker::default();
        breaker.record_failure(Some(503));
        breaker.record_failure(Some(503));
        let open_until = breaker.open_until_ms.load(Ordering::Acquire);

        breaker.record_failure(Some(503));
        assert_eq!(breaker.open_until_ms.load(Ordering::Acquire), open_until);

        breaker.record_success();
        assert_eq!(breaker.open_until_ms.load(Ordering::Acquire), open_until);
        assert!(breaker.is_open());
    }

    #[test]
    fn guarded_endpoint_key_includes_network_chain_and_host() {
        let testnet = GuardedRpcEndpoint {
            network: "testnet".to_string(),
            chain_code: "tron".to_string(),
            host: "api.nileex.io".to_string(),
        };
        let mainnet = GuardedRpcEndpoint { network: "mainnet".to_string(), ..testnet.clone() };
        let other_chain = GuardedRpcEndpoint { chain_code: "eth".to_string(), ..testnet.clone() };
        let other_host = GuardedRpcEndpoint { host: "rpc.example.com".to_string(), ..testnet };

        assert_ne!(mainnet.key(), other_chain.key());
        assert_ne!(mainnet.key(), other_host.key());
        assert_ne!(other_chain.key(), other_host.key());
    }

    #[test]
    fn breaker_open_detail_keeps_network_fields() {
        let detail = ChainRpcBreakerOpenDetail {
            network: "testnet".to_string(),
            chain_code: "tron".to_string(),
            host: "api.nileex.io".to_string(),
            remaining: Duration::from_secs(1),
            last_error_code: Some(503),
        };
        assert_eq!(detail.network, "testnet");
        assert_eq!(detail.chain_code, "tron");
    }

    #[tokio::test]
    async fn guarded_permit_acquire_times_out_without_permit() {
        let endpoint_sem = Arc::new(Semaphore::new(0));
        let global_sem = Arc::new(Semaphore::new(1));
        let result = acquire_guarded_host_permit(
            "api.nileex.io".to_string(),
            endpoint_sem,
            global_sem,
            Duration::from_millis(1),
        )
        .await;

        assert!(matches!(
            result,
            ChainRpcGuardAcquire::TimedOut { host, timeout }
                if host == "api.nileex.io" && timeout == Duration::from_millis(1)
        ));
    }

    #[tokio::test]
    async fn guarded_permit_acquire_returns_permit_when_available() {
        let endpoint_sem = Arc::new(Semaphore::new(1));
        let global_sem = Arc::new(Semaphore::new(1));
        let result = acquire_guarded_host_permit(
            "api.nileex.io".to_string(),
            endpoint_sem,
            global_sem,
            Duration::from_secs(1),
        )
        .await;

        assert!(matches!(
            result,
            ChainRpcGuardAcquire::Acquired { host, .. } if host == "api.nileex.io"
        ));
    }

    #[tokio::test]
    async fn queued_request_rechecks_breaker_after_permit_acquire() {
        let endpoint_sem = Arc::new(Semaphore::new(1));
        let global_sem = Arc::new(Semaphore::new(1));
        let held_endpoint_permit = endpoint_sem.clone().acquire_owned().await.unwrap();
        let breaker = Arc::new(RpcCircuitBreaker::default());

        let queued = {
            let endpoint_sem = endpoint_sem.clone();
            let global_sem = global_sem.clone();
            let breaker = breaker.clone();
            tokio::spawn(async move {
                let acquired = acquire_guarded_host_permit(
                    "api.nileex.io".to_string(),
                    endpoint_sem,
                    global_sem,
                    Duration::from_secs(1),
                )
                .await;
                assert!(matches!(acquired, ChainRpcGuardAcquire::Acquired { .. }));
                breaker.try_admit()
            })
        };

        tokio::task::yield_now().await;
        breaker.record_failure(Some(503));
        breaker.record_failure(Some(503));
        drop(held_endpoint_permit);

        assert!(matches!(
            queued.await.unwrap(),
            RpcCircuitBreakerAdmission::Open(remaining) if remaining > Duration::ZERO
        ));
    }

    #[tokio::test]
    async fn endpoint_permits_are_isolated_from_each_other() {
        let endpoint_a = Arc::new(Semaphore::new(1));
        let endpoint_b = Arc::new(Semaphore::new(1));
        let global = Arc::new(Semaphore::new(2));

        let first = acquire_guarded_host_permit(
            "api.nileex.io".to_string(),
            endpoint_a.clone(),
            global.clone(),
            Duration::from_secs(1),
        )
        .await;
        assert!(matches!(first, ChainRpcGuardAcquire::Acquired { .. }));

        let blocked_same_endpoint = acquire_guarded_host_permit(
            "api.nileex.io".to_string(),
            endpoint_a,
            global.clone(),
            Duration::from_millis(1),
        )
        .await;
        assert!(matches!(blocked_same_endpoint, ChainRpcGuardAcquire::TimedOut { .. }));

        let different_endpoint = acquire_guarded_host_permit(
            "rpc.example.com".to_string(),
            endpoint_b,
            global,
            Duration::from_secs(1),
        )
        .await;
        assert!(matches!(different_endpoint, ChainRpcGuardAcquire::Acquired { .. }));
    }

    #[tokio::test]
    async fn rpc_timeout_returns_elapsed_for_slow_future() {
        let result: Result<u8, tokio::time::error::Elapsed> = run_with_rpc_timeout_duration(
            async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                1_u8
            },
            Duration::from_millis(1),
            "tron",
            "T_timeout",
            "broadcast",
            "test",
        )
        .await;

        assert!(result.is_err());
    }
}
