use anyhow::Result;
use wallet_api::testkit::env::{get_manager, get_manager_with_config};
use wallet_database::entities::api_withdraw::ApiWithdrawStatus;

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local test data, and fixed withdraw order"]
async fn reject_withdraw_order_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let trade_no = "W2020535510761119744";

    let res = wallet_manager.reject_api_withdrawal_order(trade_no).await;
    tracing::info!("reject_api_withdrawal_order result: {res:?}");
    assert!(res.is_ok(), "reject_api_withdrawal_order failed: {res:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured API-wallet backend, local test data, and fixed withdraw uid"]
async fn page_withdraw_orders_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager().await?;
    wallet_manager.init_api_swap().await?;

    let uid = "5bdb1b748bb617d6683f57565b1493cfa5f9e45f3086daf265ca2e0cd325c15e";
    let page = wallet_manager
        .page_api_withdraw_order(
            uid,
            vec![ApiWithdrawStatus::AuditReject as u8, ApiWithdrawStatus::SendingTxFailed as u8],
            0,
            10,
        )
        .await?;

    for order in &page.data {
        tracing::info!("withdraw order: {}", serde_json::to_string(order).unwrap());
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires configured client4.toml, API-wallet backend, and fixed withdraw order"]
async fn sign_withdraw_order_live_smoke() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager_with_config("client4.toml").await?;
    wallet_manager.init_api_swap().await?;

    let trade_no = "W2059218395332771840";

    let res = wallet_manager.sign_api_withdrawal_order(trade_no).await;
    tracing::info!("sign_api_withdrawal_order result: {res:?}");
    assert!(res.is_ok(), "sign_api_withdrawal_order failed: {res:?}");
    Ok(())
}
