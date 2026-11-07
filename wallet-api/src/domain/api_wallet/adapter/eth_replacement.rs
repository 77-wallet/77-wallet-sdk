//! Signed-envelope validation and replacement fee floors. No signer or sender here.
use crate::{domain::api_wallet::adapter::tx::RawTx, error::service::ServiceError};
use alloy::{
    consensus::{Transaction as _, TxEnvelope},
    network::eip2718::Decodable2718,
    primitives::U256,
};
use serde::{Deserialize, Serialize};
use wallet_chain_interact::eth::FeeSetting;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReplacementFeeContext {
    pub old_raw: String,
    pub old_hash: String,
    pub from_addr: String,
    pub nonce: u64,
}
fn invalid() -> ServiceError {
    ServiceError::Parameter("invalid scoped EVM replacement envelope".into())
}
fn bump(v: u128) -> Result<U256, ServiceError> {
    v.checked_mul(125)
        .and_then(|v| v.checked_div(100))
        .and_then(|v| v.checked_add(1))
        .map(U256::from)
        .ok_or_else(invalid)
}
/// Absence means ordinary flow; malformed explicit context must fail closed.
pub(crate) fn context(
    metadata: Option<&str>,
) -> Result<Option<ReplacementFeeContext>, ServiceError> {
    let Some(meta) = metadata else { return Ok(None) };
    let value: serde_json::Value = match serde_json::from_str(meta) {
        Ok(v) => v,
        Err(_) if !meta.contains("collectReplacement") => return Ok(None),
        Err(_) => return Err(invalid()),
    };
    value
        .get("collectReplacement")
        .map(|v| serde_json::from_value(v.clone()).map_err(|_| invalid()))
        .transpose()
}
/// Explicit latest state excludes the competing pending spend without overriding balances.
pub(super) async fn estimate_gas_latest<F, Fut>(
    transaction: alloy::rpc::types::TransactionRequest,
    nonce: u64,
    rpc: F,
) -> Result<U256, ServiceError>
where
    F: Fn(&'static str, Vec<serde_json::Value>) -> Fut,
    Fut: std::future::Future<Output = Result<String, ServiceError>>,
{
    use alloy::network::TransactionBuilder as _;
    let result = rpc(
        "eth_estimateGas",
        vec![serde_json::json!(transaction.with_nonce(nonce)), serde_json::json!("latest")],
    )
    .await?;
    let gas = wallet_utils::unit::u256_from_str(&result)?;
    if gas == U256::ZERO {
        return Err(invalid());
    }
    gas.checked_mul(U256::from(10))
        .map(|v| v / U256::from(100))
        .and_then(|buffer| gas.checked_add(buffer))
        .ok_or_else(invalid)
}
pub(super) async fn latest_nonce<F, Fut>(address: &str, rpc: F) -> Result<u64, ServiceError>
where
    F: Fn(&'static str, Vec<serde_json::Value>) -> Fut,
    Fut: std::future::Future<Output = Result<String, ServiceError>>,
{
    let result = rpc(
        "eth_getTransactionCount",
        vec![serde_json::json!(address), serde_json::json!("latest")],
    )
    .await?;
    let n = wallet_utils::unit::u256_from_str(&result)?;
    u64::try_from(n).map_err(|_| invalid())
}
pub(super) fn apply(
    metadata: Option<&str>,
    chain_id: u64,
    from_addr: &str,
    nonce: u64,
    fee: &mut FeeSetting,
) -> Result<(), ServiceError> {
    let Some(c) = context(metadata)? else { return Ok(()) };
    if c.nonce != nonce || !c.from_addr.trim().eq_ignore_ascii_case(from_addr.trim()) {
        return Err(invalid());
    }
    let raw: RawTx = serde_json::from_str(&c.old_raw).map_err(|_| invalid())?;
    let RawTx::Evm(bytes, _) = raw else { return Err(invalid()) };
    let hash = format!("0x{}", hex::encode(alloy::primitives::keccak256(&bytes)));
    if !hash.eq_ignore_ascii_case(&c.old_hash) {
        return Err(invalid());
    }
    let mut input = bytes.as_slice();
    let tx = TxEnvelope::decode_2718(&mut input).map_err(|_| invalid())?;
    if !matches!(tx, TxEnvelope::Legacy(_) | TxEnvelope::Eip1559(_) | TxEnvelope::Eip2930(_)) {
        return Err(invalid());
    }
    if !input.is_empty() || tx.chain_id() != Some(chain_id) || tx.nonce() != c.nonce {
        return Err(invalid());
    }
    let signer = tx.recover_signer().map_err(|_| invalid())?;
    if !format!("{signer:#x}").eq_ignore_ascii_case(c.from_addr.trim()) {
        return Err(invalid());
    }
    fee.max_fee_per_gas = fee.max_fee_per_gas.max(bump(tx.max_fee_per_gas())?);
    fee.max_priority_fee_per_gas = fee
        .max_priority_fee_per_gas
        .max(bump(tx.max_priority_fee_per_gas().unwrap_or(tx.max_fee_per_gas()))?);
    fee.max_fee_per_gas = fee
        .max_fee_per_gas
        .max(fee.base_fee.checked_add(fee.max_priority_fee_per_gas).ok_or_else(invalid)?);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn replacement_estimation_uses_latest_original_value_and_fixed_nonce() {
        use serde_json::json;
        let tx=serde_json::from_value(json!({"from":"0x1111111111111111111111111111111111111111","to":"0x2222222222222222222222222222222222222222","value":"0x1c6ec6b3e6b7f","nonce":"0x7"})).unwrap();
        let gas = estimate_gas_latest(tx, 6, |method, params| async move {
            assert_eq!(method, "eth_estimateGas");
            assert_eq!(params.len(), 2);
            assert_eq!(params[1], "latest");
            assert_eq!(params[0]["nonce"], "0x6");
            assert_eq!(params[0]["value"], "0x1c6ec6b3e6b7f");
            assert_eq!(params[0]["from"], "0x1111111111111111111111111111111111111111");
            Ok("0x5208".into())
        })
        .await
        .unwrap();
        assert_eq!(gas, U256::from(23100));
    }
    #[tokio::test]
    async fn replacement_latest_reads_do_not_fallback_or_accept_overflow() {
        let nonce = latest_nonce("sender", |method, params| async move {
            assert_eq!(method, "eth_getTransactionCount");
            assert_eq!(
                params,
                serde_json::json!(["sender", "latest"]).as_array().unwrap().to_vec()
            );
            Ok("0x6".into())
        })
        .await
        .unwrap();
        assert_eq!(nonce, 6);
        assert!(latest_nonce("sender", |_, _| async { Err(ServiceError::Timeout) }).await.is_err());
        assert!(
            latest_nonce("sender", |_, _| async { Ok("0x10000000000000000".into()) })
                .await
                .is_err()
        );
        assert!(
            estimate_gas_latest(Default::default(), 6, |_, _| async {
                Ok(format!("{:#x}", U256::MAX))
            })
            .await
            .is_err()
        );
        assert!(
            estimate_gas_latest(Default::default(), 6, |_, _| async { Err(ServiceError::Timeout) })
                .await
                .is_err()
        );
    }

    #[test]
    fn replacement_fee_integer_bump_is_strict_and_checked() {
        assert_eq!(bump(100).unwrap(), U256::from(126));
        assert_eq!(bump(0).unwrap(), U256::from(1));
        assert!(bump(u128::MAX).is_err());
    }
    #[test]
    fn replacement_invalid_envelope_is_rejected() {
        let mut fee = FeeSetting {
            base_fee: U256::ZERO,
            max_fee_per_gas: U256::from(1),
            max_priority_fee_per_gas: U256::ZERO,
            gas_limit: U256::from(21000),
        };
        assert!(apply(Some(r#"{"collectReplacement":{"old_raw":"bad","old_hash":"bad","from_addr":"bad","nonce":6}}"#),534352,"bad",6,&mut fee).is_err());
        assert!(apply(None, 534352, "bad", 6, &mut fee).is_ok());
    }
    #[test]
    fn replacement_fee_validates_signed_identity_and_bumps_both_caps() {
        use alloy::{
            consensus::{SignableTransaction, TxEip1559},
            network::eip2718::Encodable2718,
            signers::{SignerSync, local::PrivateKeySigner},
        };
        let signer = PrivateKeySigner::random();
        let tx = TxEip1559 {
            chain_id: 534352,
            nonce: 6,
            gas_limit: 21000,
            max_fee_per_gas: 100,
            max_priority_fee_per_gas: 10,
            ..Default::default()
        };
        let signature = signer.sign_hash_sync(&tx.signature_hash()).unwrap();
        let tx: TxEnvelope = tx.into_signed(signature).into();
        let bytes = tx.encoded_2718();
        let context = ReplacementFeeContext {
            old_raw: serde_json::to_string(&RawTx::Evm(bytes.clone(), U256::ZERO)).unwrap(),
            old_hash: format!("0x{}", hex::encode(alloy::primitives::keccak256(&bytes))),
            from_addr: format!("{:#x}", signer.address()),
            nonce: 6,
        };
        let metadata = serde_json::json!({"collectReplacement":context}).to_string();
        let mut fee = FeeSetting {
            base_fee: U256::from(80),
            max_fee_per_gas: U256::from(90),
            max_priority_fee_per_gas: U256::from(1),
            gas_limit: U256::from(21000),
        };
        apply(Some(&metadata), 534352, &context.from_addr, 6, &mut fee).unwrap();
        assert_eq!(fee.max_fee_per_gas, U256::from(126));
        assert_eq!(fee.max_priority_fee_per_gas, U256::from(13));
        assert!(apply(Some(&metadata), 534352, &context.from_addr, 7, &mut fee).is_err());
        assert!(apply(Some(&metadata), 1, &context.from_addr, 6, &mut fee).is_err());
    }
}
