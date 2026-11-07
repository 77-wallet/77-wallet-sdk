use crate::get_manager;
use anyhow::Result;

use wallet_api::request::transaction::{
    ApproveReq, DexRoute, QuoteReq, RouteInDex, SwapReq, SwapTokenInfo, SwapTokenListReq,
};

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_default_quote() -> Result<()> {
    let wallet_manager = get_manager().await;

    let chain_code = "sol".to_string();
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
async fn test_token_list() -> Result<()> {
    let wallet_manager = get_manager().await;

    let req = SwapTokenListReq {
        chain_code: "sol".to_string(),
        search: "".to_string(),
        wallet_address: "0x67Fd771e4698D04f19ef4b29B8A95d5637f52B7C".to_string(),
        account_id: 1,
        exclude_token: vec![],
        page_num: 0,
        page_size: 3,
    };

    let resp = wallet_manager.token_list(req).await?;
    println!("{}", serde_json::to_string(&resp).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_approve() -> Result<()> {
    let wallet_manager = get_manager().await;
    // TMrVocuPpNqf3fpPSSWy7V8kyAers3p1Jc
    let params = ApproveReq {
        from: "TQACP632EQvyecJTG5wTvMuqy8a4f4TJVr".to_string(),
        spender: "TTXoDkNZWeTCFqeRs8G5QPjewZduUstPcT".to_string(),
        contract: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string(),
        value: "2".to_string(),
        chain_code: "tron".to_string(),
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
        from: "TQACP632EQvyecJTG5wTvMuqy8a4f4TJVr".to_string(),
        spender: "TTXoDkNZWeTCFqeRs8G5QPjewZduUstPcT".to_string(),
        contract: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string(),
        value: "2".to_string(),
        chain_code: "tron".to_string(),
    };

    let resp = wallet_manager.approve_fee(params, false).await?;
    println!("{}", serde_json::to_string(&resp).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, aggregator, chain state, and seeded swap wallet"]
async fn test_approve_list() -> Result<()> {
    let wallet_manager = get_manager().await;

    let uid = "0b859b5fc29eb644d7ec048f10339cb4b38cc67d9ec327d7e6531bed7984303d".to_string();
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
        from: "TYskFdYh9zsx4XcVRtGY6KhdwgwinmEhSZ".to_string(),
        spender: "TMrVocuPpNqf3fpPSSWy7V8kyAers3p1Jc".to_string(),
        contract: "TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string(),
        value: "15".to_string(),
        chain_code: "tron".to_string(),
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

    // TNUC9Qb1rRpS5CbWLmNMxXBjyFoydXjWFR wtrx
    // TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t usdt
    // TSSMHYeV2uE9qYH95DqyoCuNCzEL1NvU3S sun

    // testnet wtrx TYsbWxNnyTgsZaTFaue9hqpxkU3Fkco94a

    let token_in =
        SwapTokenInfo { token_addr: "".to_string(), symbol: "trx".to_string(), decimals: 6 };

    let token_out = SwapTokenInfo {
        token_addr: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string(),
        symbol: "WTRX".to_string(),
        decimals: 6,
    };

    let req = QuoteReq {
        aggregator_addr: "TSPSEgjQS1gChQLXW8qiVQtwrPdu1E4iCn".to_string(),
        recipient: "TQACP632EQvyecJTG5wTvMuqy8a4f4TJVr".to_string(),
        chain_code: "tron".to_string(),
        amount_in: "1".to_string(),
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

    let o_value = "2";

    let amount_in = wallet_utils::unit::convert_to_u256(&o_value, 6).unwrap();

    let dex_route1 = DexRoute {
        percentage: "10000".to_string(),
        amount_in: amount_in.to_string(),
        amount_out: "0".to_string(),
        route_in_dex: vec![RouteInDex {
            dex_id: 3,
            pool_id: "TSUUVjysXV8YqHytSNjfkNXnnB49QDvZpx".to_string(),
            in_token_symbol: "TUSD".to_string(),
            in_token_addr: "TNUC9Qb1rRpS5CbWLmNMxXBjyFoydXjWFR".to_string(),
            out_token_addr: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string(),
            out_token_symbol: "USDT".to_string(),
            zero_for_one: true,
            fee: "500".to_string(),
            amount_in: 0.to_string(),
            min_amount_out: 0.to_string(),
        }],
    };

    let token_in = SwapTokenInfo {
        token_addr: "TNUC9Qb1rRpS5CbWLmNMxXBjyFoydXjWFR".to_string(),
        symbol: "WTRX".to_string(),
        decimals: 6,
    };

    let token_out =
        SwapTokenInfo { token_addr: "".to_string(), symbol: "TRX".to_string(), decimals: 6 };

    let req = SwapReq {
        aggregator_addr: "TTXoDkNZWeTCFqeRs8G5QPjewZduUstPcT".to_string(),
        amount_in: o_value.to_string(),
        amount_out: "2".to_string(),
        min_amount_out: "2".to_string(),
        chain_code: "tron".to_string(),
        recipient: "TQACP632EQvyecJTG5wTvMuqy8a4f4TJVr".to_string(),
        token_in,
        token_out,
        dex_router: vec![dex_route1],
        allow_partial_fill: false,
    };

    let fee =
                r#"{"gasLimit":3000000,"baseFee":"0","priorityFee":"1000000000","maxFeePerGas":"1000000000"}"#
                    .to_string();
    let password = "123456".to_string();

    let result = wallet_manager.swap(req, fee, password).await?;
    tracing::warn!("swap hash = {}", serde_json::to_string(&result).unwrap());
    Ok(())
}
