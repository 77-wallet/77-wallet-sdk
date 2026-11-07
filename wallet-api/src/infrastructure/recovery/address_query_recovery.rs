use std::{sync::Arc, time::Duration};
use tokio::time::sleep;
use tracing::trace;

use wallet_database::repositories::api_wallet::address_query_state::AddressQueryStateRepo;

use crate::{error::service::ServiceError, infrastructure::recovery::pool::BackgroundTaskPool};

/// 启动地址恢复Worker
/// 1. 等待系统就绪后，启动时扫描一次可恢复任务（Failed + 卡住的 Running）
/// 2. 然后每5秒扫描一次可恢复任务
pub async fn start_address_recover_worker(
    ctx: &'static crate::context::Context,
    background_task_pool: Arc<BackgroundTaskPool>,
) -> Result<(), ServiceError> {
    trace!("启动地址恢复Worker");

    tokio::spawn(async move {
        crate::infrastructure::system_ready::wait_system_ready().await;
        trace!("地址恢复Worker检测到系统就绪，开始扫描可恢复任务");

        if let Err(e) = scan_and_dispatch(ctx, true, background_task_pool.clone()).await {
            tracing::error!("地址恢复Worker启动扫描失败: {:?}", e);
        }

        loop {
            sleep(Duration::from_secs(5)).await;
            if let Err(e) = scan_and_dispatch(ctx, false, background_task_pool.clone()).await {
                tracing::error!("地址恢复Worker扫描失败: {:?}", e);
            }
        }
    });

    Ok(())
}

/// 扫描并分发地址恢复任务
/// - is_startup: true 表示启动扫描；false 表示周期扫描
/// - 两者都只处理可恢复任务（Failed + 卡住超过10分钟的Running）
pub async fn scan_and_dispatch(
    ctx: &'static crate::context::Context,
    is_startup: bool,
    background_task_pool: Arc<BackgroundTaskPool>,
) -> Result<(), ServiceError> {
    trace!(is_startup = is_startup, "开始扫描地址查询状态");

    let pool = ctx.api_wallet_pool()?;

    let query_states = AddressQueryStateRepo::list_recoverable_tasks(&pool, true).await?;

    trace!(is_startup = is_startup, total = query_states.len(), "扫描到待处理的地址查询状态数量");

    // 处理每个状态
    for state in query_states {
        let state_clone = state.clone();

        // 将任务推送到现有的BackgroundTaskPool
        background_task_pool.push(async move {
            trace!(uid = %state_clone.uid, chain_code = %state_clone.chain_code, "开始处理地址恢复任务");
            // 使用ApiAccountDomain的continue_recover方法继续恢复
            let res = crate::domain::api_wallet::account::ApiAccountDomain::continue_recover_with_ctx(ctx, &state_clone).await;
            if let Err(e) = res {
                tracing::error!(uid = %state_clone.uid, chain_code = %state_clone.chain_code, "地址恢复任务失败: {:?}", e);
            } else {
                trace!(uid = %state_clone.uid, chain_code = %state_clone.chain_code, "地址恢复任务完成");
            }
            Ok(())
        }).await;
    }

    Ok(())
}
