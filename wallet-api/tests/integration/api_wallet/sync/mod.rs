mod support;

use serial_test::serial;

use support::{ScenarioRoles, SyncAssetsGiven, SyncAssetsScenario, SyncAssetsThen, SyncAssetsWhen};

#[tokio::test]
#[serial]
async fn sync_api_assets_by_wallet_updates_api_assets_from_chain() -> anyhow::Result<()> {
    let scenario = SyncAssetsScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let fixture = given.withdrawal_bnb_asset().await?;
    given.chain_balance(123);

    when.sync_api_assets_by_wallet_runs(&fixture).await?;

    then.chain_balance_was_queried_once();
    then.asset_balance_is_chain_balance(&fixture, 123).await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn sync_api_assets_by_wallet_traverses_multiple_address_pages() -> anyhow::Result<()> {
    let scenario = SyncAssetsScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let fixture = given.withdrawal_bnb_assets(201).await?;
    given.chain_balance(123);

    when.sync_api_assets_by_wallet_runs(&fixture).await?;

    then.chain_balance_was_queried(201);
    then.asset_balance_is_chain_balance(&fixture, 123).await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn sync_empty_withdrawal_wallet_does_not_sync_other_wallet_assets() -> anyhow::Result<()> {
    let scenario = SyncAssetsScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let (empty_wallet, other_wallet) = given.empty_withdrawal_wallet_with_other_asset().await?;
    given.chain_balance(123);

    when.sync_api_assets_by_wallet_runs(&empty_wallet).await?;

    then.chain_balance_was_not_queried();
    then.asset_balance_is_zero(&other_wallet).await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn sync_api_assets_by_wallet_keeps_balance_when_chain_query_fails() -> anyhow::Result<()> {
    let scenario = SyncAssetsScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let fixture = given.withdrawal_bnb_asset().await?;
    given.chain_balance_query_fails();

    let result = when.sync_api_assets_by_wallet_returns(&fixture).await;

    then.sync_result_is_ok(result);
    then.chain_balance_was_queried_once();
    then.asset_balance_is_zero(&fixture).await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn sync_api_assets_by_wallet_skips_subaccount_wallet() -> anyhow::Result<()> {
    let scenario = SyncAssetsScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    let fixture = given.subaccount_bnb_asset().await?;
    given.chain_balance(123);

    let result = when.sync_api_assets_by_wallet_returns(&fixture).await;

    then.sync_result_is_ok(result);
    then.chain_balance_was_not_queried();
    then.asset_balance_is_zero(&fixture).await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn sync_api_assets_resolves_funded_reminder_without_another_balance_query()
-> anyhow::Result<()> {
    let scenario = SyncAssetsScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();
    let fixture = given.withdrawal_bnb_asset().await?;
    given.same_address_on_other_chain(&fixture).await?;
    given.balance_reminder(&fixture, "0.000000000000000100").await?;
    given.chain_balance(123);

    when.sync_api_assets_by_wallet_runs(&fixture).await?;

    then.chain_balance_was_queried_once();
    then.asset_balance_is_chain_balance(&fixture, 123).await?;
    then.balance_reminder_resolved(&fixture, true).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn sync_api_assets_keeps_reminder_when_balance_is_insufficient() -> anyhow::Result<()> {
    let scenario = SyncAssetsScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();
    let fixture = given.withdrawal_bnb_asset().await?;
    given.balance_reminder(&fixture, "1").await?;
    given.chain_balance(123);

    when.sync_api_assets_by_wallet_runs(&fixture).await?;

    then.chain_balance_was_queried_once();
    then.balance_reminder_resolved(&fixture, false).await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn sync_api_assets_keeps_reminder_when_balance_query_fails() -> anyhow::Result<()> {
    let scenario = SyncAssetsScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();
    let fixture = given.withdrawal_bnb_asset().await?;
    given.balance_reminder(&fixture, "0.000000000000000100").await?;
    given.chain_balance_query_fails();

    when.sync_api_assets_by_wallet_runs(&fixture).await?;

    then.chain_balance_was_queried_once();
    then.asset_balance_is_zero(&fixture).await?;
    then.balance_reminder_resolved(&fixture, false).await?;
    Ok(())
}
