use super::{
    RecoverAuthRetryState, RecoverQueryErrorDecision, RecoverQueryOutcome,
    classify_evm_missing_tx_hash_state, recover_query_outcome,
};
use wallet_chain_interact::QueryTransactionResult;

fn gateway_error(code: i64) -> wallet_chain_interact::Error {
    wallet_transport::TransportError::NodeResponseError(
        wallet_transport::errors::NodeResponseError::new(
            code,
            "http://127.0.0.1:1/base",
            Some("request:{...} request:{\"jsonrpc\":\"2.0\",\"result\":null}".into()),
        ),
    )
    .into()
}

#[test]
fn receipt_query_gateway_errors_stay_uncertain_then_next_scan_can_confirm() {
    let mut auth = RecoverAuthRetryState::default();
    for code in [500, 429, 502, 503, 504] {
        let RecoverQueryOutcome::QueryError(err) = recover_query_outcome(Err(gateway_error(code)))
        else {
            panic!("gateway failure must not become a missing receipt or chain result");
        };
        assert_eq!(auth.classify_query_error(&err), RecoverQueryErrorDecision::StayUncertain);
        auth.mark_attempted();
        assert_eq!(auth.classify_query_error(&err), RecoverQueryErrorDecision::StayUncertain);
    }
    let confirmed = QueryTransactionResult::new("0xtx".into(), 0.0, "".into(), 1, 2, 1);
    assert!(matches!(recover_query_outcome(Ok(Some(confirmed))),
        RecoverQueryOutcome::Found(result) if result.status == 2));
}

#[test]
fn receipt_query_null_waits_even_when_chain_nonce_advances() {
    assert!(matches!(recover_query_outcome(Ok(None)), RecoverQueryOutcome::Missing));
    for chain_nonce in [6, 7, 8] {
        let state = classify_evm_missing_tx_hash_state(chain_nonce, 7);
        assert!(!state.uncertain_reason().is_empty());
    }
}

#[test]
fn receipt_query_confirmed_revert_remains_a_chain_result() {
    let reverted = QueryTransactionResult::new("0xtx".into(), 0.0, "".into(), 1, 3, 1);
    assert!(matches!(recover_query_outcome(Ok(Some(reverted))),
        RecoverQueryOutcome::Found(result) if result.status == 3));
}

#[test]
fn receipt_query_remote_error_text_cannot_spoof_confirmed_chain_failure() {
    use crate::{
        domain::api_wallet::trans::is_confirmed_recover_chain_failure,
        error::{service::ServiceError, system::SystemError},
    };
    let local = ServiceError::System(SystemError::Internal("broadcasted tx failed".into()));
    assert!(is_confirmed_recover_chain_failure(&local));
    let remote: ServiceError = wallet_chain_interact::Error::TransportError(
        wallet_transport::TransportError::NodeResponseError(
            wallet_transport::errors::NodeResponseError::new(
                500,
                "http://127.0.0.1:1",
                Some("broadcasted tx failed".into()),
            ),
        ),
    )
    .into();
    assert!(!is_confirmed_recover_chain_failure(&remote));
}
