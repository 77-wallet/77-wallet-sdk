//! Historical state locates a candidate; only finalized block inclusion proves conflict.
use std::future::Future;

use serde_json::{Value, json};
use wallet_database::repositories::api_wallet::collect::FinalizedCollectNonceConflict;

use crate::error::service::ServiceError;

fn quantity(value: &Value) -> Option<u64> {
    u64::from_str_radix(value.as_str()?.strip_prefix("0x")?, 16).ok()
}

fn hex(value: &str, bytes: usize) -> bool {
    value.len() == 2 + bytes * 2
        && value.starts_with("0x")
        && value[2..].bytes().all(|c| c.is_ascii_hexdigit())
}

pub(super) async fn probe<F, Fut>(
    chain_code: &str,
    address: &str,
    nonce: i32,
    original_hash: &str,
    rpc: F,
) -> Result<Option<FinalizedCollectNonceConflict>, ServiceError>
where
    F: Fn(&'static str, Vec<Value>) -> Fut,
    Fut: Future<Output = Result<Value, ServiceError>>,
{
    let address = address.trim();
    if !crate::domain::api_wallet::trans::shared_mechanism::is_evm_chain(chain_code)
        || nonce < 0
        || !hex(address, 20)
        || !hex(original_hash, 32)
    {
        return Ok(None);
    }
    let anchor = rpc("eth_getBlockByNumber", vec![json!("finalized"), json!(false)]).await?;
    let Some(height) = quantity(&anchor["number"]).filter(|n| *n > 0) else {
        return Ok(None);
    };
    let Some(anchor_hash) = anchor["hash"].as_str().filter(|h| hex(h, 32)) else {
        return Ok(None);
    };
    // Pin the upper-bound state to the finalized block, not pending/latest state.
    // Nodes without historical/EIP-1898 support return an error and remain uncertain.
    let count = rpc(
        "eth_getTransactionCount",
        vec![json!(address), json!({"blockHash": anchor_hash, "requireCanonical": true})],
    )
    .await?;
    if !quantity(&count).is_some_and(|n| n > nonce as u64) {
        return Ok(None);
    }

    // Search recent history first: ordinary full nodes can resolve recent conflicts
    // without reading state halfway back to genesis. Both search phases share a
    // 64-query budget and the adapter's total timeout.
    let mut budget = 64;
    let mut step = 1_u64;
    let mut high = height;
    let mut low;
    loop {
        if budget == 0 {
            return Ok(None);
        }
        budget -= 1;
        let previous = height.saturating_sub(step);
        let count =
            rpc("eth_getTransactionCount", vec![json!(address), json!(format!("0x{previous:x}"))])
                .await?;
        let Some(count) = quantity(&count) else {
            return Ok(None);
        };
        if count <= nonce as u64 {
            low = previous + 1;
            break;
        }
        if previous == 0 {
            return Ok(None);
        }
        high = previous;
        step = step.saturating_mul(2);
    }
    // The first block with a larger account nonce must contain the consuming tx.
    while low < high {
        if budget == 0 {
            return Ok(None);
        }
        budget -= 1;
        let mid = low + (high - low) / 2;
        let count =
            rpc("eth_getTransactionCount", vec![json!(address), json!(format!("0x{mid:x}"))])
                .await?;
        let Some(count) = quantity(&count) else {
            return Ok(None);
        };
        if count > nonce as u64 {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    let block = rpc("eth_getBlockByNumber", vec![json!(format!("0x{low:x}")), json!(true)]).await?;
    if quantity(&block["number"]) != Some(low) {
        return Ok(None);
    }
    let Some(block_hash) = block["hash"].as_str().filter(|h| hex(h, 32)) else {
        return Ok(None);
    };
    let Some(transactions) = block["transactions"].as_array() else {
        return Ok(None);
    };
    let mut matching = transactions.iter().filter(|tx| {
        tx["from"].as_str().is_some_and(|from| from.eq_ignore_ascii_case(address))
            && quantity(&tx["nonce"]) == Some(nonce as u64)
    });
    let Some(tx) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some()
        || quantity(&tx["blockNumber"]) != Some(low)
        || !tx["blockHash"].as_str().is_some_and(|hash| hash.eq_ignore_ascii_case(block_hash))
    {
        return Ok(None);
    }
    let Some(hash) = tx["hash"].as_str().filter(|h| hex(h, 32)) else {
        return Ok(None);
    };
    if hash.eq_ignore_ascii_case(original_hash) {
        return Ok(None);
    }
    // Recheck candidate inclusion against the canonical header, including when the
    // consuming block is itself the finalized anchor.
    let canonical_block =
        rpc("eth_getBlockByNumber", vec![json!(format!("0x{low:x}")), json!(false)]).await?;
    if quantity(&canonical_block["number"]) != Some(low)
        || !canonical_block["hash"]
            .as_str()
            .is_some_and(|hash| hash.eq_ignore_ascii_case(block_hash))
        || (low == height && !block_hash.eq_ignore_ascii_case(anchor_hash))
    {
        return Ok(None);
    }
    // Reject inconsistent/reorganized observations before emitting a durable failure fact.
    let canonical_anchor =
        rpc("eth_getBlockByNumber", vec![json!(format!("0x{height:x}")), json!(false)]).await?;
    if quantity(&canonical_anchor["number"]) != Some(height)
        || !canonical_anchor["hash"]
            .as_str()
            .is_some_and(|hash| hash.eq_ignore_ascii_case(anchor_hash))
    {
        return Ok(None);
    }
    Ok(Some(FinalizedCollectNonceConflict {
        chain_code: chain_code.to_owned(),
        from_addr: address.to_owned(),
        nonce,
        conflicting_tx_hash: hash.to_owned(),
        block_hash: block_hash.to_owned(),
        block_number: low,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const ADDRESS: &str = "0x08932ba7a633FdEA40Dfd68afab610340e6375BF";

    async fn run_on_chain(
        chain_code: &str,
        mode: &str,
    ) -> (Result<Option<FinalizedCollectNonceConflict>, ServiceError>, usize) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let probe_address =
            if mode == "padded" { format!(" {ADDRESS} ") } else { ADDRESS.to_owned() };
        let result = probe(chain_code, &probe_address, 1, &format!("0x{}", "11".repeat(32)), |method, params| {
            recorded.lock().unwrap().push((method, params.clone()));
            let value = match method {
                "eth_getTransactionCount" if params[1].is_object() => {
                    assert_eq!(params[0], ADDRESS);
                    assert_eq!(params[1]["requireCanonical"], true);
                    json!(if mode == "pending_only" { "0x1" } else { "0x2" })
                }
                "eth_getTransactionCount" => {
                    let height = quantity(&params[1]).unwrap();
                    json!(if mode == "budget_exhausted" || height >= 8 { "0x2" } else { "0x1" })
                }
                "eth_getBlockByNumber" if params[1] == true => {
                    assert_eq!(params[0], "0x8");
                    let block_hash = format!("0x{}", "aa".repeat(32));
                    let mut tx = json!({
                        "from": ADDRESS.to_ascii_lowercase(), "nonce": "0x1",
                        "hash": format!("0x{}", if mode == "original" { "11" } else { "22" }.repeat(32)),
                        "blockNumber": "0x8", "blockHash": block_hash,
                    });
                    match mode {
                        "wrong_sender" => tx["from"] = json!(format!("0x{}", "33".repeat(20))),
                        "wrong_nonce" => tx["nonce"] = json!("0x2"),
                        "wrong_block" => tx["blockHash"] = json!(format!("0x{}", "bb".repeat(32))),
                        "invalid_hash" => tx["hash"] = json!("0xbad"),
                        _ => {}
                    }
                    json!({"number":"0x8", "hash":block_hash,
                        "transactions": if mode == "missing_tx" { vec![] } else { vec![tx] }})
                }
                "eth_getBlockByNumber" if params[0] == "0x8" => json!({
                    "number":"0x8", "hash":format!("0x{}",
                        if mode == "changed_candidate" { "bb" } else { "aa" }.repeat(32))
                }),
                "eth_getBlockByNumber" => json!({"number": if mode == "budget_exhausted" { "0xffffffffffffffff" } else { "0x10" }, "hash":format!("0x{}",
                    if mode == "changed_anchor" && params[0] != "finalized" { "bb" } else { "aa" }.repeat(32))}),
                _ => panic!("unexpected RPC {method}"),
            };
            async move {
                if mode == "rpc_error" || (mode == "historical_error" && method == "eth_getTransactionCount" && params[1].is_string()) { Err(ServiceError::Timeout) }
                else if mode == "missing_block" { Ok(Value::Null) }
                else { Ok(value) }
            }
        }).await;
        let count = calls.lock().unwrap().len();
        (result, count)
    }

    async fn run(
        mode: &str,
    ) -> (Result<Option<FinalizedCollectNonceConflict>, ServiceError>, usize) {
        run_on_chain("eth", mode).await
    }

    #[tokio::test]
    async fn finalized_inclusion_proves_different_transaction_consumed_nonce() {
        for mode in ["conflict", "padded"] {
            let (result, calls) = run(mode).await;
            let evidence = result.unwrap().unwrap();
            assert_eq!(evidence.nonce, 1);
            assert_eq!(evidence.block_number, 8);
            assert_eq!(evidence.conflicting_tx_hash, format!("0x{}", "22".repeat(32)));
            assert!(calls <= 13);
            assert_eq!(evidence.from_addr, ADDRESS);
        }
    }

    #[tokio::test]
    async fn added_evm_chain_can_prove_nonce_conflict() {
        let (result, calls) = run_on_chain("polygon", "conflict").await;

        let evidence = result.unwrap().unwrap();
        assert_eq!(evidence.chain_code, "polygon");
        assert_eq!(evidence.nonce, 1);
        assert!(calls <= 13);
    }

    #[tokio::test]
    async fn incomplete_or_inconsistent_observations_never_prove_failure() {
        for mode in [
            "pending_only",
            "original",
            "wrong_sender",
            "wrong_nonce",
            "wrong_block",
            "invalid_hash",
            "missing_tx",
            "changed_anchor",
            "changed_candidate",
            "missing_block",
            "budget_exhausted",
        ] {
            let (result, calls) = run(mode).await;
            assert!(result.unwrap().is_none(), "{mode}");
            assert!(calls <= 66, "bounded historical search: {mode}");
        }
    }

    #[tokio::test]
    async fn unavailable_rpc_is_an_observation_error() {
        for mode in ["rpc_error", "historical_error"] {
            let (result, calls) = run(mode).await;
            assert!(matches!(result, Err(ServiceError::Timeout)));
            assert!(calls <= 3);
        }
    }
}
