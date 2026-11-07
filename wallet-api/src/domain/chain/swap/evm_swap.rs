// tron 和 eth 系列的交易参数
use crate::request::transaction::{DexRoute, QuoteReq};
use alloy::{
    primitives::{Address, U256},
    sol,
};
use wallet_types::chain::chain::ChainCode;

// evm 系列调用合约的方法
sol!(
    #[derive(Debug)]
    struct DexRouterParam1 {
        SubDexRouterParam1[] subDexRouters;
    }

    #[derive(Debug)]
    struct SubDexRouterParam1 {
        uint16 dexId;
        address poolId;
        bool zeroForOne;
        uint256 amountIn;
        uint256 minAmountOut;
    }

    #[derive(Debug)]
    function dexSwap1(
        DexRouterParam1[] calldata routerParam,
        address tokenIn,
        address tokenOut,
        uint256 amountIn,
        uint256 minAmountOut,
        address recipient,
        bool allowPartialFill
    ) external nonReentrant returns (uint256 usedAmountIn, uint256 amountOut);
);

//  聚合器合约参数
pub struct SwapParams {
    // 聚合器地址
    pub aggregator_addr: Address,
    // 转账金额
    pub amount_in: U256,
    // 最小输出
    pub min_amount_out: U256,
    // 接收地址
    pub recipient: Address,
    // 输入token
    pub token_in: Address,
    // 输出token
    pub token_out: Address,
    // 路由数据
    pub dex_router: Vec<DexRoute>,
    // 允许部分兑换
    pub allow_partial_fill: bool,
}

impl SwapParams {
    // eth 地址类型转为tron的地址类型
    pub fn aggregator_tron_addr(&self) -> Result<String, crate::error::service::ServiceError> {
        let address = self.aggregator_addr.to_string();

        Ok(wallet_utils::address::eth_addr_to_tron_addr(&address)?)
    }

    pub fn recipient_tron_addr(&self) -> Result<String, crate::error::service::ServiceError> {
        let address = self.recipient.to_string();

        Ok(wallet_utils::address::eth_addr_to_tron_addr(&address)?)
    }

    // 如果是主币,swap 合约接受zero地址
    pub fn eth_parse_or_zero_addr(
        addr: &str,
    ) -> Result<alloy::primitives::Address, crate::error::service::ServiceError> {
        if addr.is_empty() {
            Ok(alloy::primitives::Address::ZERO)
        } else {
            Ok(wallet_utils::address::parse_eth_address(addr)?)
        }
    }

    pub fn tron_parse_or_zero_addr(
        addr: &str,
    ) -> Result<alloy::primitives::Address, crate::error::service::ServiceError> {
        if addr.is_empty() {
            Ok(alloy::primitives::Address::ZERO)
        } else {
            Ok(QuoteReq::addr_tron_to_eth(addr)?)
        }
    }

    // 是否是主币兑换
    pub fn main_coin_swap(&self) -> bool {
        self.token_in == alloy::primitives::Address::ZERO
    }
}

impl TryFrom<(&SwapParams, ChainCode)> for dexSwap1Call {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: (&SwapParams, ChainCode)) -> Result<Self, Self::Error> {
        use wallet_utils::{address::parse_eth_address, unit::u256_from_str};

        let mut router_param = Vec::with_capacity(value.0.dex_router.len());

        for quote in value.0.dex_router.iter() {
            let mut sub_routes = Vec::with_capacity(quote.route_in_dex.len());

            let amount_in = u256_from_str(&quote.amount_in)?;

            for pool in &quote.route_in_dex {
                let pool_id = if value.1 == ChainCode::Ethereum {
                    parse_eth_address(&pool.pool_id)?
                } else {
                    QuoteReq::addr_tron_to_eth(&pool.pool_id)?
                };

                let mut sub_route = SubDexRouterParam1 {
                    dexId: pool.dex_id,
                    poolId: pool_id,
                    zeroForOne: pool.zero_for_one,
                    amountIn: u256_from_str(&pool.amount_in)?,
                    minAmountOut: u256_from_str(&pool.min_amount_out)?,
                };

                if sub_routes.len() == 0 {
                    sub_route.amountIn = amount_in;
                }

                sub_routes.push(sub_route);
            }

            router_param.push(DexRouterParam1 { subDexRouters: sub_routes });
        }

        Ok(dexSwap1Call {
            routerParam: router_param,
            tokenIn: value.0.token_in,
            tokenOut: value.0.token_out,
            amountIn: value.0.amount_in,
            minAmountOut: value.0.min_amount_out,
            recipient: value.0.recipient,
            allowPartialFill: value.0.allow_partial_fill,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{DexRouterParam1, SubDexRouterParam1};
    use crate::domain::chain::swap::evm_swap::dexSwap1Call;
    use alloy::sol_types::SolCall;

    #[test]
    fn test_build_params() {
        let dex_router_param1 = SubDexRouterParam1 {
            dexId: 1,
            poolId: alloy::primitives::Address::default(),
            zeroForOne: true,
            amountIn: alloy::primitives::U256::from(1),
            minAmountOut: alloy::primitives::U256::from(1),
        };

        let dex_router_param = DexRouterParam1 {
            subDexRouters: vec![dex_router_param1],
            // amountIn: alloy::primitives::U256::from(1),
            // minAmountOut: alloy::primitives::U256::from(1),
        };

        let call_val = dexSwap1Call {
            routerParam: vec![dex_router_param],
            tokenIn: alloy::primitives::Address::default(),
            tokenOut: alloy::primitives::Address::default(),
            amountIn: alloy::primitives::U256::from(1),
            minAmountOut: alloy::primitives::U256::from(1),
            recipient: alloy::primitives::Address::default(),
            allowPartialFill: true,
        };

        println!("{:?}", call_val.abi_encode());
    }
}
