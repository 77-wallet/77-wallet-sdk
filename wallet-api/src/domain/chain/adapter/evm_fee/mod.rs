use crate::{
    domain::chain::transaction::ChainTransDomain,
    error::service::ServiceError,
    request::transaction::BaseTransferReq,
    response_vo::{FeeDetails, scroll_fee_transaction},
};
use alloy::primitives::U256;
use wallet_chain_interact::{
    eth::{EthChain, FeeSetting, operations::TransferOpt},
    types::Transaction as _,
};
use wallet_transport_backend::{api::BackendApi, response_vo::coin::TokenCurrency};
use wallet_types::chain::chain::ChainCode;
use wallet_utils::unit;

#[cfg(test)]
mod flow_tests;

#[cfg(all(test, feature = "smoke-live"))]
#[path = "../../../../../tests/smoke/transactions/evm_fee_live.rs"]
mod live_smoke_tests;

pub(super) fn execution_fee_reserve(chain_code: ChainCode, fee: &FeeSetting) -> U256 {
    match chain_code {
        ChainCode::BnbSmartChain => fee.transaction_fee(),
        _ => fee.gas_limit * fee.max_fee_per_gas,
    }
}

pub(super) async fn required_send_fee(
    chain: &EthChain,
    transfer: &TransferOpt,
    execution_fee: U256,
    fee_setting: &FeeSetting,
) -> Result<U256, ServiceError> {
    let mut tx = transfer.build_transaction()?;
    if chain.chain_code == ChainCode::Scroll {
        tx = scroll_fee_transaction(tx, fee_setting)?;
    }
    let additional_fee = chain.estimate_additional_fee(tx).await?;
    if chain.chain_code == ChainCode::Scroll {
        tracing::info!(target: "scroll_diagnostic", stage = "send_preflight",
            from = %transfer.base.from, to = %transfer.base.to, token = ?transfer.contract,
            value_wei = %transfer.base.value, execution_reserve_wei = %execution_fee,
            estimated_l1_fee_wei = %additional_fee.unwrap_or_default(),
            required_fee_wei = %(execution_fee + additional_fee.unwrap_or_default()),
            "Scroll diagnostic");
    }
    Ok(execution_fee + additional_fee.unwrap_or_default())
}

/// Reserve the maximum execution fee plus additional fees for the final value.
/// Keep the largest reservation seen so fluctuating estimates cannot release it.
pub(super) async fn prepare_native_maximum(
    chain: &EthChain,
    transfer: &mut TransferOpt,
    balance: U256,
    fee_setting: &mut FeeSetting,
    is_contract: bool,
) -> Result<U256, ServiceError> {
    let mut reserve = execution_fee_reserve(chain.chain_code, fee_setting);
    for _ in 0..4 {
        if balance <= reserve {
            return Err(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::InsufficientFeeBalance,
            ))?;
        }
        let amount = balance - reserve;
        transfer.base.value = amount;
        if is_contract {
            // Payable contracts can consume different gas for different values.
            let gas = chain.provider.estimate_gas(transfer.build_transaction()?).await?;
            fee_setting.gas_limit = fee_setting.gas_limit.max(gas);
        }
        let execution_reserve = execution_fee_reserve(chain.chain_code, fee_setting);
        let required = required_send_fee(chain, transfer, execution_reserve, fee_setting).await?;
        if required <= reserve {
            return Ok(amount);
        }
        reserve = required;
    }
    // No signing or broadcast until a final-amount estimate fits the reservation.
    Err(crate::error::business::BusinessError::Chain(
        crate::error::business::chain::ChainError::InsufficientFeeBalance,
    ))?
}

pub(super) fn ensure_send_fee_balance(
    available_native: U256,
    required_fee: U256,
) -> Result<(), ServiceError> {
    if available_native < required_fee {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }
    Ok(())
}

pub(super) async fn estimate_transfer_fee(
    chain: &EthChain,
    req: &BaseTransferReq,
    backend_api: &BackendApi,
    currency: &str,
    token_currency: TokenCurrency,
) -> Result<String, ServiceError> {
    let token_address = req.token_address.to_chain_token_option();
    let value = unit::convert_to_u256(&req.value, req.decimals)?;
    let balance = chain.balance(&req.from, token_address.clone()).await?;
    let value = if req.spend_all && token_address.is_none() { balance } else { value };
    if balance < value {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                crate::error::business::chain::InsufficientBalanceDetail::new()
                    .from_addr(req.from.clone())
                    .to_addr(req.to.clone())
                    .chain_code(req.chain_code.clone())
                    .token_addr(token_address.clone().unwrap_or_default())
                    .value(value.to_string())
                    .balance(balance.to_string())
                    .need(value.to_string())
                    .reason("token balance is insufficient for fee estimation"),
            ),
        ))?;
    }

    let quote_id = uuid::Uuid::new_v4().to_string();
    let quoted_at_ms = chrono::Utc::now().timestamp_millis();
    let (gas_oracle, quote_source, quote_block, quote_base) =
        fresh_gas_oracle(chain, backend_api, &quote_id).await?;
    let params = TransferOpt::new(&req.from, &req.to, value, token_address)?;
    let tx = params.build_transaction()?;
    // Scroll needs gas/fee widths before the L1 reserve; retain other chains' order.
    let additional_fee = if chain.chain_code == ChainCode::Scroll {
        None
    } else {
        chain.estimate_additional_fee(tx.clone()).await?
    };
    tracing::info!(target: "evm_fee_diagnostic", stage = "estimate_gas_request",
        chain_code = ?chain.chain_code, from = %params.base.from, to = %params.base.to,
        rpc_method = "eth_estimateGas", gas_limit = ?tx.gas, gas_price_wei = ?tx.gas_price,
        max_fee_per_gas_wei = ?tx.max_fee_per_gas,
        max_priority_fee_per_gas_wei = ?tx.max_priority_fee_per_gas,
        "EVM fee estimateGas request");
    let gas = match chain.estimate_gas(params).await {
        Ok(gas) => {
            tracing::info!(target: "evm_fee_diagnostic", stage = "estimate_gas_success",
                chain_code = ?chain.chain_code, from = %req.from, to = %req.to,
                gas_limit_with_margin = gas.consume,
                "EVM fee estimateGas succeeded");
            gas
        }
        Err(error) => {
            // NodeResponseError's Display includes the RPC URL, which may contain credentials.
            let (error_kind, rpc_code, rpc_message) = match &error {
                wallet_chain_interact::Error::TransportError(
                    wallet_transport::errors::TransportError::NodeResponseError(node),
                ) => ("node_response", Some(node.code), node.message.as_deref()),
                wallet_chain_interact::Error::TransportError(_) => ("transport", None, None),
                _ => ("estimate_gas", None, None),
            };
            tracing::warn!(target: "evm_fee_diagnostic", stage = "estimate_gas_error",
                chain_code = ?chain.chain_code, from = %req.from, to = %req.to,
                error_kind, rpc_code, rpc_message,
                "EVM fee estimateGas failed");
            return Err(error.into());
        }
    };
    let fee = FeeDetails::for_standard_transfer(gas_oracle, gas.consume, chain.chain_code)?;
    let additional_fee = if chain.chain_code == ChainCode::Scroll {
        chain
            .estimate_additional_fee(scroll_fee_transaction(tx, &fee.reserve_fee_setting()?)?)
            .await?
    } else {
        additional_fee
    };
    if chain.chain_code == ChainCode::Scroll {
        tracing::info!(target: "scroll_diagnostic", stage = "fee_quote",
            from = %req.from, to = %req.to, token = ?req.token_address.to_chain_token_option(),
            value_wei = %value, spend_all = req.spend_all,
            estimated_l1_fee_wei = %additional_fee.unwrap_or_default(),
            "Scroll diagnostic");
    }
    let fee = match additional_fee {
        Some(additional_fee) => {
            let fee = fee.with_additional_native_fee(additional_fee)?;
            if chain.chain_code == ChainCode::Base {
                fee.with_base_l1_fee_margin(additional_fee)?
            } else {
                fee
            }
        }
        None => fee,
    };
    let mut response =
        wallet_utils::serde_func::serde_to_value(&fee.to_resp(token_currency, currency)?)?;
    // Optional diagnostics remain outside execution settings; older clients ignore them.
    if checks_mainnet_fee(chain) {
        response["quoteId"] = serde_json::json!(quote_id);
        response["quotedAtMs"] = serde_json::json!(quoted_at_ms);
        response["quoteSource"] = serde_json::json!(quote_source);
        tracing::info!(metric="ordinary_evm_fee", stage="quote", chain=%chain.chain_code,
            from=%req.from, to=%req.to, quote_id, quoted_at_ms, quote_source,
            block_number=?quote_block, current_base_fee_wei=?quote_base,
            tiers=%response["data"], "ordinary EVM fee quote prepared");
    }
    Ok(wallet_utils::serde_func::serde_to_string(&response)?)
}

/// BNB retains its special fee field mapping; testnets retain Core fee refresh.
pub(super) fn checks_mainnet_fee(chain: &EthChain) -> bool {
    chain.network == wallet_types::chain::network::NetworkKind::Mainnet
        && matches!(
            chain.chain_code,
            ChainCode::Ethereum
                | ChainCode::Polygon
                | ChainCode::Avalanche
                | ChainCode::Arbitrum
                | ChainCode::Base
                | ChainCode::Scroll
        )
}

async fn fresh_gas_oracle(
    chain: &EthChain,
    backend: &BackendApi,
    quote_id: &str,
) -> Result<
    (
        wallet_transport_backend::response_vo::chain::GasOracle,
        &'static str,
        Option<u64>,
        Option<u64>,
    ),
    ServiceError,
> {
    let (mut oracle, mut source) = ChainTransDomain::gas_oracle_with_source(
        &chain.chain_code.to_string(),
        &chain.provider,
        backend,
    )
    .await?;
    if !checks_mainnet_fee(chain) {
        return Ok((oracle, source, None, None));
    }
    let reported_base =
        unit::convert_to_u256(oracle.suggest_base_fee.as_deref().unwrap_or("0"), 9)?;
    let quoted_prices: Vec<Option<U256>> =
        [&oracle.safe_gas_price, &oracle.propose_gas_price, &oracle.fast_gas_price]
            .into_iter()
            .map(|value| value.as_deref().and_then(|v| unit::convert_to_u256(v, 9).ok()))
            .collect();
    tracing::info!(metric="ordinary_evm_fee", stage="quote_input", chain=%chain.chain_code,
        quote_id, quote_source=source, received_at_ms=chrono::Utc::now().timestamp_millis(),
        reported_base_fee_wei=%reported_base, prices_wei=?quoted_prices,
        "received fee oracle input before freshness validation");

    let block = chain.provider.latest_block().await.map_err(ServiceError::from).inspect_err(|error| {
        let (error_kind, rpc_code) = crate::service::transaction::TransactionService::query_error_kind(error);
        tracing::warn!(metric="ordinary_evm_fee", stage="quote_snapshot_error", chain=%chain.chain_code, error_kind, rpc_code, "fee quote snapshot unavailable");
    })?;
    let current_base = block.header.base_fee_per_gas.ok_or_else(|| {
        ServiceError::Parameter(
            "selected EIP-1559 node returned no block base fee; refresh and retry".into(),
        )
    })?;
    {
        let base = current_base;
        let reported_base =
            unit::convert_to_u256(oracle.suggest_base_fee.as_deref().unwrap_or("0"), 9)?;
        let invalid_price =
            [&oracle.safe_gas_price, &oracle.propose_gas_price, &oracle.fast_gas_price]
                .into_iter()
                .flatten()
                .any(|price| {
                    unit::convert_to_u256(price, 9).map(|v| v < reported_base).unwrap_or(true)
                });
        if reported_base < U256::from(base) || invalid_price {
            tracing::info!(metric="ordinary_evm_fee", stage="stale_quote", chain=%chain.chain_code,
                quote_id, quote_source=source, reported_base_fee_wei=%reported_base, current_base_fee_wei=base,
                block_number=block.header.number, "refreshing stale or inconsistent fee quote from selected node");
            if invalid_price {
                // Invalid tiers cannot provide a trustworthy priority-fee policy.
                oracle = ChainTransDomain::default_gas_oracle(&chain.provider).await?;
            } else {
                // A newer base fee does not invalidate the backend's tier tips.
                // Reprice only the offered tiers against this same block snapshot.
                for price in [
                    &mut oracle.safe_gas_price,
                    &mut oracle.propose_gas_price,
                    &mut oracle.fast_gas_price,
                ]
                .into_iter()
                .flatten()
                {
                    let tip = unit::convert_to_u256(price, 9)? - reported_base;
                    *price = unit::format_to_string(U256::from(base) + tip, 9)?;
                }
                oracle.suggest_base_fee = Some(unit::format_to_string(U256::from(base), 9)?);
            }
            source = "node";
        }
    }
    Ok((oracle, source, Some(block.header.number), block.header.base_fee_per_gas))
}

pub(super) async fn check_before_signing(
    chain: &EthChain,
    tx: &alloy::rpc::types::TransactionRequest,
    nonce: u64,
    quote: Option<&str>,
) -> Result<(), ServiceError> {
    if !checks_mainnet_fee(chain) {
        return Ok(());
    }
    // Diagnostics supplied by clients are untrusted and never authorize a fee.
    let metadata: serde_json::Value =
        quote.and_then(|v| serde_json::from_str(v).ok()).unwrap_or_default();
    let quote_id = metadata["quoteId"]
        .as_str()
        .and_then(|v| uuid::Uuid::parse_str(v).ok())
        .map(|v| v.to_string());
    let quoted_at_ms = metadata["quotedAtMs"].as_i64().filter(|v| *v > 0);
    let block = chain.provider.latest_block().await.map_err(ServiceError::from).inspect_err(|error| {
        let (error_kind, rpc_code) = crate::service::transaction::TransactionService::query_error_kind(error);
        tracing::warn!(metric="ordinary_evm_fee", stage="pre_sign_snapshot_error", chain=%chain.chain_code, nonce, ?quote_id, error_kind, rpc_code, "fee validation failed before signing");
    })?;
    let max_fee = tx
        .max_fee_per_gas
        .ok_or_else(|| ServiceError::Parameter("EVM fee cap missing before signing".into()))?;
    let current_base = block.header.base_fee_per_gas.ok_or_else(|| {
        ServiceError::Parameter(
            "selected EIP-1559 node returned no block base fee; refresh and retry".into(),
        )
    })?;
    let insufficient = max_fee < u128::from(current_base);
    tracing::info!(metric="ordinary_evm_fee", stage="pre_sign", chain=%chain.chain_code, nonce,
        from=?tx.from, ?quote_id, ?quoted_at_ms, checked_at_ms=chrono::Utc::now().timestamp_millis(),
        block_number=block.header.number, current_base_fee_wei=?block.header.base_fee_per_gas,
        max_fee_per_gas_wei=max_fee, priority_fee_wei=?tx.max_priority_fee_per_gas,
        outcome=if insufficient {"fee_quote_expired"} else {"valid"},
        "checked actual transaction fee cap before signing");
    if insufficient {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::FeeQuoteExpired,
        )
        .into());
    }
    Ok(())
}
