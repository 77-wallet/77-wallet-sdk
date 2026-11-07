use super::catalog_labels_test_support::{fixture, stored};

#[tokio::test]
async fn catalog_labels_view_upgrade_preserves_existing_rows() {
    let pool = fixture("catalog_labels_upgrade").await;
    let before = stored(&pool).await;
    crate::init::Migrator::ApiWallet.migrator().unwrap().run(pool.write_ref()).await.unwrap();
    assert_eq!(stored(&pool).await, before);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM pragma_table_info('api_assets') WHERE name IN ('name','symbol')"
        )
        .fetch_one(pool.read_ref())
        .await
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn catalog_labels_view_partitions_match_existing_evm_uniqueness_scope() {
    let pool = fixture("catalog_labels_routes").await;
    let ddl: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE name='api_assets_with_coin_labels'",
    )
    .fetch_one(pool.read_ref())
    .await
    .unwrap();
    let expected: std::collections::BTreeSet<_> =
        crate::entities::api_withdraw::signed_evm_chain_codes().iter().cloned().collect();
    let partitions: Vec<std::collections::BTreeSet<String>> = ddl
        .split("IN (")
        .skip(1)
        .map(|part| {
            part.split(')')
                .next()
                .unwrap()
                .split(',')
                .map(|s| s.trim().trim_matches('\'').to_string())
                .collect()
        })
        .collect();
    assert_eq!(partitions.len(), 2);
    assert!(partitions.iter().all(|part| part == &expected));
}

#[tokio::test]
async fn catalog_labels_view_scoped_reads_probe_catalog_by_full_identity() {
    let pool = fixture("catalog_labels_plan").await;
    sqlx::raw_sql(
        "WITH RECURSIVE seq(i) AS(VALUES(0) UNION ALL SELECT i+1 FROM seq WHERE i<999)
        INSERT INTO api_assets(decimals,address,token_address,chain_code,status,created_at)
        SELECT 6,'extra-'||i,'0xabc','eth',1,CURRENT_TIMESTAMP FROM seq;ANALYZE;",
    )
    .execute(pool.write_ref())
    .await
    .unwrap();
    let rows:Vec<(i64,i64,i64,String)>=sqlx::query_as("EXPLAIN QUERY PLAN SELECT name,symbol FROM api_assets_with_coin_labels WHERE address=? AND chain_code=? AND token_address=?")
        .bind("owner").bind("eth").bind("0xabc").fetch_all(pool.write_ref()).await.unwrap();
    let plan: Vec<_> = rows.iter().map(|row| row.3.as_str()).collect();
    assert!(
        plan.iter().any(|s| s.contains("SEARCH a") && s.contains("address=?")),
        "asset lookup not scoped: {plan:?}"
    );
    for index in ["api_coin_metadata_nocase_idx", "api_coin_chain_code_token_address_idx"] {
        assert!(
            plan.iter()
                .any(|s| s.contains(index) && s.contains("chain_code=? AND token_address=?")),
            "catalog key not complete: {plan:?}"
        );
    }
}
