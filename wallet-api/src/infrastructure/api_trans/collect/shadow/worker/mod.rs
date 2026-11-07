mod collect_worker;
mod side_effect_worker;

use crate::{
    error::service::ServiceError,
    response_vo::{EthereumFeeDetails, FeeDetailsVo},
};
use alloy::primitives::U256;
use wallet_chain_interact::eth;
use wallet_utils::unit;

pub use collect_worker::{ShadowCollectCommand, ShadowCollectWorker};
pub use side_effect_worker::{SideEffectCommand, SideEffectWorker};

fn evm_propose_max_fee(fee: &str) -> Result<String, ServiceError> {
    let res: FeeDetailsVo<EthereumFeeDetails> = wallet_utils::serde_func::serde_from_str(fee)?;
    let propose = res.data.into_iter().find(|item| item.types == "propose").ok_or(
        crate::error::business::BusinessError::ApiWallet(
            crate::error::business::api_wallet::ApiWalletError::GasOracle,
        ),
    )?;

    let gas_limit = U256::from(propose.fee_setting.gas_limit);
    let max_fee_per_gas = unit::u256_from_str(&propose.fee_setting.max_fee_per_gas)?;
    unit::format_to_string(gas_limit * max_fee_per_gas, eth::consts::ETH_DECIMAL)
        .map_err(ServiceError::from)
}

/// The same native budget is quoted before funding and before building.
/// Base/Scroll's separate L1 fee cannot be reconstructed from gas settings.
async fn estimate_evm_collect_fee(
    adapter: &(dyn crate::domain::api_wallet::Tx + Send + Sync),
    chain_code: wallet_types::chain::chain::ChainCode,
    params: crate::request::api_wallet::trans::ApiBaseTransferReq,
    main_symbol: &str,
) -> Result<String, ServiceError> {
    use wallet_types::chain::chain::ChainCode;
    let replacement = crate::domain::api_wallet::adapter::eth_tx::replacement::context(
        params.metadata.as_deref(),
    )?
    .is_some();
    if replacement || matches!(chain_code, ChainCode::Base | ChainCode::Scroll) {
        let wei = adapter.estimate_native_fee_wei(params).await?;
        return unit::format_to_string(wei, eth::consts::ETH_DECIMAL).map_err(ServiceError::from);
    }
    let response = adapter.estimate_fee_without_balance_check(params, main_symbol).await?;
    evm_propose_max_fee(&response)
}

/// A single payment must cover quote drift while the fee transfer is confirmed.
/// Keep admission at the fresh exact quote; headroom belongs only to funding.
fn evm_funding_budget(fee: &str) -> Result<String, ServiceError> {
    let wei = unit::convert_to_u256(fee, eth::consts::ETH_DECIMAL)?;
    let budget = wei
        .checked_mul(U256::from(6))
        .and_then(|value| value.checked_add(U256::from(4)))
        .map(|value| value / U256::from(5))
        .ok_or_else(|| ServiceError::Parameter("EVM funding budget overflow".to_string()))?;
    unit::format_to_string(budget, eth::consts::ETH_DECIMAL).map_err(ServiceError::from)
}

#[cfg(test)]
mod fee_recovery_tests;

#[cfg(test)]
mod fee_quote_tests;
