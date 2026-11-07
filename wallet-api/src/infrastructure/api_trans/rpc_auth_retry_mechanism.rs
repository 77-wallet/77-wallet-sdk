use crate::{
    context::Context, domain::api_wallet::adapter_factory::ApiChainAdapterFactory,
    error::service::ServiceError,
};

pub(crate) async fn refresh_rpc_auth_and_prepare_retry(
    ctx: &'static Context,
    chain_code: &str,
    op: &str,
    rpc: Option<&str>,
    err: &ServiceError,
) -> Result<(), ServiceError> {
    let rpc = rpc.unwrap_or("<unknown>");
    tracing::warn!(
        chain_code = %chain_code,
        op = %op,
        rpc = %rpc,
        error = %err,
        retry = "auth_refresh_once",
        "rpc auth unauthorized detected"
    );

    ctx.invalidate_rpc_token_cache().await;

    tracing::warn!(
        chain_code = %chain_code,
        op = %op,
        rpc = %rpc,
        client_id = %ctx.get_client_id(),
        "force refresh rpc token"
    );
    match ctx.get_rpc_header_force_refresh().await {
        Ok(_) => {
            tracing::info!(
                chain_code = %chain_code,
                op = %op,
                rpc = %rpc,
                client_id = %ctx.get_client_id(),
                "force refresh rpc token success"
            );
        }
        Err(refresh_err) => {
            tracing::error!(
                chain_code = %chain_code,
                op = %op,
                rpc = %rpc,
                error = %refresh_err,
                "force refresh rpc token failed"
            );
            return Err(refresh_err);
        }
    }

    let removed_count = ApiChainAdapterFactory::invalidate_all_cached_adapters();
    tracing::warn!(
        chain_code = %chain_code,
        op = %op,
        rpc = %rpc,
        removed_count = removed_count,
        "invalidate cached adapters after rpc auth refresh"
    );
    Ok(())
}
