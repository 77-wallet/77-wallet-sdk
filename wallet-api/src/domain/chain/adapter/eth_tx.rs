use crate::{
    domain::chain::{
        TransferResp, pare_fee_setting,
        swap::{
            EstimateSwapResult,
            evm_swap::{SwapParams, dexSwap1Call},
        },
    },
    request::transaction::{ApproveReq, DepositReq, WithdrawReq},
};
use alloy::{
    network::TransactionBuilder as _,
    primitives::U256,
    rpc::types::TransactionRequest,
    sol_types::{SolCall as _, SolValue},
};
use wallet_chain_interact::{
    ResourceConsume,
    eth::{
        self, EthChain, FeeSetting,
        operations::erc::{Allowance, Approve, Deposit, Withdraw},
    },
    types::{ChainPrivateKey, Transaction},
};
use wallet_types::chain::chain::ChainCode;
use wallet_utils::unit;

pub(super) async fn approve(
    chain: &EthChain,
    req: &ApproveReq,
    value: alloy::primitives::U256,
    key: ChainPrivateKey,
) -> Result<TransferResp, crate::error::service::ServiceError> {
    let approve = Approve::new(&req.from, &req.spender, value, &req.contract)?;

    let gas_price = chain.provider.get_default_fee().await?;

    let priority = if gas_price.priority_fee_per_gas.is_zero() {
        U256::from(10000)
    } else {
        gas_price.priority_fee_per_gas
    };

    let gas_limit = chain.provider.estimate_gas(approve.build_transaction()?).await?;

    let fee_setting = FeeSetting {
        base_fee: gas_price.base_fee,
        max_priority_fee_per_gas: priority,
        max_fee_per_gas: gas_price.base_fee + gas_price.priority_fee_per_gas,
        gas_limit,
    };

    let fee = fee_setting.transaction_fee();
    let balance = chain.balance(&req.from, None).await?;
    if balance < fee {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }

    // exec tx
    let tx_hash = chain.exec_transaction(approve, fee_setting, key, None).await?;

    Ok(TransferResp::new(tx_hash, unit::format_to_string(fee, eth::consts::ETH_DECIMAL)?))
}

pub(super) async fn approve_fee(
    chain: &EthChain,
    req: &ApproveReq,
    value: alloy::primitives::U256,
) -> Result<ResourceConsume, crate::error::service::ServiceError> {
    let approve = Approve::new(&req.from, &req.spender, value, &req.contract)?;

    let fee = chain.estimate_gas(approve).await?;

    Ok(fee)
}

pub(super) async fn withdraw_fee(
    chain: &EthChain,
    req: &WithdrawReq,
    value: alloy::primitives::U256,
) -> Result<ResourceConsume, crate::error::service::ServiceError> {
    let withdraw = Withdraw::new(&req.from, &req.token, value)?;

    Ok(chain.estimate_gas(withdraw).await?)
}

pub(super) async fn withdraw(
    chain: &EthChain,
    req: &WithdrawReq,
    value: alloy::primitives::U256,
    fee: String,
    key: ChainPrivateKey,
) -> Result<TransferResp, crate::error::service::ServiceError> {
    let withdraw = Withdraw::new(&req.from, &req.token, value)?;

    // 使用默认的手续费配置
    let fee_setting = pare_fee_setting(fee.as_str())?;
    let transfer_fee = fee_setting.transaction_fee();

    let balance = chain.balance(&req.from, None).await?;
    if balance < transfer_fee {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }

    // exec tx
    let tx_hash = chain.exec_transaction(withdraw, fee_setting, key, None).await?;

    Ok(TransferResp::new(tx_hash, unit::format_to_string(transfer_fee, eth::consts::ETH_DECIMAL)?))
}

pub(super) async fn deposit_fee(
    chain: &EthChain,
    req: &DepositReq,
    amount: alloy::primitives::U256,
) -> Result<ResourceConsume, crate::error::service::ServiceError> {
    let approve = Deposit::new(&req.from, &req.token, amount)?;

    Ok(chain.estimate_gas(approve).await?)
}

pub(super) async fn deposit(
    chain: &EthChain,
    req: &DepositReq,
    amount: alloy::primitives::U256,
    fee: String,
    key: ChainPrivateKey,
) -> Result<TransferResp, crate::error::service::ServiceError> {
    let approve = Deposit::new(&req.from, &req.token, amount)?;

    // 使用默认的手续费配置
    let fee_setting = pare_fee_setting(fee.as_str())?;
    let transfer_fee = fee_setting.transaction_fee();

    let balance = chain.balance(&req.from, None).await?;
    if balance < transfer_fee {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }

    // exec tx
    let tx_hash = chain.exec_transaction(approve, fee_setting, key, None).await?;

    Ok(TransferResp::new(tx_hash, unit::format_to_string(transfer_fee, eth::consts::ETH_DECIMAL)?))
}

pub(super) async fn allowance(
    chain: &EthChain,
    from: &str,
    token: &str,
    spender: &str,
) -> Result<U256, crate::error::service::ServiceError> {
    let approve = Allowance::new(from, token, spender)?;

    let amount = chain.eth_call::<_, U256>(approve).await?;

    Ok(amount)
}

fn build_base_swap_tx(
    swap_params: &SwapParams,
) -> Result<TransactionRequest, crate::error::service::ServiceError> {
    let call_value = dexSwap1Call::try_from((swap_params, ChainCode::Ethereum))?;

    let tx = TransactionRequest::default()
        .from(swap_params.recipient)
        .to(swap_params.aggregator_addr)
        .with_input(call_value.abi_encode());

    // from token 如果是主币添加默认的币
    let tx = if swap_params.token_in.is_zero() { tx.with_value(swap_params.amount_in) } else { tx };

    Ok(tx)
}
// 有3个网络的io
pub(super) async fn estimate_swap(
    swap_params: SwapParams,
    chain: &EthChain,
) -> Result<EstimateSwapResult<U256>, crate::error::service::ServiceError> {
    let tx = build_base_swap_tx(&swap_params)?;

    // estimate_gas_limit
    let gas_limit = chain.provider.estimate_gas(tx.clone()).await?;
    let tx = tx.with_gas_limit(gas_limit.to::<u64>());

    let result = chain.provider.eth_call(tx).await.map_err(|e| {
        crate::error::service::ServiceError::AggregatorError {
            code: 10500,
            agg_code: 0,
            msg: format!("eth call error: {}", e),
        }
    })?;
    let bytes = wallet_utils::hex_func::hex_decode(&result[2..])?;

    let (amount_in, amount_out): (U256, U256) = <(U256, U256)>::abi_decode_params(&bytes, true)
        .map_err(|e| crate::error::service::ServiceError::AggregatorError {
            code: -1,
            agg_code: 0,
            msg: e.to_string(),
        })?;

    let resp = EstimateSwapResult { amount_in, amount_out, consumer: gas_limit };
    Ok(resp)
}

pub(super) async fn swap(
    chain: &EthChain,
    swap_params: &SwapParams,
    fee: String,
    key: ChainPrivateKey,
) -> Result<TransferResp, crate::error::service::ServiceError> {
    let fee_setting = pare_fee_setting(fee.as_str())?;
    let transfer_fee = fee_setting.transaction_fee();
    let mut check_bal = transfer_fee;

    let balance = chain.balance(&swap_params.recipient.to_string(), None).await?;

    if swap_params.main_coin_swap() {
        check_bal += swap_params.amount_in;
    }
    if balance < check_bal {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }

    let tx = build_base_swap_tx(swap_params)?;
    let tx = chain.provider.set_transaction_fee(tx, fee_setting, chain.chain_code).await?;

    let tx_hash = chain.provider.send_raw_transaction(tx, &key, None).await?;

    Ok(TransferResp::new(tx_hash, unit::format_to_string(transfer_fee, eth::consts::ETH_DECIMAL)?))
}
