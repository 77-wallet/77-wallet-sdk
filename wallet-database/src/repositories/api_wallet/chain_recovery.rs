//! Missing-chain recovery storage policy. No separate recovery table.
use crate::{
    TaskDbPool,
    entities::task_queue::{TaskName, TaskQueueEntity},
    repositories::task_queue::TaskQueueRepo,
};
pub const OWNER: &str = "api_missing_chain";
pub const MAX_ACTIVE: i64 = 64;
pub const DUE_PAGE_SIZE: u32 = 100;
pub struct ChainRecoveryRepo;
impl ChainRecoveryRepo {
    pub async fn due(
        pool: &TaskDbPool,
        now_ms: i64,
        limit: u32,
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        sqlx::query_as::<_, TaskQueueEntity>("SELECT id,task_name,request_body,type,status,err_msg,remark,created_at,updated_at FROM task_queue WHERE status IN (0,1,3,4) AND CASE WHEN json_valid(remark) THEN json_extract(remark,'$.owner') END = ? AND COALESCE(CASE WHEN json_valid(remark) THEN json_extract(remark,'$.next_retry_at_ms') END,0) <= ? AND COALESCE(CASE WHEN json_valid(remark) THEN json_extract(remark,'$.replaced_by_children') END,0)=0 ORDER BY COALESCE(updated_at,created_at),id LIMIT ?")
            .bind(OWNER).bind(now_ms).bind(i64::from(limit.min(DUE_PAGE_SIZE))).fetch_all(pool.read_ref()).await.map_err(|e|crate::Error::Database(e.into()))
    }
    pub async fn rearm(
        pool: &TaskDbPool,
        id: &str,
        name: TaskName,
        remark: &str,
        status: u8,
    ) -> Result<u64, crate::Error> {
        TaskQueueRepo::rearm_owned_done(pool, id, OWNER, name, remark, status, MAX_ACTIVE).await
    }
    pub async fn split(
        pool: &TaskDbPool,
        id: &str,
        remark: &str,
        children: &[TaskQueueEntity],
    ) -> Result<bool, crate::Error> {
        TaskQueueRepo::replace_owned_with_children(pool, id, OWNER, remark, children, MAX_ACTIVE)
            .await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::task_queue::KnownTaskName;
    async fn setup_task_pool(name: &str) -> TaskDbPool {
        let dir = std::env::temp_dir()
            .join(format!("{name}-{}", wallet_utils::snowflake::get_uid().unwrap()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::SqliteContext::new(dir.to_str().unwrap(), Some("task.db"))
            .await
            .unwrap()
            .into_task_db_pool()
            .unwrap()
    }
    fn owned_row(id: &str, deadline: i64) -> TaskQueueEntity {
        let now = chrono::Utc::now();
        TaskQueueEntity {
            id: id.into(),
            task_name: TaskName::Known(KnownTaskName::BackendApi),
            request_body: "{}".into(),
            r#type: 1,
            status: 0,
            err_msg: None,
            remark: Some(format!(
                r#"{{"owner":"api_missing_chain","attempts":12,"next_retry_at_ms":{deadline}}}"#
            )),
            created_at: now,
            updated_at: Some(now),
        }
    }

    #[tokio::test]
    async fn api_chain_queue_capacity_due_limit_and_deadline_are_sqlite_facts() {
        let pool = setup_task_pool("a_queue_capacity").await;
        let owner = OWNER;
        for i in 0..70 {
            TaskQueueRepo::insert_owned_with_capacity(
                &pool,
                &owned_row(&format!("a-{i:03}"), if i % 2 == 0 { 100 } else { 200 }),
                owner,
                64,
            )
            .await
            .unwrap();
        }
        assert_eq!(TaskQueueRepo::active_for_owner(&pool, owner).await.unwrap(), 64);
        assert_eq!(ChainRecoveryRepo::due(&pool, 99, 100).await.unwrap().len(), 0);
        let first = ChainRecoveryRepo::due(&pool, 100, 7).await.unwrap();
        assert_eq!(first.len(), 7);
        let remark = r#"{"owner":"api_missing_chain","attempts":13,"next_retry_at_ms":300}"#;
        for row in first {
            TaskQueueRepo::defer_owned(&pool, &row.id, owner, remark, "synthetic").await.unwrap();
        }
        let next = ChainRecoveryRepo::due(&pool, 100, 100).await.unwrap();
        assert_eq!(next.len(), 25, "deferred rows yield the next due page");
        assert_eq!(
            ChainRecoveryRepo::due(&pool, 300, 100).await.unwrap().len(),
            64,
            "failed rows remain retryable after ten attempts"
        );
    }

    #[tokio::test]
    async fn api_chain_split_commits_children_and_replacement_atomically_or_rolls_back() {
        let pool = setup_task_pool("a_queue_split").await;
        let owner = OWNER;
        TaskQueueRepo::insert_batch_task_ignore_conflict(&pool, &[owned_row("parent", 0)])
            .await
            .unwrap();
        sqlx::raw_sql("CREATE TRIGGER reject_child BEFORE INSERT ON task_queue WHEN NEW.id='right' BEGIN SELECT RAISE(ABORT,'synthetic child write failure'); END;").execute(pool.write_ref()).await.unwrap();
        let replaced = r#"{"owner":"api_missing_chain","attempts":0,"next_retry_at_ms":0,"replaced_by_children":true}"#;
        let children = [owned_row("left", 0), owned_row("right", 0)];
        assert!(
            TaskQueueRepo::replace_owned_with_children(
                &pool, "parent", owner, replaced, &children, MAX_ACTIVE
            )
            .await
            .is_err()
        );
        assert_eq!(TaskQueueRepo::task_detail(&pool, "parent").await.unwrap().unwrap().status, 0);
        assert!(TaskQueueRepo::task_detail(&pool, "left").await.unwrap().is_none());
        sqlx::query("DROP TRIGGER reject_child").execute(pool.write_ref()).await.unwrap();
        TaskQueueRepo::replace_owned_with_children(
            &pool, "parent", owner, replaced, &children, MAX_ACTIVE,
        )
        .await
        .unwrap();
        let due = ChainRecoveryRepo::due(&pool, 0, 100).await.unwrap();
        assert_eq!(
            due.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            vec!["left", "right"]
        );
        TaskQueueRepo::replace_owned_with_children(
            &pool, "parent", owner, replaced, &children, MAX_ACTIVE,
        )
        .await
        .unwrap();
        assert_eq!(
            TaskQueueRepo::all_tasks_queue(&pool).await.unwrap().len(),
            3,
            "duplicate splitting is idempotent"
        );
        TaskQueueRepo::defer_owned(&pool, "parent", owner, replaced, "late failure").await.unwrap();
        assert_eq!(
            TaskQueueRepo::task_detail(&pool, "parent").await.unwrap().unwrap().status,
            2,
            "late errors cannot undo completion"
        );
    }
    #[tokio::test]
    async fn api_chain_split_at_capacity_does_not_exceed_active_bound() {
        let pool = setup_task_pool("a_split_full").await;
        let owner = OWNER;
        for i in 0..64 {
            TaskQueueRepo::insert_owned_with_capacity(
                &pool,
                &owned_row(&format!("full-{i}"), 0),
                owner,
                64,
            )
            .await
            .unwrap();
        }
        let replaced = r#"{"owner":"api_missing_chain","attempts":0,"next_retry_at_ms":0,"replaced_by_children":true}"#;
        TaskQueueRepo::replace_owned_with_children(
            &pool,
            "full-0",
            owner,
            replaced,
            &[owned_row("new-left", 0), owned_row("new-right", 0)],
            MAX_ACTIVE,
        )
        .await
        .unwrap();
        assert!(
            TaskQueueRepo::active_for_owner(&pool, owner).await.unwrap() <= 64,
            "split must reserve its additional slot atomically"
        );
        assert_eq!(
            TaskQueueRepo::task_detail(&pool, "full-0").await.unwrap().unwrap().status,
            0,
            "capacity defers original rather than losing it"
        );
    }

    #[tokio::test]
    async fn api_chain_rearm_and_failed_claim_obey_capacity_and_parents_wait_durably() {
        let pool = setup_task_pool("a_rearm_full").await;
        let owner = OWNER;
        for i in 0..64 {
            TaskQueueRepo::insert_owned_with_capacity(
                &pool,
                &owned_row(&format!("full-{i}"), 0),
                owner,
                64,
            )
            .await
            .unwrap();
        }
        let mut done = owned_row("completed", 0);
        done.status = 2;
        let mut failed = owned_row("failed", 0);
        failed.status = 3;
        let mut parent = owned_row("parent", 0);
        parent.status = 2;
        parent.task_name = TaskName::Unknown("ReconcileApiChainAccounts".into());
        for row in [&done, &failed, &parent] {
            TaskQueueRepo::insert_batch_task_ignore_conflict(&pool, std::slice::from_ref(row))
                .await
                .unwrap();
        }
        assert_eq!(
            TaskQueueRepo::rearm_owned_done(
                &pool,
                &done.id,
                owner,
                done.task_name.clone(),
                done.remark.as_deref().unwrap(),
                0,
                MAX_ACTIVE
            )
            .await
            .unwrap(),
            0
        );
        assert!(!TaskQueueRepo::claim_owned_running(&pool, "failed", owner, 64).await.unwrap());
        TaskQueueRepo::rearm_owned_done(
            &pool,
            &parent.id,
            owner,
            parent.task_name.clone(),
            parent.remark.as_deref().unwrap(),
            3,
            MAX_ACTIVE,
        )
        .await
        .unwrap();
        assert_eq!(
            TaskQueueRepo::task_detail(&pool, "parent").await.unwrap().unwrap().status,
            3,
            "waiting parent remains durable outside active cap"
        );
        assert_eq!(TaskQueueRepo::active_for_owner(&pool, owner).await.unwrap(), 64);
        TaskQueueRepo::task_done(&pool, "full-0").await.unwrap();
        assert!(TaskQueueRepo::claim_owned_running(&pool, "failed", owner, 64).await.unwrap());
        assert_eq!(TaskQueueRepo::active_for_owner(&pool, owner).await.unwrap(), 64);
        TaskQueueRepo::task_done(&pool, "full-1").await.unwrap();
        assert_eq!(
            TaskQueueRepo::rearm_owned_done(
                &pool,
                &done.id,
                owner,
                done.task_name.clone(),
                done.remark.as_deref().unwrap(),
                0,
                MAX_ACTIVE
            )
            .await
            .unwrap(),
            1
        );
        assert_eq!(TaskQueueRepo::active_for_owner(&pool, owner).await.unwrap(), 64);
    }
}
