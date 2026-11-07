use super::*;
use crate::{
    manager::WalletManager,
    response_vo::standard_wallet::chain::ChainList,
    service::api_wallet::{
        business_notification::global_business_query_tests::isolated_context,
        chain::ApiChainService,
    },
};

async fn fixture() -> (tempfile::TempDir, &'static Context, WalletManager) {
    let (dir, ctx) = isolated_context().await;
    let pool = ctx.api_wallet_pool().unwrap();
    for chain in ["base", "scroll"] {
        sqlx::query("INSERT INTO api_chain(name,chain_code,protocols,main_symbol,status,created_at) VALUES(?,?,'[]','ETH',1,'2026-01-01T00:00:00Z')").bind(chain).bind(chain).execute(pool.write_ref()).await.unwrap();
        sqlx::query("INSERT INTO api_coin(name,symbol,chain_code,token_address,price,decimals,is_default,status,created_at) VALUES('USDT','USDT',?,'0xabc','1',6,1,1,'2026-01-01T00:00:00Z')").bind(chain).execute(pool.write_ref()).await.unwrap();
    }
    for (wallet, account, address, chain, balance) in [
        ("wallet", 3, "owner", "base", "10"),
        ("wallet", 3, "owner", "scroll", "4"),
        ("wallet", 4, "other-account", "base", "100"),
        ("other-wallet", 3, "other-owner", "base", "1000"),
    ] {
        sqlx::query("INSERT INTO api_account(account_id,name,address,wallet_address,chain_code,api_wallet_type,status,is_init,created_at) VALUES(?,'fixture',?,?,?,1,1,1,'2026-01-01T00:00:00Z')").bind(account).bind(address).bind(wallet).bind(chain).execute(pool.write_ref()).await.unwrap();
        sqlx::query("INSERT INTO api_assets(decimals,address,chain_code,token_address,status,balance,created_at) VALUES(6,?,?,'0xabc',1,?,'2026-01-01T00:00:00Z')").bind(address).bind(chain).bind(balance).execute(pool.write_ref()).await.unwrap();
    }
    (dir, ctx, WalletManager { ctx, handles: None })
}

fn tokens() -> HashMap<String, String> {
    HashMap::from([("base".into(), "0xabc".into()), ("scroll".into(), "0xabc".into())])
}

#[tokio::test]
async fn api_asset_visibility_missing_account_never_reads_other_scope() {
    // Arrange: populated wallet/account and unrelated wallet/account balances.
    let (_dir, ctx, manager) = fixture().await;
    // Act / Assert: the public v2 and legacy service must keep an empty scope empty.
    for (wallet, account) in [("wallet", 999), ("missing-wallet", 3)] {
        let public = manager.get_api_chain_list(wallet, account, tokens()).await.unwrap();
        assert!(public.is_empty(), "missing scope {wallet}/{account} returned other balances");
        let legacy = ApiChainService::new(ctx)
            .get_chain_assets_list(wallet, Some(account), tokens())
            .await
            .unwrap();
        assert!(legacy.is_empty());
    }
    let valid = manager.get_api_chain_list("wallet", 3, tokens()).await.unwrap();
    assert_eq!(valid.iter().map(|a| a.balance.amount).sum::<f64>(), 14.0);
    assert!(valid.iter().all(|a| a.address == "owner"));
    // Preserve v2's zero placeholder when the account exists but the token does not.
    let missing_token = manager
        .get_api_chain_list("wallet", 3, HashMap::from([("base".into(), "0xdef".into())]))
        .await
        .unwrap();
    assert_eq!(missing_token.len(), 1);
    assert_eq!(missing_token[0].balance.amount, 0.0);
}

#[tokio::test]
async fn api_asset_visibility_removed_account_cannot_fall_back_to_other_balances() {
    let (_dir, ctx, manager) = fixture().await;
    assert_eq!(manager.get_api_chain_list("wallet", 3, tokens()).await.unwrap().len(), 2);
    // A stale page can ask for a previously valid account after its rows are removed.
    sqlx::query("DELETE FROM api_account WHERE wallet_address='wallet' AND account_id=3")
        .execute(ctx.api_wallet_pool().unwrap().write_ref())
        .await
        .unwrap();
    assert!(manager.get_api_chain_list("wallet", 3, tokens()).await.unwrap().is_empty());
    assert!(
        ApiChainService::new(ctx)
            .get_chain_assets_list("wallet", Some(3), tokens())
            .await
            .unwrap()
            .is_empty()
    );
    let other = manager.get_api_chain_list("wallet", 4, tokens()).await.unwrap();
    assert_eq!(other.iter().map(|a| a.balance.amount).sum::<f64>(), 100.0);
}

#[tokio::test]
async fn api_asset_visibility_remove_keeps_homepage_and_chains_consistent() {
    let (_dir, ctx, manager) = fixture().await;
    let before = manager.get_api_assets_list("wallet", Some(3), None, None, false).await.unwrap();
    assert_eq!(before[0].balance.amount, 14.0);
    // Use the real removal API rather than constructing a hidden row directly.
    manager
        .api_remove_assets(
            "wallet",
            Some(3),
            ChainList(HashMap::from([("base".into(), "0xabc".into())])),
        )
        .await
        .unwrap();
    for hide_zero in [false, true] {
        let home =
            manager.get_api_assets_list("wallet", Some(3), None, None, hide_zero).await.unwrap();
        let chains = manager.get_api_chain_list("wallet", 3, tokens()).await.unwrap();
        assert_eq!(home.len(), 1);
        assert_eq!(home[0].balance.amount, 4.0);
        assert_eq!(home[0].balance.amount, chains.iter().map(|a| a.balance.amount).sum::<f64>());
        assert_eq!(home[0].balance.fiat_value, Some(4.0));
        assert!(
            manager
                .get_api_assets_list("wallet", Some(3), Some("base".into()), None, hide_zero)
                .await
                .unwrap()
                .is_empty()
        );
    }
    let other = manager.get_api_assets_list("wallet", Some(4), None, None, false).await.unwrap();
    assert_eq!(other[0].balance.amount, 100.0);
    let stored: (i64, String) = sqlx::query_as(
        "SELECT status,balance FROM api_assets WHERE address='owner' AND chain_code='base'",
    )
    .fetch_one(ctx.api_wallet_pool().unwrap().read_ref())
    .await
    .unwrap();
    assert_eq!(stored, (0, "10".into()));
}
