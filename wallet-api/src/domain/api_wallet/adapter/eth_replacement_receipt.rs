//! Selected-RPC canonical receipt proof, using normal collect inclusion semantics.
use crate::{domain::api_wallet::adapter::tx::ReplacementReceipt, error::service::ServiceError};
use alloy::primitives::U256;
use serde_json::{Value, json};
use std::future::Future;
fn quantity(v: &Value) -> Option<u64> {
    u64::from_str_radix(v.as_str()?.strip_prefix("0x")?, 16).ok()
}
pub(super) async fn probe<F, Fut>(
    hash: &str,
    address: &str,
    nonce: u64,
    requires_l1_fee: bool,
    rpc: F,
) -> Result<Option<ReplacementReceipt>, ServiceError>
where
    F: Fn(&'static str, Vec<Value>) -> Fut,
    Fut: Future<Output = Result<Value, ServiceError>>,
{
    let receipt = rpc("eth_getTransactionReceipt", vec![json!(hash)]).await?;
    if receipt.is_null() {
        return Ok(None);
    }
    let Some(height) = quantity(&receipt["blockNumber"]).filter(|n| *n > 0) else {
        return Ok(None);
    };
    let Some(block_hash) = receipt["blockHash"].as_str().filter(|h| h.len() == 66) else {
        return Ok(None);
    };
    if !receipt["transactionHash"].as_str().is_some_and(|h| h.eq_ignore_ascii_case(hash)) {
        return Ok(None);
    }
    let Some(status) = quantity(&receipt["status"]).filter(|n| *n <= 1) else { return Ok(None) };
    let tx = rpc("eth_getTransactionByHash", vec![json!(hash)]).await?;
    if !tx["hash"].as_str().is_some_and(|h| h.eq_ignore_ascii_case(hash))
        || !tx["from"].as_str().is_some_and(|a| a.eq_ignore_ascii_case(address.trim()))
        || quantity(&tx["nonce"]) != Some(nonce)
        || quantity(&tx["blockNumber"]) != Some(height)
        || tx["blockHash"].as_str() != Some(block_hash)
    {
        return Ok(None);
    }
    // Linked old/new transactions follow the existing collect receipt policy:
    // verify canonical inclusion, without introducing an L1-finalized gate.
    let block =
        rpc("eth_getBlockByNumber", vec![json!(format!("0x{height:x}")), json!(false)]).await?;
    if quantity(&block["number"]) != Some(height)
        || block["hash"].as_str() != Some(block_hash)
        || !block["transactions"].as_array().is_some_and(|ts| {
            ts.iter().any(|t| t.as_str().is_some_and(|h| h.eq_ignore_ascii_case(hash)))
        })
    {
        return Ok(None);
    }
    let Some(timestamp) = quantity(&block["timestamp"]) else { return Ok(None) };
    let Ok(timestamp) = i64::try_from(timestamp) else { return Ok(None) };
    let Some(time) = chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0) else {
        return Ok(None);
    };
    let checked_block =
        rpc("eth_getBlockByNumber", vec![json!(format!("0x{height:x}")), json!(false)]).await?;
    if quantity(&checked_block["number"]) != Some(height)
        || checked_block["hash"].as_str() != Some(block_hash)
    {
        return Ok(None);
    }
    let parse = |key: &str| {
        receipt[key]
            .as_str()
            .and_then(|s| s.strip_prefix("0x"))
            .and_then(|s| U256::from_str_radix(s, 16).ok())
    };
    let (Some(gas), Some(price)) = (parse("gasUsed"), parse("effectiveGasPrice")) else {
        return Ok(None);
    };
    let l1 = if requires_l1_fee {
        let Some(l1) = parse("l1Fee") else {
            return Ok(None);
        };
        l1
    } else {
        U256::ZERO
    };
    let Some(fee) = gas.checked_mul(price).and_then(|n| n.checked_add(l1)) else { return Ok(None) };
    Ok(Some(ReplacementReceipt {
        transaction_fee: wallet_utils::unit::format_to_string(fee, 18)?,
        resource_consume: gas.to_string(),
        tx_hash: hash.to_owned(),
        success: status == 1,
        block_hash: block_hash.to_owned(),
        block_number: height,
        transaction_time: time.to_rfc3339(),
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn replacement_missing_receipt_never_proves_consumption() {
        assert!(
            probe("h", "a", 6, true, |_, _| async { Ok(Value::Null) }).await.unwrap().is_none()
        );
    }
    #[tokio::test]
    async fn replacement_rpc_error_never_proves_consumption() {
        assert!(
            probe("h", "a", 6, true, |_, _| async { Err(ServiceError::Timeout) }).await.is_err()
        );
    }
    #[tokio::test]
    async fn replacement_receipt_checks_winner_identity_canonical_block_and_revert() {
        let hash = format!("0x{}", "11".repeat(32));
        let block = format!("0x{}", "22".repeat(32));
        for (
            status,
            tx_nonce,
            wrong_sender,
            changed_block,
            wrong_height,
            requires_l1,
            l1_present,
            expected,
        ) in [
            (1, 6, false, false, false, true, true, Some(true)),
            (0, 6, false, false, false, true, true, Some(false)),
            (1, 7, false, false, false, true, true, None),
            (1, 6, true, false, false, true, true, None),
            (1, 6, false, true, false, true, true, None),
            (1, 6, false, false, true, true, true, None),
            (1, 6, false, false, false, false, false, Some(true)),
            (1, 6, false, false, false, true, false, None),
            (2, 6, false, false, false, true, true, None),
        ] {
            let reads = std::sync::atomic::AtomicUsize::new(0);
            let result=probe(&hash,"sender",6,requires_l1,|method,params|{
                let hash=hash.clone();let block=block.clone();
                assert!(method!="eth_getBlockByNumber" || params[0]!="finalized","linked replacement must follow normal included-receipt policy");
                let second=method=="eth_getBlockByNumber" && reads.fetch_add(1,std::sync::atomic::Ordering::SeqCst)>0;
                async move {Ok(match method {
                    "eth_getTransactionReceipt"=>json!({"transactionHash":hash,"blockHash":block,"blockNumber":"0x2a","status":format!("0x{status:x}"),"gasUsed":"0x5208","effectiveGasPrice":"0x1","l1Fee":if l1_present {json!("0x7")} else {Value::Null}}),
                    "eth_getTransactionByHash"=>json!({"hash":hash,"from":if wrong_sender{"wrong"}else{"sender"},"nonce":format!("0x{tx_nonce:x}"),"blockNumber":"0x2a","blockHash":block}),
                    "eth_getBlockByNumber"=>json!({"number":if wrong_height{"0x2b"}else{"0x2a"},"hash":if changed_block && second{format!("0x{}","33".repeat(32))}else{block},"transactions":[hash],"timestamp":"0x68f40000"}),
                    _=>panic!("unexpected proof method: {method}"),
                })}
            }).await.unwrap();
            if let Some(ref r) = result {
                assert_eq!(
                    r.transaction_fee,
                    if requires_l1 { "0.000000000000021007" } else { "0.000000000000021" }
                );
                assert_eq!(r.resource_consume, "21000");
            }
            assert_eq!(result.map(|r| r.success), expected);
        }
    }
}
