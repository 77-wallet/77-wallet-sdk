use crate::get_manager;
use wallet_api::request::transaction::{DexRoute, QuoteReq, SwapReq, SwapTokenInfo};

// Ethereum aggregator address
const AGGREGATOR: &str = "FqhFY3ycmuif2T7YJZDwytyvrxEhnr6MYV4k2qxENkVH";

const RECIPIENT: &str = "37qZgmfhQNvjTfycUeXte3sAucAY4iaqoTZfhFxZb7L1";

// usdt Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB
// usdc EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
// wsol So11111111111111111111111111111111111111112
fn token_in_out() -> (SwapTokenInfo, SwapTokenInfo) {
    let token_in =
        SwapTokenInfo { token_addr: "".to_string(), symbol: "USDT".to_string(), decimals: 9 };
    let token_out = SwapTokenInfo {
        token_addr: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB".to_string(),
        symbol: "sol".to_string(),
        decimals: 6,
    };

    (token_in, token_out)
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_quote() {
    let wallet_manager = get_manager().await;

    let (token_in, token_out) = token_in_out();

    let req = QuoteReq {
        aggregator_addr: AGGREGATOR.to_string(),
        recipient: RECIPIENT.to_string(),
        chain_code: "sol".to_string(),
        amount_in: "0.002".to_string(),
        token_in,
        token_out,
        dex_list: vec![3],
        slippage: None,
        allow_partial_fill: false,
    };

    let result = wallet_manager.quote(req).await.unwrap();
    tracing::warn!("quote = {}", serde_json::to_string(&result).unwrap());
}

// (选择的流动性)流动性
#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_swap() {
    let wallet_manager = get_manager().await;

    let (token_in, token_out) = token_in_out();
    let o_value = "0.002";

    let dex_route_str = r#"[{"amountIn":"1600400","amountOut":"335816","percentage":"8002","routeInDex":[{"dexId":3,"poolId":"2QdhepnKRTLjjSqPL1PtKNwqrUkoLee5Gqs8bvZhRdMv","inTokenSymbol":"WSOL","inTokenAddr":"So11111111111111111111111111111111111111112","outTokenSymbol":"USDC","outTokenAddr":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","zeroForOne":true,"fee":"5","amountIn":"0","minAmountOut":"0"},{"dexId":3,"poolId":"BZtgQEyS6eXUXicYPHecYQ7PybqodXQMvkjUbP4R8mUU","inTokenSymbol":"USDC","inTokenAddr":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","outTokenSymbol":"USDT","outTokenAddr":"Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB","zeroForOne":true,"fee":"1","amountIn":"0","minAmountOut":"0"}]},{"amountIn":"399600","amountOut":"84449","percentage":"1998","routeInDex":[{"dexId":3,"poolId":"GN3W5LnmeGt5caNhNQavcufGj2bZfRwry5UQrezukYiL","inTokenSymbol":"WSOL","inTokenAddr":"So11111111111111111111111111111111111111112","outTokenSymbol":"USDT","outTokenAddr":"Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB","zeroForOne":true,"fee":"400","amountIn":"0","minAmountOut":"0"}]}]"#;

    let dex_router: Vec<DexRoute> =
        wallet_utils::serde_func::serde_from_str(dex_route_str).unwrap();

    let req = SwapReq {
        aggregator_addr: AGGREGATOR.to_string(),
        amount_in: o_value.to_string(),
        amount_out: "0.1".to_string(),
        min_amount_out: "0.08".to_string(),
        chain_code: "sol".to_string(),
        recipient: RECIPIENT.to_string(),
        token_in,
        token_out,
        dex_router,
        allow_partial_fill: false,
    };

    let fee = r#"{"base_fee":995880,"priority_fee_per_compute_unit":null,"compute_units_consumed":100000,"extra_fee":5000000}"#.to_string();
    let password = "123456".to_string();

    let result = wallet_manager.swap(req, fee, password).await.unwrap();
    tracing::warn!("swap hash = {}", serde_json::to_string(&result).unwrap());
}
