use super::ApiAccountDao;
use crate::{ApiWalletDbPool, repositories::test_helper::setup_api_wallet_pool};
use sqlx::Connection as _;

async fn seed_accounts(pool: &ApiWalletDbPool, count: i64) {
    sqlx::query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<?) INSERT INTO api_account(account_id,name,address,wallet_address,uid,chain_code,api_wallet_type,status,is_init,created_at) SELECT x,'synthetic','selected-'||x,'selected','selected','base',1,x%2,1,'2026-11-03T00:00:00Z' FROM n")
        .bind(count).execute(pool.write_ref()).await.unwrap();
}

#[tokio::test]
async fn deleting_last_wallet_avoids_per_row_index_rewrites() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let pool = setup_api_wallet_pool("delete_all_account_work").await;
    seed_accounts(&pool, 1000).await;
    let mut conn = pool.write_ref().acquire().await.unwrap();
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let steps = Arc::new(AtomicUsize::new(0));
    let measured = steps.clone();
    tx.as_mut().lock_handle().await.unwrap().set_progress_handler(1, move || {
        measured.fetch_add(1, Ordering::Relaxed);
        true
    });
    let count =
        ApiAccountDao::delete_for_wallets(tx.as_mut(), &["selected", "paired"]).await.unwrap();
    tx.as_mut().lock_handle().await.unwrap().remove_progress_handler();
    assert_eq!(count, 1000);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM api_account")
            .fetch_one(tx.as_mut())
            .await
            .unwrap(),
        0
    );
    let steps = steps.load(Ordering::Relaxed);
    println!("last-wallet-delete rows=1000 sqlite_vm_steps={steps}");
    assert!(
        steps < 12000,
        "last-wallet deletion must avoid rewriting every index row: {steps} VM steps"
    );
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn deletion_preserves_inactive_and_orphan_accounts_outside_scope() {
    let pool = setup_api_wallet_pool("delete_account_scope").await;
    seed_accounts(&pool, 10).await;
    sqlx::query("INSERT INTO api_account(account_id,name,address,wallet_address,uid,chain_code,api_wallet_type,status,is_init,created_at) VALUES(1,'other','other-1','other','other','base',1,1,1,'2026-11-03'),(1,'orphan','orphan-1','orphan','orphan','base',1,0,1,'2026-11-03')")
        .execute(pool.write_ref()).await.unwrap();
    let before: Vec<(i64,String,String,i32)> = sqlx::query_as("SELECT id,address,wallet_address,status FROM api_account WHERE wallet_address!='selected' ORDER BY id").fetch_all(pool.read_ref()).await.unwrap();
    let mut tx = pool.write_ref().begin_with("BEGIN IMMEDIATE").await.unwrap();
    assert_eq!(
        ApiAccountDao::delete_for_wallets(tx.as_mut(), &["selected", "paired"]).await.unwrap(),
        10
    );
    tx.commit().await.unwrap();
    let after: Vec<(i64, String, String, i32)> =
        sqlx::query_as("SELECT id,address,wallet_address,status FROM api_account ORDER BY id")
            .fetch_all(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(after, before);
}

#[tokio::test]
async fn all_target_deletion_rolls_back_and_can_be_retried() {
    let pool = setup_api_wallet_pool("delete_all_account_rollback").await;
    seed_accounts(&pool, 10).await;
    let mut tx = pool.write_ref().begin_with("BEGIN IMMEDIATE").await.unwrap();
    assert_eq!(ApiAccountDao::delete_for_wallets(tx.as_mut(), &["selected"]).await.unwrap(), 10);
    tx.rollback().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM api_account")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        10
    );
    let mut tx = pool.write_ref().begin_with("BEGIN IMMEDIATE").await.unwrap();
    assert_eq!(ApiAccountDao::delete_for_wallets(tx.as_mut(), &["selected"]).await.unwrap(), 10);
    tx.commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM api_account")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn empty_deletion_scope_keeps_all_accounts() {
    let pool = setup_api_wallet_pool("delete_account_empty_scope").await;
    seed_accounts(&pool, 10).await;
    let mut tx = pool.write_ref().begin_with("BEGIN IMMEDIATE").await.unwrap();
    assert_eq!(ApiAccountDao::delete_for_wallets(tx.as_mut(), &[]).await.unwrap(), 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM api_account")
            .fetch_one(tx.as_mut())
            .await
            .unwrap(),
        10
    );
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn deletion_retains_sqlite_delete_trigger_semantics() {
    let pool = setup_api_wallet_pool("delete_account_trigger").await;
    seed_accounts(&pool, 10).await;
    sqlx::raw_sql("CREATE TABLE delete_audit(id INTEGER); CREATE TRIGGER audit_accounts AFTER DELETE ON api_account BEGIN INSERT INTO delete_audit VALUES(OLD.id); END;").execute(pool.write_ref()).await.unwrap();
    let mut tx = pool.write_ref().begin_with("BEGIN IMMEDIATE").await.unwrap();
    assert_eq!(ApiAccountDao::delete_for_wallets(tx.as_mut(), &["selected"]).await.unwrap(), 10);
    tx.commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM delete_audit")
            .fetch_one(pool.read_ref())
            .await
            .unwrap(),
        10
    );
}
