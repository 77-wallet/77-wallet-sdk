use super::*;
use crate::service::{
    api_wallet::business_notification::global_business_query_tests::isolated_context,
    asset::AssetsService,
};

#[tokio::test]
async fn ordinary_evm_token_identity_homepage_and_chain_list_agree_for_legacy_request() {
    // Arrange: the migrated five-chain snapshot, plus another account to exclude.
    let (_dir, ctx) = isolated_context().await;
    let pool = ctx.core_pool().unwrap();
    for chain in ["base", "arbitrum", "polygon", "scroll", "avalanche"] {
        sqlx::query("INSERT INTO chain(name,chain_code,protocols,main_symbol,status,created_at) VALUES (?,?,'[]','ETH',1,'2026-01-01T00:00:00Z')")
            .bind(chain).bind(chain).execute(pool.write_ref()).await.unwrap();
        sqlx::query("INSERT INTO coin(name,symbol,chain_code,token_address,price,decimals,is_default,status,created_at) VALUES ('USDT','USDT',?,'0xabc','1',6,1,1,'2026-01-01T00:00:00Z')")
            .bind(chain).execute(pool.write_ref()).await.unwrap();
        for (account, address, balance) in [(3, "owner", "2"), (4, "other", "100")] {
            sqlx::query("INSERT INTO account(account_id,name,address,wallet_address,chain_code,status,is_init,created_at) VALUES (?,'fixture',?,'wallet',?,1,1,'2026-01-01T00:00:00Z')")
                .bind(account).bind(address).bind(chain).execute(pool.write_ref()).await.unwrap();
            sqlx::query("INSERT INTO assets(name,symbol,decimals,address,chain_code,token_address,status,balance,created_at) VALUES ('USDT','USDT',6,?,?,'0xabc',1,?,'2026-01-01T00:00:00Z')")
                .bind(address).bind(chain).bind(balance).execute(pool.write_ref()).await.unwrap();
        }
    }
    sqlx::query("INSERT INTO ordinary_asset_identity_refresh(address,chain_code,token_address) SELECT address,chain_code,token_address FROM assets WHERE address='owner'")
        .execute(pool.write_ref()).await.unwrap();
    // Act: the same account, including an older App's mixed-case contract map.
    let home = AssetsService::new(ctx)
        .get_account_chain_assets_v2("wallet", Some(3), None, Some(false))
        .await
        .unwrap();
    assert_eq!(home.len(), 1);
    let mixed =
        home[0].chain_list.iter().map(|(chain, _)| (chain.clone(), "0xAbC".into())).collect();
    let detail = ChainService::new(ctx)
        .get_chain_assets_list("wallet", Some(3), mixed, Some(false))
        .await
        .unwrap();
    // Assert: five nonzero balances, no account4 rows, and matching total/fiat.
    assert_eq!(home[0].balance.amount, 10.0);
    // Isolated Context has no worker handles: unsuccessful dispatch preserves
    // every durable hint for a later real account-list read.
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ordinary_asset_identity_refresh")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        5
    );
    assert_eq!(detail.len(), 5);
    assert!(detail.iter().all(|r| r.address == "owner" && r.balance.amount == 2.0));
    assert_eq!(detail.iter().map(|r| r.balance.amount).sum::<f64>(), home[0].balance.amount);
    assert_eq!(
        detail.iter().map(|r| r.balance.fiat_value.unwrap()).sum::<f64>(),
        home[0].balance.fiat_value.unwrap()
    );
}

#[tokio::test]
async fn ordinary_evm_token_identity_non_evm_chain_requests_remain_case_sensitive() {
    let (_dir, ctx) = isolated_context().await;
    let pool = ctx.core_pool().unwrap();
    sqlx::raw_sql("INSERT INTO chain(name,chain_code,protocols,main_symbol,status,created_at) VALUES ('TRON','tron','[]','TRX',1,'2026-01-01T00:00:00Z');
        INSERT INTO account(account_id,name,address,wallet_address,chain_code,status,is_init,created_at) VALUES (3,'fixture','owner','wallet','tron',1,1,'2026-01-01T00:00:00Z');
        INSERT INTO coin(name,symbol,chain_code,token_address,price,decimals,is_default,status,created_at) VALUES ('USDT','USDT','tron','TAbC','1',6,1,1,'2026-01-01T00:00:00Z');
        INSERT INTO assets(name,symbol,decimals,address,chain_code,token_address,status,balance,created_at) VALUES ('USDT','USDT',6,'owner','tron','TAbC',1,'2','2026-01-01T00:00:00Z');")
        .execute(pool.write_ref()).await.unwrap();
    for (token, count) in [("TAbC", 1), ("Tabc", 0)] {
        let detail = ChainService::new(ctx)
            .get_chain_assets_list(
                "wallet",
                Some(3),
                HashMap::from([("tron".into(), token.into())]),
                Some(false),
            )
            .await
            .unwrap();
        assert_eq!(detail.len(), count);
    }
}
