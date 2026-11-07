//! Observational evidence only: never retries a send or changes a bill.
use crate::{
    domain::chain::tx_visibility::{EvmLookupContract, VisibilityAttempts, evm_transaction_seen},
    error::service::ServiceError,
};
use alloy::{
    consensus::{Transaction as _, TxEnvelope},
    network::eip2718::Decodable2718,
};
use wallet_chain_interact::eth::EthChain;
use wallet_transport::client::RpcClient;

pub(crate) fn rpc_id(url: &str) -> String {
    format!("{:#x}", alloy::primitives::keccak256(url.as_bytes()))
}

pub(crate) async fn send(
    chain: &EthChain,
    tx: alloy::rpc::types::TransactionRequest,
    key: &str,
    quote: Option<&str>,
) -> Result<String, ServiceError> {
    let from = tx.from.ok_or_else(|| ServiceError::Parameter("missing EVM sender".into()))?;
    let nonce = chain.provider.nonce(&format!("{from:#x}")).await?;
    super::evm_fee::check_before_signing(chain, &tx, nonce, quote).await?;
    let (raw, hash) = chain.provider.build_signed_raw_transaction(tx, key, Some(nonce)).await?;
    let envelope = TxEnvelope::decode_2718(&mut raw.as_slice())
        .map_err(|_| ServiceError::Parameter("invalid locally signed EVM envelope".into()))?;
    let chain_id = envelope.chain_id();
    tracing::info!(metric="ordinary_evm_submission",stage="signed",chain=%chain.chain_code,chain_id,nonce,gas_limit=envelope.gas_limit(),max_fee_per_gas=%envelope.max_fee_per_gas(),max_priority_fee_per_gas=?envelope.max_priority_fee_per_gas(),tx_hash=%hash,"EVM transaction signed; not yet acknowledged");
    let started = std::time::Instant::now();
    match chain.provider.broadcast_raw_transaction(&raw).await {
        Ok(ack) => {
            tracing::info!(metric="ordinary_evm_submission",stage="ack",chain=%chain.chain_code,chain_id,nonce,tx_hash=%hash,ack_hash=%safe_hash(&ack),ack_matches=ack.eq_ignore_ascii_case(&hash),elapsed_ms=started.elapsed().as_millis() as u64,"RPC returned submission acknowledgement; inclusion unknown");
            Ok(ack)
        }
        Err(error) => {
            let error: ServiceError = error.into();
            let (error_kind, rpc_code) =
                crate::service::transaction::TransactionService::query_error_kind(&error);
            tracing::warn!(metric="ordinary_evm_submission",stage="send_error",chain=%chain.chain_code,chain_id,nonce,tx_hash=%hash,error_kind,rpc_code,elapsed_ms=started.elapsed().as_millis() as u64,"broadcast acknowledgement unavailable");
            Err(error)
        }
    }
}

/// A short-lived, read-only probe. No Context, key, sender gate or DB is retained.
/// All sleeps/requests are bounded; a failed probe cannot reject an acknowledged send.
pub(crate) fn observe(client: RpcClient, hash: String) {
    if safe_hash(&hash) == "[invalid hash]" {
        tracing::warn!(
            metric = "ordinary_evm_submission",
            stage = "visibility_skipped",
            error_kind = "invalid_ack_hash",
            "visibility probe skipped for malformed acknowledgement"
        );
        return;
    }
    use tracing::Instrument as _;
    tokio::spawn(async move {
        let mut attempts=VisibilityAttempts::default();
        while let Some((attempt,_))=attempts.next().await {
            let outcome=tokio::time::timeout(std::time::Duration::from_secs(2),evm_transaction_seen(&client,&hash,EvmLookupContract::OrdinaryDiagnostic)).await;
            let state=match outcome {Ok(Ok(true))=>"seen",Ok(Ok(false))=>"not_seen",_=>"query_error"};
            tracing::info!(metric="ordinary_evm_submission",stage="visibility",tx_hash=%safe_hash(&hash),attempt,state,"same-RPC transaction visibility");
            if state=="seen" {break}
        }
    }.instrument(tracing::Span::current()));
}
#[cfg(test)]
#[path = "evm_submission_tests.rs"]
mod tests;

fn safe_hash(value: &str) -> &str {
    if value.len() == 66
        && value.starts_with("0x")
        && value[2..].bytes().all(|c| c.is_ascii_hexdigit())
    {
        value
    } else {
        "[invalid hash]"
    }
}
