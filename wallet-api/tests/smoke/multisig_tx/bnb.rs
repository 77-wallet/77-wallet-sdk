use crate::get_manager;
use anyhow::Result;
use wallet_api::response_vo::standard_wallet::transaction::TransferParams;
use wallet_database::entities::multisig_member::MemberVo;

#[tokio::test]
#[ignore = "requires backend, EVM chain state, and seeded multisig wallet"]
async fn test_create_multisig_account() -> Result<()> {
    let wallet_manager = get_manager().await;
    let address = "0xdc4778f200c36a1C9dEeb3164cEE8366aD1F9455".to_string();
    let chain_code = "eth".to_string();
    let threshold = 2;

    let member1 = MemberVo::new(
        "alice".to_string(),
        "0xdc4778f200c36a1C9dEeb3164cEE8366aD1F9455".to_string(),
    );

    let member2 =
        MemberVo::new("bob".to_string(), "0xB0E488276b0467aa98ba3c74e098F3C845B78974".to_string());

    // let member3 = Member::new(
    //     "charlie".to_string(),
    //     "0x0fE1bF79406257BaF7114EacA9BE5b78F27A4441".to_string(),
    // );
    let member_list = vec![member1, member2];

    let res = wallet_manager
        .create_multisig_account(
            "multisig".to_string(),
            address,
            chain_code,
            threshold,
            member_list,
            None,
        )
        .await?;

    tracing::info!("{:?}", serde_json::to_string(&res));
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, EVM chain state, and seeded multisig wallet"]
async fn test_balance() -> Result<()> {
    let wallet_manager = get_manager().await;

    let addr = "0xdc4778f200c36a1C9dEeb3164cEE8366aD1F9455";
    let chain_code = "eth";
    // let symbol = "bnb";
    let symbol = "ETH";
    let token_address = None;
    let balance = wallet_manager.chain_balance(addr, chain_code, &symbol, token_address).await?;

    tracing::info!("balance: {:?}", balance);
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, EVM chain state, and seeded multisig wallet"]
async fn test_create_transfer() -> Result<()> {
    let manager = get_manager().await;

    let password = "123456".to_string();
    let params = TransferParams {
        from: "0x8F1E2a99CB688587c02B8b836Ba9Ca39dC60D63B".to_owned(),
        to: "0x148805B49819371EEF9A822f7F880b42Cf67834D".to_owned(),
        value: "0.001".to_owned(),
        expiration: Some(10),
        chain_code: "eth".to_owned(),
        symbol: "ETH".to_owned(),
        notes: Some("salary".to_string()),
        spend_all: false,
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
#[ignore = "requires backend, EVM chain state, and seeded multisig wallet"]
async fn test_queue_list() -> Result<()> {
    let manager = get_manager().await;

    let res = manager.multisig_queue_list(None, None, 1, 0, 10).await?;
    let res = serde_json::to_string(&res).unwrap();
    tracing::info!("queue list = {:?}", res);
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, EVM chain state, and seeded multisig wallet"]
async fn test_queue_info() -> Result<()> {
    let manager = get_manager().await;

    // 队列详情
    let id = "177281407150854144".to_string();
    let res = manager.multisig_queue_info(id).await?;
    let res = serde_json::to_string(&res).unwrap();
    tracing::info!("queue info = {:?}", res);
    Ok(())
}

// 签名交易
#[tokio::test]
#[ignore = "requires backend, EVM chain state, and seeded multisig wallet"]
async fn test_sign_transaction() -> Result<()> {
    let wallet_manager = get_manager().await;

    let queue_id = "169234572050042880".to_owned();
    let status = 1;
    let password = "123456".to_string();
    let sign = wallet_manager.sign_transaction(queue_id, status, password, None).await?;

    tracing::info!("sign res  = {:?}", sign);
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, EVM chain state, and seeded multisig wallet"]
async fn test_multisig_transfer_fee() -> Result<()> {
    let wallet_manager = get_manager().await;

    let queue_id = "203256774709612544".to_owned();
    let fee = wallet_manager.estimate_multisig_transfer_fee(queue_id).await?;

    tracing::info!("transfer fee = {:?}", serde_json::to_string(&fee).unwrap());
    Ok(())
}

// 执行交易
#[tokio::test]
#[ignore = "requires backend, EVM chain state, and seeded multisig wallet"]
async fn test_execute() -> Result<()> {
    let wallet_manager = get_manager().await;
    let id = "177281407150854144".to_string();

    let fee = r#"{"gasLimit": 262975,"baseFee": "3329262291","priorityFee": "0","maxFeePerGas": "3995114749"}"#
    .to_string();
    let fee = Some(fee);

    let password = "123456".to_string();
    let result = wallet_manager.exec_transaction(id, password, fee, None).await?;
    tracing::info!("execute res = {:?}", serde_json::to_string(&result).unwrap());
    Ok(())
}
