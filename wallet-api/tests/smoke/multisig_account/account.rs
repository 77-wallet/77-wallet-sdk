use crate::get_manager;
use anyhow::Result;
use serial_test::serial;
use wallet_api::request::transaction::ServiceFeePayer;

#[tokio::test]
#[serial]
#[ignore = "requires backend and seeded remote multisig data"]
async fn test_fetch_deposit_address() -> Result<()> {
    let wallet_manager = get_manager().await;
    let chain_code = "tron".to_string();
    let fee = wallet_manager.fetch_deposit_address(chain_code).await?;
    tracing::info!("{}", serde_json::to_string(&fee).unwrap());
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "requires seeded remote multisig fee data"]
async fn test_get_service_fee() -> Result<()> {
    let wallet_manager = get_manager().await;

    let chain_code = "tron".to_string();
    let pay_chain = "tron".to_string();
    let pay_address = "TCDFBkjsZFpyeQcpGUHFJzK2cfFHn6rP5S".to_string();
    let info = wallet_manager.get_multisig_service_fee(pay_chain, chain_code, pay_address).await?;

    tracing::info!("{}", serde_json::to_string(&info).unwrap());
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "requires seeded remote multisig account id"]
async fn test_deploy_multisig_fee() -> Result<()> {
    let wallet_manager = get_manager().await;
    let account_id = "193751654670143488".to_string();
    let fee = wallet_manager.get_account_fee(account_id).await?;

    tracing::info!("{}", serde_json::to_string(&fee).unwrap());
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "requires seeded remote multisig account and signer state"]
async fn test_deploy_multisig_account() -> Result<()> {
    let wallet_manager = get_manager().await;

    let account_id = "255390516227739648".to_string();

    let fee_setting = r#"{"gasLimit": 262975,"baseFee": "3329262291","priorityFee": "0","maxFeePerGas": "3995114749"}"#.to_string();
    let fee_setting = Some(fee_setting);
    // let fee_setting = None;

    let payer = ServiceFeePayer {
        from: "TQnSwWGaFkT2zjumDJkbaFi4uRAvEq4An1".to_string(),
        chain_code: "tron".to_string(),
        symbol: "USDT".to_string(),
        fee_setting: fee_setting.clone(),
        request_resource_id: None,
        token_address: None,
    };
    let deploy_fee = fee_setting;
    let password = "123456".to_string();

    let res = wallet_manager
        .deploy_multisig_account(account_id, deploy_fee, Some(payer), password)
        .await?;
    tracing::info!("部署多签合约{:?}", res);
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "requires backend and seeded remote multisig data"]
async fn test_multisig_account_list() -> Result<()> {
    let wallet_manager = get_manager().await;

    let chain_code = Some("sol".to_string());
    let list = wallet_manager.multisig_account_lists(true, chain_code, 0, 1).await?;

    tracing::info!("account list{}", serde_json::to_string(&list).unwrap());
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "requires seeded remote multisig account id"]
async fn test_cancel_account() -> Result<()> {
    let wallet_manager = get_manager().await;

    let id = "268136908197072896".to_string();
    let list = wallet_manager.cancel_multisig(id).await?;
    println!("{:?}", list);
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "requires backend and seeded remote multisig data"]
async fn test_multisig_account_info() -> Result<()> {
    let wallet_manager = get_manager().await;

    let address = "187341364088934400".to_string();
    let info = wallet_manager.multisig_account_by_id(address).await?;

    tracing::info!("{}", serde_json::to_string(&info).unwrap());
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "requires backend and seeded remote multisig data"]
async fn test_multisig_account_info_by_address() -> Result<()> {
    let wallet_manager = get_manager().await;

    let address = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string();
    let info = wallet_manager.multisig_account_by_address(address).await?;

    tracing::info!("{}", serde_json::to_string(&info).unwrap());
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "requires seeded remote multisig participant id"]
async fn test_check_participant_exists() -> Result<()> {
    let wallet_manager = get_manager().await;

    let id = "185879020414570496".to_string();
    let info = wallet_manager.check_participant_exists(id).await?;

    tracing::info!("{}", serde_json::to_string(&info).unwrap());
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "requires seeded remote multisig participant id"]
async fn test_confirm_participation() -> Result<()> {
    let wallet_manager = get_manager().await;

    let id = "257926276604628992".to_string();
    let info = wallet_manager.confirm_participation(id).await?;

    tracing::info!("{}", serde_json::to_string(&info).unwrap());

    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "requires backend and seeded remote multisig data"]
async fn test_update_multisig_name() -> Result<()> {
    let wallet_manager = get_manager().await;

    let id = "173938720146329600".to_string();
    let name = "name11".to_string();
    let info = wallet_manager.update_multisig_name(id, name).await?;

    tracing::info!("{:?}", serde_json::to_string(&info).unwrap());
    Ok(())
}

#[tokio::test]
#[serial]
#[ignore = "requires backend and seeded remote multisig data"]
async fn test_whether_multisig_address() -> Result<()> {
    let wallet_manager = get_manager().await;

    let address = "TQJSAZj4T5q9BHbQ1HgwPHMrd8PHh81vQe".to_string();
    let chain_code = "tron".to_string();

    let info = wallet_manager.whether_multisig_address(address, chain_code).await?;

    tracing::info!("{}", serde_json::to_string(&info).unwrap());
    Ok(())
}
