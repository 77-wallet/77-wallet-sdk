use crate::get_manager;
use anyhow::Result;
use wallet_api::request::transaction;

// 余额测试
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_balance() -> Result<()> {
    let wallet_manager = get_manager().await;

    let addr = "0x998522f928A37837Fa8d6743713170243b95f98a";
    let chain_code = "eth";
    let symbol = "ETH";
    // let token_address = Some("0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string());
    let token_address = None;

    let balance = wallet_manager.chain_balance(addr, chain_code, &symbol, token_address).await?;

    println!("balance: {:?}", balance);
    Ok(())
}

//交易的手续费
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_fee() -> Result<()> {
    let wallet_manager = get_manager().await;

    let from = "0xdc4778f200c36a1C9dEeb3164cEE8366aD1F9455";
    let to = "0x998522f928A37837Fa8d6743713170243b95f98a";
    let value = "0.000001";
    let chain_code = "eth";
    let symbol = "ETH";
    // let symbol = "USDT";

    let params = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);

    let res = wallet_manager.transaction_fee(params).await?;
    tracing::info!("token_fee: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// 转账
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_transfer() -> Result<()> {
    let wallet_manager = get_manager().await;

    let from = "0xdc4778f200c36a1C9dEeb3164cEE8366aD1F9455";
    let to = "0x998522f928A37837Fa8d6743713170243b95f98a";
    let value = "0.000000908698916";
    let chain_code = "eth";
    let symbol = "ETH";
    // let symbol = "USDT";
    let password = "123456";
    // let notes = "test";

    let base = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);
    let params = transaction::TransferReq {
        base,
        password: password.to_string(),
        fee_setting:  r#"{"gasLimit":23100,"baseFee":"8030957221","priorityFee":"74260000","maxFeePerGas":"12100000000"}"#.to_string(),
        signer:None,
    };

    let token_fee = wallet_manager.transfer(params).await?;
    tracing::info!("test_transfer: {}", serde_json::to_string(&token_fee).unwrap());
    Ok(())
}
