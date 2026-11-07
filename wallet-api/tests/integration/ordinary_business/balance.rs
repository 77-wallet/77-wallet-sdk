use crate::harness::ordinary_business::{OWNER, Scenario, TOKEN};
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn ordinary_business_rpc_failure_preserves_balance_then_recovers() -> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_asset(OWNER, "eth", "", 18, "7").await?;
    s.rpc.fail("eth_getBalance");
    s.when_refresh().await?;
    s.then_balance(OWNER, "eth", "", "7").await?;
    s.rpc.reply("eth_getBalance", json!("0x1bc16d674ec80000"));
    s.when_refresh().await?;
    s.then_balance(OWNER, "eth", "", "2").await?;
    s.rpc.then_consumed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_partial_failure_saves_success_without_zeroing_failure()
-> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_asset(OWNER, "eth", "", 18, "7").await?;
    s.given_asset(OWNER, "eth", TOKEN, 6, "9").await?;
    s.rpc.reply("eth_getBalance", json!("0x1bc16d674ec80000"));
    s.rpc.fail("eth_call");
    s.when_refresh().await?;
    s.then_balance(OWNER, "eth", "", "2").await?;
    s.then_balance(OWNER, "eth", TOKEN, "9").await?;
    s.rpc.reply("eth_getBalance", json!("0x29a2241af62c0000"));
    s.rpc.reply("eth_call", json!(format!("0x{:064x}", 4_000_000u64)));
    s.when_refresh().await?;
    s.then_balance(OWNER, "eth", "", "3").await?;
    s.then_balance(OWNER, "eth", TOKEN, "4").await?;
    s.rpc.then_consumed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_late_old_response_cannot_replace_new_balance() -> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_asset(OWNER, "eth", "", 18, "7").await?;
    let gate = s.rpc.hold("eth_getBalance", json!("0xde0b6b3a7640000"));
    let manager = s.manager.clone();
    let old =
        tokio::spawn(
            async move { manager.sync_assets(OWNER.into(), Some("eth".into()), vec![]).await },
        );
    gate.reached().await;
    s.rpc.reply("eth_getBalance", json!("0x1bc16d674ec80000"));
    s.when_refresh().await?;
    s.then_balance(OWNER, "eth", "", "2").await?;
    gate.release();
    old.await??;
    s.rpc.then_consumed();
    s.then_balance(OWNER, "eth", "", "2").await?;
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_transport_timeout_preserves_balance_then_recovers() -> anyhow::Result<()>
{
    let s = Scenario::given().await?;
    s.given_asset(OWNER, "eth", "", 18, "7").await?;
    s.rpc.timeout("eth_getBalance");
    let started = std::time::Instant::now();
    s.when_refresh().await?;
    assert!(
        started.elapsed() >= std::time::Duration::from_secs(25),
        "must exercise transport timeout, not JSON-RPC error"
    );
    s.then_balance(OWNER, "eth", "", "7").await?;
    s.rpc.reply("eth_getBalance", json!("0x1bc16d674ec80000"));
    s.when_refresh().await?;
    s.then_balance(OWNER, "eth", "", "2").await?;
    s.rpc.then_consumed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_single_coin_and_bulk_refresh_share_ordering() -> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_asset(OWNER, "eth", "", 18, "7").await?;
    let gate = s.rpc.hold("eth_getBalance", json!("0xde0b6b3a7640000"));
    let manager = s.manager.clone();
    let old =
        tokio::spawn(
            async move { manager.sync_assets(OWNER.into(), Some("eth".into()), vec![]).await },
        );
    gate.reached().await;
    s.rpc.reply("eth_getBalance", json!("0x1bc16d674ec80000"));
    let balance = s.manager.chain_balance(OWNER, "eth", "ETH", None).await?;
    assert_eq!(balance.balance, "2");
    gate.release();
    old.await??;
    s.then_balance(OWNER, "eth", "", "2").await?;
    s.rpc.then_consumed();
    Ok(())
}
#[tokio::test]
#[serial]
async fn ordinary_business_newer_failure_rejects_old_response_and_allows_retry()
-> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_asset(OWNER, "eth", "", 18, "7").await?;
    let gate = s.rpc.hold("eth_getBalance", json!("0xde0b6b3a7640000"));
    let manager = s.manager.clone();
    let old =
        tokio::spawn(
            async move { manager.sync_assets(OWNER.into(), Some("eth".into()), vec![]).await },
        );
    gate.reached().await;
    s.rpc.fail("eth_getBalance");
    s.when_refresh().await?;
    gate.release();
    old.await??;
    s.then_balance(OWNER, "eth", "", "7").await?;
    s.rpc.reply("eth_getBalance", json!("0x1bc16d674ec80000"));
    s.when_refresh().await?;
    s.then_balance(OWNER, "eth", "", "2").await?;
    s.rpc.then_consumed();
    Ok(())
}
#[tokio::test]
#[serial]
async fn ordinary_business_cancelled_refresh_allows_successful_retry() -> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_asset(OWNER, "eth", "", 18, "7").await?;
    let gate = s.rpc.hold("eth_getBalance", json!("0xde0b6b3a7640000"));
    let manager = s.manager.clone();
    let old =
        tokio::spawn(
            async move { manager.sync_assets(OWNER.into(), Some("eth".into()), vec![]).await },
        );
    gate.reached().await;
    old.abort();
    assert!(old.await.unwrap_err().is_cancelled());
    gate.release();
    s.rpc.reply("eth_getBalance", json!("0x1bc16d674ec80000"));
    s.when_refresh().await?;
    s.then_balance(OWNER, "eth", "", "2").await?;
    s.rpc.then_consumed();
    Ok(())
}
#[tokio::test]
#[serial]
async fn ordinary_business_refresh_versions_do_not_cross_contexts() -> anyhow::Result<()> {
    let a = Scenario::given().await?;
    let b = Scenario::given().await?;
    for s in [&a, &b] {
        s.given_asset(OWNER, "eth", "", 18, "7").await?;
    }
    let gate = a.rpc.hold("eth_getBalance", json!("0xde0b6b3a7640000"));
    let manager = a.manager.clone();
    let old =
        tokio::spawn(
            async move { manager.sync_assets(OWNER.into(), Some("eth".into()), vec![]).await },
        );
    gate.reached().await;
    b.rpc.reply("eth_getBalance", json!("0x1bc16d674ec80000"));
    b.when_refresh().await?;
    gate.release();
    old.await??;
    a.then_balance(OWNER, "eth", "", "1").await?;
    b.then_balance(OWNER, "eth", "", "2").await?;
    a.rpc.then_consumed();
    b.rpc.then_consumed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_backend_entries_cannot_overwrite_new_rpc_balance() -> anyhow::Result<()>
{
    use crate::harness::worker::start_mock_backend_server;
    use wallet_api::service::asset::AssetsService;
    for wallet_entry in [false, true] {
        let (url, recorder) = start_mock_backend_server()?;
        let s = Scenario::given_backend(&url).await?;
        s.given_asset(OWNER, "eth", "", 18, "7").await?;
        recorder.succeed_next_backend_call_with_data(json!({"list":[{
            "address":OWNER,"chainCode":"eth","contractAddress":null,"symbol":"ETH","amount":"1"
        }]}));
        let gate = recorder.hold_next_response();
        let ctx = s.ctx;
        let old = tokio::spawn(async move {
            let service = AssetsService::new(ctx);
            if wallet_entry {
                service.sync_assets_by_wallet_backend("wallet-a".into(), Some(1), vec![]).await
            } else {
                service.sync_assets_from_backend(OWNER.into(), Some("eth".into()), vec![]).await
            }
        });
        gate.wait_for_request().await;
        s.rpc.reply("eth_getBalance", json!("0x1bc16d674ec80000"));
        s.when_refresh().await?;
        gate.release();
        old.await??;
        s.then_balance(OWNER, "eth", "", "2").await?;
        let requests = recorder.snapshot();
        let path = if wallet_entry { "/wallet/assets/list" } else { "/wallet/assets/chain/list" };
        assert_eq!(requests.iter().filter(|r| r.path == path).count(), 1, "{requests:?}");
        assert!(
            requests.iter().all(|r| r.path == path || r.path == "/app/rpc/token"),
            "{requests:?}"
        );
        s.rpc.then_consumed();
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_backend_preserves_hidden_and_inactive_identity_scope()
-> anyhow::Result<()> {
    use crate::harness::worker::start_mock_backend_server;
    use wallet_api::service::asset::AssetsService;
    for wallet_entry in [false, true] {
        let (url, recorder) = start_mock_backend_server()?;
        let s = Scenario::given_backend(&url).await?;
        s.given_asset(OWNER, "eth", "", 18, "7").await?;
        super::support::given_hidden_inactive_identity(&s).await?;
        recorder.succeed_next_backend_call_with_data(json!({"list":[{
            "address":OWNER,"chainCode":"eth","contractAddress":null,"symbol":"ETH","amount":"2"
        }]}));
        let service = AssetsService::new(s.ctx);
        if wallet_entry {
            service.sync_assets_by_wallet_backend("wallet-a".into(), Some(1), vec![]).await?;
        } else {
            service.sync_assets_from_backend(OWNER.into(), Some("eth".into()), vec![]).await?;
        }
        s.then_balance(OWNER, "eth", "", "2").await?;
        super::support::then_asset_is_hidden(&s).await?;
        assert_eq!(recorder.snapshot().len(), 1);
        s.rpc.then_consumed();
    }
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_empty_backend_account_does_not_invalidate_other_refresh()
-> anyhow::Result<()> {
    use crate::harness::worker::start_mock_backend_server;
    use wallet_api::service::asset::AssetsService;
    let (url, recorder) = start_mock_backend_server()?;
    let s = Scenario::given_backend(&url).await?;
    s.given_asset(OWNER, "eth", "", 18, "7").await?;
    let gate = s.rpc.hold("eth_getBalance", json!("0xde0b6b3a7640000"));
    let manager = s.manager.clone();
    let old =
        tokio::spawn(
            async move { manager.sync_assets(OWNER.into(), Some("eth".into()), vec![]).await },
        );
    gate.reached().await;
    recorder.succeed_next_backend_call_with_data(json!({"list":[]}));
    AssetsService::new(s.ctx)
        .sync_assets_by_wallet_backend("wallet-a".into(), Some(99), vec![])
        .await?;
    gate.release();
    old.await??;
    s.then_balance(OWNER, "eth", "", "1").await?;
    let requests = recorder.snapshot();
    assert_eq!(
        requests.iter().filter(|r| r.path == "/wallet/assets/list").count(),
        1,
        "{requests:?}"
    );
    assert!(
        requests.iter().all(|r| r.path == "/wallet/assets/list" || r.path == "/app/rpc/token"),
        "{requests:?}"
    );
    s.rpc.then_consumed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_stale_bill_balance_keeps_new_balance_and_confirmation()
-> anyhow::Result<()> {
    use wallet_api::service::transaction::TransactionService;
    let s = Scenario::given().await?;
    s.given_asset(OWNER, "eth", "", 18, "7").await?;
    let (hash, bill_id) = super::support::given_successful_bill_response(&s).await?;
    let gate = s.rpc.hold("eth_getBalance", json!("0xde0b6b3a7640000"));
    let ctx = s.ctx;
    let old =
        tokio::spawn(
            async move { TransactionService::new(ctx).query_tx_result(vec![bill_id]).await },
        );
    gate.reached().await;
    s.rpc.reply("eth_getBalance", json!("0x1bc16d674ec80000"));
    s.when_refresh().await?;
    gate.release();
    let result = old.await??;
    assert_eq!(result.len(), 1);
    s.then_balance(OWNER, "eth", "", "2").await?;
    super::support::then_bill_is_successful(&s, &hash).await?;
    s.rpc.then_consumed();
    Ok(())
}
