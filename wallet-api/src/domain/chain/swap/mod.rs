use wallet_types::chain::chain::ChainCode;
pub mod evm_swap;

// 各个链包装代币的地址
pub const W_ETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
pub const W_BNB: &str = "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c";
pub const W_TRON: &str = "TNUC9Qb1rRpS5CbWLmNMxXBjyFoydXjWFR";
pub const W_SOL: &str = "So11111111111111111111111111111111111111112";

// 滑点万分位
pub const SLIPPAGE: f64 = 10000.0;

pub fn get_warp_address(
    chain_code: ChainCode,
) -> Result<&'static str, crate::error::service::ServiceError> {
    match chain_code {
        ChainCode::Ethereum => Ok(W_ETH),
        ChainCode::BnbSmartChain => Ok(W_BNB),
        ChainCode::Tron => Ok(W_TRON),
        ChainCode::Solana => Ok(W_SOL),
        _ => Err(crate::error::service::ServiceError::Parameter(
            "unsupport chain to swap".to_string(),
        ))?,
    }
}

// 滑点计算
// slippage: 0.01 min value
// 万分之一
pub fn calc_slippage(amount: alloy::primitives::U256, slippage: f64) -> alloy::primitives::U256 {
    let slippage_bps = (slippage * SLIPPAGE).round() as u64;

    let numerator = alloy::primitives::U256::from(SLIPPAGE as u64 - slippage_bps);
    let denominator = alloy::primitives::U256::from(SLIPPAGE as u64);

    amount * numerator / denominator
}

#[derive(Debug, serde::Serialize)]
pub struct EstimateSwapResult<T> {
    pub amount_in: alloy::primitives::U256,
    pub amount_out: alloy::primitives::U256,
    // 资源消耗
    pub consumer: T,
}
