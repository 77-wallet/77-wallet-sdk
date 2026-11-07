use crate::get_manager;
use anyhow::Result;
use wallet_api::request::transaction;

// 余额测试
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_balance() -> Result<()> {
    let wallet_manager = get_manager().await;

    let addr = "UQAj45nzNLyAKtnP038PCrqGxwUEpgdrGyz9keGedamIafpw";
    let chain_code = "ton";
    let symbol = "USDT";
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

    let from = "UQAPwCDD910mi8FO1cd5qYdfTHWwEyqMB-RsGkRv-PI2w05u";
    let to = "UQBbNvHNr_Lcqe-Vq8xrLUONWdK-3Di4LbeuWNuLxnRfThq6";
    let value = "0.02";
    let chain_code = "ton";
    let symbol = "TON";

    let mut params = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);
    params.with_spend_all(true);

    let res = wallet_manager.transaction_fee(params).await?;

    tracing::info!("res: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// 转账
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_transfer() -> Result<()> {
    let wallet_manager = get_manager().await;

    let from = "UQDaL1eH_9TU3hceiO7ZsPDEdcmwDhZ0eDZ_NCOIrmjHoSQb";
    let to = "UQAPwCDD910mi8FO1cd5qYdfTHWwEyqMB-RsGkRv-PI2w05u";
    let value = "1";
    let chain_code = "ton";
    let symbol = "USDT";
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
    println!("token transaction: {:?}", token_fee);
    Ok(())
}

#[test]
#[ignore = "manual fee scratch check; legacy smoke entry"]
fn fee_cal() {
    let before = 0.077260348;
    let after = 0.074920963;

    let diff = before - after;
    let fee = 0.002574306;

    println!("diff: {}", diff);
    println!("feee: {}", fee);
}
