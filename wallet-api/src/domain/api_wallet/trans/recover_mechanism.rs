use crate::error::service::ServiceError;
use wallet_chain_interact::QueryTransactionResult;
use wallet_types::chain::chain::ChainCode;
use wallet_utils::RetryableError as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecoverAdapterErrorDecision {
    RefreshAuthAndRetry,
    StayUncertain,
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecoverQueryErrorDecision {
    RefreshAuthAndRetry,
    StayUncertain,
}

pub(super) enum RecoverQueryOutcome {
    Found(QueryTransactionResult),
    Missing,
    QueryError(ServiceError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EvmRecoverMissingTxHashState {
    /// The chain nonce has advanced past the locally recorded nonce.
    /// This means the recorded hash is no longer the active pending tx,
    /// but it does not prove final failure.
    ChainNonceConsumedLocalHashMissing,
    /// The chain nonce still matches the next expected nonce.
    /// The tx may still be in flight, dropped, or only accepted by the RPC.
    ChainNonceMatchesLocalNonce,
    /// Local nonce is ahead of the chain nonce. This usually means the local
    /// snapshot is stale or another flow has not yet advanced the chain.
    LocalNonceAheadOfChainNonce,
}

impl EvmRecoverMissingTxHashState {
    pub(super) fn uncertain_reason(self) -> &'static str {
        match self {
            EvmRecoverMissingTxHashState::ChainNonceConsumedLocalHashMissing => {
                "EVM recover: local tx hash missing but nonce already consumed on-chain; keep uncertain"
            }
            EvmRecoverMissingTxHashState::ChainNonceMatchesLocalNonce => {
                "EVM recover: nonce matches next expected nonce but tx hash missing (rpc accepted only / not propagated / dropped)"
            }
            EvmRecoverMissingTxHashState::LocalNonceAheadOfChainNonce => {
                "EVM recover: local nonce ahead of chain nonce (future nonce / nonce gap suspected)"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecoverMissingTxRoute {
    SolanaVisibilityProbe,
    TronPendingPool,
    EvmNonceCheck,
    WaitForScanner,
}

impl RecoverMissingTxRoute {
    pub(super) fn wait_reason(self) -> Option<&'static str> {
        match self {
            RecoverMissingTxRoute::SolanaVisibilityProbe => {
                Some("Solana链上未找到该交易，等待下一轮Scanner重试")
            }
            RecoverMissingTxRoute::WaitForScanner => {
                Some("非EVM链，链上未找到该交易，等待下一轮Scanner重试")
            }
            RecoverMissingTxRoute::TronPendingPool | RecoverMissingTxRoute::EvmNonceCheck => None,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct RecoverAuthRetryState {
    attempted: bool,
}

impl RecoverAuthRetryState {
    pub(super) fn attempted(&self) -> bool {
        self.attempted
    }

    pub(super) fn mark_attempted(&mut self) {
        self.attempted = true;
    }

    pub(super) fn classify_adapter_error(&self, err: &ServiceError) -> RecoverAdapterErrorDecision {
        classify_recover_adapter_error(err, self.attempted)
    }

    pub(super) fn classify_query_error(&self, err: &ServiceError) -> RecoverQueryErrorDecision {
        classify_recover_query_error(err, self.attempted)
    }
}

pub(super) fn classify_evm_missing_tx_hash_state(
    chain_nonce: u64,
    local_nonce: u64,
) -> EvmRecoverMissingTxHashState {
    use std::cmp::Ordering;

    match chain_nonce.cmp(&local_nonce) {
        Ordering::Greater => EvmRecoverMissingTxHashState::ChainNonceConsumedLocalHashMissing,
        Ordering::Equal => EvmRecoverMissingTxHashState::ChainNonceMatchesLocalNonce,
        Ordering::Less => EvmRecoverMissingTxHashState::LocalNonceAheadOfChainNonce,
    }
}

pub(super) fn classify_recover_missing_tx_route(chain_code: &str) -> RecoverMissingTxRoute {
    if chain_code == ChainCode::Solana.to_string() {
        RecoverMissingTxRoute::SolanaVisibilityProbe
    } else if chain_code == ChainCode::Tron.to_string() {
        RecoverMissingTxRoute::TronPendingPool
    } else if super::shared_mechanism::is_evm_chain(chain_code) {
        RecoverMissingTxRoute::EvmNonceCheck
    } else {
        RecoverMissingTxRoute::WaitForScanner
    }
}

pub(super) fn classify_recover_adapter_error(
    err: &ServiceError,
    auth_retry_attempted: bool,
) -> RecoverAdapterErrorDecision {
    if !auth_retry_attempted && err.is_rpc_auth_unauthorized() {
        RecoverAdapterErrorDecision::RefreshAuthAndRetry
    } else if err.is_delay_retryable() {
        RecoverAdapterErrorDecision::StayUncertain
    } else {
        RecoverAdapterErrorDecision::Fatal
    }
}

pub(super) fn classify_recover_query_error(
    err: &ServiceError,
    auth_retry_attempted: bool,
) -> RecoverQueryErrorDecision {
    if !auth_retry_attempted && err.is_rpc_auth_unauthorized() {
        RecoverQueryErrorDecision::RefreshAuthAndRetry
    } else {
        RecoverQueryErrorDecision::StayUncertain
    }
}

pub(super) fn recover_query_outcome(
    result: Result<Option<QueryTransactionResult>, wallet_chain_interact::Error>,
) -> RecoverQueryOutcome {
    match result {
        Ok(Some(tx_result)) => RecoverQueryOutcome::Found(tx_result),
        Ok(None) => RecoverQueryOutcome::Missing,
        Err(err) => RecoverQueryOutcome::QueryError(err.into()),
    }
}

pub(super) fn log_evm_missing_tx_hash_state(tx_hash: &str, chain_nonce: u64, local_nonce: u64) {
    let state = classify_evm_missing_tx_hash_state(chain_nonce, local_nonce);
    tracing::warn!(
        trade_no=?tx_hash,
        chain_nonce = chain_nonce,
        local_nonce = local_nonce,
        reason = state.uncertain_reason(),
        "EVM recover: tx hash missing on-chain; keep uncertain"
    );
}

#[cfg(test)]
mod tests {
    use super::{
        EvmRecoverMissingTxHashState, RecoverAdapterErrorDecision, RecoverAuthRetryState,
        RecoverMissingTxRoute, RecoverQueryErrorDecision, RecoverQueryOutcome,
        classify_evm_missing_tx_hash_state, classify_recover_adapter_error,
        classify_recover_missing_tx_route, classify_recover_query_error,
        log_evm_missing_tx_hash_state, recover_query_outcome,
    };
    use crate::error::service::ServiceError;
    use wallet_chain_interact::QueryTransactionResult;
    use wallet_types::chain::chain::ChainCode;

    #[test]
    fn evm_recover_missing_tx_hash_treats_nonce_consumed_as_uncertain() {
        let state = classify_evm_missing_tx_hash_state(4, 3);
        assert_eq!(state, EvmRecoverMissingTxHashState::ChainNonceConsumedLocalHashMissing);
        assert!(state.uncertain_reason().contains("nonce already consumed"));
    }

    #[test]
    fn evm_recover_missing_tx_hash_detects_equal_nonce_as_uncertain() {
        let state = classify_evm_missing_tx_hash_state(3, 3);
        assert_eq!(state, EvmRecoverMissingTxHashState::ChainNonceMatchesLocalNonce);
        assert!(state.uncertain_reason().contains("nonce matches"));
    }

    #[test]
    fn evm_recover_missing_tx_hash_detects_local_nonce_ahead() {
        let state = classify_evm_missing_tx_hash_state(2, 3);
        assert_eq!(state, EvmRecoverMissingTxHashState::LocalNonceAheadOfChainNonce);
        assert!(state.uncertain_reason().contains("local nonce ahead"));
    }

    #[test]
    fn evm_recover_missing_tx_hash_state_log_keeps_uncertain() {
        log_evm_missing_tx_hash_state("0xtx", 4, 3);
    }

    #[test]
    fn recover_adapter_unauthorized_refreshes_auth_before_retry() {
        let err = ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(
            401,
            Some("Unauthorized".to_string()),
        ));

        let decision = classify_recover_adapter_error(&err, false);

        assert_eq!(decision, RecoverAdapterErrorDecision::RefreshAuthAndRetry);
    }

    #[test]
    fn recover_adapter_unauthorized_after_auth_retry_is_fatal() {
        let err = ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(
            401,
            Some("Unauthorized".to_string()),
        ));

        let decision = classify_recover_adapter_error(&err, true);

        assert_eq!(decision, RecoverAdapterErrorDecision::Fatal);
    }

    #[test]
    fn recover_adapter_rate_limited_stays_uncertain() {
        let err = ServiceError::TransportBackend(wallet_transport_backend::Error::RateLimited);

        let decision = classify_recover_adapter_error(&err, false);

        assert_eq!(decision, RecoverAdapterErrorDecision::StayUncertain);
    }

    #[test]
    fn recover_query_unauthorized_refreshes_auth_before_retry() {
        let err = ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(
            401,
            Some("Unauthorized".to_string()),
        ));

        let decision = classify_recover_query_error(&err, false);

        assert_eq!(decision, RecoverQueryErrorDecision::RefreshAuthAndRetry);
    }

    #[test]
    fn recover_query_unauthorized_after_auth_retry_stays_uncertain() {
        let err = ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(
            401,
            Some("Unauthorized".to_string()),
        ));

        let decision = classify_recover_query_error(&err, true);

        assert_eq!(decision, RecoverQueryErrorDecision::StayUncertain);
    }

    #[test]
    fn recover_query_outcome_maps_missing_tx() {
        let outcome = recover_query_outcome(Ok(None));

        assert!(matches!(outcome, RecoverQueryOutcome::Missing));
    }

    #[test]
    fn recover_query_outcome_maps_found_tx() {
        let tx_result = QueryTransactionResult::new(
            "0xtx".to_string(),
            1.23,
            "resource".to_string(),
            1_746_040_371_123,
            2,
            42,
        );

        let outcome = recover_query_outcome(Ok(Some(tx_result)));

        match outcome {
            RecoverQueryOutcome::Found(found) => {
                assert_eq!(found.hash, "0xtx");
                assert_eq!(found.status, 2);
            }
            RecoverQueryOutcome::Missing | RecoverQueryOutcome::QueryError(_) => {
                panic!("expected found tx result")
            }
        }
    }

    #[test]
    fn recover_query_outcome_maps_query_error() {
        let outcome = recover_query_outcome(Err(wallet_chain_interact::Error::RpcError(
            "temporarily unavailable".to_string(),
        )));

        match outcome {
            RecoverQueryOutcome::QueryError(err) => {
                assert!(err.to_string().contains("temporarily unavailable"));
            }
            RecoverQueryOutcome::Found(_) | RecoverQueryOutcome::Missing => {
                panic!("expected query error")
            }
        }
    }

    #[test]
    fn recover_auth_retry_state_tracks_single_retry_attempt() {
        let mut state = RecoverAuthRetryState::default();
        assert!(!state.attempted());

        state.mark_attempted();

        assert!(state.attempted());
    }

    #[test]
    fn recover_auth_retry_state_classifies_adapter_error_with_attempt_state() {
        let err = ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(
            401,
            Some("Unauthorized".to_string()),
        ));
        let mut state = RecoverAuthRetryState::default();

        assert_eq!(
            state.classify_adapter_error(&err),
            RecoverAdapterErrorDecision::RefreshAuthAndRetry
        );

        state.mark_attempted();

        assert_eq!(state.classify_adapter_error(&err), RecoverAdapterErrorDecision::Fatal);
    }

    #[test]
    fn recover_auth_retry_state_classifies_query_error_with_attempt_state() {
        let err = ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(
            401,
            Some("Unauthorized".to_string()),
        ));
        let mut state = RecoverAuthRetryState::default();

        assert_eq!(
            state.classify_query_error(&err),
            RecoverQueryErrorDecision::RefreshAuthAndRetry
        );

        state.mark_attempted();

        assert_eq!(state.classify_query_error(&err), RecoverQueryErrorDecision::StayUncertain);
    }

    #[test]
    fn recover_missing_tx_routes_solana_to_visibility_probe() {
        let route = classify_recover_missing_tx_route(&ChainCode::Solana.to_string());
        assert_eq!(route, RecoverMissingTxRoute::SolanaVisibilityProbe);
        assert_eq!(route.wait_reason(), Some("Solana链上未找到该交易，等待下一轮Scanner重试"));
    }

    #[test]
    fn recover_missing_tx_routes_tron_to_pending_pool() {
        let route = classify_recover_missing_tx_route(&ChainCode::Tron.to_string());
        assert_eq!(route, RecoverMissingTxRoute::TronPendingPool);
        assert_eq!(route.wait_reason(), None);
    }

    #[test]
    fn recover_missing_tx_routes_evm_chains_to_nonce_check() {
        for chain_code in [
            ChainCode::Ethereum,
            ChainCode::BnbSmartChain,
            ChainCode::Polygon,
            ChainCode::Avalanche,
            ChainCode::Fantom,
            ChainCode::Arbitrum,
            ChainCode::Base,
            ChainCode::Scroll,
        ] {
            let chain_code = chain_code.to_string();
            let route = classify_recover_missing_tx_route(&chain_code);
            assert_eq!(route, RecoverMissingTxRoute::EvmNonceCheck);
            assert_eq!(route.wait_reason(), None);
        }
    }

    #[test]
    fn recover_missing_tx_routes_unknown_chain_to_wait() {
        let route = classify_recover_missing_tx_route("aptos");
        assert_eq!(route, RecoverMissingTxRoute::WaitForScanner);
        assert_eq!(route.wait_reason(), Some("非EVM链，链上未找到该交易，等待下一轮Scanner重试"));
    }
}

#[cfg(test)]
#[path = "recover_mechanism/receipt_query_tests.rs"]
mod receipt_query_tests;

// Match only the local, confirmed-chain-failure sentinel. A remote HTTP/RPC
// message containing the same words is still merely an observation failure.
pub(crate) fn is_confirmed_recover_chain_failure(err: &ServiceError) -> bool {
    matches!(err, ServiceError::System(crate::error::system::SystemError::Internal(message))
        if message == "broadcasted tx failed")
}
