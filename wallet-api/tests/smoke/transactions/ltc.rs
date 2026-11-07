use crate::get_manager;
use anyhow::Result;
use wallet_api::request::transaction;

// 余额测试
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_balance() -> Result<()> {
    let wallet_manager = get_manager().await;

    let addr = "LPksEuS2ZeN89BwKQkJw4HAAivrruFDn3j";
    let chain_code = "ltc";
    let symbol = "LTC";
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

    let from = "LPksEuS2ZeN89BwKQkJw4HAAivrruFDn3j";
    let to = "ltc1q4qj00nf5ye30a6ctfgegczfsjja0749ysthwms";

    let value = "0.001";
    let chain_code = "ltc";
    let symbol = "LTC";

    let mut params = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);
    params.with_spend_all(false);

    let res = wallet_manager.transaction_fee(params).await?;
    tracing::info!("token_fee: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// 转账
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_transfer() -> Result<()> {
    let wallet_manager = get_manager().await;

    let from = "LPksEuS2ZeN89BwKQkJw4HAAivrruFDn3j";
    let to = "ltc1q4qj00nf5ye30a6ctfgegczfsjja0749ysthwms";
    let value = "0.001";
    let chain_code = "ltc";
    let symbol = "LTC";
    let password = "123456";

    let mut base = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);
    base.with_spend_all(false);
    let params = transaction::TransferReq {
        base,
        password: password.to_string(),
        fee_setting: "".to_string(),
        signer: None,
    };

    let token_fee = wallet_manager.transfer(params).await?;
    tracing::info!("test_transfer: {}", serde_json::to_string(&token_fee).unwrap());
    Ok(())
}
