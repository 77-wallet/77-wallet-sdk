use crate::{
    domain::chain::{TransferResp, swap::EstimateSwapResult},
    error::{
        business::{BusinessError, chain::ChainError},
        service::ServiceError,
    },
    infrastructure::swap_client::SolInstructResp,
    request::transaction::{DepositReq, SwapReq, WithdrawReq},
};
use alloy::primitives::U256;
use wallet_chain_interact::{
    sol::{
        SolFeeSetting, SolanaChain,
        consts::SOL_DECIMAL,
        operations::{
            SolInstructionOperation,
            native_coin::{Deposit, Swap, Withdraw},
        },
    },
    types::ChainPrivateKey,
};
use wallet_utils::serde_func;

// 默认系统账号的租金
pub const SYSTEM_ACCOUNT_RENT: f64 = 0.000990880;

// 默认token账号的租金
pub const TOKEN_ACCOUNT_RENT: u64 = 2500000;

// 默认的手续费(用于展示)
pub const DEFAULT_UNITS_FEE: u64 = 5000;

pub(crate) fn native_spend_all_amount(
    balance: U256,
    fee_reserve: u64,
) -> Result<U256, ServiceError> {
    let fee_reserve = U256::from(fee_reserve);
    if balance <= fee_reserve {
        return Err(BusinessError::Chain(ChainError::InsufficientFeeBalance))?;
    }

    Ok(balance - fee_reserve)
}

pub(super) async fn estimate_swap(
    payer: &str,
    instructions: SolInstructResp,
    chain: &SolanaChain,
) -> Result<EstimateSwapResult<u64>, crate::error::service::ServiceError> {
    let payer = wallet_utils::address::parse_sol_address(payer)?;

    let res = chain
        .similar_transaction(&payer, &instructions.ins, Some(instructions.alts.clone()))
        .await?;
    if res.value.err.is_some() {
        tracing::error!("swap simulate error: {:?}", res);
        // let _res =
        //     FrontendNotifyEvent::send_debug(&wallet_utils::serde_func::serde_to_value(&res.value)?)
        //         .await;
        let log = res.value.logs.join(",");

        // 如果有超时的错误
        let error = if log.contains("RequireGteViolated") {
            crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::SolSwapTime(log),
            )
        } else {
            crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::SwapSimulate(log),
            )
        };

        return Err(error)?;
    }

    //
    let return_data = res.value.return_data;

    let mut amount_in = 0_u64;
    let mut amount_out = 0_u64;
    if let Some(return_data) = return_data {
        let bytes = wallet_utils::base64_to_bytes(&return_data.data[0])?;
        amount_in = u64::from_le_bytes(bytes[0..8].try_into().map_err(|_| {
            crate::error::service::ServiceError::Parameter(
                "sol simultate parse return data error in".to_string(),
            )
        })?);

        amount_out = u64::from_le_bytes(bytes[8..16].try_into().map_err(|_| {
            crate::error::service::ServiceError::Parameter(
                "sol simultate parse return data error out".to_string(),
            )
        })?);
    }

    let consumer = res.value.units_consumed;

    let res = EstimateSwapResult {
        amount_in: U256::from(amount_in),
        amount_out: U256::from(amount_out),
        consumer,
    };

    Ok(res)
}

pub(super) async fn swap(
    chain: &SolanaChain,
    req: &SwapReq,
    fee: String,
    instructions: SolInstructResp,
    key: ChainPrivateKey,
) -> Result<TransferResp, crate::error::service::ServiceError> {
    let fee_setting = wallet_utils::serde_func::serde_from_str::<SolFeeSetting>(&fee)?;
    let fee = fee_setting.transaction_fee().to_string();

    // check balance (sol 减租金)
    let mut balance = chain.balance(&req.recipient, None).await?;
    balance -= wallet_utils::unit::convert_to_u256(&SYSTEM_ACCOUNT_RENT.to_string(), SOL_DECIMAL)?;

    // 如果是主币兑换那么 需要考虑余额的情况
    let mut origin_fee = U256::from(fee_setting.original_fee());
    if req.token_in.token_addr.is_empty() {
        let amount_in =
            wallet_utils::unit::convert_to_u256(&req.amount_in, req.token_in.decimals as u8)?;
        origin_fee += amount_in;
    }
    if origin_fee > balance {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }

    let params = Swap::new(&req.recipient)?;

    let tx_hash = chain
        .exec_v0_transaction(params, key, Some(fee_setting), instructions.ins, instructions.alts)
        .await?;

    let res = TransferResp { tx_hash, fee, consumer: None, transaction_time_ms: None };
    Ok(res)
}

pub(super) async fn deposit_fee(
    chain: &SolanaChain,
    req: &DepositReq,
    amount: alloy::primitives::U256,
) -> Result<SolFeeSetting, crate::error::service::ServiceError> {
    let params = Deposit::new_wsol(&req.from, amount, chain.get_provider())?;

    let instructions = params.instructions().await?;

    Ok(chain.estimate_fee_v1(&instructions, &params).await?)
}

pub(super) async fn deposit(
    chain: &SolanaChain,
    req: &DepositReq,
    amount: alloy::primitives::U256,
    fee: String,
    key: ChainPrivateKey,
) -> Result<TransferResp, crate::error::service::ServiceError> {
    let fee_setting = serde_func::serde_from_str::<SolFeeSetting>(&fee)?;
    let fee = fee_setting.transaction_fee().to_string();
    let params = Deposit::new_wsol(&req.from, amount, chain.get_provider())?;

    let instructions = params.instructions().await?;

    // check balance
    let mut balance = chain.balance(&req.from, None).await?;
    balance -= U256::from(SYSTEM_ACCOUNT_RENT);
    if U256::from(fee_setting.original_fee()) + amount > balance {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }

    // 验证手续费,进行拦截
    let tx_hash = chain.exec_transaction(params, key, Some(fee_setting), instructions, 0).await?;

    Ok(TransferResp { tx_hash, fee, consumer: None, transaction_time_ms: None })
}

pub(super) async fn withdraw_fee(
    chain: &SolanaChain,
    req: &WithdrawReq,
    amount: alloy::primitives::U256,
) -> Result<SolFeeSetting, crate::error::service::ServiceError> {
    let params = Withdraw::new_wsol(&req.from, amount, chain.get_provider())?;

    let instructions = params.instructions().await?;

    Ok(chain.estimate_fee_v1(&instructions, &params).await?)
}

pub(super) async fn withdraw(
    chain: &SolanaChain,
    req: &WithdrawReq,
    amount: alloy::primitives::U256,
    fee: String,
    key: ChainPrivateKey,
) -> Result<TransferResp, crate::error::service::ServiceError> {
    let fee_setting = serde_func::serde_from_str::<SolFeeSetting>(&fee)?;
    let fee = fee_setting.transaction_fee().to_string();

    let params = Withdraw::new_wsol(&req.from, amount, chain.get_provider())?;
    let instructions = params.instructions().await?;

    // check balance
    let mut balance = chain.balance(&req.from, None).await?;
    balance -= U256::from(SYSTEM_ACCOUNT_RENT);
    if U256::from(fee_setting.original_fee()) > balance {
        return Err(crate::error::business::BusinessError::Chain(
            crate::error::business::chain::ChainError::InsufficientFeeBalance,
        ))?;
    }

    // 验证手续费,进行拦截
    let tx_hash = chain.exec_transaction(params, key, None, instructions, 0).await?;

    Ok(TransferResp { tx_hash, fee, consumer: None, transaction_time_ms: None })
}

#[cfg(test)]
mod tests {
    use super::native_spend_all_amount;
    use crate::error::service::ServiceError;
    use alloy::primitives::U256;

    #[test]
    fn native_spend_all_amount_reserves_fee_only() {
        let fee_reserve = 5_000_u64;
        let balance = U256::from(fee_reserve) + U256::from(123_u64);

        let amount =
            native_spend_all_amount(balance, fee_reserve).expect("expected spend-all amount");

        assert_eq!(amount, U256::from(123_u64));
    }

    #[test]
    fn native_spend_all_amount_rejects_insufficient_balance() {
        let err = native_spend_all_amount(U256::from(5_000_u64), 5_000_u64)
            .expect_err("expected insufficient fee balance");

        assert!(matches!(
            err,
            ServiceError::Business(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::InsufficientFeeBalance
            ))
        ));
    }
}
