use crate::get_manager;
use anyhow::Result;
use wallet_api::{request::transaction::Signer, response_vo::TransferParams};
use wallet_database::entities::multisig_member::MemberVo;

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded multisig wallet"]
async fn test_create_multisig_account() -> Result<()> {
    let wallet_manager = get_manager().await;
    let address = "TKfzG9aNQ5vBNwQHGB3w7cCHkGZ6YQBcT9".to_string();
    let chain_code = "tron".to_string();

    let threshold = 2;
    let member1 =
        MemberVo::new("发起人".to_string(), "TKfzG9aNQ5vBNwQHGB3w7cCHkGZ6YQBcT9".to_string());

    let member2 =
        MemberVo::new("account_0".to_string(), "TKKjkyjSMZ9iy8ATJsLp1X4yNqr39Q5v8Q".to_string());

    // let member3 = MemberVo::new(
    //     "account_3".to_string(),
    //     "TF5qaPzkzB9s8o8omFP4wNwW1Gxtcx4zQr".to_string(),
    // );

    let member_list = vec![member1, member2];

    let res = wallet_manager
        .create_multisig_account(
            "picker01".to_string(),
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
#[ignore = "requires backend, TRON chain state, and seeded multisig wallet"]
async fn test_create_transfer() -> Result<()> {
    let manager = get_manager().await;

    let password = "123456".to_string();

    let _signer =
        Signer { address: "TKDDywzwyYJD8n1BMy5cqr7cxjEFaKJ8h3".to_string(), permission_id: 2 };
    let signer = None;

    let params = TransferParams {
        from: "TQnSwWGaFkT2zjumDJkbaFi4uRAvEq4An1".to_owned(),
        to: "TXjTCY6MvvTpxiNaHdkYsRk5FUZ4kh3fUh".to_owned(),
        value: "2".to_owned(),
        expiration: Some(5),
        chain_code: "tron".to_owned(),
        symbol: "TRX".to_owned(),
        notes: Some("salary".to_string()),
        spend_all: false,
        signer,
        token_address: None,
    };

    // 创建交易
    let res = manager.create_multisig_queue(params, password).await?;
    let res = serde_json::to_string(&res).unwrap();
    tracing::info!("tx info of = {:?}", res);
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded multisig wallet"]
async fn test_queue_list() -> Result<()> {
    let manager = get_manager().await;

    // 列表
    let res = manager.multisig_queue_list(None, None, 2, 0, 10).await?;
    let res = serde_json::to_string(&res).unwrap();
    tracing::info!("queue list = {}", res);
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded multisig wallet"]
async fn test_queue_info() -> Result<()> {
    let manager = get_manager().await;

    // 队列详情
    let id = "213831908549857280".to_string();
    let res = manager.multisig_queue_info(id).await?;
    let res = serde_json::to_string(&res).unwrap();
    tracing::info!("queue info = {}", res);
    Ok(())
}

// 签名交易
#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded multisig wallet"]
async fn test_sign_transaction() -> Result<()> {
    let wallet_manager = get_manager().await;

    let queue_id = "254347646645440512".to_owned();
    let status = 1;
    let password = "123456".to_string();
    let sign = wallet_manager.sign_transaction(queue_id, status, password, None).await?;

    tracing::info!("sign res  = {:?}", sign);
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded multisig wallet"]
async fn test_multisig_transfer_fee() -> Result<()> {
    let wallet_manager = get_manager().await;

    let queue_id = "255392028073005056".to_owned();
    let fee = wallet_manager.estimate_multisig_transfer_fee(queue_id).await?;

    tracing::info!("transfer fee = {}", serde_json::to_string(&fee).unwrap());
    Ok(())
}

// 执行交易
#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded multisig wallet"]
async fn test_execute() -> Result<()> {
    let wallet_manager = get_manager().await;
    let id = "255436715676798976".to_string();

    let password = "123456".to_string();
    let fee = None;

    let result = wallet_manager.exec_transaction(id, password, fee, None).await?;
    tracing::info!("execute res = {}", serde_json::to_string(&result).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded multisig wallet"]
async fn test_check_ongoing() -> Result<()> {
    let wallet_manager = get_manager().await;

    let chain_code = "btc".to_string();
    let address = "7xFhDzUVuirPCW8buDk9AqFcyuZ6CzMYv1Ah1GzK6Q5a".to_string();
    let rs = wallet_manager.check_ongoing_queue(chain_code, address).await?;

    tracing::info!("res {}", serde_json::to_string(&rs).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "requires backend, TRON chain state, and seeded multisig wallet"]
async fn test_cancel_queue() -> Result<()> {
    let wallet_manager = get_manager().await;

    let queue_id = "255435582476521472".to_string();
    let rs = wallet_manager.cancel_queue(queue_id).await?;

    tracing::info!("res {}", serde_json::to_string(&rs).unwrap());
    Ok(())
}
