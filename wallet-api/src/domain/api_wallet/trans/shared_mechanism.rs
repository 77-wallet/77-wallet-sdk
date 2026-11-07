use crate::{
    domain::api_wallet::adapter::tx::{RawTx, TxVisibilityProbe},
    error::{service::ServiceError, system::SystemError},
};
use chrono::{DateTime, Utc};
use wallet_types::chain::chain::ChainCode;

pub(crate) const TRON_RAW_EXPIRY_GUARD_MS: i64 = 3_000;

pub(crate) fn is_evm_chain(chain_code: &str) -> bool {
    let normalized = chain_code.to_ascii_lowercase();
    ChainCode::try_from(normalized.as_str()).is_ok_and(crate::domain::chain::is_evm_chain)
}

pub(crate) fn is_sol_chain(chain_code: &str) -> bool {
    chain_code.eq_ignore_ascii_case(&ChainCode::Solana.to_string())
}

pub(crate) fn is_tron_chain(chain_code: &str) -> bool {
    chain_code.eq_ignore_ascii_case(&ChainCode::Tron.to_string())
}

pub(crate) fn tron_withdraw_resource_needs_recovery(
    chain_code: &str,
    token_addr: Option<&str>,
    resource_consume: &str,
) -> bool {
    if !is_tron_chain(chain_code) || token_addr.is_none_or(|value| value.trim().is_empty()) {
        return false;
    }

    let raw = resource_consume.trim();
    if raw.is_empty() || raw.chars().all(|value| value.is_ascii_digit()) {
        return true;
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return true;
    };
    let bandwidth = value
        .get("bandwidth")
        .or_else(|| value.get("net_used"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let energy = value
        .get("energy")
        .or_else(|| value.get("energy_used"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();

    bandwidth == 0 && energy == 0
}

pub(crate) fn need_broadcast_visibility_check(chain_code: &str) -> bool {
    is_evm_chain(chain_code) || is_sol_chain(chain_code)
}

pub(crate) fn tracks_broadcast_uncertain_state(chain_code: &str) -> bool {
    is_evm_chain(chain_code) || is_sol_chain(chain_code) || is_tron_chain(chain_code)
}

pub(crate) fn broadcast_uncertain_backoff_secs(
    retry_count: u32,
    mid_secs: i64,
    max_secs: i64,
) -> i64 {
    match retry_count {
        0..=3 => 0,
        4..=6 => mid_secs,
        _ => max_secs,
    }
}

pub(crate) fn broadcast_uncertain_elapsed_secs(
    since: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<i64> {
    since.map(|since| now.signed_duration_since(since).num_seconds().max(0))
}

pub(crate) fn is_broadcast_uncertain_timed_out(
    since: Option<DateTime<Utc>>,
    timeout_secs: i64,
    now: DateTime<Utc>,
) -> bool {
    broadcast_uncertain_elapsed_secs(since, now).is_some_and(|elapsed| elapsed >= timeout_secs)
}

pub(crate) fn should_throttle_broadcast_uncertain_after_last_check(
    last_checked_at: Option<DateTime<Utc>>,
    wait_secs: i64,
    now: DateTime<Utc>,
) -> bool {
    if wait_secs <= 0 {
        return false;
    }
    let Some(last_checked_at) = last_checked_at else {
        return false;
    };
    now.signed_duration_since(last_checked_at).num_seconds() < wait_secs
}

pub(crate) fn should_throttle_broadcast_uncertain_recover(
    since: Option<DateTime<Utc>>,
    last_checked_at: Option<DateTime<Utc>>,
    retry_count: u32,
    timeout_secs: i64,
    mid_backoff_secs: i64,
    max_backoff_secs: i64,
    now: DateTime<Utc>,
) -> bool {
    if is_broadcast_uncertain_timed_out(since, timeout_secs, now) {
        return false;
    }
    let wait_secs =
        broadcast_uncertain_backoff_secs(retry_count, mid_backoff_secs, max_backoff_secs);
    should_throttle_broadcast_uncertain_after_last_check(last_checked_at, wait_secs, now)
}

pub(crate) fn normalized_tx_hash(tx_hash: Option<&str>) -> Option<&str> {
    tx_hash.map(str::trim).filter(|s| !s.is_empty())
}

pub(crate) fn validate_recovered_tx_hash(
    trade_no: &str,
    existing_tx_hash: Option<&str>,
    recovered_tx_hash: &str,
    source: &str,
) -> Result<(), ServiceError> {
    if let Some(existing_tx_hash) = existing_tx_hash {
        if recovered_tx_hash != existing_tx_hash {
            tracing::error!(
                observability_event = "shadow_recover_hash_mismatch",
                reason = "tx_hash_fact_conflict",
                trade_no = %trade_no,
                existing_tx_hash = %existing_tx_hash,
                recover_tx_hash = %recovered_tx_hash,
                source = %source,
                "tx_hash mismatch during recover - fact integrity violated"
            );
            return Err(ServiceError::System(SystemError::Internal(
                "recover tx_hash mismatch".to_string(),
            )));
        }
    }

    Ok(())
}

pub(crate) fn is_stale_slot(
    started_at: Option<DateTime<Utc>>,
    stale_secs: i64,
    now: DateTime<Utc>,
) -> bool {
    let Some(started_at) = started_at else {
        return false;
    };
    now.signed_duration_since(started_at).num_seconds() >= stale_secs
}

pub(crate) fn should_force_align_nonce_gap(chain_nonce: u64, local_nonce: u64) -> bool {
    local_nonce > chain_nonce && local_nonce.saturating_sub(chain_nonce) >= 2
}

pub(crate) fn tron_raw_expiration_ms(raw_tx: &RawTx) -> Option<i64> {
    let RawTx::Tron(raw, ..) = raw_tx else { return None };
    let v: serde_json::Value = serde_json::from_str(&raw.raw_data).ok()?;
    v.get("expiration").and_then(|x| x.as_i64().or_else(|| x.as_u64().map(|u| u as i64)))
}

pub(crate) fn should_invalidate_expired_tron_raw(
    chain_code: &str,
    raw_tx_json: &str,
    now_ms: i64,
) -> bool {
    if !is_tron_chain(chain_code) {
        return false;
    }
    let raw_tx: RawTx = match wallet_utils::serde_func::serde_from_str(raw_tx_json) {
        Ok(raw_tx) => raw_tx,
        Err(_) => return false,
    };
    let Some(exp_ms) = tron_raw_expiration_ms(&raw_tx) else {
        return false;
    };
    exp_ms <= now_ms.saturating_add(TRON_RAW_EXPIRY_GUARD_MS)
}

pub(crate) fn should_fail_expired_tron_missing_confirmed(
    chain_code: &str,
    raw_tx_json: Option<&str>,
    now_ms: i64,
    settle_buffer_ms: i64,
) -> bool {
    if !is_tron_chain(chain_code) {
        return false;
    }
    let Some(raw_tx_json) = raw_tx_json else {
        return false;
    };
    let raw_tx: RawTx = match wallet_utils::serde_func::serde_from_str(raw_tx_json) {
        Ok(raw_tx) => raw_tx,
        Err(_) => return false,
    };
    let Some(exp_ms) = tron_raw_expiration_ms(&raw_tx) else {
        return false;
    };
    now_ms >= exp_ms.saturating_add(settle_buffer_ms.max(0))
}

pub(crate) fn should_invalidate_expired_tron_raw_for_recover(
    chain_code: &str,
    raw_tx_json: &str,
    last_broadcast_at_present: bool,
    tx_hash_present: bool,
    now_ms: i64,
) -> bool {
    !last_broadcast_at_present
        && !tx_hash_present
        && should_invalidate_expired_tron_raw(chain_code, raw_tx_json, now_ms)
}

pub(crate) fn is_tron_missing_confirmed_and_pending_error(err: &ServiceError) -> bool {
    err.to_string().contains("tron tx missing from confirmed and pending pools")
}

pub(crate) fn should_retry_after_tron_missing_confirmed_and_pending(
    chain_code: &str,
    last_broadcast_at: Option<DateTime<Utc>>,
    broadcast_uncertain_since_at: Option<DateTime<Utc>>,
    timeout_secs: i64,
    now: DateTime<Utc>,
) -> bool {
    if !is_tron_chain(chain_code) {
        return false;
    }
    let Some(timeout_base) = broadcast_uncertain_since_at.or(last_broadcast_at) else {
        return false;
    };
    now.signed_duration_since(timeout_base).num_seconds() >= timeout_secs
}

pub(crate) fn log_visibility_probe(
    chain_code: &str,
    tx_hash: &str,
    rpc: &str,
    visibility_kind: &str,
    probe: &TxVisibilityProbe,
    attempt: Option<usize>,
    delay_ms: Option<u64>,
    message: &str,
) {
    let signature_status = probe.signature_status.as_deref().unwrap_or("missing");
    let transaction_result = probe.transaction_result.as_deref().unwrap_or("missing");
    let health = probe.health.as_deref().unwrap_or("missing");
    let visibility_reason = probe.visibility_reason.as_deref().unwrap_or("missing");

    match (attempt, delay_ms) {
        (Some(attempt), Some(delay_ms)) => tracing::info!(
            chain_code = %chain_code,
            tx_hash = %tx_hash,
            rpc = %rpc,
            signature_status = %signature_status,
            transaction_result = %transaction_result,
            health = %health,
            slot = ?probe.slot,
            visibility_reason = %visibility_reason,
            attempt = attempt,
            delay_ms = delay_ms,
            visibility_kind = %visibility_kind,
            message = %message,
            "broadcast visibility probe"
        ),
        _ => tracing::info!(
            chain_code = %chain_code,
            tx_hash = %tx_hash,
            rpc = %rpc,
            signature_status = %signature_status,
            transaction_result = %transaction_result,
            health = %health,
            slot = ?probe.slot,
            visibility_reason = %visibility_reason,
            visibility_kind = %visibility_kind,
            message = %message,
            "broadcast visibility probe"
        ),
    }
}

pub(crate) fn normalize_chain_time_to_ms(raw: u128) -> Result<u128, ServiceError> {
    const SECOND_THRESHOLD: u128 = 100_000_000_000; // < 1e11 视为秒
    const SEC_TO_MS: u128 = 1000;

    if raw == 0 {
        return Err(ServiceError::System(crate::error::system::SystemError::Internal(
            "invalid chain transaction_time: 0".to_string(),
        )));
    }

    let normalized = if raw < SECOND_THRESHOLD {
        raw.checked_mul(SEC_TO_MS).ok_or_else(|| {
            ServiceError::System(crate::error::system::SystemError::Internal(
                "chain transaction_time seconds->milliseconds overflow".to_string(),
            ))
        })?
    } else {
        raw
    };

    if normalized > i64::MAX as u128 {
        return Err(ServiceError::System(crate::error::system::SystemError::Internal(
            "chain transaction_time out of range for DateTime::from_timestamp_millis".to_string(),
        )));
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::{
        broadcast_uncertain_backoff_secs, broadcast_uncertain_elapsed_secs,
        is_broadcast_uncertain_timed_out, is_evm_chain, is_sol_chain, is_stale_slot, is_tron_chain,
        normalize_chain_time_to_ms, normalized_tx_hash, should_fail_expired_tron_missing_confirmed,
        should_force_align_nonce_gap, should_invalidate_expired_tron_raw_for_recover,
        should_retry_after_tron_missing_confirmed_and_pending,
        should_throttle_broadcast_uncertain_after_last_check,
        should_throttle_broadcast_uncertain_recover, tracks_broadcast_uncertain_state,
        validate_recovered_tx_hash,
    };
    use crate::domain::api_wallet::adapter::tx::RawTx;
    use chrono::{Duration, Utc};
    use wallet_chain_interact::{BillResourceConsume, tron::operations::RawTransactionParams};

    #[test]
    fn chain_kind_helpers_are_case_insensitive() {
        for chain_code in
            ["eth", "BNB", "polygon", "AVALANCHE", "fantom", "ARBITRUM", "base", "SCROLL"]
        {
            assert!(is_evm_chain(chain_code), "{chain_code} must be classified as EVM");
        }
        assert!(!is_evm_chain("matic"));
        assert!(!is_evm_chain("unknown"));
        assert!(is_sol_chain("sol"));
        assert!(is_sol_chain("SOL"));
        assert!(is_tron_chain("tron"));
        assert!(is_tron_chain("TRON"));
    }

    #[test]
    fn uncertain_state_tracking_chains_are_explicit() {
        for chain_code in
            ["eth", "bnb", "polygon", "avalanche", "fantom", "arbitrum", "base", "scroll"]
        {
            assert!(tracks_broadcast_uncertain_state(chain_code));
        }
        assert!(tracks_broadcast_uncertain_state("sol"));
        assert!(tracks_broadcast_uncertain_state("tron"));
        assert!(!tracks_broadcast_uncertain_state("unknown"));
    }

    #[test]
    fn normalized_tx_hash_trims_blank_values() {
        assert_eq!(normalized_tx_hash(Some(" 0xabc ")), Some("0xabc"));
        assert_eq!(normalized_tx_hash(Some("   ")), None);
        assert_eq!(normalized_tx_hash(None), None);
    }

    #[test]
    fn recover_hash_validation_allows_backfill_and_rejects_mismatch() {
        validate_recovered_tx_hash("trade-no", None, "0xrecover", "test")
            .expect("missing local hash should be accepted for backfill");
        validate_recovered_tx_hash("trade-no", Some("0xrecover"), "0xrecover", "test")
            .expect("matching hash should be accepted");

        let err = validate_recovered_tx_hash("trade-no", Some("0xlocal"), "0xrecover", "test")
            .expect_err("mismatched hash must fail");

        assert!(err.to_string().contains("recover tx_hash mismatch"));
    }

    #[test]
    fn stale_slot_requires_started_at_and_elapsed_timeout() {
        let now = Utc::now();

        assert!(!is_stale_slot(None, 30, now));
        assert!(!is_stale_slot(Some(now - Duration::seconds(29)), 30, now));
        assert!(is_stale_slot(Some(now - Duration::seconds(30)), 30, now));
    }

    #[test]
    fn nonce_gap_alignment_requires_local_nonce_at_least_two_ahead() {
        assert!(should_force_align_nonce_gap(3, 5));
        assert!(!should_force_align_nonce_gap(3, 4));
        assert!(!should_force_align_nonce_gap(5, 3));
    }

    #[test]
    fn broadcast_uncertain_backoff_has_fast_mid_and_max_windows() {
        assert_eq!(broadcast_uncertain_backoff_secs(0, 15, 30), 0);
        assert_eq!(broadcast_uncertain_backoff_secs(3, 15, 30), 0);
        assert_eq!(broadcast_uncertain_backoff_secs(4, 15, 30), 15);
        assert_eq!(broadcast_uncertain_backoff_secs(6, 15, 30), 15);
        assert_eq!(broadcast_uncertain_backoff_secs(7, 15, 30), 30);
    }

    #[test]
    fn broadcast_uncertain_throttle_stops_at_timeout() {
        let now = Utc::now();
        let since = Some(now - Duration::seconds(20));
        let last_checked = Some(now - Duration::seconds(5));

        assert_eq!(broadcast_uncertain_elapsed_secs(since, now), Some(20));
        assert!(!is_broadcast_uncertain_timed_out(since, 30, now));
        assert!(should_throttle_broadcast_uncertain_recover(
            since,
            last_checked,
            4,
            30,
            15,
            30,
            now
        ));
        assert!(!should_throttle_broadcast_uncertain_recover(
            Some(now - Duration::seconds(30)),
            last_checked,
            4,
            30,
            15,
            30,
            now
        ));
    }

    #[test]
    fn broadcast_uncertain_last_check_requires_positive_wait_and_timestamp() {
        let now = Utc::now();

        assert!(!should_throttle_broadcast_uncertain_after_last_check(None, 15, now));
        assert!(!should_throttle_broadcast_uncertain_after_last_check(
            Some(now - Duration::seconds(1)),
            0,
            now
        ));
        assert!(should_throttle_broadcast_uncertain_after_last_check(
            Some(now - Duration::seconds(1)),
            15,
            now
        ));
        assert!(!should_throttle_broadcast_uncertain_after_last_check(
            Some(now - Duration::seconds(15)),
            15,
            now
        ));
    }

    fn tron_raw_tx_json(expiration_ms: i64) -> String {
        let raw = RawTransactionParams {
            raw_data: serde_json::json!({ "expiration": expiration_ms }).to_string(),
            raw_data_hex: "00".to_string(),
            tx_id: "txid".to_string(),
            signature: vec![],
        };
        let raw_tx =
            RawTx::Tron(raw, BillResourceConsume { net_used: 0, energy_used: 0 }, String::new());
        wallet_utils::serde_func::serde_to_string(&raw_tx).expect("serialize tron raw tx")
    }

    #[test]
    fn expired_tron_raw_recover_guard_requires_no_broadcast_evidence() {
        let raw_tx_json = tron_raw_tx_json(123);
        let now_ms = Utc::now().timestamp_millis();

        assert!(should_invalidate_expired_tron_raw_for_recover(
            "tron",
            &raw_tx_json,
            false,
            false,
            now_ms
        ));
        assert!(!should_invalidate_expired_tron_raw_for_recover(
            "tron",
            &raw_tx_json,
            true,
            false,
            now_ms
        ));
        assert!(!should_invalidate_expired_tron_raw_for_recover(
            "tron",
            &raw_tx_json,
            false,
            true,
            now_ms
        ));
        assert!(!should_invalidate_expired_tron_raw_for_recover(
            "sol",
            &raw_tx_json,
            false,
            false,
            now_ms
        ));
    }

    #[test]
    fn tron_missing_confirmed_and_pending_retry_waits_for_timeout() {
        let now = Utc::now();

        assert!(should_retry_after_tron_missing_confirmed_and_pending(
            "tron",
            Some(now - Duration::seconds(10)),
            Some(now - Duration::seconds(120)),
            60,
            now
        ));
        assert!(should_retry_after_tron_missing_confirmed_and_pending(
            "tron",
            Some(now - Duration::seconds(60)),
            None,
            60,
            now
        ));
        assert!(should_retry_after_tron_missing_confirmed_and_pending(
            "tron",
            None,
            Some(now - Duration::seconds(60)),
            60,
            now
        ));
        assert!(!should_retry_after_tron_missing_confirmed_and_pending(
            "sol",
            Some(now - Duration::seconds(60)),
            Some(now - Duration::seconds(60)),
            60,
            now
        ));
        assert!(!should_retry_after_tron_missing_confirmed_and_pending(
            "tron", None, None, 60, now
        ));
    }

    #[test]
    fn expired_tron_missing_confirmed_fails_after_settle_buffer() {
        let now_ms = Utc::now().timestamp_millis();
        let raw_tx_json = tron_raw_tx_json(now_ms - 30_000);

        assert!(!should_fail_expired_tron_missing_confirmed(
            "tron",
            Some(&raw_tx_json),
            now_ms,
            60_000
        ));
        assert!(should_fail_expired_tron_missing_confirmed(
            "tron",
            Some(&raw_tx_json),
            now_ms,
            10_000
        ));
        assert!(!should_fail_expired_tron_missing_confirmed(
            "eth",
            Some(&raw_tx_json),
            now_ms,
            10_000
        ));
        assert!(!should_fail_expired_tron_missing_confirmed("tron", None, now_ms, 10_000));
    }

    #[test]
    fn normalize_chain_time_seconds_to_ms() {
        let sec = 1_746_040_371_u128; // 2025-05-xx second-level timestamp
        let ms = normalize_chain_time_to_ms(sec).unwrap();
        assert_eq!(ms, sec * 1000);
    }

    #[test]
    fn normalize_chain_time_milliseconds_kept_as_is() {
        let ms = 1_746_040_371_123_u128;
        let normalized = normalize_chain_time_to_ms(ms).unwrap();
        assert_eq!(normalized, ms);
    }

    #[test]
    fn normalize_chain_time_zero_should_fail() {
        assert!(normalize_chain_time_to_ms(0).is_err());
    }
}
