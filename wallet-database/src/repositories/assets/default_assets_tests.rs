use super::*;
use std::sync::Arc;

async fn fixture() -> CoreDbPool {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./schema/core/migrations").run(&db).await.unwrap();
    let pool = CoreDbPool::new(Arc::new(db));
    sqlx::query("INSERT INTO wallet(name,uid,address,status,is_init,created_at) VALUES ('fixture','fixture','wallet',1,1,'2026-01-01T00:00:00Z')").execute(pool.write_ref()).await.unwrap();
    for chain in ["scroll", "base"] {
        sqlx::query("INSERT INTO chain(name,chain_code,protocols,main_symbol,status,created_at) VALUES ('fixture',?,'[]','ETH',1,'2026-01-01T00:00:00Z')").bind(chain).execute(pool.write_ref()).await.unwrap();
    }
    pool
}
async fn account(pool: &CoreDbPool, address: &str, chain: &str) {
    sqlx::query("INSERT INTO account(account_id,name,address,wallet_address,chain_code,status,is_init,created_at) VALUES (1,'fixture',?,'wallet',?,1,1,'2026-01-01T00:00:00Z')").bind(address).bind(chain).execute(pool.write_ref()).await.unwrap();
}
async fn coin(
    pool: &CoreDbPool,
    chain: &str,
    symbol: &str,
    token: &str,
    default: i64,
    status: i64,
    deleted: i64,
) {
    sqlx::query("INSERT INTO coin(name,symbol,chain_code,token_address,decimals,is_default,is_popular,is_custom,status,is_del,created_at,updated_at) VALUES ('fixture',?,?,?,6,?,0,0,?,?, '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
        .bind(symbol).bind(chain).bind(token).bind(default).bind(status).bind(deleted).execute(pool.write_ref()).await.unwrap();
}
async fn existing(
    pool: &CoreDbPool,
    address: &str,
    chain: &str,
    token: Option<&str>,
    status: i64,
    multisig: i64,
) {
    sqlx::query("INSERT INTO assets(name,symbol,decimals,address,chain_code,token_address,status,is_multisig,balance,created_at,updated_at) VALUES ('saved','OLD',8,?,?,?, ?,?, '123.456','2026-01-01T00:00:00Z','2026-01-02T00:00:00Z')")
        .bind(address).bind(chain).bind(token).bind(status).bind(multisig).execute(pool.write_ref()).await.unwrap();
}
async fn snapshot(
    pool: &CoreDbPool,
) -> Vec<(String, String, String, String, i64, i64, String, String, String)> {
    sqlx::query_as("SELECT address,chain_code,COALESCE(token_address,''),symbol,status,is_multisig,balance,created_at,updated_at FROM assets ORDER BY chain_code,address,token_address,symbol").fetch_all(pool.read_ref()).await.unwrap()
}

#[tokio::test]
async fn ordinary_default_assets_insert_existing_account_token_and_repeat_is_noop() {
    let pool = fixture().await;
    account(&pool, "owner", "scroll").await;
    coin(&pool, "scroll", "USDT", "0xtoken", 1, 1, 0).await;
    assert_eq!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.unwrap(), 1);
    let rows = snapshot(&pool).await;
    assert_eq!(
        (&rows[0].0, &rows[0].1, &rows[0].2, &rows[0].3),
        (&"owner".into(), &"scroll".into(), &"0xtoken".into(), &"USDT".into())
    );
    assert_eq!((rows[0].4, rows[0].5, rows[0].6.as_str()), (1, 0, "0"));
    assert_eq!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.unwrap(), 0);
    assert_eq!(snapshot(&pool).await, rows);
}

#[tokio::test]
async fn ordinary_default_assets_filter_coin_and_account_scope() {
    let pool = fixture().await;
    for (addr, chain) in [
        ("owner", "scroll"),
        ("other-chain", "base"),
        ("disabled-account", "scroll"),
        ("disabled-wallet", "scroll"),
    ] {
        account(&pool, addr, chain).await;
    }
    sqlx::query("UPDATE account SET status=0 WHERE address='disabled-account'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    sqlx::query("INSERT INTO wallet(name,uid,address,status,is_init,created_at) VALUES ('fixture','fixture','disabled',0,1,'2026-01-01T00:00:00Z')").execute(pool.write_ref()).await.unwrap();
    sqlx::query("UPDATE account SET wallet_address='disabled' WHERE address='disabled-wallet'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    for (symbol, token, default, status, deleted) in [
        ("GOOD", "good", 1, 1, 0),
        ("NONDEFAULT", "nondefault", 0, 1, 0),
        ("DISABLED", "disabled", 1, 0, 0),
        ("DELETED", "deleted", 1, 1, 1),
    ] {
        coin(&pool, "scroll", symbol, token, default, status, deleted).await;
    }
    coin(&pool, "base", "BASE", "base-token", 1, 1, 0).await;
    assert_eq!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.unwrap(), 2);
    let rows = snapshot(&pool).await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|r| r.0 == "owner" && r.1 == "scroll" && r.2 == "good"));
    assert!(rows.iter().any(|r| r.0 == "other-chain" && r.1 == "base" && r.2 == "base-token"));
}

#[tokio::test]
async fn ordinary_default_assets_preserve_hidden_balances_and_multisig_even_with_old_symbol() {
    let pool = fixture().await;
    for addr in ["hidden", "saved", "multisig", "legacy-native"] {
        account(&pool, addr, "scroll").await;
    }
    for (addr, token, status, multi) in [
        ("hidden", Some("0xtoken"), 0, 0),
        ("saved", Some("0xtoken"), 1, 0),
        ("multisig", Some("0xtoken"), 1, 1),
        ("legacy-native", None, 0, 0),
    ] {
        existing(&pool, addr, "scroll", token, status, multi).await;
    }
    coin(&pool, "scroll", "NEW", "0xtoken", 1, 1, 0).await;
    coin(&pool, "scroll", "ETH", "", 1, 1, 0).await;
    let before = snapshot(&pool).await;
    // Four opposite-token missing rows, never overwrite/reactivate existing rows.
    assert_eq!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.unwrap(), 4);
    let after = snapshot(&pool).await;
    for row in before {
        assert!(after.contains(&row));
    }
    assert_eq!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.unwrap(), 0);
    assert_eq!(snapshot(&pool).await, after);
}

#[tokio::test]
async fn ordinary_default_assets_disabled_chain_and_multisig_only_address_are_excluded() {
    let pool = fixture().await;
    account(&pool, "ordinary", "base").await;
    sqlx::query("UPDATE chain SET status=0 WHERE chain_code='base'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    coin(&pool, "base", "USDT", "token", 1, 1, 0).await;
    coin(&pool, "scroll", "USDT", "token", 1, 1, 0).await;
    existing(&pool, "multisig-only", "scroll", Some("other"), 1, 1).await;
    let before = snapshot(&pool).await;
    assert_eq!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.unwrap(), 0);
    assert_eq!(snapshot(&pool).await, before);
}

#[tokio::test]
async fn ordinary_default_assets_multiple_pages_and_concurrent_runs_are_idempotent() {
    let pool = fixture().await;
    for i in 0..451 {
        account(&pool, &format!("owner-{i}"), "scroll").await;
    }
    coin(&pool, "scroll", "USDT", "token", 1, 1, 0).await;
    let (a, b) = tokio::join!(
        AssetsRepo::reconcile_ordinary_default_assets(&pool),
        AssetsRepo::reconcile_ordinary_default_assets(&pool)
    );
    assert_eq!(a.unwrap() + b.unwrap(), 451);
    assert_eq!(snapshot(&pool).await.len(), 451);
    assert_eq!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.unwrap(), 0);
    AssetsRepo::delete_multi_assets(
        &pool,
        vec![AssetsId::new(
            "owner-0",
            "scroll",
            crate::entities::asset_token_key::AssetTokenKey::from_raw(Some("token")),
        )],
    )
    .await
    .unwrap();
    assert_eq!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.unwrap(), 0);
    assert_eq!(snapshot(&pool).await.iter().find(|r| r.0 == "owner-0").unwrap().4, 0);
}

#[tokio::test]
async fn ordinary_default_assets_failed_page_preserves_prior_pages_and_retry_completes() {
    let pool = fixture().await;
    for i in 0..451 {
        account(&pool, &format!("owner-{i}"), "scroll").await;
    }
    coin(&pool, "scroll", "USDT", "token", 1, 1, 0).await;
    sqlx::query("CREATE TRIGGER fail_later BEFORE INSERT ON assets WHEN NEW.address='owner-200' BEGIN SELECT RAISE(ABORT,'controlled page failure'); END").execute(pool.write_ref()).await.unwrap();
    assert!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.is_err());
    assert_eq!(snapshot(&pool).await.len(), 200);
    sqlx::query("DROP TRIGGER fail_later").execute(pool.write_ref()).await.unwrap();
    assert_eq!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.unwrap(), 251);
    assert_eq!(snapshot(&pool).await.len(), 451);
}

#[tokio::test]
async fn ordinary_default_assets_recheck_hidden_and_disabled_between_selection_and_insert() {
    let pool = fixture().await;
    for addr in ["hidden-later", "disabled-later", "allowed"] {
        account(&pool, addr, "scroll").await;
    }
    coin(&pool, "scroll", "USDT", "token", 1, 1, 0).await;
    let candidates = {
        let mut conn = pool.write_ref().acquire().await.unwrap();
        let upper = AssetsDao::ordinary_default_asset_upper_bounds(&mut conn).await.unwrap();
        AssetsDao::ordinary_default_asset_candidates(&mut conn, (0, 0), upper).await.unwrap()
    };
    assert_eq!(candidates.len(), 3);
    existing(&pool, "hidden-later", "scroll", Some("token"), 0, 0).await;
    sqlx::query("UPDATE account SET status=0 WHERE address='disabled-later'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    let mut conn = pool.write_ref().acquire().await.unwrap();
    assert_eq!(
        AssetsDao::insert_ordinary_default_asset_candidates(&mut conn, &candidates).await.unwrap(),
        1
    );
    drop(conn);
    let rows = snapshot(&pool).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.iter().find(|r| r.0 == "hidden-later").unwrap().4, 0);
    assert!(rows.iter().any(|r| r.0 == "allowed" && r.4 == 1));
}

#[tokio::test]
async fn ordinary_default_assets_same_token_aliases_create_one_row() {
    let pool = fixture().await;
    account(&pool, "owner", "scroll").await;
    coin(&pool, "scroll", "OLD", "token", 1, 1, 0).await;
    coin(&pool, "scroll", "NEW", "token", 1, 1, 0).await;
    assert_eq!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.unwrap(), 1);
    assert_eq!(snapshot(&pool).await.len(), 1);
    assert_eq!(AssetsRepo::reconcile_ordinary_default_assets(&pool).await.unwrap(), 0);
}
