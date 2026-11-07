use crate::get_manager;
use anyhow::Result;
use wallet_api::request::transaction::{self, Signer};

#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_balance() -> Result<()> {
    let wallet_manager = get_manager().await;

    let addr = "TQACP632EQvyecJTG5wTvMuqy8a4f4TJVr";
    let chain_code = "tron";
    let symbol = "TRX";
    // let symbol = "USDT";
    let token_address = None;
    let balance = wallet_manager.chain_balance(addr, chain_code, &symbol, token_address).await?;

    tracing::info!("balance: {:?}", balance);
    Ok(())
}

// 交易手续费
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_fee() -> Result<()> {
    let wallet_manager = get_manager().await;

    let from = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1";
    let to = "TP6WfoVEG8PayHXP5Kmrww961kMLRv9VJv";
    let value = "500";
    let chain_code = "tron";
    let symbol = "TRX";
    // let symbol = "USDT";

    let mut params = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);
    params.with_notes("test aa".to_string());

    let res = wallet_manager.transaction_fee(params).await?;
    tracing::info!("res: {}", serde_json::to_string(&res).unwrap());
    Ok(())
}

// 转账
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_transfer() -> Result<()> {
    let wallet_manager = get_manager().await;

    let from = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1";
    let to = "TYskFdYh9zsx4XcVRtGY6KhdwgwinmEhSZ";
    let value = "800";
    let symbol = "TRX";
    let chain_code = "tron";
    let password = "123456";
    // let password = "1111111";
    let notes = "test".to_string();

    let _signer =
        Signer { address: "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string(), permission_id: 3 };
    let mut base = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);
    base.with_notes(notes);

    let params = transaction::TransferReq {
        base,
        password: password.to_string(),
        fee_setting: "".to_string(),
        signer: None,
    };

    let transfer = wallet_manager.transfer(params).await?;
    println!("transfer: {:?}", transfer);
    Ok(())
}

// 转账有补贴的情况
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_transfer_with_subsidy() -> Result<()> {
    let wallet_manager = get_manager().await;

    let from = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1";
    let to = "TTofbJMU2iMRhA39AJh51sYvhguWUnzeB1";
    let value = "0.1";
    let symbol = "USDT";
    let chain_code = "tron";
    let password = "123456";

    // request delegate
    let res = wallet_manager.request_energy(from.to_string(), 50000).await?;
    tracing::warn!("request_energy: {:?}", res);

    let mut base = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);
    base.request_resource_id = Some(res);

    let params = transaction::TransferReq {
        base,
        password: password.to_string(),
        fee_setting: "".to_string(),
        signer: None,
    };

    let transfer = wallet_manager.transfer(params).await?;
    tracing::info!("transfer: {:?}", transfer);
    Ok(())
}
