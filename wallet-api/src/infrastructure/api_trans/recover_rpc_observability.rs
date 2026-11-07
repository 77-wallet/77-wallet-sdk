use crate::{context::Context, error::service::ServiceError, infrastructure::chain_rpc_guard};
use std::fmt::Display;

pub(crate) const RECOVER_OP_GET_ADAPTER: &str = "process_recovered_tx:get_adapter";
pub(crate) const RECOVER_OP_QUERY_TX_RES: &str = "process_recovered_tx:query_tx_res";
pub(crate) const RECOVER_OP_SOL_VISIBILITY_PROBE: &str =
    "process_recovered_tx:sol_visibility_probe";
pub(crate) const RECOVER_OP_TRON_PENDING_POOL: &str = "process_recovered_tx:tron_pending_pool";

pub(crate) async fn record_recover_rpc_success(ctx: &'static Context, chain_code: &str) {
    chain_rpc_guard::record_success_for_chain_code_with_ctx(ctx, chain_code).await;
}

fn recover_failure_log<'a>(
    chain_code: &'a str,
    tx_hash: &'a str,
    rpc: &'a str,
    op: &'a str,
) -> chain_rpc_guard::ChainRpcFailureLog<'a> {
    chain_rpc_guard::ChainRpcFailureLog::new()
        .trade_no(tx_hash)
        .tx_hash(tx_hash)
        .chain_code(chain_code)
        .rpc(rpc)
        .op(op)
}

pub(crate) async fn log_recover_query_error_with_ctx(
    ctx: &'static Context,
    chain_code: &str,
    tx_hash: &str,
    rpc: &str,
    service_err: &ServiceError,
    auth_retry_attempted: bool,
) {
    let failure_log = recover_failure_log(chain_code, tx_hash, rpc, RECOVER_OP_QUERY_TX_RES);
    let rpc_error = chain_rpc_guard::ChainRpcErrorContext::from_display(service_err);
    if rpc_error.is_transient() {
        failure_log
            .warn(&rpc_error, "查询链上状态失败(瞬时); recovered tx result will stay uncertain");
    } else {
        failure_log.error(&rpc_error, "查询链上状态失败; recovered tx result will stay uncertain");
    }
    rpc_error.record_transient_failure_for_chain_code_with_ctx(ctx, chain_code).await;
    if auth_retry_attempted {
        tracing::warn!(chain_code=%chain_code, rpc=%rpc, op=RECOVER_OP_QUERY_TX_RES, error=%service_err, "auth retry failed");
    }
}

pub(crate) async fn log_sol_visibility_probe_error_with_ctx(
    ctx: &'static Context,
    chain_code: &str,
    tx_hash: &str,
    rpc: &str,
    err: &impl Display,
) {
    let rpc_error = chain_rpc_guard::ChainRpcErrorContext::from_display(err);
    recover_failure_log(chain_code, tx_hash, rpc, RECOVER_OP_SOL_VISIBILITY_PROBE)
        .warn(&rpc_error, "Solana visibility probe failed during recover");
    rpc_error.record_transient_failure_for_chain_code_with_ctx(ctx, chain_code).await;
}

pub(crate) async fn log_tron_pending_pool_query_error_with_ctx(
    ctx: &'static Context,
    chain_code: &str,
    tx_hash: &str,
    rpc: &str,
    err: &impl Display,
) {
    let rpc_error = chain_rpc_guard::ChainRpcErrorContext::from_display(err);
    recover_failure_log(chain_code, tx_hash, rpc, RECOVER_OP_TRON_PENDING_POOL)
        .warn(&rpc_error, "Tron pending pool query failed; keep uncertain");
    rpc_error.record_transient_failure_for_chain_code_with_ctx(ctx, chain_code).await;
}

pub(crate) fn log_recover_query_error(
    chain_code: &str,
    tx_hash: &str,
    rpc: &str,
    service_err: &ServiceError,
    auth_retry_attempted: bool,
) {
    let failure_log = recover_failure_log(chain_code, tx_hash, rpc, RECOVER_OP_QUERY_TX_RES);
    let rpc_error = chain_rpc_guard::ChainRpcErrorContext::from_display(service_err);
    failure_log.warn(&rpc_error, "查询链上状态失败; recovered tx result will stay uncertain");
    rpc_error.record_transient_failure();
    if auth_retry_attempted {
        tracing::warn!(chain_code=%chain_code, rpc=%rpc, op=RECOVER_OP_QUERY_TX_RES, error=%service_err, "auth retry failed");
    }
}

pub(crate) fn log_sol_visibility_probe_error(
    chain_code: &str,
    tx_hash: &str,
    rpc: &str,
    err: &impl Display,
) {
    let rpc_error = chain_rpc_guard::ChainRpcErrorContext::from_display(err);
    recover_failure_log(chain_code, tx_hash, rpc, RECOVER_OP_SOL_VISIBILITY_PROBE)
        .warn(&rpc_error, "Solana visibility probe failed during recover");
    rpc_error.record_transient_failure();
}

pub(crate) fn log_tron_pending_pool_query_error(
    chain_code: &str,
    tx_hash: &str,
    rpc: &str,
    err: &impl Display,
) {
    let rpc_error = chain_rpc_guard::ChainRpcErrorContext::from_display(err);
    recover_failure_log(chain_code, tx_hash, rpc, RECOVER_OP_TRON_PENDING_POOL)
        .warn(&rpc_error, "Tron pending pool query failed; keep uncertain");
    rpc_error.record_transient_failure();
}

#[cfg(test)]
mod tests {
    use super::{
        log_recover_query_error, log_sol_visibility_probe_error, log_tron_pending_pool_query_error,
    };
    use crate::error::service::ServiceError;

    #[test]
    fn sol_visibility_probe_error_log_keeps_uncertain() {
        let err = ServiceError::Parameter("Node response error: code=503".to_string());

        log_sol_visibility_probe_error("sol", "solsig", "https://sol.example", &err);
    }

    #[test]
    fn tron_pending_pool_query_error_log_keeps_uncertain() {
        let err = ServiceError::Parameter("Node response error: code=503".to_string());

        log_tron_pending_pool_query_error("tron", "0xtx", "https://api.nileex.io", &err);
    }

    #[test]
    fn recover_query_error_log_keeps_uncertain_for_503() {
        let err = ServiceError::Parameter(
            "Node response error: code=503, rpc=https://api.nileex.io/wallet/gettransactioninfobyid"
                .to_string(),
        );

        log_recover_query_error("tron", "0xtx", "https://api.nileex.io", &err, false);
    }

    #[test]
    fn recover_query_error_log_includes_auth_retry_failure() {
        let err = ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(
            401,
            Some("Unauthorized".to_string()),
        ));

        log_recover_query_error("tron", "0xtx", "https://api.nileex.io", &err, true);
    }
}
