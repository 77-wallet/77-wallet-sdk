use super::ApiAssetsDao;
use sqlx::Connection;

#[tokio::test]
async fn api_asset_visibility_summary_excludes_hidden_assets_in_all_filters() {
    // Arrange: hidden Base 10 and active Scroll 4 for the same account/token.
    let mut db = sqlx::SqliteConnection::connect("sqlite::memory:").await.unwrap();
    for migration in crate::init::Migrator::ApiWallet.migrator().unwrap().iter() {
        sqlx::raw_sql(&migration.sql).execute(&mut db).await.unwrap();
    }
    for (chain, balance, status) in [("base", "10", 0), ("scroll", "4", 1)] {
        sqlx::query("INSERT INTO api_chain(name,chain_code,protocols,main_symbol,status,created_at) VALUES (?,?, '[]','ETH',1,CURRENT_TIMESTAMP)").bind(chain).bind(chain).execute(&mut db).await.unwrap();
        sqlx::query("INSERT INTO api_account(account_id,name,address,wallet_address,chain_code,api_wallet_type,status,is_init,created_at) VALUES(3,'fixture','owner','wallet',?,1,1,1,CURRENT_TIMESTAMP)").bind(chain).execute(&mut db).await.unwrap();
        sqlx::query("INSERT INTO api_coin(name,symbol,chain_code,token_address,price,decimals,is_default,status,created_at) VALUES ('USDT','USDT',?,'0xabc','1',6,1,1,CURRENT_TIMESTAMP)").bind(chain).execute(&mut db).await.unwrap();
        sqlx::query("INSERT INTO api_assets(decimals,address,chain_code,token_address,status,balance,created_at) VALUES (6,'owner',?,'0xabc',?,?,CURRENT_TIMESTAMP)").bind(chain).bind(status).bind(balance).execute(&mut db).await.unwrap();
    }
    // Act / Assert: both display modes and optional chain scoping share visibility.
    for hide_zero in [false, true] {
        let rows =
            ApiAssetsDao::get_api_wallet_assets_v2(&mut db, "wallet", Some(3), None, hide_zero)
                .await
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total_coins_quantity, 4.0);
        assert_eq!(rows[0].total_account_amount, Some(4.0));
        let base = ApiAssetsDao::get_api_wallet_assets_v2(
            &mut db,
            "wallet",
            Some(3),
            Some("base"),
            hide_zero,
        )
        .await
        .unwrap();
        assert!(base.is_empty());
    }
    // Query is read-only: a hidden balance is neither zeroed nor deleted.
    let stored: (i64, String) =
        sqlx::query_as("SELECT status,balance FROM api_assets WHERE chain_code='base'")
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert_eq!(stored, (0, "10".into()));
}
