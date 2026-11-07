use chrono::{DateTime, Utc};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;

const DEFAULT_RECOVER_COOLDOWN: Duration = Duration::from_secs(60);
const FRESH_BROADCAST_RECOVER_COOLDOWN: Duration = Duration::from_secs(3);
const FRESH_BROADCAST_RECOVER_WINDOW_SECS: i64 = 120;
const MAX_RECOVER_COOLDOWN: Duration = Duration::from_secs(300);
const BREAKER_WARN_COOLDOWN: Duration = Duration::from_secs(10);
const MAX_RECOVER_COOLDOWN_MAP_SIZE: usize = 100_000;
const INTENT_REPORT_WINDOW_SECS: u64 = 60;
const SHARED_RESOURCE_GATE_CONCURRENCY: usize = 4;

#[derive(Clone, Copy)]
struct RecoverDispatchState {
    last_dispatch: Instant,
    next_cooldown: Duration,
}

static RECOVER_LAST_DISPATCH_TS: Lazy<DashMap<String, RecoverDispatchState>> =
    Lazy::new(DashMap::new);
static BREAKER_WARN_LAST_LOG_TS: Lazy<DashMap<String, Instant>> = Lazy::new(DashMap::new);
static BROADCAST_INTENT_DISPATCH_COUNT: AtomicU64 = AtomicU64::new(0);
static RECOVER_INTENT_DISPATCH_COUNT: AtomicU64 = AtomicU64::new(0);
static LAST_INTENT_REPORT_TS: AtomicU64 = AtomicU64::new(0);
static SHARED_RESOURCE_GATE_SEMAPHORE: Lazy<Arc<Semaphore>> =
    Lazy::new(|| Arc::new(Semaphore::new(SHARED_RESOURCE_GATE_CONCURRENCY)));

pub(crate) fn shared_resource_gate_semaphore() -> Arc<Semaphore> {
    SHARED_RESOURCE_GATE_SEMAPHORE.clone()
}

pub(crate) fn shared_resource_gate_concurrency() -> usize {
    SHARED_RESOURCE_GATE_CONCURRENCY
}

fn parse_usize_env(raw: Option<&str>, default: usize, min: usize, max: usize) -> usize {
    let parsed = raw.and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(default);
    parsed.clamp(min, max)
}

fn parse_u64_env(raw: Option<&str>, default: u64, min: u64, max: u64) -> u64 {
    let parsed = raw.and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(default);
    parsed.clamp(min, max)
}

pub(crate) fn read_usize_env(name: &str, default: usize, min: usize, max: usize) -> usize {
    parse_usize_env(std::env::var(name).ok().as_deref(), default, min, max)
}

pub(crate) fn read_u64_env(name: &str, default: u64, min: u64, max: u64) -> u64 {
    parse_u64_env(std::env::var(name).ok().as_deref(), default, min, max)
}

fn next_recover_cooldown(current: Duration) -> Duration {
    match current.as_secs() {
        0..=3 => Duration::from_secs(8),
        4..=8 => Duration::from_secs(15),
        9..=15 => Duration::from_secs(30),
        16..=30 => DEFAULT_RECOVER_COOLDOWN,
        _ => current.saturating_mul(2).min(MAX_RECOVER_COOLDOWN),
    }
}

pub(crate) fn allow_recover_dispatch(key: &str) -> bool {
    allow_recover_dispatch_with_initial_cooldown(key, DEFAULT_RECOVER_COOLDOWN)
}

pub(crate) fn allow_fresh_broadcast_recover_dispatch(key: &str) -> bool {
    allow_recover_dispatch_with_initial_cooldown(key, FRESH_BROADCAST_RECOVER_COOLDOWN)
}

pub(crate) fn allow_broadcast_recover_dispatch(
    key: &str,
    last_broadcast_at: Option<DateTime<Utc>>,
) -> bool {
    let fresh_broadcast = last_broadcast_at.is_some_and(|broadcast_at| {
        let age = Utc::now().signed_duration_since(broadcast_at);
        age >= chrono::Duration::zero() && age.num_seconds() <= FRESH_BROADCAST_RECOVER_WINDOW_SECS
    });

    if fresh_broadcast {
        allow_fresh_broadcast_recover_dispatch(key)
    } else {
        allow_recover_dispatch(key)
    }
}

fn allow_recover_dispatch_with_initial_cooldown(key: &str, initial: Duration) -> bool {
    let now = Instant::now();
    if let Some(state) = RECOVER_LAST_DISPATCH_TS.get(key) {
        let effective_cooldown = state.next_cooldown.max(initial);
        if now.duration_since(state.last_dispatch) < effective_cooldown {
            return false;
        }
    }

    let next_cooldown = RECOVER_LAST_DISPATCH_TS
        .get(key)
        .map(|state| next_recover_cooldown(state.next_cooldown.max(initial)))
        .unwrap_or(initial);
    RECOVER_LAST_DISPATCH_TS
        .insert(key.to_string(), RecoverDispatchState { last_dispatch: now, next_cooldown });

    if RECOVER_LAST_DISPATCH_TS.len() > MAX_RECOVER_COOLDOWN_MAP_SIZE {
        let ttl = MAX_RECOVER_COOLDOWN.saturating_mul(2);
        RECOVER_LAST_DISPATCH_TS.retain(|_, state| now.duration_since(state.last_dispatch) < ttl);
    }

    true
}

pub(crate) fn recover_dispatch_key(trade_no: &str) -> String {
    format!("recover:{trade_no}")
}

pub(crate) fn reset_recover_dispatch(key: &str) -> bool {
    RECOVER_LAST_DISPATCH_TS.remove(key).is_some()
}

pub(crate) fn resource_recover_dispatch_key(trade_no: &str) -> String {
    format!("recover-resource:{trade_no}")
}

pub(crate) fn recover_dispatch_cooldown(key: &str) -> Duration {
    RECOVER_LAST_DISPATCH_TS
        .get(key)
        .map(|state| state.next_cooldown)
        .unwrap_or(DEFAULT_RECOVER_COOLDOWN)
}

pub(crate) fn recover_cooldown() -> Duration {
    DEFAULT_RECOVER_COOLDOWN
}

pub(crate) async fn breaker_open_for_chain_code(
    ctx: &crate::context::Context,
    chain_code: &str,
) -> Option<(String, Duration)> {
    crate::infrastructure::chain_rpc_guard::breaker_open_for_chain_code_with_ctx(ctx, chain_code)
        .await
}

pub(crate) fn should_emit_breaker_warn(key: &str) -> bool {
    let now = Instant::now();
    if let Some(last_ts) = BREAKER_WARN_LAST_LOG_TS.get(key) {
        if now.duration_since(*last_ts) < BREAKER_WARN_COOLDOWN {
            return false;
        }
    }

    BREAKER_WARN_LAST_LOG_TS.insert(key.to_string(), now);
    true
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::from_secs(0)).as_secs()
}

pub(crate) fn record_chain_intent_dispatch(intent_kind: &str) {
    match intent_kind {
        "broadcast" => {
            BROADCAST_INTENT_DISPATCH_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        "recover" => {
            RECOVER_INTENT_DISPATCH_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        _ => return,
    }

    let now = now_secs();
    let last = LAST_INTENT_REPORT_TS.load(Ordering::Relaxed);

    if last == 0 {
        let _ =
            LAST_INTENT_REPORT_TS.compare_exchange(0, now, Ordering::Relaxed, Ordering::Relaxed);
        return;
    }

    if now.saturating_sub(last) < INTENT_REPORT_WINDOW_SECS {
        return;
    }

    if LAST_INTENT_REPORT_TS
        .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        let broadcast = BROADCAST_INTENT_DISPATCH_COUNT.swap(0, Ordering::Relaxed);
        let recover = RECOVER_INTENT_DISPATCH_COUNT.swap(0, Ordering::Relaxed);
        tracing::info!(
            broadcast = broadcast,
            recover = recover,
            "shadow intent dispatch count in last minute"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RECOVER_LAST_DISPATCH_TS, SHARED_RESOURCE_GATE_CONCURRENCY,
        allow_broadcast_recover_dispatch, allow_fresh_broadcast_recover_dispatch,
        allow_recover_dispatch, next_recover_cooldown, parse_u64_env, parse_usize_env,
        recover_dispatch_cooldown, recover_dispatch_key, reset_recover_dispatch,
        resource_recover_dispatch_key, shared_resource_gate_concurrency,
        shared_resource_gate_semaphore,
    };
    use chrono::{Duration as ChronoDuration, Utc};
    use std::{sync::Arc, time::Duration};

    #[test]
    fn recover_backoff_doubles_and_is_capped() {
        assert_eq!(next_recover_cooldown(Duration::from_secs(3)), Duration::from_secs(8));
        assert_eq!(next_recover_cooldown(Duration::from_secs(8)), Duration::from_secs(15));
        assert_eq!(next_recover_cooldown(Duration::from_secs(15)), Duration::from_secs(30));
        assert_eq!(next_recover_cooldown(Duration::from_secs(30)), Duration::from_secs(60));
        assert_eq!(next_recover_cooldown(Duration::from_secs(60)), Duration::from_secs(120));
        assert_eq!(next_recover_cooldown(Duration::from_secs(120)), Duration::from_secs(240));
        assert_eq!(next_recover_cooldown(Duration::from_secs(240)), Duration::from_secs(300));
        assert_eq!(next_recover_cooldown(Duration::from_secs(300)), Duration::from_secs(300));
    }

    #[test]
    fn fresh_recover_uses_fast_sequence_then_returns_to_default_backoff() {
        let key = "fresh-recover-sequence";
        RECOVER_LAST_DISPATCH_TS.remove(key);

        assert!(allow_fresh_broadcast_recover_dispatch(key));
        assert_eq!(recover_dispatch_cooldown(key), Duration::from_secs(3));

        RECOVER_LAST_DISPATCH_TS.get_mut(key).unwrap().last_dispatch -= Duration::from_secs(3);
        assert!(allow_fresh_broadcast_recover_dispatch(key));
        assert_eq!(recover_dispatch_cooldown(key), Duration::from_secs(8));

        RECOVER_LAST_DISPATCH_TS.get_mut(key).unwrap().last_dispatch -= Duration::from_secs(60);
        assert!(allow_recover_dispatch(key));
        assert_eq!(recover_dispatch_cooldown(key), Duration::from_secs(120));

        RECOVER_LAST_DISPATCH_TS.remove(key);
    }

    #[test]
    fn successful_broadcast_resets_pre_broadcast_recover_cooldown() {
        let key = "withdraw-pre-broadcast-recover";
        let resource_key = resource_recover_dispatch_key(key);
        RECOVER_LAST_DISPATCH_TS.remove(key);
        RECOVER_LAST_DISPATCH_TS.remove(&resource_key);

        assert!(allow_recover_dispatch(key));
        assert_eq!(recover_dispatch_cooldown(key), Duration::from_secs(60));
        assert!(allow_recover_dispatch(&resource_key));

        assert!(reset_recover_dispatch(key));

        assert!(allow_fresh_broadcast_recover_dispatch(key));
        assert_eq!(recover_dispatch_cooldown(key), Duration::from_secs(3));
        assert!(!allow_recover_dispatch(&resource_key));

        RECOVER_LAST_DISPATCH_TS.remove(key);
        RECOVER_LAST_DISPATCH_TS.remove(&resource_key);
    }

    #[test]
    fn collect_fresh_broadcast_recover_policy_uses_fast_then_generic_admission() {
        let fresh_key = "collect-fresh-broadcast-recover";
        RECOVER_LAST_DISPATCH_TS.remove(fresh_key);
        assert!(allow_broadcast_recover_dispatch(fresh_key, Some(Utc::now())));
        assert_eq!(recover_dispatch_cooldown(fresh_key), Duration::from_secs(3));

        let old_key = "collect-old-broadcast-recover";
        RECOVER_LAST_DISPATCH_TS.remove(old_key);
        assert!(allow_broadcast_recover_dispatch(
            old_key,
            Some(Utc::now() - ChronoDuration::seconds(121))
        ));
        assert_eq!(recover_dispatch_cooldown(old_key), Duration::from_secs(60));

        RECOVER_LAST_DISPATCH_TS.remove(fresh_key);
        RECOVER_LAST_DISPATCH_TS.remove(old_key);
    }

    #[test]
    fn recover_dispatch_key_is_shared_across_recover_entry_points() {
        assert_eq!(recover_dispatch_key("trade-1"), "recover:trade-1");
    }

    #[test]
    fn resource_recover_cooldown_is_independent_from_confirmation() {
        let trade_no = "resource-cooldown-independent";
        let confirmation_key = recover_dispatch_key(trade_no);
        let resource_key = resource_recover_dispatch_key(trade_no);

        assert!(allow_recover_dispatch(&confirmation_key));
        assert!(!allow_recover_dispatch(&confirmation_key));
        assert!(allow_recover_dispatch(&resource_key));
        assert_ne!(confirmation_key, resource_key);
    }

    #[test]
    fn shared_resource_gate_concurrency_is_fixed_small_pool() {
        assert_eq!(shared_resource_gate_concurrency(), 4);
        assert_eq!(SHARED_RESOURCE_GATE_CONCURRENCY, 4);
    }

    #[test]
    fn shared_resource_gate_semaphore_uses_one_shared_cross_flow_pool() {
        let withdraw_handle = shared_resource_gate_semaphore();
        let collect_handle = shared_resource_gate_semaphore();
        assert!(Arc::ptr_eq(&withdraw_handle, &collect_handle));

        let permits: Vec<_> = (0..withdraw_handle.available_permits())
            .map(|_| withdraw_handle.clone().try_acquire_owned().unwrap())
            .collect();
        assert!(collect_handle.clone().try_acquire_owned().is_err());
        drop(permits);
        assert!(collect_handle.try_acquire_owned().is_ok());
    }

    #[test]
    fn parse_usize_env_uses_default_when_missing_or_invalid() {
        assert_eq!(parse_usize_env(None, 24, 4, 100), 24);
        assert_eq!(parse_usize_env(Some("bad"), 24, 4, 100), 24);
    }

    #[test]
    fn parse_usize_env_clamps_range() {
        assert_eq!(parse_usize_env(Some("1"), 24, 4, 100), 4);
        assert_eq!(parse_usize_env(Some("300"), 24, 4, 100), 100);
        assert_eq!(parse_usize_env(Some("32"), 24, 4, 100), 32);
    }

    #[test]
    fn parse_u64_env_uses_default_when_missing_or_invalid() {
        assert_eq!(parse_u64_env(None, 5000, 1000, 30000), 5000);
        assert_eq!(parse_u64_env(Some("bad"), 5000, 1000, 30000), 5000);
    }

    #[test]
    fn parse_u64_env_clamps_range() {
        assert_eq!(parse_u64_env(Some("999"), 5000, 1000, 30000), 1000);
        assert_eq!(parse_u64_env(Some("30001"), 5000, 1000, 30000), 30000);
        assert_eq!(parse_u64_env(Some("8000"), 5000, 1000, 30000), 8000);
    }
}
