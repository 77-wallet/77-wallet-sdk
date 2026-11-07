use crate::repositories::test_helper::setup_api_wallet_pool_before_label_removal as setup_api_wallet_pool;

#[tokio::test]
async fn coin_metadata_access_additive_migration_preserves_existing_rows() {
    let pool = setup_api_wallet_pool("coin_metadata_upgrade").await;
    // Simulate a database immediately before the additive index migration.
    sqlx::raw_sql("DROP VIEW IF EXISTS api_assets_with_coin_labels;
        DELETE FROM _sqlx_migrations WHERE version=20261105010000;
        DROP INDEX IF EXISTS api_assets_metadata_nocase_idx;
        DROP INDEX IF EXISTS api_coin_metadata_nocase_idx;
        DELETE FROM _sqlx_migrations WHERE version=20261105000000;
        INSERT INTO api_assets(name,symbol,decimals,address,token_address,chain_code,status,balance,is_multisig,created_at,updated_at)
        VALUES('legacy','OLD',6,'synthetic','0xAbC','polygon',0,'12.345678',2,'2026-01-01','2026-01-02');")
        .execute(pool.write_ref()).await.unwrap();
    let snapshot = "SELECT name,symbol,decimals,token_address,status,balance,is_multisig,created_at,updated_at FROM api_assets";
    type Row = (String, String, i64, String, i64, String, i64, String, String);
    let before: Vec<Row> = sqlx::query_as(snapshot).fetch_all(pool.read_ref()).await.unwrap();
    let mut migrator = crate::init::Migrator::ApiWallet.migrator().unwrap();
    migrator.migrations = std::borrow::Cow::Owned(
        migrator.iter().filter(|m| m.version < 20261105020000).cloned().collect(),
    );
    migrator.run(pool.write_ref()).await.unwrap();
    let after: Vec<Row> = sqlx::query_as(snapshot).fetch_all(pool.read_ref()).await.unwrap();
    assert_eq!(before, after);
    let indexes:i64=sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN ('api_assets_metadata_nocase_idx','api_coin_metadata_nocase_idx')")
        .fetch_one(pool.read_ref()).await.unwrap();
    assert_eq!(indexes, 2, "existing DB must gain both lookup indexes");
}
