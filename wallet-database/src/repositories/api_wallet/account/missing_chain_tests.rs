use super::ApiAccountRepo;
use crate::{
    entities::{api_account::CreateApiAccountVo, api_wallet::ApiWalletType},
    repositories::test_helper::setup_api_wallet_pool,
};

fn account(id: u32, chain: &str, kind: ApiWalletType, suffix: &str) -> CreateApiAccountVo {
    CreateApiAccountVo::new(
        id,
        &format!("address-{id}-{chain}-{kind:?}-{suffix}"),
        "public",
        "wallet",
        "uid",
        "path",
        0,
        chain,
        "name",
        kind,
    )
}

#[tokio::test]
async fn missing_chain_pages_are_typed_deduplicated_and_stable_during_inserts() {
    let pool = setup_api_wallet_pool("missing_chain_cursor").await;
    let mut rows = vec![];
    for id in 0..20_000 {
        for kind in [ApiWalletType::SubAccount, ApiWalletType::Withdrawal] {
            rows.push(account(id, "eth", kind, "source"));
            rows.push(account(id, "bnb", kind, "duplicate"));
        }
    }
    ApiAccountRepo::upsert_account_multi(&pool, rows).await.unwrap();
    let cutoff = ApiAccountRepo::account_high_watermark(&pool).await.unwrap();
    let mut initialized = account(0, "tron", ApiWalletType::SubAccount, "initialized");
    initialized.is_init = 1;
    ApiAccountRepo::upsert_account_multi(&pool, vec![initialized]).await.unwrap();
    sqlx::query("UPDATE api_account SET status=0 WHERE account_id=1 AND api_wallet_type=2")
        .execute(pool.write_ref())
        .await
        .unwrap();
    let first =
        ApiAccountRepo::missing_chain_account_page(&pool, "tron", 0, cutoff, 37).await.unwrap();
    assert_eq!(first.len(), 37);
    assert_eq!(first[0].api_wallet_type, ApiWalletType::Withdrawal);
    assert_eq!(first[0].account_id, 0);
    ApiAccountRepo::upsert_account_multi(
        &pool,
        vec![
            account(20_000, "eth", ApiWalletType::SubAccount, "late"),
            account(2, "tron", ApiWalletType::SubAccount, "derived"),
        ],
    )
    .await
    .unwrap();
    let pending = ApiAccountRepo::target_uninitialized_for_logical(
        &pool,
        "wallet",
        2,
        ApiWalletType::SubAccount,
        "tron",
        1,
    )
    .await
    .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].chain_code, "tron");
    assert_eq!(pending[0].is_init, 0);
    let mut seen = std::collections::HashSet::new();
    let mut page = first;
    loop {
        for row in &page {
            assert!(row.id <= cutoff);
            assert!(seen.insert((row.account_id, row.api_wallet_type as u8)));
        }
        let after = page.last().unwrap().id;
        page = ApiAccountRepo::missing_chain_account_page(&pool, "tron", after, cutoff, 257)
            .await
            .unwrap();
        assert!(page.len() <= 257);
        if page.is_empty() {
            break;
        }
    }
    assert_eq!(seen.len(), 39_998);
    assert!(!seen.contains(&(20_000, 1)));
    let replay =
        ApiAccountRepo::missing_chain_account_page(&pool, "tron", 0, cutoff, 1).await.unwrap();
    assert_eq!(replay[0].account_id, 0);
    assert_eq!(replay[0].api_wallet_type, ApiWalletType::Withdrawal);
    assert!(
        ApiAccountRepo::missing_chain_account_page(&pool, "tron", 0, cutoff, 0)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn missing_chain_index_migrates_existing_facts_and_reopen_recovers_pending() {
    use crate::repositories::test_helper::make_temp_dir;
    let directory = make_temp_dir("missing_chain_reopen");
    let pool = crate::SqliteContext::new(&directory, Some("api_wallet.db"))
        .await
        .unwrap()
        .into_api_wallet_db_pool()
        .unwrap();
    ApiAccountRepo::upsert_account_multi(
        &pool,
        vec![
            account(7, "eth", ApiWalletType::Withdrawal, "existing"),
            account(7, "tron", ApiWalletType::Withdrawal, "pending"),
        ],
    )
    .await
    .unwrap();
    sqlx::query("DROP INDEX api_account_logical_status_id_idx")
        .execute(pool.write_ref())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../schema/api_wallet/migrations/20261005000000_api_account_logical_cursor.sql"
    ))
    .execute(pool.write_ref())
    .await
    .unwrap();
    let columns: Vec<(i64, i64, String)> = {
        let mut connection = pool.read_ref().acquire().await.unwrap();
        // A read pool opened before migration can have a stale schema for PRAGMA.
        // A normal table query refreshes it; inspect that same read connection.
        sqlx::query("SELECT id FROM api_account LIMIT 1")
            .fetch_optional(&mut *connection)
            .await
            .unwrap();
        sqlx::query_as("PRAGMA index_info(api_account_logical_status_id_idx)")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
    };
    assert_eq!(
        columns.iter().map(|column| column.2.as_str()).collect::<Vec<_>>(),
        ["wallet_address", "account_id", "api_wallet_type", "status"]
    );
    // Reapplying the unpublished DDL is a no-op for an already four-column index.
    sqlx::raw_sql(include_str!(
        "../../../../schema/api_wallet/migrations/20261005000000_api_account_logical_cursor.sql"
    ))
    .execute(pool.write_ref())
    .await
    .unwrap();
    let plan: Vec<(i64, i64, i64, String)> = sqlx::query_as("EXPLAIN QUERY PLAN SELECT 1 FROM api_account WHERE wallet_address=? AND account_id=? AND api_wallet_type=? AND status=1 AND id<?")
        .bind("wallet").bind(7).bind(2).bind(i64::MAX).fetch_all(pool.read_ref()).await.unwrap();
    assert!(
        plan.iter()
            .any(|row| row.3.contains("api_account_logical_status_id_idx")
                && row.3.contains("rowid<?")),
        "{plan:?}"
    );
    let cutoff = ApiAccountRepo::account_high_watermark(&pool).await.unwrap();
    let mut transaction = pool.write_ref().begin().await.unwrap();
    sqlx::query("UPDATE api_account SET is_init=1 WHERE chain_code='tron'")
        .execute(transaction.as_mut())
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    drop(pool);
    let reopened = crate::SqliteContext::new(&directory, Some("api_wallet.db"))
        .await
        .unwrap()
        .into_api_wallet_db_pool()
        .unwrap();
    let recovered =
        ApiAccountRepo::missing_chain_account_page(&reopened, "tron", 0, cutoff, 1).await.unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].account_id, 7);
    let pending = ApiAccountRepo::target_uninitialized_for_logical(
        &reopened,
        "wallet",
        7,
        ApiWalletType::Withdrawal,
        "tron",
        8,
    )
    .await
    .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].is_init, 0);
    sqlx::query("UPDATE api_account SET is_init=1 WHERE chain_code='tron'")
        .execute(reopened.write_ref())
        .await
        .unwrap();
    assert!(
        ApiAccountRepo::missing_chain_account_page(&reopened, "tron", 0, cutoff, 1)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        ApiAccountRepo::target_uninitialized_for_logical(
            &reopened,
            "wallet",
            7,
            ApiWalletType::Withdrawal,
            "tron",
            8
        )
        .await
        .unwrap()
        .is_empty()
    );
}

#[tokio::test]
async fn missing_chain_variants_retry_partial_btc_and_preserve_disabled_addresses() {
    let pool = setup_api_wallet_pool("missing_chain_variants").await;
    let mut facts = vec![];
    for id in 1..=3 {
        for kind in [ApiWalletType::SubAccount, ApiWalletType::Withdrawal] {
            facts.push(account(id, "eth", kind, "source"));
            let mut legacy = account(id, "btc", kind, "legacy");
            legacy.address_type = "legacy".into();
            legacy.is_init = 1;
            facts.push(legacy);
            if id != 2 || kind == ApiWalletType::Withdrawal {
                let mut segwit = account(id, "btc", kind, "segwit");
                segwit.address_type = "segwit".into();
                segwit.is_init = if kind == ApiWalletType::Withdrawal { 1 } else { 0 };
                facts.push(segwit);
            }
        }
    }
    ApiAccountRepo::upsert_account_multi(&pool, facts).await.unwrap();
    sqlx::query("UPDATE api_account SET status=0 WHERE account_id=3 AND api_wallet_type=1 AND chain_code='btc' AND address_type='segwit'")
        .execute(pool.write_ref()).await.unwrap();
    let cutoff = ApiAccountRepo::account_high_watermark(&pool).await.unwrap();
    let page =
        ApiAccountRepo::missing_chain_account_page_for_variants(&pool, "btc", 0, cutoff, 10, 2)
            .await
            .unwrap();
    assert_eq!(
        page.iter().map(|row| (row.account_id, row.api_wallet_type as u8)).collect::<Vec<_>>(),
        vec![(1, 1), (2, 1)]
    );
    let pending = ApiAccountRepo::target_uninitialized_for_logical(
        &pool,
        "wallet",
        1,
        ApiWalletType::SubAccount,
        "btc",
        3,
    )
    .await
    .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].address_type, "segwit");
    assert!(
        ApiAccountRepo::target_uninitialized_for_logical(
            &pool,
            "wallet",
            3,
            ApiWalletType::SubAccount,
            "btc",
            3
        )
        .await
        .unwrap()
        .is_empty()
    );
    sqlx::query("UPDATE api_account SET is_init=1 WHERE account_id=1 AND chain_code='btc'")
        .execute(pool.write_ref())
        .await
        .unwrap();
    let remaining =
        ApiAccountRepo::missing_chain_account_page_for_variants(&pool, "btc", 0, cutoff, 10, 2)
            .await
            .unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].account_id, 2);
    let disabled: (i32, i32) = sqlx::query_as("SELECT status,is_init FROM api_account WHERE account_id=3 AND api_wallet_type=1 AND chain_code='btc' AND address_type='segwit'").fetch_one(pool.read_ref()).await.unwrap();
    assert_eq!(disabled, (0, 0));
}

#[tokio::test]
async fn existing_five_column_index_is_not_silently_rewritten_or_checksum_bypassed() {
    use sqlx::migrate::{MigrateError, Migration, MigrationType, Migrator};
    use std::borrow::Cow;

    const VERSION: i64 = 20261005000000;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../schema/api_wallet/migrations/20250723082607_api_account.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let four = include_str!(
        "../../../../schema/api_wallet/migrations/20261005000000_api_account_logical_cursor.sql"
    );
    let five = four.replace("status);", "status, id);");
    assert_ne!(four, five);
    let migration = |sql: String| {
        Migration::new(
            VERSION,
            Cow::Borrowed("api account logical cursor"),
            MigrationType::Simple,
            Cow::Owned(sql),
            false,
        )
    };
    let old = Migrator { migrations: Cow::Owned(vec![migration(five)]), ..Migrator::DEFAULT };
    old.run(&pool).await.unwrap();
    let original_checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = ?")
            .bind(VERSION)
            .fetch_one(&pool)
            .await
            .unwrap();
    let changed =
        Migrator { migrations: Cow::Owned(vec![migration(four.to_owned())]), ..Migrator::DEFAULT };
    assert!(matches!(changed.run(&pool).await, Err(MigrateError::VersionMismatch(VERSION))));
    // IF NOT EXISTS also leaves an existing same-name five-column index intact.
    sqlx::raw_sql(four).execute(&pool).await.unwrap();
    let columns: Vec<(i64, i64, String)> =
        sqlx::query_as("PRAGMA index_info(api_account_logical_status_id_idx)")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        columns.iter().map(|column| column.2.as_str()).collect::<Vec<_>>(),
        ["wallet_address", "account_id", "api_wallet_type", "status", "id"]
    );
    let checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = ?")
            .bind(VERSION)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(checksum, original_checksum);
    crate::dao::api_account::ApiAccountDao::upsert_multi(
        &mut *pool.acquire().await.unwrap(),
        vec![
            account(7, "eth", ApiWalletType::Withdrawal, "old"),
            account(7, "bnb", ApiWalletType::Withdrawal, "duplicate"),
        ],
    )
    .await
    .unwrap();
    let page = crate::dao::api_account::ApiAccountDao::missing_chain_account_page(
        &pool,
        "tron",
        0,
        i64::MAX,
        10,
    )
    .await
    .unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].account_id, 7);
    assert_eq!(page[0].api_wallet_type, ApiWalletType::Withdrawal);
}
