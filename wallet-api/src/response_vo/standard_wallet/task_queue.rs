use serde::Serialize;
use wallet_database::entities::{
    address_query_state::AddressQueryStateEntity, expand_batch::ExpandBatchEntity,
    expand_batch_item::ExpandBatchItemEntity, task_queue::TaskQueueEntity,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryProgress {
    pub uid: String,
    pub chain_code: String,
    pub local_count: usize,
    pub total_remote: usize,
    pub percent: f32,
    pub done: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQueueStatus {
    pub all_tasks: usize,
    pub running_tasks: usize,
    pub pending_tasks: usize,
    pub failed_tasks: usize,
    pub done_tasks: usize,
    pub bill_count: i64,
    pub failed_tasks_list: Vec<TaskQueueEntity>,
    pub expand_batches: Vec<ExpandBatchEntity>,

    // 各状态扩容项数量统计
    pub creating_items_count: usize,
    pub initing_items_count: usize,
    pub done_items_count: usize,
    pub failed_items_count: usize,
    pub expand_batch_items: Vec<ExpandBatchItemEntity>,
    pub address_query_states: Vec<AddressQueryStateEntity>,

    // 地址恢复进度统计
    pub address_recovery_progress: Vec<RecoveryProgress>,
    pub overall_percent: f32,
}
