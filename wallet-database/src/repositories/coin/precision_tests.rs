use super::*;
use std::sync::Arc;

async fn fixture() -> CoreDbPool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./schema/core/migrations").run(&pool).await.unwrap();
    let pool = CoreDbPool::new(Arc::new(pool));
    CoinRepo::upsert_multi_coin(&pool, vec![coin(9)]).await.unwrap();
    for (address, chain, token, decimals, balance) in [
        ("owner-a", "avalanche", "", 9, "896724588.534944613"),
        ("owner-b", "avalanche", "", 9, "0"),
        ("owner-a", "base", "", 18, "0.003"),
        ("owner-a", "avalanche", "0xtoken", 6, "3"),
    ] {
        sqlx::query("INSERT INTO assets (name, symbol, address, chain_code, token_address, decimals, balance, status, is_multisig, created_at) VALUES ('asset', 'AVAX', ?, ?, ?, ?, ?, 1, 0, '2026-10-01T00:00:00Z')")
            .bind(address).bind(chain).bind(token).bind(decimals).bind(balance)
            .execute(pool.write_ref()).await.unwrap();
    }
    pool
}

fn coin(decimals: u8) -> CoinData {
    CoinData::new(
        Some("Avalanche".into()),
        "AVAX",
        "avalanche",
        AssetTokenKey::Native,
        Some("11".into()),
        None,
        decimals,
        1,
        1,
        1,
        false,
        Utc::now(),
        Utc::now(),
    )
}

async fn snapshot(pool: &CoreDbPool) -> Vec<(String, String, String, i64, String)> {
    sqlx::query_as("SELECT address, chain_code, token_address, decimals, balance FROM assets ORDER BY chain_code, token_address, address")
        .fetch_all(pool.read_ref()).await.unwrap()
}

async fn update(pool: &CoreDbPool, unit: Option<u8>) -> Result<(), crate::Error> {
    CoinRepo::update_price_unit(
        pool.clone(),
        &CoinId::new("avalanche", "AVAX", AssetTokenKey::Native),
        "12",
        unit,
        None,
        None,
        None,
        None,
    )
    .await
}

#[tokio::test]
async fn precision_update_repairs_existing_balances_and_is_idempotent() {
    let pool = fixture().await;
    update(&pool, Some(18)).await.unwrap();
    let after = snapshot(&pool).await;
    assert_eq!(after[0].3, 18);
    assert_eq!(after[0].4, "0.896724588534944613");
    assert_eq!(after[1].3, 18);
    assert_eq!(after[1].4, "0");
    assert_eq!(after[2].3, 6);
    assert_eq!(after[2].4, "3");
    assert_eq!(after[3].3, 18);
    assert_eq!(after[3].4, "0.003");
    update(&pool, Some(18)).await.unwrap();
    assert_eq!(snapshot(&pool).await, after);
}

#[tokio::test]
async fn precision_upsert_repairs_assets_even_if_coin_is_already_correct() {
    let pool = fixture().await;
    sqlx::query("UPDATE coin SET decimals = 18 WHERE chain_code = 'avalanche'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    CoinRepo::upsert_multi_coin(&pool, vec![coin(18)]).await.unwrap();
    let after = snapshot(&pool).await;
    assert_eq!(after[0].3, 18);
    assert_eq!(after[0].4, "0.896724588534944613");
}

#[tokio::test]
async fn precision_invalid_cached_balance_rolls_back_coin_and_assets() {
    let pool = fixture().await;
    sqlx::query("UPDATE assets SET balance = 'invalid' WHERE address = 'owner-b' AND chain_code = 'avalanche'")
        .execute(pool.write_ref()).await.unwrap();
    let before = snapshot(&pool).await;
    assert!(update(&pool, Some(18)).await.is_err());
    assert_eq!(snapshot(&pool).await, before);
    let metadata =
        CoinRepo::coin_by_chain_token_key("avalanche", AssetTokenKey::Native, &pool).await.unwrap();
    assert_eq!(metadata.decimals, 9);
    assert_eq!(metadata.price, "11");
}

#[tokio::test]
async fn precision_price_only_update_preserves_assets() {
    let pool = fixture().await;
    let before = snapshot(&pool).await;
    update(&pool, None).await.unwrap();
    assert_eq!(snapshot(&pool).await, before);
}

#[tokio::test]
async fn precision_zero_decimals_preserves_integer_trailing_zeroes() {
    let pool = fixture().await;
    sqlx::query("UPDATE assets SET balance = '0.0000001' WHERE address = 'owner-a' AND token_address = '' AND chain_code = 'avalanche'")
        .execute(pool.write_ref()).await.unwrap();
    update(&pool, Some(0)).await.unwrap();
    let after = snapshot(&pool).await;
    assert_eq!(after[0].4, "100");
    assert_eq!(after[1].4, "0");
}

#[tokio::test]
async fn precision_batch_handles_many_tokens_and_owners_without_touching_other_tokens() {
    let (pool, coins) = batch_fixture(450, 2).await;
    CoinRepo::upsert_multi_coin(&pool, coins).await.unwrap();
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT decimals, balance FROM assets WHERE address LIKE 'batch-%'")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(rows.len(), 900);
    assert!(rows.iter().all(|(decimals, balance)| *decimals == 18 && balance == "1"));
    let other: (i64, String) =
        sqlx::query_as("SELECT decimals, balance FROM assets WHERE token_address = '0xtoken'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(other, (6, "3".into()));
}

async fn batch_fixture(token_count: usize, owner_count: usize) -> (CoreDbPool, Vec<CoinData>) {
    let pool = fixture().await;
    let mut coins = vec![coin(18)];
    let mut tx = pool.write_ref().begin().await.unwrap();
    for i in 0..token_count {
        let mut token = coin(18);
        token.symbol = format!("TOKEN{i}");
        token.token_address = AssetTokenKey::Contract(format!("0xtoken{i}"));
        for owner in 0..owner_count {
            sqlx::query("INSERT INTO assets (name, symbol, address, chain_code, token_address, decimals, balance, status, created_at) VALUES ('token', ?, ?, 'avalanche', ?, 9, '1000000000', 1, '2026-10-02T00:00:00Z')")
                .bind(&token.symbol).bind(format!("batch-{owner}")).bind(token.token_address.as_db_str())
                .execute(tx.as_mut()).await.unwrap();
        }
        coins.push(token);
    }
    tx.commit().await.unwrap();
    // Duplicate delivery must not produce duplicate repairs in an UPDATE batch.
    coins.push(coin(18));
    (pool, coins)
}

#[tokio::test]
#[ignore = "manual offline scale measurement; no timing threshold"]
async fn precision_scale_1000_tokens_20000_assets() {
    let (pool, coins) = batch_fixture(1000, 20).await;
    let mut initial = Some(coins);
    for round in 0..3 {
        let coins = initial.take().unwrap_or_else(|| {
            let mut coins = vec![coin(18)];
            for i in 0..1000 {
                let mut token = coin(18);
                token.symbol = format!("TOKEN{i}");
                token.token_address = AssetTokenKey::Contract(format!("0xtoken{i}"));
                coins.push(token);
            }
            coins
        });
        let start = std::time::Instant::now();
        CoinRepo::upsert_multi_coin(&pool, coins).await.unwrap();
        eprintln!(
            "precision scale: 1000 tokens / 20000 assets, round={round}, elapsed={:?}",
            start.elapsed()
        );
        let correct: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM assets WHERE address LIKE 'batch-%' AND decimals = 18 AND balance = '1'")
            .fetch_one(pool.read_ref()).await.unwrap();
        assert_eq!(correct, 20000);
    }
}

#[tokio::test]
async fn precision_batch_failure_rolls_back_all_asset_batches_and_coin_upsert() {
    let pool = fixture().await;
    let mut tx = pool.write_ref().begin().await.unwrap();
    for i in 0..450 {
        sqlx::query("INSERT INTO assets (name, symbol, address, chain_code, token_address, decimals, balance, status, created_at) VALUES ('AVAX', 'AVAX', ?, 'avalanche', '', 9, ?, 1, '2026-10-02T00:00:00Z')")
            .bind(format!("owner-{i:04}"))
            .bind(if i == 449 { "invalid" } else { "1000000000" })
            .execute(tx.as_mut()).await.unwrap();
    }
    tx.commit().await.unwrap();
    let before = snapshot(&pool).await;
    assert!(CoinRepo::upsert_multi_coin(&pool, vec![coin(18)]).await.is_err());
    assert_eq!(snapshot(&pool).await, before);
    let metadata =
        CoinRepo::coin_by_chain_token_key("avalanche", AssetTokenKey::Native, &pool).await.unwrap();
    assert_eq!(metadata.decimals, 9);
}
