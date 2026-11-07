use super::ApiCoinRepo;
use crate::{
    ApiWalletDbPool, entities::api_coin::ApiCoinData,
    repositories::test_helper::setup_api_wallet_pool_before_label_removal as setup_api_wallet_pool,
};
use chrono::Utc;

fn coin(symbol: &str, name: &str) -> ApiCoinData {
    ApiCoinData::new(
        Some(name.into()),
        symbol,
        "polygon",
        "0xAbC".into(),
        Some("1".into()),
        None,
        6,
        1,
        0,
        1,
        Utc::now(),
        None,
    )
}

async fn fixture() -> ApiWalletDbPool {
    let pool = setup_api_wallet_pool("api_coin_metadata_rename").await;
    ApiCoinRepo::upsert_multi_coin(&pool, vec![coin("USDT0", "Old")]).await.unwrap();
    for (address, chain, token, status, balance) in [
        ("active", "polygon", "0xabc", 1, "9.956788"),
        ("hidden", "polygon", "0xAbC", 0, "12.300000"),
        ("other-token", "polygon", "0xdef", 1, "7"),
        ("other-chain", "base", "0xabc", 1, "8"),
    ] {
        sqlx::query("INSERT INTO api_assets(name,symbol,decimals,address,token_address,chain_code,status,balance,is_multisig,created_at,updated_at) VALUES ('Old','USDT0',6,?,?,?,?,?,2,'2026-01-01','2026-01-02')")
            .bind(address).bind(token).bind(chain).bind(status).bind(balance)
            .execute(pool.write_ref()).await.unwrap();
    }
    pool
}

async fn assert_renamed(pool: &ApiWalletDbPool) {
    let coins = ApiCoinRepo::coin_list(pool).await.unwrap();
    assert_eq!((coins[0].symbol.as_str(), coins[0].name.as_str()), ("USDT", "Tether"));
    let rows: Vec<(String,String,String,String,i64,i64,String,String,i64)> = sqlx::query_as(
        "SELECT address,name,symbol,balance,status,decimals,created_at,updated_at,is_multisig FROM api_assets_with_coin_labels ORDER BY address")
        .fetch_all(pool.read_ref()).await.unwrap();
    assert_eq!(rows.len(), 4);
    for (address, name, symbol, balance, status, decimals, created, updated, multisig) in rows {
        let target = address == "active" || address == "hidden";
        assert_eq!(
            (name.as_str(), symbol.as_str()),
            if target { ("Tether", "USDT") } else { ("Old", "USDT0") }
        );
        assert_eq!(
            balance,
            match address.as_str() {
                "active" => "9.956788",
                "hidden" => "12.300000",
                "other-token" => "7",
                _ => "8",
            }
        );
        assert_eq!(status, if address == "hidden" { 0 } else { 1 });
        assert_eq!(
            (decimals, created.as_str(), updated.as_str(), multisig),
            (6, "2026-01-01", "2026-01-02", 2)
        );
    }
}

#[tokio::test]
async fn api_coin_metadata_rename_upsert_preserves_balances_visibility_and_scope() {
    let pool = fixture().await;
    // Act twice: catalog replay must repair existing metadata and remain idempotent.
    for _ in 0..2 {
        ApiCoinRepo::upsert_multi_coin(&pool, vec![coin("USDT", "Tether")]).await.unwrap();
        assert_renamed(&pool).await;
    }
}

#[tokio::test]
async fn api_coin_catalog_rename_avoids_compat_label_writes() {
    let pool = fixture().await;
    sqlx::raw_sql("CREATE TRIGGER reject_asset_rename BEFORE UPDATE OF name,symbol ON api_assets BEGIN SELECT RAISE(ABORT,'unexpected compatibility label write'); END;")
        .execute(pool.write_ref()).await.unwrap();
    ApiCoinRepo::upsert_multi_coin(&pool, vec![coin("USDT", "Tether")]).await.unwrap();
    assert_renamed(&pool).await;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM api_assets WHERE name='Old' AND symbol='USDT0'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(count, 4);
}

#[tokio::test]
async fn api_coin_metadata_rename_incremental_and_price_only_preserve_scope() {
    let pool = fixture().await;
    let id = crate::entities::coin::CoinId {
        chain_code: "polygon".into(),
        symbol: "USDT".into(),
        token_address: "0xAbC".into(),
    };
    ApiCoinRepo::update_price_unit(&id, "2", None, None, None, None, None, &pool).await.unwrap();
    let old: (String, String) =
        sqlx::query_as("SELECT name,symbol FROM api_assets WHERE address='active'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(old, ("Old".into(), "USDT0".into()));
    ApiCoinRepo::update_price_unit(
        &id,
        "3",
        None,
        None,
        None,
        Some("USDT".into()),
        Some("Tether".into()),
        &pool,
    )
    .await
    .unwrap();
    assert_renamed(&pool).await;
    // Replaying an update without a new name must retain the catalog's existing name.
    ApiCoinRepo::update_price_unit(&id, "4", None, None, None, Some("USDT".into()), None, &pool)
        .await
        .unwrap();
    assert_renamed(&pool).await;
}

#[tokio::test]
async fn api_coin_catalog_incremental_failure_is_atomic() {
    let pool = fixture().await;
    sqlx::raw_sql("CREATE TRIGGER reject_catalog_rename BEFORE UPDATE ON api_coin BEGIN SELECT RAISE(ABORT,'injected catalog failure'); END;")
        .execute(pool.write_ref()).await.unwrap();
    let id = crate::entities::coin::CoinId {
        chain_code: "polygon".into(),
        symbol: "USDT".into(),
        token_address: "0xabc".into(),
    };
    assert!(
        ApiCoinRepo::update_price_unit(
            &id,
            "3",
            None,
            None,
            None,
            Some("USDT".into()),
            Some("Tether".into()),
            &pool
        )
        .await
        .is_err()
    );
    let got = ApiCoinRepo::coin_list(&pool).await.unwrap();
    assert_eq!(
        (got[0].symbol.as_str(), got[0].name.as_str(), got[0].price.as_str()),
        ("USDT0", "Old", "1")
    );
}

#[tokio::test]
async fn api_coin_metadata_rename_native_and_case_sensitive_tokens_keep_identity() {
    let pool = setup_api_wallet_pool("api_coin_metadata_rename_native").await;
    for (chain, token) in [("polygon", ""), ("sol", "AbC"), ("sol", "abc")] {
        let mut initial = coin("OLD", "Old");
        initial.chain_code = chain.into();
        initial.token_address = token.into();
        ApiCoinRepo::upsert_multi_coin(&pool, vec![initial]).await.unwrap();
        sqlx::query("INSERT INTO api_assets(name,symbol,decimals,address,token_address,chain_code,status,balance,created_at) VALUES('Old','OLD',6,'owner',?,?,0,'5',CURRENT_TIMESTAMP)")
            .bind(token).bind(chain).execute(pool.write_ref()).await.unwrap();
    }
    for (chain, token) in [("polygon", ""), ("sol", "AbC")] {
        let mut updated = coin("NEW", "New");
        updated.chain_code = chain.into();
        updated.token_address = token.into();
        ApiCoinRepo::upsert_multi_coin(&pool, vec![updated]).await.unwrap();
    }
    let rows:Vec<(String,String,String,String,i64)>=sqlx::query_as("SELECT chain_code,token_address,symbol,balance,status FROM api_assets_with_coin_labels ORDER BY chain_code,token_address")
        .fetch_all(pool.read_ref()).await.unwrap();
    assert_eq!(
        rows,
        vec![
            ("polygon".into(), "".into(), "NEW".into(), "5".into(), 0),
            ("sol".into(), "AbC".into(), "NEW".into(), "5".into(), 0),
            ("sol".into(), "abc".into(), "OLD".into(), "5".into(), 0)
        ]
    );
}

#[tokio::test]
async fn api_coin_metadata_replay_repairs_drift_when_catalog_is_unchanged() {
    let pool = fixture().await;
    ApiCoinRepo::upsert_multi_coin(&pool, vec![coin("USDT", "Tether")]).await.unwrap();
    sqlx::query(
        "UPDATE api_assets SET name='stale',symbol='STALE' WHERE address IN ('active','hidden')",
    )
    .execute(pool.write_ref())
    .await
    .unwrap();
    ApiCoinRepo::upsert_multi_coin(&pool, vec![coin("USDT", "Tether")]).await.unwrap();
    assert_renamed(&pool).await;
    let cached: (String, String) =
        sqlx::query_as("SELECT name,symbol FROM api_assets WHERE address='active'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(cached, ("stale".into(), "STALE".into()));
}

#[tokio::test]
async fn api_coin_metadata_batch_failure_preserves_catalog_and_assets_atomically() {
    let pool = fixture().await;
    let mut second = coin("SECOND", "Second");
    second.token_address = "0xdef".into();
    ApiCoinRepo::upsert_multi_coin(&pool, vec![second]).await.unwrap();
    let coin_query = "SELECT token_address,name,symbol,price FROM api_coin ORDER BY token_address";
    let asset_query = "SELECT address,name,symbol,balance,status FROM api_assets_with_coin_labels ORDER BY address";
    type CoinRow = (String, String, String, String);
    type AssetRow = (String, String, String, String, i64);
    let before_coins: Vec<CoinRow> =
        sqlx::query_as(coin_query).fetch_all(pool.read_ref()).await.unwrap();
    let before_assets: Vec<AssetRow> =
        sqlx::query_as(asset_query).fetch_all(pool.read_ref()).await.unwrap();
    sqlx::raw_sql("CREATE TRIGGER fail_second_metadata BEFORE UPDATE OF name,symbol ON api_coin WHEN NEW.token_address='0xdef' BEGIN SELECT RAISE(ABORT,'injected batch metadata failure'); END;")
        .execute(pool.write_ref()).await.unwrap();
    let mut second = coin("NEW2", "Renamed Second");
    second.token_address = "0xdef".into();
    let mut inserted = coin("NEW3", "New Coin");
    inserted.token_address = "0x123".into();
    assert!(
        ApiCoinRepo::upsert_multi_coin(&pool, vec![coin("USDT", "Tether"), inserted, second])
            .await
            .is_err()
    );
    let after_coins: Vec<CoinRow> =
        sqlx::query_as(coin_query).fetch_all(pool.read_ref()).await.unwrap();
    let after_assets: Vec<AssetRow> =
        sqlx::query_as(asset_query).fetch_all(pool.read_ref()).await.unwrap();
    assert_eq!(before_coins, after_coins);
    assert_eq!(before_assets, after_assets);
}
