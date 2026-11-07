use crate::get_manager;
use anyhow::Result;
use wallet_api::request::transaction::{
    ApproveReq, DexRoute, QuoteReq, RouteInDex, SwapReq, SwapTokenInfo,
};

// Ethereum aggregator address
const AGGREGATOR: &str = "0xD084CbC32648346DC98BF667B2535d61E4243a95";

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_default_quote() -> Result<()> {
    let wallet_manager = get_manager().await;

    let chain_code = "eth".to_string();
    let token_in = "".to_string();
    // let token_out = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string();

    let resp = wallet_manager.default_quote(chain_code, token_in).await?;
    println!("{}", serde_json::to_string(&resp).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_support_chain() -> Result<()> {
    let wallet_manager = get_manager().await;

    let resp = wallet_manager.chain_list().await?;
    println!("{}", serde_json::to_string(&resp).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_approve() -> Result<()> {
    let wallet_manager = get_manager().await;
    let params = ApproveReq {
        from: "0x998522f928A37837Fa8d6743713170243b95f98a".to_string(),
        spender: AGGREGATOR.to_string(),
        contract: "0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string(),
        value: "2".to_string(),
        chain_code: "eth".to_string(),
    };

    let password = "123456".to_string();

    let resp = wallet_manager.approve(params, password).await?;
    println!("{}", serde_json::to_string(&resp).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_approve_fee() -> Result<()> {
    let wallet_manager = get_manager().await;

    let params = ApproveReq {
        from: "0x998522f928A37837Fa8d6743713170243b95f98a".to_string(),
        spender: "0xb0635793E700E8A5AFbB94e12086E921FB0E3E3E".to_string(),
        contract: "0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string(),
        value: "0".to_string(),
        chain_code: "eth".to_string(),
    };

    let is_cancel = true;

    let resp = wallet_manager.approve_fee(params, is_cancel).await?;
    println!("{}", serde_json::to_string(&resp).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_approve_list() -> Result<()> {
    let wallet_manager = get_manager().await;

    let uid = "41028d217798181a73225cc57ad401a66da21c5b0853e83a50f377dffc75651d".to_string();
    let account_id = 1;

    let resp = wallet_manager.approve_list(uid, account_id).await?;
    println!("{}", serde_json::to_string(&resp).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_approve_cancel() -> Result<()> {
    let wallet_manager = get_manager().await;

    let params = ApproveReq {
        from: "0x998522f928A37837Fa8d6743713170243b95f98a".to_string(),
        spender: AGGREGATOR.to_string(),
        contract: "0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string(),
        value: "15".to_string(),
        chain_code: "eth".to_string(),
    };

    let password = "123456".to_string();

    let resp = wallet_manager.approve_cancel(params, password).await?;
    println!("{}", serde_json::to_string(&resp).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_quote() -> Result<()> {
    let wallet_manager = get_manager().await;

    let token_in =
        SwapTokenInfo { token_addr: "".to_string(), symbol: "bnb".to_string(), decimals: 18 };

    let token_out = SwapTokenInfo {
        token_addr: "0x55d398326f99059fF775485246999027B3197955".to_string(),
        symbol: "usdt".to_string(),
        decimals: 6,
    };

    let req = QuoteReq {
        aggregator_addr: "0xD084CbC32648346DC98BF667B2535d61E4243a95".to_string(),
        recipient: "0x998522f928A37837Fa8d6743713170243b95f98a".to_string(),
        chain_code: "bnb".to_string(),
        amount_in: "0.003".to_string(),
        token_in,
        token_out,
        dex_list: vec![2, 3],
        slippage: None,
        allow_partial_fill: false,
    };

    let result = wallet_manager.quote(req).await?;
    tracing::warn!("quote = {}", serde_json::to_string(&result).unwrap());
    Ok(())
}

// (选择的流动性)流动性
#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_swap() -> Result<()> {
    let wallet_manager = get_manager().await;

    let o_value = "5";

    let amount_in = wallet_utils::unit::convert_to_u256(&o_value, 6).unwrap();

    let dex_route1 = DexRoute {
        percentage: "10000".to_string(),
        amount_in: amount_in.to_string(),
        amount_out: "0".to_string(),
        route_in_dex: vec![
            RouteInDex {
                dex_id: 3,
                pool_id: "0x5289a8dbf7029ee0b0498a84777ed3941d9acfec".to_string(),
                in_token_symbol: "USDT".to_string(),
                in_token_addr: "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c".to_string(),
                out_token_addr: "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d".to_string(),
                out_token_symbol: "WETH".to_string(),
                zero_for_one: false,
                fee: "500".to_string(),
                amount_in: 0.to_string(),
                min_amount_out: 0.to_string(),
            },
            RouteInDex {
                dex_id: 3,
                pool_id: "0x2c3c320d49019d4f9a92352e947c7e5acfe47d68".to_string(),
                in_token_symbol: "USDT".to_string(),
                in_token_addr: "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d".to_string(),
                out_token_addr: "0x55d398326f99059ff775485246999027b3197955".to_string(),
                out_token_symbol: "WETH".to_string(),
                zero_for_one: false,
                fee: "100".to_string(),
                amount_in: 0.to_string(),
                min_amount_out: 0.to_string(),
            },
        ],
    };

    let token_in =
        SwapTokenInfo { token_addr: "".to_string(), symbol: "BNB".to_string(), decimals: 18 };

    let token_out = SwapTokenInfo {
        token_addr: "0x55d398326f99059fF775485246999027B3197955".to_string(),
        symbol: "usdt".to_string(),
        decimals: 6,
    };
    let req = SwapReq {
        aggregator_addr: AGGREGATOR.to_string(),
        amount_in: o_value.to_string(),
        amount_out: "0.003".to_string(),
        min_amount_out: "0.01".to_string(),
        chain_code: "eth".to_string(),
        recipient: "0x998522f928A37837Fa8d6743713170243b95f98a".to_string(),
        token_in,
        token_out,
        dex_router: vec![dex_route1],
        allow_partial_fill: false,
    };

    let fee =
                r#"{"gasLimit":314848,"baseFee":"0","priorityFee":"1000000000","maxFeePerGas":"1200000000"}"#
                    .to_string();
    let password = "123456".to_string();

    let result = wallet_manager.swap(req, fee, password).await?;
    tracing::warn!("swap hash = {}", serde_json::to_string(&result).unwrap());
    Ok(())
}
