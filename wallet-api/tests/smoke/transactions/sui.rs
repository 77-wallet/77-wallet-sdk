use crate::get_manager;
use anyhow::Result;
use wallet_api::request::transaction;

// 余额测试
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_balance() {
    let wallet_manager = get_manager().await;

    let addr = "0xfba1550112b16f3608669c8ab4268366c7bacb3a2cb844594ad67c21af85a1dd";
    let chain_code = "sui";
    let symbol = "USDT";
    let token_address = None;

    let balance = wallet_manager.chain_balance(addr, chain_code, &symbol, token_address).await;

    println!("balance: {:?}", balance);
}

//交易的手续费
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_fee() -> Result<()> {
    let wallet_manager = get_manager().await;

    let from = "0xfba1550112b16f3608669c8ab4268366c7bacb3a2cb844594ad67c21af85a1dd";
    let to = "0xa042c3ba8208964374cc050922ec94e85fdffe9fc0cd656fb623642ae2fdb4c0";
    let value = "0.01";
    let chain_code = "sui";
    let symbol = "USDT";

    let params = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);
    let res = wallet_manager.transaction_fee(params).await?;

    tracing::info!("res: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// 转账
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_transfer() -> Result<()> {
    let wallet_manager = get_manager().await;

    let from = "0xca93df9d481ff298080047e612dac1ff537d3e24a843e2608428848a108083ec";
    let to = "0x4c1cd48f7f203870be350d7a18c5a827131cecc7322b1571b9a69aeae7dda5f2";
    let value = "0.1";
    let chain_code = "sui";
    let symbol = "SUI";
    let password = "123456";

    let base = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);
    let params = transaction::TransferReq {
        base,
        password: password.to_string(),
        fee_setting: r#""#.to_string(),
        signer: None,
    };

    let token_fee = wallet_manager.transfer(params).await?;
    println!("token transaction: {:?}", token_fee);
    Ok(())
}
