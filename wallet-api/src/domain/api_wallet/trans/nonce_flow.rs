use super::ApiTransDomain;
use crate::{
    context::Context, domain::api_wallet::adapter_factory::ApiChainAdapterFactory,
    error::service::ServiceError, infrastructure::api_trans::rpc_auth_retry_mechanism,
};
use std::time::Instant;
use wallet_utils::RetryableError as _;

impl ApiTransDomain {
    pub async fn nonce(
        ctx: &'static Context,
        from_addr: &str,
        chain_code: &str,
    ) -> Result<u64, ServiceError> {
        Self::nonce_with_ctx(ctx, from_addr, chain_code).await
    }

    pub async fn nonce_with_ctx(
        ctx: &'static Context,
        from_addr: &str,
        chain_code: &str,
    ) -> Result<u64, ServiceError> {
        let start_time = Instant::now();
        tracing::info!(
            "nonce (开始): from_addr: {}, chain_code: {}, 时间: {:?}",
            from_addr,
            chain_code,
            start_time
        );

        let chain_code_time = Instant::now();
        tracing::info!("nonce (链代码转换): 完成, 耗时: {:?}", chain_code_time.elapsed());

        let adapter_time = Instant::now();
        let mut auth_retry_attempted = false;
        loop {
            let adapter =
                match ApiChainAdapterFactory::get_transaction_adapter_with_ctx(ctx, chain_code)
                    .await
                {
                    Ok(adapter) => adapter,
                    Err(e) if !auth_retry_attempted && e.is_rpc_auth_unauthorized() => {
                        auth_retry_attempted = true;
                        rpc_auth_retry_mechanism::refresh_rpc_auth_and_prepare_retry(
                            ctx,
                            chain_code,
                            "nonce:get_adapter",
                            None,
                            &e,
                        )
                        .await?;
                        continue;
                    }
                    Err(e) => {
                        tracing::info!("nonce (结束): 总耗时: {:?}", start_time.elapsed());
                        return Err(e);
                    }
                };
            tracing::info!("nonce (适配器创建): 完成, 耗时: {:?}", adapter_time.elapsed());
            let rpc = adapter.rpc_endpoint_for_log().unwrap_or_else(|| "<unknown>".to_string());

            match adapter.nonce(from_addr).await {
                Ok(resp) => {
                    tracing::info!("nonce (结束): 总耗时: {:?}", start_time.elapsed());
                    if auth_retry_attempted {
                        tracing::info!(chain_code=%chain_code, rpc=%rpc, op="nonce", "auth retry succeeded");
                    }
                    return Ok(resp);
                }
                Err(e) if !auth_retry_attempted && e.is_rpc_auth_unauthorized() => {
                    auth_retry_attempted = true;
                    rpc_auth_retry_mechanism::refresh_rpc_auth_and_prepare_retry(
                        ctx,
                        chain_code,
                        "nonce:rpc_call",
                        Some(&rpc),
                        &e,
                    )
                    .await?;
                    continue;
                }
                Err(e) => {
                    tracing::info!("nonce (结束): 总耗时: {:?}", start_time.elapsed());
                    if auth_retry_attempted {
                        tracing::warn!(chain_code=%chain_code, rpc=%rpc, op="nonce", error=%e, "auth retry failed");
                    }
                    return Err(e);
                }
            }
        }
    }
}
