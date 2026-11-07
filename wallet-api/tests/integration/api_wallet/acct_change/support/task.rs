use anyhow::Result;
use wallet_api::{Context, testkit::mqtt::task_pool};
use wallet_database::repositories::task_queue::TaskQueueRepo;

pub(crate) async fn wait_task_done(ctx: &'static Context, msg_id: &str) -> Result<u8> {
    let task_pool = task_pool(ctx)?;
    for _ in 0..80 {
        if let Some(task) = TaskQueueRepo::task_detail(&task_pool, msg_id).await? {
            if task.status == 2 || task.status == 3 {
                return Ok(task.status);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    anyhow::bail!("timeout waiting task status, msg_id={}", msg_id);
}
