use crate::{
    context::{Context, get_context},
    error::service::ServiceError,
    infrastructure::api_trans::rpc_auth_retry_mechanism,
};
use std::future::Future;

pub(crate) fn should_retry_after_rpc_auth_error(err: &ServiceError) -> bool {
    err.is_rpc_auth_unauthorized()
}

pub(crate) async fn refresh_and_prepare_retry(
    ctx: &'static Context,
    chain_code: &str,
    operation: &'static str,
    resource_trade_no: &str,
    err: &ServiceError,
) -> Result<(), ServiceError> {
    rpc_auth_retry_mechanism::refresh_rpc_auth_and_prepare_retry(
        ctx,
        chain_code,
        operation,
        Some(resource_trade_no),
        err,
    )
    .await
}

pub(crate) async fn refresh_and_prepare_retry_from_global(
    chain_code: &str,
    operation: &'static str,
    resource_trade_no: &str,
    err: &ServiceError,
) -> Result<(), ServiceError> {
    refresh_and_prepare_retry(get_context()?, chain_code, operation, resource_trade_no, err).await
}

pub(crate) async fn run_with_rpc_auth_retry<T, Fut>(
    chain_code: &str,
    operation: &'static str,
    resource_trade_no: &str,
    mut action: impl FnMut() -> Fut,
) -> Result<T, ServiceError>
where
    Fut: Future<Output = Result<T, ServiceError>>,
{
    let mut auth_retry_attempted = false;

    loop {
        match action().await {
            Ok(value) => return Ok(value),
            Err(err) if !auth_retry_attempted && should_retry_after_rpc_auth_error(&err) => {
                auth_retry_attempted = true;
                refresh_and_prepare_retry_from_global(
                    chain_code,
                    operation,
                    resource_trade_no,
                    &err,
                )
                .await?;
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::should_retry_after_rpc_auth_error;

    #[test]
    fn resource_rpc_auth_retry_matches_backend_401() {
        let err = crate::error::service::ServiceError::TransportBackend(
            wallet_transport_backend::Error::ApiBackend(401, Some("Unauthorized".to_string())),
        );

        assert!(should_retry_after_rpc_auth_error(&err));
    }
}
