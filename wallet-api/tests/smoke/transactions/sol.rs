use crate::get_manager;
use alloy::primitives::U256;
use wallet_api::{
    request::transaction, testkit::sol_transaction::sol_native_transfer_rent_precheck,
};

// 余额测试
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_balance() {
    let wallet_manager = get_manager().await;

    let addr = "37qZgmfhQNvjTfycUeXte3sAucAY4iaqoTZfhFxZb7L1";
    let chain_code = "sol";
    let symbol = "SOL";
    // let token_address = Some("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string());
    let token_address = None;

    let balance = wallet_manager.chain_balance(addr, chain_code, &symbol, token_address).await;

    println!("balance: {:?}", balance);
}

//交易的手续费
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_fee() {
    let wallet_manager = get_manager().await;

    let from = "HVWfJZUBx4Vxn8TgQivP4p3j5gm5pXzBDPeq3JUqZSW";
    let to = "E3LwNpfHFWKnzqdBjmEbjBdYYByfrQCgeDYoemxs3yLu";
    let value = "0.01";
    let chain_code = "sol";
    let symbol = "USDT";

    let params = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);
    let res = wallet_manager.transaction_fee(params).await;
    tracing::info!("fee: {:#?}", res);
}

// 转账
#[tokio::test]
#[ignore = "requires backend/chain providers, fixed addresses, and local wallet credentials"]
async fn test_transfer() {
    let wallet_manager = get_manager().await;

    let from = "CynRxb8RZTuX3cAdTAPoKLLysUnDS482E9z54ySLimQ";
    let to = "GE93MHXVvnsbhxu7Ttpp7zTiipJeCX3QFXueSK2dCJe6";
    let value = "0.005";
    let chain_code = "sol";
    let symbol = "SOL";
    // let symbol = "STK";
    let password = "123456";
    // let notes = "test";

    let base = transaction::BaseTransferReq::new(from, to, value, chain_code, symbol);
    let params = transaction::TransferReq {
        base,
        password: password.to_string(),
        fee_setting: "".to_string(),
        signer: None,
    };

    let token_fee = wallet_manager.transfer(params).await;
    println!("token transaction: {:?}", token_fee);
}

#[tokio::test]
#[ignore = "legacy smoke entry; active rent-precheck unit coverage lives in sol_tx"]
async fn test_withdraw_sol_transfer_requires_rent_for_uninitialized_recipient() {
    // Withdraw uses the shared SolTx native transfer guard, so this regression
    // covers the withdraw-style path without spinning up the full wallet manager.
    let from = "CynRxb8RZTuX3cAdTAPoKLLysUnDS482E9z54ySLimQ";
    let to = "3m2vk1NSfKJK444bCLFCtigFyeHP4cHgvLrtjCJr7nrW";
    let err = sol_native_transfer_rent_precheck(
        from,
        to,
        false,
        U256::from(7_309_206_u64),
        U256::from(15_000_u64),
        U256::from(990_880_u64),
    )
    .expect_err("withdraw-style sol transfer should fail on uninitialized recipient");
    let msg = err.to_string();
    assert!(msg.contains("recipient account is not initialized"));
    assert!(msg.contains("rent-exempt minimum"));
}
