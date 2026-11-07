use crate::get_manager;
use anyhow::Result;
use wallet_api::domain::app::config::ConfigDomain;

#[tokio::test]
#[ignore = "smoke test requires config backend availability"]
async fn test_config_list() -> Result<()> {
    let wallet_manager = get_manager().await;

    let configs = wallet_manager.get_configs().await?;

    tracing::info!("{}", serde_json::to_string(&configs).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires config backend availability"]
async fn test_set_min_value_config() -> Result<()> {
    let wallet_manager = get_manager().await;

    let symbol = "TRx".to_string();
    let value = 1.0;
    let switch = true;

    let configs = wallet_manager.set_min_value_config(symbol, value, switch).await?;
    tracing::info!("{}", serde_json::to_string(&configs).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires config backend availability"]
async fn test_get_min_value_config() -> Result<()> {
    let wallet_manager = get_manager().await;

    let symbol = "dai".to_string();

    let configs = wallet_manager.get_min_value_config(symbol).await?;
    tracing::info!("{}", serde_json::to_string(&configs).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires global message backend availability"]
async fn test_global_msg() -> Result<()> {
    let wallet_manager = get_manager().await;

    let res = wallet_manager.global_msg().await?;

    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires invite backend availability"]
async fn test_request_invite_summary() -> Result<()> {
    let wallet_manager = get_manager().await;

    let endpoint = "invite/summary".to_string();
    let body =
        r#"{"uid":"573648e4144e13848374bc68bdea1c5f862d94ba0eb8b48fdc1461cd25d3fe2b"}"#.to_string();

    let res = wallet_manager.request(endpoint, body).await?;

    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test exercises jpush processing with environment state"]
async fn test_jpush() -> Result<()> {
    let wallet_manager = get_manager().await;

    let message = r#"{"appId":"13065ffa4e8f69587b6","bizType":"ACCT_CHANGE","body":{"blockHeight":56190122,"chainCode":"tron","fromAddr":"TDji1rYpkQsePhW1QeLDZewCPM9nopNvzm","isMultisig":0,"noticeContent":"发送地址 TDji1rYpkQsePhW1QeLDZewCPM9nopNvzm","noticeTitle":"TRX: 0.285008 收款成功","queueId":"","signer":[],"status":true,"symbol":"trx","toAddr":"TKLQSZK4TnS48bQwTX5PrUyZ8yopA3gR4D","token":"","transactionFee":0.267,"transactionTime":"2025-04-17 11:04:48","transferType":0,"txHash":"edfd72beffd6081a18ceb666ab56aa6ceed59504175bd03ff338030e67d41a1e","txKind":1,"value":0.285008,"valueUsdt":0.07035597310382247},"clientId":"52cc52dc39c4fa52fc54b43ac2020bae","deviceType":"ANDROID","sn":"debug_860_9580b55ec4a1d3d3af85077ae0c4c901885b1123e50f830cbd5bfbbe0cb161a3","msgId":"6800e058c0987e3486cba503"}"#;

    let res = wallet_manager.process_jpush_message(message).await?;

    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test reads environment mqtt config"]
async fn test_mqtt_uri() {
    let wallet_manager = get_manager().await;

    let rs = ConfigDomain::get_mqtt_uri(wallet_manager.ctx()).await.unwrap();

    tracing::info!("uri : {}", serde_json::to_string(&rs).unwrap());
}

#[tokio::test]
#[ignore = "smoke test requires backend config endpoint"]
async fn test_backend_config() -> Result<()> {
    let wallet_manager = get_manager().await;

    let res = wallet_manager.backend_config().await?;

    tracing::info!("uri : {}", serde_json::to_string(&res).unwrap());
    Ok(())
}
