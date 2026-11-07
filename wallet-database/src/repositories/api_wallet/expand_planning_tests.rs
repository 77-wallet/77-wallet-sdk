use super::expand_batch::ExpandBatchRepo;
use crate::{
    entities::{address_query_state::AddressQueryStatus, expand_batch::ExpandBatchStatus},
    repositories::{
        api_wallet::address_query_state::AddressQueryStateRepo, test_helper::setup_api_wallet_pool,
    },
};

async fn given(count: i32) -> crate::ApiWalletDbPool {
    let pool = setup_api_wallet_pool("expand_planning").await;
    ExpandBatchRepo::create_batch(&pool, "uid", "batch", "serial", "base", count).await.unwrap();
    pool
}

async fn assert_pending_empty(pool: &crate::ApiWalletDbPool) {
    let row = ExpandBatchRepo::get_batch(pool, "batch").await.unwrap().unwrap();
    assert_eq!(row.status, ExpandBatchStatus::Pending);
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM expand_batch_item WHERE batch_id='batch'")
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn expand_planning_all_chunks_and_claim_rollback_then_retry() {
    let pool = given(151).await;
    sqlx::query("CREATE TRIGGER reject_second_chunk BEFORE INSERT ON expand_batch_item WHEN NEW.input_index=150 BEGIN SELECT RAISE(FAIL,'second chunk failed'); END")
        .execute(pool.write_ref()).await.unwrap();
    let result: Result<usize, crate::Error> =
        ExpandBatchRepo::plan_items(&pool, "batch", |_| async { Ok((0..151).collect()) }).await;
    assert!(result.is_err());
    assert_pending_empty(&pool).await;
    sqlx::query("DROP TRIGGER reject_second_chunk").execute(pool.write_ref()).await.unwrap();
    let created: Result<usize, crate::Error> =
        ExpandBatchRepo::plan_items(&pool, "batch", |_| async { Ok((0..151).collect()) }).await;
    assert_eq!(created.unwrap(), 151);
    let repeated: Result<usize, crate::Error> =
        ExpandBatchRepo::plan_items(&pool, "batch", |_| async {
            panic!("frozen batch must not allocate again")
        })
        .await;
    assert_eq!(repeated.unwrap(), 0);
}

#[tokio::test]
async fn expand_planning_cancellation_rolls_back_running_claim() {
    let pool = given(2).await;
    let cloned = pool.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let result: Result<usize, crate::Error> =
            ExpandBatchRepo::plan_items(&cloned, "batch", |_| async move {
                tx.send(()).unwrap();
                std::future::pending().await
            })
            .await;
        result
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), rx).await.unwrap().unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    // A writer query waits for SQLx's queued rollback before inspecting facts.
    let status: i32 = sqlx::query_scalar("SELECT status FROM expand_batch WHERE batch_id='batch'")
        .fetch_one(pool.write_ref())
        .await
        .unwrap();
    assert_eq!(status, ExpandBatchStatus::Pending as i32);
    assert_pending_empty(&pool).await;
}

#[tokio::test]
async fn expand_planning_write_claim_rechecks_query_gate_and_resumes_empty_running_batch() {
    let pool = given(2).await;
    assert!(ExpandBatchRepo::mark_running_if_pending(&pool, "batch").await.unwrap());
    AddressQueryStateRepo::upsert(
        &pool,
        AddressQueryStateRepo::build_create_state("uid", "base", AddressQueryStatus::Running),
    )
    .await
    .unwrap();
    let blocked: Result<usize, crate::Error> =
        ExpandBatchRepo::plan_items(&pool, "batch", |_| async { panic!("query still running") })
            .await;
    assert_eq!(blocked.unwrap(), 0);
    AddressQueryStateRepo::update_status(&pool, "uid", "base", AddressQueryStatus::Done)
        .await
        .unwrap();
    let resumed: Result<usize, crate::Error> =
        ExpandBatchRepo::plan_items(&pool, "batch", |_| async { Ok(vec![0, 1]) }).await;
    assert_eq!(resumed.unwrap(), 2);
    let candidates = ExpandBatchRepo::get_batches_for_planning(&pool).await.unwrap();
    assert!(candidates.is_empty(), "nonempty frozen batches are never reselected");
}
