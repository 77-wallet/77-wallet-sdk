use crate::get_manager;
use anyhow::Result;
use wallet_api::response_vo::{MultisigQueueFeeParams, TransferParams};
use wallet_database::entities::multisig_member::MemberVo;

#[tokio::test]
#[ignore = "requires backend, BTC chain state, and seeded multisig wallet"]
async fn test_create_multisig_account() -> Result<()> {
    let wallet_manager = get_manager().await;
    let address = "bc1qgzfqayxf0gw4vnu440uugyawlmvqyzw5c73k79".to_string();
    let chain_code = "btc".to_string();
    let threshold = 2;

    let member1 = MemberVo::new(
        "alice".to_string(),
        "bc1qgzfqayxf0gw4vnu440uugyawlmvqyzw5c73k79".to_string(),
    );

    let member2 =
        MemberVo::new("bob".to_string(), "3HoUeYmrHPQuXr4fJ1zQsF31rntE3JLkg4".to_string());

    // let member3 = MemberVo::new(
    //     "charlie".to_string(),
    //     "bcrt1p8xnk9fmtdsrar7yt9aczejqk4jyfxynsn4fd6gqxpkh8j958caashk0tcp".to_string(),
    // );
    let member_list = vec![member1, member2];

    // let address_type = "Legacy".to_string();
    // let address_type = "Nested SegWit".to_string();
    // let address_type = "Native SegWit".to_string();
    let address_type = "Taproot".to_string();

    let res = wallet_manager
        .create_multisig_account(
            "".to_string(),
            address,
            chain_code,
            threshold,
            member_list,
            Some(address_type),
        )
        .await?;

    tracing::info!("{:?}", serde_json::to_string(&res));
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, BTC chain state, and seeded multisig wallet"]
async fn test_balance() -> Result<()> {
    let wallet_manager = get_manager().await;

    let addr = "bcrt1qrdx3vk9shh7yshszmss0qrdyd8mqxs4muldel0fnrd64wm4zsh8q87d3y5";
    let chain_code = "btc";
    let symbol = "BTC";
    let token_address = None;
    let balance = wallet_manager.chain_balance(addr, chain_code, &symbol, token_address).await?;

    tracing::info!("balance: {:?}", balance);
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, BTC chain state, and seeded multisig wallet"]
async fn test_create_queue_fee() -> Result<()> {
    let manager = get_manager().await;

    let params = MultisigQueueFeeParams {
        from: "38oNcLwirxkiZZbx99BLFQxnawrpQ5GaUf".to_owned(),
        to: "bc1qx7j2a0qce322xusret0upxpg2dgd4unmcf9ec0rgc7kwf5zsmjsqagsk7e".to_owned(),
        value: "0.0001".to_owned(),
        chain_code: "btc".to_owned(),
        symbol: "BTC".to_owned(),
        spend_all: Some(true),
        token_address: None,
    };

    // 创建交易
    let res = manager.create_queue_fee(params).await?;
    let res = serde_json::to_string(&res).unwrap();
    tracing::info!("transaction fee = {}", res);
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, BTC chain state, and seeded multisig wallet"]
async fn test_create_transfer() -> Result<()> {
    let manager = get_manager().await;

    let password = "123456".to_string();
    let params = TransferParams {
        from: "38oNcLwirxkiZZbx99BLFQxnawrpQ5GaUf".to_owned(),
        to: "bc1qx7j2a0qce322xusret0upxpg2dgd4unmcf9ec0rgc7kwf5zsmjsqagsk7e".to_owned(),
        value: "0.00009133".to_owned(),
        expiration: Some(1),
        chain_code: "btc".to_owned(),
        symbol: "BTC".to_owned(),
        notes: Some("salary".to_string()),
        spend_all: true,
        signer: None,
        token_address: None,
    };

    // 创建交易
    let res = manager.create_multisig_queue(params, password).await?;
    let res = serde_json::to_string(&res).unwrap();
    tracing::info!("tx info of = {:?}", res);
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, BTC chain state, and seeded multisig wallet"]
async fn test_queue_list() -> Result<()> {
    let manager = get_manager().await;

    // 创建交易
    let res = manager.multisig_queue_list(None, None, 1, 0, 10).await?;
    let res = serde_json::to_string(&res).unwrap();
    tracing::info!("queue list = {:?}", res);
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, BTC chain state, and seeded multisig wallet"]
async fn test_queue_info() -> Result<()> {
    let manager = get_manager().await;

    // 队列详情
    let id = "159405911742484480".to_string();
    let res = manager.multisig_queue_info(id).await?;
    let res = serde_json::to_string(&res).unwrap();
    tracing::info!("queue info = {:?}", res);
    Ok(())
}

// 签名交易
#[tokio::test]
#[ignore = "requires backend, BTC chain state, and seeded multisig wallet"]
async fn test_sign_transaction() -> Result<()> {
    let wallet_manager = get_manager().await;

    let queue_id = "218846320662810624".to_owned();
    let status = 1;
    let password = "123456".to_string();
    let sign = wallet_manager.sign_transaction(queue_id, status, password, None).await?;

    tracing::info!("sign res  = {:?}", sign);
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, BTC chain state, and seeded multisig wallet"]
async fn test_multisig_transfer_fee() -> Result<()> {
    let wallet_manager = get_manager().await;

    let queue_id = "218846320662810624".to_owned();
    let fee = wallet_manager.estimate_multisig_transfer_fee(queue_id).await?;

    tracing::info!("transfer fee = {}", serde_json::to_string(&fee).unwrap());
    Ok(())
}

// 执行交易
#[tokio::test]
#[ignore = "requires backend, BTC chain state, and seeded multisig wallet"]
async fn test_execute() -> Result<()> {
    let wallet_manager = get_manager().await;

    let id = "218846320662810624".to_string();
    let pass = "123456".to_string();
    let fee_setting = None;
    let request_resource_id = None;

    let result =
        wallet_manager.exec_transaction(id, pass, fee_setting, request_resource_id).await?;
    tracing::info!("execute res = {:?}", serde_json::to_string(&result));
    Ok(())
}
