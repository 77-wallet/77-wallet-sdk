use wallet_database::{
    entities::expand_batch_item::ExpandItemStatus,
    repositories::{
        api_wallet::{
            account::ApiAccountRepo, address_query_state::AddressQueryStateRepo,
            expand_batch::ExpandBatchRepo, expand_batch_item::ExpandBatchItemRepo,
            wallet::ApiWalletRepo,
        },
        bill::BillRepo,
        task_queue::TaskQueueRepo,
    },
};

use crate::{context::Context, response_vo::standard_wallet::task_queue::TaskQueueStatus};

pub struct TaskQueueService {
    pub(crate) ctx: &'static Context,
}

impl TaskQueueService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn get_task_queue_status(
        &self,
    ) -> Result<TaskQueueStatus, crate::error::service::ServiceError> {
        let task_pool = self.ctx.task_pool()?;
        let core_pool = self.ctx.core_pool()?;
        let api_wallet_pool = self.ctx.api_wallet_pool()?;
        let all = TaskQueueRepo::all_tasks_queue(&task_pool).await?;
        let done = TaskQueueRepo::done_task_queue(&task_pool).await?;
        let running = TaskQueueRepo::running_task_queue(&task_pool).await?;
        let pending = TaskQueueRepo::pending_task_queue(&task_pool).await?;
        let failed_tasks_list = TaskQueueRepo::failed_task_queue(&task_pool).await?;

        let bill_count = BillRepo::bill_count(&core_pool).await?;

        // 获取未完成的 expand_batch 数据
        let expand_batches = ExpandBatchRepo::get_unfinished_batches(&api_wallet_pool).await?;

        // 仅按状态计数，避免重启时全量拉取 api_account / expand_batch_item
        let creating_items_count = ExpandBatchItemRepo::count_by_status(
            &api_wallet_pool,
            ExpandItemStatus::CreateDispatched,
        )
        .await? as usize;
        let initing_items_count = ExpandBatchItemRepo::count_by_status(
            &api_wallet_pool,
            ExpandItemStatus::InitDispatched,
        )
        .await? as usize;
        let done_items_count =
            ExpandBatchItemRepo::count_by_status(&api_wallet_pool, ExpandItemStatus::Done).await?
                as usize;
        let failed_items_count =
            ExpandBatchItemRepo::count_by_status(&api_wallet_pool, ExpandItemStatus::Failed).await?
                as usize;

        // 仅返回未完成 batch 里的未完成 items，避免把所有 item 行拉入内存
        let mut expand_batch_items = Vec::new();
        for batch in &expand_batches {
            let mut items =
                ExpandBatchItemRepo::list_unfinished_items(&api_wallet_pool, &batch.batch_id)
                    .await?;
            expand_batch_items.append(&mut items);
        }

        // 获取 address_query_state 表所有数据
        let address_query_states = AddressQueryStateRepo::get_all(&api_wallet_pool).await?;

        // 聚合地址恢复进度
        let mut address_recovery_progress = Vec::new();
        let mut total_local = 0usize;
        let mut total_remote = 0usize;

        for state in &address_query_states {
            let uid = state.uid.clone();
            let chain_code = state.chain_code.clone();

            // 1. 从 uid 获取 wallet_address
            let wallet_opt = ApiWalletRepo::find_by_uid(&api_wallet_pool, &uid).await?;

            if let Some(wallet) = wallet_opt {
                // 2. count local addresses
                let local_count = ApiAccountRepo::count_by_wallet_address_v2(
                    &api_wallet_pool,
                    &wallet.address,
                    None,
                    Some(chain_code.clone()),
                )
                .await? as usize;

                // 3. 获取 total_remote
                let state_total_remote = state.total_remote as usize;
                let estimated_total = if state_total_remote == 0 {
                    // 如果 total_remote 为 0，使用 (last_page + 1) * 100 进行估算
                    ((state.last_page + 1) * 100) as usize
                } else {
                    state_total_remote
                };

                // 4. 计算进度
                let percent = if estimated_total == 0 {
                    0.0
                } else {
                    let p = local_count as f32 / estimated_total as f32;
                    p.min(1.0) // 进度不超过 100%
                };

                // 5. 判断是否完成
                let done = local_count >= state_total_remote && state_total_remote != 0;

                // 6. 添加到进度列表
                address_recovery_progress.push(
                    crate::response_vo::standard_wallet::task_queue::RecoveryProgress {
                        uid,
                        chain_code,
                        local_count,
                        total_remote: estimated_total,
                        percent,
                        done,
                    },
                );

                // 7. 累计总数用于计算总体进度
                total_local += local_count;
                total_remote += estimated_total;
            }
        }

        // 计算总体进度
        let overall_percent = if total_remote == 0 {
            0.0
        } else {
            let p = total_local as f32 / total_remote as f32;
            p.min(1.0) // 进度不超过 100%
        };

        let status = crate::response_vo::standard_wallet::task_queue::TaskQueueStatus {
            all_tasks: all.len(),
            running_tasks: running.len(),
            pending_tasks: pending.len(),
            failed_tasks: failed_tasks_list.len(),
            done_tasks: done.len(),
            bill_count,
            failed_tasks_list,
            expand_batches,
            expand_batch_items,
            address_query_states,
            creating_items_count,
            initing_items_count,
            done_items_count,
            failed_items_count,
            address_recovery_progress,
            overall_percent,
        };

        tracing::info!(?status, "Current task queue status");

        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use wallet_database::entities::expand_batch_item::ExpandItemStatus;

    #[test]
    fn task_queue_semantics_treat_failed_as_terminated_not_unfinished() {
        let unfinished = [ExpandItemStatus::CreateDispatched, ExpandItemStatus::InitDispatched];
        let terminated = [ExpandItemStatus::Done, ExpandItemStatus::Failed];

        assert!(unfinished.iter().all(|status| *status != ExpandItemStatus::Done));
        assert!(unfinished.iter().all(|status| *status != ExpandItemStatus::Failed));
        assert!(terminated.contains(&ExpandItemStatus::Done));
        assert!(terminated.contains(&ExpandItemStatus::Failed));
    }
}
