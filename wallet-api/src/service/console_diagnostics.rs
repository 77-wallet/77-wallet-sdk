use wallet_database::{
    entities::api_trade_type::ApiTradeType,
    repositories::api_wallet::{collect::ApiCollectRepo, withdraw::ApiWithdrawRepo},
};

use crate::{
    context::Context,
    response_vo::standard_wallet::diagnostics::{
        ConsoleFailedTaskSummary, ConsoleTaskQueueSummary, TradeDiagnosticRecord, TradeDiagnostics,
    },
    service::task_queue::TaskQueueService,
};

pub struct ConsoleDiagnosticsService {
    ctx: &'static Context,
}

impl ConsoleDiagnosticsService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn task_queue_summary(
        &self,
    ) -> Result<ConsoleTaskQueueSummary, crate::error::service::ServiceError> {
        let status = TaskQueueService::new(self.ctx).get_task_queue_status().await?;
        let failed_tasks = status
            .failed_tasks_list
            .into_iter()
            .map(|task| ConsoleFailedTaskSummary {
                id: task.id,
                task_name: format!("{:?}", task.task_name),
                status: task.status,
                err_msg: task.err_msg,
                remark: task.remark,
            })
            .collect();

        Ok(ConsoleTaskQueueSummary {
            all_tasks: status.all_tasks,
            running_tasks: status.running_tasks,
            pending_tasks: status.pending_tasks,
            failed_task_count: status.failed_tasks,
            done_tasks: status.done_tasks,
            bill_count: status.bill_count,
            expand_batches: status.expand_batches.len(),
            unfinished_expand_items: status.expand_batch_items.len(),
            address_query_states: status.address_query_states.len(),
            overall_percent: status.overall_percent,
            failed_tasks,
        })
    }

    pub async fn diagnose_trade(
        &self,
        trade_no: &str,
    ) -> Result<TradeDiagnostics, crate::error::service::ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        let mut records = Vec::new();

        if let Some(withdraw) =
            ApiWithdrawRepo::find_api_withdraw_by_trade_no(&pool, trade_no, ApiTradeType::Withdraw)
                .await?
        {
            records.push(TradeDiagnosticRecord {
                flow: "withdraw".to_string(),
                status: format!("{:?}", withdraw.status),
                chain_code: withdraw.chain_code,
                symbol: withdraw.symbol,
                value: withdraw.value,
                from_addr: withdraw.from_addr,
                to_addr: withdraw.to_addr,
                tx_hash: withdraw.tx_hash,
                err_msg: withdraw.err_msg,
                notes: withdraw.notes,
            });
        }

        if let Some(collect) = ApiCollectRepo::find_api_collect_by_trade_no(&pool, trade_no).await?
        {
            records.push(TradeDiagnosticRecord {
                flow: "collect".to_string(),
                status: format!("{:?}", collect.status),
                chain_code: collect.chain_code,
                symbol: collect.symbol,
                value: collect.value,
                from_addr: collect.from_addr,
                to_addr: collect.to_addr,
                tx_hash: collect.tx_hash,
                err_msg: collect.err_msg,
                notes: collect.notes,
            });
        }

        Ok(TradeDiagnostics { trade_no: trade_no.to_string(), records })
    }
}
