//! Shared API-wallet visibility protocol and short-check schedule.
//! Wallet owners retain acknowledgement, retry/auth, breaker and state decisions.
use crate::error::service::ServiceError;
use serde_json::Value;
use wallet_transport::{
    client::RpcClient,
    types::{JsonRpcParams, JsonRpcResult},
};

/// Preserve the two existing query contracts while sharing request/decoding.
#[derive(Clone, Copy)]
pub(crate) enum EvmLookupContract<'a> {
    /// Legacy invoke_request<Option<Value>>: missing/null result is EmptyResult;
    /// any non-null JSON is visible. Keep its RPC error URL/code for auth policy.
    ApiWallet(&'a str),
    /// Diagnostic only: explicit null is absent, and an object must match hash.
    OrdinaryDiagnostic,
}

pub(crate) async fn evm_transaction_seen(
    client: &RpcClient,
    hash: &str,
    contract: EvmLookupContract<'_>,
) -> Result<bool, ServiceError> {
    let body = client
        .set_params(JsonRpcParams::default().method("eth_getTransactionByHash").params(vec![hash]))
        .do_request()
        .await?;
    decode_evm_visibility(&body, hash, contract)
}
fn decode_evm_visibility(
    body: &str,
    hash: &str,
    contract: EvmLookupContract<'_>,
) -> Result<bool, ServiceError> {
    let response: JsonRpcResult<Option<Value>> = wallet_utils::serde_func::serde_from_str(body)
        .map_err(wallet_transport::TransportError::from)?;
    if let Some(error) = response.error {
        return Err(match contract {
            EvmLookupContract::ApiWallet(url) => {
                wallet_transport::TransportError::NodeResponseError(
                    wallet_transport::errors::NodeResponseError::new(
                        error.code,
                        url,
                        Some(error.message),
                    ),
                )
                .into()
            }
            EvmLookupContract::OrdinaryDiagnostic => {
                ServiceError::Parameter("visibility RPC error".into())
            }
        });
    }
    match response.result.flatten() {
        Some(tx) => match contract {
            EvmLookupContract::ApiWallet(_) => Ok(true),
            EvmLookupContract::OrdinaryDiagnostic => {
                if tx
                    .get("hash")
                    .and_then(Value::as_str)
                    .is_some_and(|observed| observed.eq_ignore_ascii_case(hash))
                {
                    Ok(true)
                } else {
                    Err(ServiceError::Parameter("visibility identity mismatch".into()))
                }
            }
        },
        None => match contract {
            EvmLookupContract::ApiWallet(_) => {
                Err(wallet_transport::TransportError::EmptyResult.into())
            }
            EvmLookupContract::OrdinaryDiagnostic => {
                let value: Value = wallet_utils::serde_func::serde_from_str(body)
                    .map_err(wallet_transport::TransportError::from)?;
                if value.get("result").is_some_and(Value::is_null) {
                    Ok(false)
                } else {
                    Err(ServiceError::Parameter("visibility result missing".into()))
                }
            }
        },
    }
}

// Extracted unchanged from API broadcast visibility checks.
const DELAYS_MS: [u64; 3] = [200, 500, 1000];
#[derive(Default)]
pub(crate) struct VisibilityAttempts {
    index: usize,
}
impl VisibilityAttempts {
    pub(crate) async fn next(&mut self) -> Option<(usize, &'static u64)> {
        let delay = DELAYS_MS.get(self.index)?;
        tokio::time::sleep(std::time::Duration::from_millis(*delay)).await;
        self.index += 1;
        Some((self.index, delay))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn shared_visibility_schedule_preserves_delays_count_and_cancellation() {
        let mut attempts = VisibilityAttempts::default();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), attempts.next())
                .await
                .is_err()
        );
        for (index, delay) in [200_u64, 500, 1000].into_iter().enumerate() {
            let start = tokio::time::Instant::now();
            assert_eq!(attempts.next().await, Some((index + 1, &delay)));
            assert!(start.elapsed() >= std::time::Duration::from_millis(delay));
        }
        assert_eq!(attempts.next().await, None);
    }
}
