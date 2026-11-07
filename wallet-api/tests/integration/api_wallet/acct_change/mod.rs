mod support;

use anyhow::Result;
use serial_test::serial;

use support::{AcctChangeGiven, AcctChangeScenario, AcctChangeThen, AcctChangeWhen, ScenarioRoles};

#[tokio::test]
#[serial]
async fn acct_change_syncs_sol_usdc_with_symbol_mismatch_by_token_address() -> Result<()> {
    let scenario = AcctChangeScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let fixture = given.api_wallet_sol_usdc_asset_with_symbol_mismatch().await?;

    let status = when.acct_change_payload_executes(&fixture).await?;

    then.api_wallet_acct_change_succeeds(status);
    then.api_wallet_asset_keeps_usdc_symbol(&fixture).await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn acct_change_normal_wallet_syncs_by_token_when_symbol_mismatch() -> Result<()> {
    let scenario = AcctChangeScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let fixture = given.normal_eth_usdt_asset_with_symbol_mismatch().await?;

    let status = when.acct_change_payload_executes(&fixture).await?;

    then.normal_wallet_acct_change_succeeds(status);
    then.normal_wallet_token_asset_keeps_usdt_symbol(&fixture).await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn acct_change_normal_wallet_syncs_native_by_empty_token_when_token_missing() -> Result<()> {
    let scenario = AcctChangeScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let fixture = given.normal_eth_native_asset_with_missing_token().await?;

    let status = when.acct_change_payload_executes(&fixture).await?;

    then.normal_wallet_native_acct_change_succeeds(status);
    then.normal_wallet_native_asset_keeps_eth_symbol(&fixture).await?;

    Ok(())
}
