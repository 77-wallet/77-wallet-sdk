use std::{sync::Arc, time::Duration};

use tokio::time::sleep;
use wallet_database::repositories::api_wallet::{
    api_asset_sync_state::ApiAssetSyncStateRepo, assets::ApiAssetsRepo,
};

use crate::{
    config::runtime_defaults, domain::api_wallet::assets::ApiAssetsDomain,
    error::service::ServiceError, infrastructure::recovery::pool::BackgroundTaskPool,
};

pub async fn start_api_asset_sync_worker(
    ctx: &'static crate::context::Context,
    background_task_pool: Arc<BackgroundTaskPool>,
) -> Result<(), ServiceError> {
    tracing::info!("启动 API 钱包资产同步恢复Worker");

    tokio::spawn(async move {
        crate::infrastructure::system_ready::wait_system_ready().await;
        tracing::info!("API 钱包资产同步恢复Worker检测到系统就绪，开始扫描可恢复任务");

        // Refresh rule authority before seeding historical reminders. Subsequent
        // binding refreshes are event driven; MQTT changes use durable tasks.
        if crate::service::api_wallet::app_warning_config::AppWarningConfigService::new(ctx)
            .sync()
            .await
            .is_err()
        {
            tracing::warn!(
                "initial warning configuration sync failed; keep persisted configuration"
            );
        }
        tokio::spawn(async move {
            loop {
                ctx.app_warning_config_refresh.notified().await;
                if crate::service::api_wallet::app_warning_config::AppWarningConfigService::new(ctx)
                    .sync()
                    .await
                    .is_err()
                {
                    tracing::warn!(
                        "warning configuration sync failed; retry on next internal trigger"
                    );
                }
            }
        });

        // One directed reminder seed after the existing readiness gate, not a new poller.
        if let Err(e) =
            crate::application::api_wallet_balance_check::seed_ready_unresolved(ctx).await
        {
            tracing::warn!(%e, "startup reminder-check seed failed; durable reminders remain for next startup");
        }

        if let Err(e) = scan_and_dispatch(ctx, true, background_task_pool.clone()).await {
            tracing::error!("API 钱包资产同步恢复Worker启动扫描失败: {:?}", e);
        }

        loop {
            sleep(Duration::from_secs(5)).await;
            if let Err(e) = scan_and_dispatch(ctx, true, background_task_pool.clone()).await {
                tracing::error!("API 钱包资产同步恢复Worker扫描失败: {:?}", e);
            }
        }
    });

    Ok(())
}

async fn scan_and_dispatch(
    ctx: &'static crate::context::Context,
    include_stuck_running: bool,
    background_task_pool: Arc<BackgroundTaskPool>,
) -> Result<(), ServiceError> {
    if ctx.is_recovery_paused() {
        tracing::debug!("API 钱包资产同步恢复Worker已暂停，跳过本轮扫描");
        return Ok(());
    }

    let defaults = runtime_defaults::recovery();
    let api_pool = ctx.api_wallet_pool()?;

    cleanup_done_states(
        &api_pool,
        defaults.api_asset_sync_done_retention_secs,
        defaults.api_asset_sync_done_max_rows,
    )
    .await?;

    let mut claimed = 0usize;
    loop {
        if ctx.is_recovery_paused() {
            break;
        }
        if claimed >= defaults.api_asset_sync_max_claims_per_round {
            tracing::debug!(
                claimed = claimed,
                metric = "api_asset_sync_claim_round_limit",
                "api asset sync recovery round reached claim limit"
            );
            break;
        }

        let task = ApiAssetSyncStateRepo::claim_next(&api_pool, include_stuck_running).await?;
        let Some(task) = task else { break };

        claimed += 1;
        let background_task_pool = background_task_pool.clone();
        background_task_pool
            .push(async move {
                process_one(ctx, task).await?;
                Ok(())
            })
            .await;
    }

    Ok(())
}

async fn cleanup_done_states(
    api_pool: &wallet_database::ApiWalletDbPool,
    retention_secs: u64,
    max_rows: u32,
) -> Result<(), ServiceError> {
    let removed_expired =
        ApiAssetSyncStateRepo::delete_done_older_than(api_pool, retention_secs).await?;
    let removed_over_limit = ApiAssetSyncStateRepo::trim_done_to_limit(api_pool, max_rows).await?;

    if removed_expired > 0 || removed_over_limit > 0 {
        tracing::info!(
            removed_expired = removed_expired,
            removed_over_limit = removed_over_limit,
            retention_secs = retention_secs,
            max_rows = max_rows,
            "已清理 API 钱包资产同步完成状态"
        );
    }

    Ok(())
}

async fn process_one(
    ctx: &'static crate::context::Context,
    task: wallet_database::entities::api_asset_sync_state::ApiAssetSyncStateEntity,
) -> Result<(), ServiceError> {
    let api_pool = ctx.api_wallet_pool()?;
    let token_key = task.token_address.clone();

    tracing::info!(
        address = %task.address,
        chain_code = %task.chain_code,
        token_address = %token_key,
        attempt_count = task.attempt_count,
        "开始处理 API 钱包持久化资产同步任务"
    );

    if !sync_asset_still_exists(&api_pool, &task.address, &task.chain_code, &token_key).await? {
        ApiAssetSyncStateRepo::mark_done(&api_pool, &task.address, &task.chain_code, token_key)
            .await?;
        tracing::info!(
            address = %task.address,
            chain_code = %task.chain_code,
            "API 钱包资产同步状态已清理：资产已不存在"
        );
        return Ok(());
    }

    let result = ApiAssetsDomain::sync_assets_by_addr_chain_durable_with_ctx(
        ctx,
        vec![task.address.clone()],
        Some(task.chain_code.clone()),
        token_key.clone(),
        task.attempt_count as u32,
    )
    .await;

    match result {
        Ok(()) => {
            ApiAssetSyncStateRepo::mark_done(&api_pool, &task.address, &task.chain_code, token_key)
                .await?;
            tracing::info!(
                address = %task.address,
                chain_code = %task.chain_code,
                "API 钱包持久化资产同步任务处理完成"
            );
        }
        Err(e) => {
            let msg = e.to_string();
            ApiAssetSyncStateRepo::mark_failed(
                &api_pool,
                &task.address,
                &task.chain_code,
                token_key,
                &msg,
            )
            .await?;
            tracing::warn!(
                address = %task.address,
                chain_code = %task.chain_code,
                error = %e,
                "API 钱包持久化资产同步任务处理失败"
            );
        }
    }

    Ok(())
}

async fn sync_asset_still_exists(
    api_pool: &wallet_database::ApiWalletDbPool,
    address: &str,
    chain_code: &str,
    token_key: &wallet_database::entities::asset_token_key::AssetTokenKey,
) -> Result<bool, ServiceError> {
    let assets =
        ApiAssetsRepo::list(api_pool, vec![address.to_string()], Some(chain_code.to_string()))
            .await?;
    Ok(assets.iter().any(|asset| asset.token_key().matches_for_chain(token_key, chain_code)))
}

#[cfg(test)]
mod token_case_tests {
    use super::sync_asset_still_exists;
    use wallet_database::{
        SqliteContext,
        entities::{
            api_assets::ApiCreateAssetsVo,
            api_chain::{ApiChainCreateVo, NodeBindType},
            api_coin::ApiCoinData,
            assets::AssetsId,
        },
        repositories::api_wallet::{assets::ApiAssetsRepo, chain::ApiChainRepo, coin::ApiCoinRepo},
    };

    #[tokio::test]
    async fn token_case_recovery_keeps_mixed_case_evm_task() {
        let dir = tempfile::tempdir().unwrap();
        let pool = SqliteContext::new(dir.path().to_str().unwrap(), Some("api_wallet.db"))
            .await
            .unwrap()
            .into_api_wallet_db_pool()
            .unwrap();
        let lower = "0xc2132d05d31c914a87c6611c10748aeb04b58e8f";
        let mixed = "0xc2132D05D31c914a87C6611C10748AEb04B58e8F";
        let chain = ApiChainCreateVo::new(
            "Polygon",
            "polygon",
            &vec!["evm".into()],
            NodeBindType::AutoLocal,
            "POL",
        );
        ApiChainRepo::add(&pool, chain).await.unwrap();
        let coin = ApiCoinData::new(
            Some("USDT".into()),
            "USDT",
            "polygon",
            lower.into(),
            Some("1".into()),
            None,
            6,
            1,
            1,
            1,
            chrono::Utc::now(),
            None,
        );
        ApiCoinRepo::upsert_multi_coin(&pool, vec![coin]).await.unwrap();
        let id = AssetsId::new("sender", "polygon", lower.into());
        ApiAssetsRepo::upsert_assets(
            &pool,
            ApiCreateAssetsVo::new(id, "USDT", 6, None, 0)
                .with_name("USDT")
                .with_balance("3.956788"),
        )
        .await
        .unwrap();
        assert!(sync_asset_still_exists(&pool, "sender", "polygon", &mixed.into()).await.unwrap());
        assert!(!sync_asset_still_exists(&pool, "other", "polygon", &mixed.into()).await.unwrap());
        assert!(
            !sync_asset_still_exists(&pool, "sender", "polygon", &"0xother".into()).await.unwrap()
        );
    }
}
