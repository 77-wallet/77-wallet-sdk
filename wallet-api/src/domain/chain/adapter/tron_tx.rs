use crate::{
    domain::{
        chain::{
            TransferResp,
            swap::{
                EstimateSwapResult,
                evm_swap::{SwapParams, dexSwap1Call},
            },
        },
        multisig::MultisigQueueDomain,
    },
    request::transaction::{ApproveReq, DepositReq, WithdrawReq},
    response_vo::TransferParams,
};
use alloy::{
    primitives::U256,
    sol_types::{SolCall, SolValue},
};
use wallet_chain_interact::{
    BillResourceConsume, abi_encode_u256,
    tron::{
        TronChain,
        operations::{
            TronConstantOperation as _,
            contract::{TriggerContractParameter, WarpContract},
            transfer::{ContractTransferOpt, TransferOpt},
            trc::{Allowance, Approve, Deposit},
        },
        params::ResourceConsumer,
    },
    types::{ChainPrivateKey, MultisigTxResp},
};
use wallet_types::chain::chain::ChainCode;

// 构建多签交易
pub(super) async fn build_build_tx(
    req: &TransferParams,
    token: Option<String>,
    value: alloy::primitives::U256,
    chain: &TronChain,
    threshold: i64,
    permission_id: Option<i64>,
) -> Result<MultisigTxResp, crate::error::service::ServiceError> {
    let expiration = MultisigQueueDomain::sub_expiration(req.expiration.unwrap_or(1));
    tracing::info!(
        msq_step = "tron_build_build_tx_start",
        from = %req.from,
        to = %req.to,
        chain_code = %req.chain_code,
        symbol = %req.symbol,
        token = ?token,
        threshold = threshold,
        permission_id = ?permission_id,
        expiration_secs = expiration,
        "start tron multisig build transaction"
    );

    if let Some(token) = token {
        tracing::info!(
            msq_step = "tron_build_build_tx_trc20_branch",
            from = %req.from,
            to = %req.to,
            token = %token,
            "build tron multisig TRC20 transfer"
        );
        let mut params =
            ContractTransferOpt::new(&token, &req.from, &req.to, value, req.notes.clone())?;

        params.permission_id = permission_id;

        let provider = chain.get_provider();
        let constant = params.constant_contract(provider).await?;
        let consumer = provider.contract_fee(constant, threshold as u8, &req.from).await?;
        params.set_fee_limit(consumer);

        let rs = chain.build_multisig_transaction(params, expiration as u64).await?;
        tracing::info!(
            msq_step = "tron_build_build_tx_done",
            from = %req.from,
            to = %req.to,
            tx_hash = %rs.tx_hash,
            "tron multisig build transaction done"
        );
        Ok(rs)
    } else {
        tracing::info!(
            msq_step = "tron_build_build_tx_trx_branch",
            from = %req.from,
            to = %req.to,
            "build tron multisig TRX transfer"
        );
        let mut params = TransferOpt::new(&req.from, &req.to, value, req.notes.clone())?;
        params.permission_id = permission_id;

        let rs = chain.build_multisig_transaction(params, expiration as u64).await?;
        tracing::info!(
            msq_step = "tron_build_build_tx_done",
            from = %req.from,
            to = %req.to,
            tx_hash = %rs.tx_hash,
            "tron multisig build transaction done"
        );
        Ok(rs)
    }
}

pub(super) async fn approve(
    chain: &TronChain,
    req: &ApproveReq,
    value: alloy::primitives::U256,
    key: ChainPrivateKey,
) -> Result<TransferResp, crate::error::service::ServiceError> {
    let approve = Approve::new(&req.from, &req.spender, &req.contract, value);
    let mut wrap = WarpContract::new(approve)?;

    // get fee
    let constant = wrap.trigger_constant_contract(&chain.provider).await?;
    let consumer = chain.provider.contract_fee(constant, 1, &req.from).await?;

    // check balance
    let balance = chain.balance(&req.from, None).await?;
    let fee = alloy::primitives::U256::from(consumer.transaction_fee_i64());
    if balance < fee {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }

    // get consumer
    let bill_consumer = BillResourceConsume::new_tron(
        consumer.act_bandwidth() as u64,
        consumer.act_energy() as u64,
    );

    // exec trans
    let raw_transaction = wrap.trigger_smart_contract(&chain.provider, &consumer).await?;
    let result = chain.exec_transaction_v1(raw_transaction, key).await?;

    let mut resp = TransferResp::new(result, consumer.transaction_fee());
    resp.with_consumer(bill_consumer);

    Ok(resp)
}

pub(super) async fn approve_fee(
    chain: &TronChain,
    req: &ApproveReq,
    value: alloy::primitives::U256,
) -> Result<ResourceConsumer, crate::error::service::ServiceError> {
    let approve = Approve::new(&req.from, &req.spender, &req.contract, value);
    let wrap = WarpContract::new(approve)?;

    // get fee
    let constant = wrap.trigger_constant_contract(&chain.provider).await?;
    let consumer = chain.provider.contract_fee(constant, 1, &req.from).await?;

    Ok(consumer)
}

pub(super) async fn deposit_fee(
    chain: &TronChain,
    req: &DepositReq,
    value: alloy::primitives::U256,
) -> Result<ResourceConsumer, crate::error::service::ServiceError> {
    let approve = Deposit::new(&req.from, &req.token, value);
    let wrap = WarpContract::new(approve)?;

    // get fee
    let constant = wrap.trigger_constant_contract(&chain.provider).await?;
    let consumer = chain.provider.contract_fee(constant, 1, &req.from).await?;

    Ok(consumer)
}

pub(super) async fn deposit(
    chain: &TronChain,
    req: &DepositReq,
    value: alloy::primitives::U256,
    key: ChainPrivateKey,
) -> Result<TransferResp, crate::error::service::ServiceError> {
    let approve = Deposit::new(&req.from, &req.token, value);
    let mut wrap = WarpContract::new(approve)?;

    // get fee
    let constant = wrap.trigger_constant_contract(&chain.provider).await?;
    let consumer = chain.provider.contract_fee(constant, 1, &req.from).await?;

    // check balance
    let balance = chain.balance(&req.from, None).await?;
    let fee = alloy::primitives::U256::from(consumer.transaction_fee_i64()) + value;
    if balance < fee {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }

    // get consumer
    let bill_consumer = BillResourceConsume::new_tron(
        consumer.act_bandwidth() as u64,
        consumer.act_energy() as u64,
    );

    // exec trans
    let raw_transaction = wrap.trigger_smart_contract(&chain.provider, &consumer).await?;
    let result = chain.exec_transaction_v1(raw_transaction, key).await?;

    let mut resp = TransferResp::new(result, consumer.transaction_fee());
    resp.with_consumer(bill_consumer);

    Ok(resp)
}

fn build_base_withdraw(
    req: &WithdrawReq,
    value: U256,
) -> Result<TriggerContractParameter, crate::error::service::ServiceError> {
    // 构建调用合约的参数
    let parameter = abi_encode_u256(value);
    let function_selector = "withdraw(uint256)";

    let contract_address = wallet_utils::address::bs58_addr_to_hex(&req.token)?;
    let owner_address = wallet_utils::address::bs58_addr_to_hex(&req.from)?;

    let value = TriggerContractParameter::new(
        &contract_address,
        &owner_address,
        function_selector,
        parameter,
    );

    Ok(value)
}

pub(super) async fn withdraw_fee(
    chain: &TronChain,
    req: &WithdrawReq,
    value: alloy::primitives::U256,
) -> Result<ResourceConsumer, crate::error::service::ServiceError> {
    let trigger = build_base_withdraw(req, value)?;
    let wrap = WarpContract { params: trigger };

    // get fee
    let constant = wrap.trigger_constant_contract(&chain.provider).await?;
    let consumer = chain.provider.contract_fee(constant, 1, &req.from).await?;

    Ok(consumer)
}

pub(super) async fn withdraw(
    chain: &TronChain,
    req: &WithdrawReq,
    value: alloy::primitives::U256,
    key: ChainPrivateKey,
) -> Result<TransferResp, crate::error::service::ServiceError> {
    let trigger = build_base_withdraw(req, value)?;
    let mut wrap = WarpContract { params: trigger };

    // get fee
    let constant = wrap.trigger_constant_contract(&chain.provider).await?;
    let consumer = chain.provider.contract_fee(constant, 1, &req.from).await?;

    let balance = chain.balance(&req.from, None).await?;
    let fee = alloy::primitives::U256::from(consumer.transaction_fee_i64());
    if balance < fee {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }

    // get consumer
    let bill_consumer = BillResourceConsume::new_tron(
        consumer.act_bandwidth() as u64,
        consumer.act_energy() as u64,
    );

    // exec trans
    let raw_transaction = wrap.trigger_smart_contract(&chain.provider, &consumer).await?;
    let result = chain.exec_transaction_v1(raw_transaction, key).await?;

    let mut resp = TransferResp::new(result, consumer.transaction_fee());
    resp.with_consumer(bill_consumer);

    Ok(resp)
}

pub(super) async fn allowance(
    chain: &TronChain,
    from: &str,
    token: &str,
    spender: &str,
) -> Result<U256, crate::error::service::ServiceError> {
    let approve = Allowance::new(from, spender, token);
    let wrap = WarpContract::new(approve)?;

    // get fee
    let constant = wrap.trigger_constant_contract(&chain.provider).await?;

    Ok(constant.parse_u256()?)
}

fn build_base_swap(
    swap_params: &SwapParams,
) -> Result<(TriggerContractParameter, String), crate::error::service::ServiceError> {
    let call_value = dexSwap1Call::try_from((swap_params, ChainCode::Tron))?;

    // tracing::warn!("call value: {:#?}", call_value);
    let contract_address = swap_params.aggregator_tron_addr()?;
    let owner_address = swap_params.recipient_tron_addr()?;

    let contract_address = wallet_utils::address::bs58_addr_to_hex(&contract_address)?;
    let owner_address = wallet_utils::address::bs58_addr_to_hex(&owner_address)?;

    let mut raw = vec![];
    call_value.abi_encode_raw(&mut raw);

    // 构建调用合约的参数
    let parameter = wallet_utils::hex_func::hex_encode(raw);
    let function_selector = "dexSwap1(((uint16,address,bool,uint256,uint256)[])[],address,address,uint256,uint256,address,bool)";
    let mut value = TriggerContractParameter::new(
        &contract_address,
        &owner_address,
        function_selector,
        parameter,
    );

    // 主币设置转账的value
    if swap_params.token_in.is_zero() {
        value.call_value = Some(swap_params.amount_in.to::<u64>());
    }

    Ok((value, owner_address))
}

pub(super) async fn estimate_swap(
    swap_params: &SwapParams,
    chain: &TronChain,
) -> Result<EstimateSwapResult<ResourceConsumer>, crate::error::service::ServiceError> {
    let (params, owner_address) = build_base_swap(swap_params)?;

    let wrap = WarpContract { params };

    // 模拟交易结果
    let constant = wrap.trigger_constant_contract(&chain.provider).await?;
    constant.is_success()?;

    let bytes = wallet_utils::hex_func::hex_decode(&constant.constant_result[0])?;

    // 模拟的结果k
    let (amount_in, amount_out): (U256, U256) = <(U256, U256)>::abi_decode_params(&bytes, true)
        .map_err(|e| crate::error::service::ServiceError::AggregatorError {
            code: -1,
            agg_code: 0,
            msg: e.to_string(),
        })?;

    // get fee
    let mut consumer = chain.provider.contract_fee(constant, 1, &owner_address).await?;
    // 手续费增加0.2trx
    consumer.set_extra_fee(200000);

    let resp = EstimateSwapResult { amount_in, amount_out, consumer };

    Ok(resp)
}

// 执行swap操作
pub(super) async fn swap(
    chain: &TronChain,
    swap_params: &SwapParams,
    key: ChainPrivateKey,
) -> Result<TransferResp, crate::error::service::ServiceError> {
    let (params, owner_address) = build_base_swap(&swap_params)?;

    let mut wrap = WarpContract { params };
    let constant = wrap.trigger_constant_contract(&chain.provider).await?;
    // get fee
    let mut consumer = chain.provider.contract_fee(constant, 1, &owner_address).await?;

    // check fee
    let balance = chain.balance(&swap_params.recipient_tron_addr()?, None).await?;
    // 手续费增加0.2trx
    consumer.set_extra_fee(200000);

    let mut fee = alloy::primitives::U256::from(consumer.transaction_fee_i64());
    if swap_params.main_coin_swap() {
        fee += swap_params.amount_in;
    }
    if balance < fee {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }

    let bill_consumer = BillResourceConsume::new_tron(
        consumer.act_bandwidth() as u64,
        consumer.act_energy() as u64,
    );

    let raw_transaction = wrap.trigger_with_fee(&chain.provider, 300).await?;

    let tx_hash = chain.exec_transaction_v1(raw_transaction, key).await?;

    let mut resp = TransferResp::new(tx_hash, consumer.transaction_fee());
    resp.with_consumer(bill_consumer);

    Ok(resp)
}
