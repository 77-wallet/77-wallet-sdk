use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use once_cell::sync::Lazy;
use tokio::sync::Semaphore;
use wallet_database::{
    ApiWalletDbPool,
    entities::{api_assets::ApiCreateAssetsVo, asset_token_key::AssetTokenKey, assets::AssetsId},
    repositories::api_wallet::{assets::ApiAssetsRepo, coin::ApiCoinRepo},
};
use wallet_transport_backend::{api::BackendApi, request::api_wallet::address::AssetListReq};

use crate::error::{service::ServiceError, system::SystemError};

const DEFAULT_ASSET_UPSERT_MAX_CONCURRENT: usize = 2;

fn read_asset_upsert_max_concurrent() -> usize {
    std::env::var("API_ASSET_UPSERT_MAX_CONCURRENT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|v| v.clamp(1, 8))
        .unwrap_or(DEFAULT_ASSET_UPSERT_MAX_CONCURRENT)
}

static ASSET_UPSERT_SEMAPHORE: Lazy<Semaphore> =
    Lazy::new(|| Semaphore::new(read_asset_upsert_max_concurrent()));

pub(crate) async fn query_and_upsert_assets(
    ctx: &'static crate::context::Context,
    api_pool: &ApiWalletDbPool,
    backend: &BackendApi,
    req: &AssetListReq,
) -> Result<(), ServiceError> {
    query_and_upsert_assets_using(ctx, api_pool, req, async {
        Ok(backend.query_asset_list(req).await?)
    })
    .await
}

async fn query_and_upsert_assets_using(
    ctx: &'static crate::context::Context,
    api_pool: &ApiWalletDbPool,
    req: &AssetListReq,
    fetch: impl std::future::Future<
        Output = Result<
            wallet_transport_backend::response_vo::api_wallet::address::AssetsListRes,
            ServiceError,
        >,
    >,
) -> Result<(), ServiceError> {
    use crate::service::api_wallet::business_notification::ApiBusinessNotificationService;
    use wallet_database::repositories::{
        api_business_notification::ApiBusinessNotificationRepo,
        api_wallet::{api_asset_sync_state::ApiAssetSyncStateRepo, wallet::ApiWalletRepo},
    };
    // Subaccount imports (including 500k-address wallets) never read alert history.
    let withdrawal = ApiWalletRepo::find_by_uid(api_pool, &req.uid).await?.is_some_and(|w| {
        w.status == 1
            && w.api_wallet_type == wallet_database::entities::api_wallet::ApiWalletType::Withdrawal
    });
    // Capture versions BEFORE starting the observation; no local account is required.
    let mut observed_events = if withdrawal {
        ApiBusinessNotificationRepo::latest_for_import(api_pool, &req.uid, &req.chain_code).await?
    } else {
        Vec::new()
    };
    let list = fetch.await?;
    let default_coins_list = ApiCoinRepo::coin_list(api_pool).await?;

    let mut all_assets: Vec<ApiCreateAssetsVo> = Vec::new();
    let mut refresh_plan: HashMap<AssetTokenKey, HashSet<String>> = HashMap::new();

    for asset in list.0 {
        for address in asset.address_list {
            for token in address.token_infos {
                if let Some(coin) = default_coins_list.iter().find(|coin| {
                    coin.chain_code == req.chain_code
                        && coin.token_address.as_db_str() == token.token_address.as_str()
                }) {
                    let assets_id = AssetsId::new(
                        &address.address,
                        &req.chain_code,
                        Some(token.token_address.clone()).into(),
                    );

                    let balance_str = token.amount.to_string();

                    let assets = ApiCreateAssetsVo::new(
                        assets_id,
                        &token.symbol.to_ascii_uppercase(),
                        coin.decimals,
                        coin.protocol.clone(),
                        0,
                    )
                    .with_name(&coin.name)
                    .with_balance(&balance_str);

                    all_assets.push(assets);
                    record_asset_refresh(
                        &mut refresh_plan,
                        &address.address,
                        AssetTokenKey::from_raw(Some(token.token_address.as_str())),
                    );
                }
            }
        }
    }

    let balances: Vec<_> = if observed_events.is_empty() {
        Vec::new()
    } else {
        all_assets.iter().map(|a| (a.assets_id.clone(), a.balance.clone())).collect()
    };
    // Only successfully returned, supported assets may affect a notification.
    observed_events.retain(|event| {
        serde_json::from_str::<crate::messaging::mqtt::topics::api_wallet::address_insufficient::AwmAddressInsufficientBody>(&event.payload)
            .ok().is_some_and(|body| balances.iter().any(|(id, _)|
                id.address == body.address && id.chain_code == body.chain_code &&
                id.token_address.normalized_for_chain(&id.chain_code) == AssetTokenKey::from_raw(Some(&body.token)).normalized_for_chain(&body.chain_code)))
    });
    if !all_assets.is_empty() {
        // Limit concurrent bulk upsert writers to avoid exhausting the shared sqlite pool.
        let gate_wait_start = Instant::now();
        let _permit = ASSET_UPSERT_SEMAPHORE.acquire().await.map_err(|_| {
            ServiceError::System(SystemError::Internal("asset upsert semaphore closed".to_string()))
        })?;
        let wait_ms = gate_wait_start.elapsed().as_millis();
        if wait_ms > 500 {
            tracing::warn!(
                wait_ms = wait_ms,
                chain_code = %req.chain_code,
                asset_count = all_assets.len(),
                "asset upsert gate wait too long"
            );
        }
        ApiAssetsRepo::upsert_assets_multi_update_balance(api_pool, all_assets).await?;
    }

    if !observed_events.is_empty() {
        let retry = match ApiBusinessNotificationService::new(ctx)
            .resolve_from_persisted_balances(&req.uid, &balances, &observed_events)
            .await
        {
            Ok(retry) => retry,
            Err(error) => {
                tracing::warn!(%error, "import notification reconciliation unavailable");
                balances.iter().filter(|(id, _)| observed_events.iter().any(|e| {
                    serde_json::from_str::<crate::messaging::mqtt::topics::api_wallet::address_insufficient::AwmAddressInsufficientBody>(&e.payload)
                        .ok().is_some_and(|body| body.address == id.address && body.chain_code == id.chain_code && AssetTokenKey::from_raw(Some(&body.token)).normalized_for_chain(&body.chain_code) == id.token_address.normalized_for_chain(&id.chain_code))
                })).map(|(id, _)| id.clone()).collect()
            }
        };
        for id in retry {
            ApiAssetSyncStateRepo::upsert_pending(
                api_pool,
                &id.address,
                &id.chain_code,
                id.token_address.clone(),
            )
            .await?;
            ApiAssetSyncStateRepo::mark_failed(
                api_pool,
                &id.address,
                &id.chain_code,
                id.token_address,
                "import notification compensation unavailable",
            )
            .await?;
        }
    }

    // Best-effort: notify inner event to trigger further sync/calculation pipeline.
    let handles = ctx.get_global_handles().await;
    if let Some(handles) = handles.upgrade() {
        let inner_event_handle = handles.get_global_inner_event_handle();
        for (token_key, addresses) in refresh_plan {
            let data = crate::infrastructure::inner_event::SyncAssetsData::new_with_token_key(
                addresses.into_iter().collect(),
                req.chain_code.clone(),
                token_key,
            )
            .with_priority(crate::infrastructure::inner_event::SyncPriority::Low);
            if let Err(e) = inner_event_handle
                .send(crate::infrastructure::inner_event::InnerEvent::ApiWalletSyncAssets(data))
            {
                tracing::error!("发送资产同步事件失败: error={}", e);
            }
        }
    }

    Ok(())
}

fn record_asset_refresh(
    refresh_plan: &mut HashMap<AssetTokenKey, HashSet<String>>,
    address: &str,
    token_key: AssetTokenKey,
) {
    refresh_plan.entry(token_key).or_default().insert(address.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_asset_query_dispatches_updated_token_events() {
        let address = "TMgMcdSzdBJgGV3quNCdeBRjBFSZ2jf5eY";
        let jst = AssetTokenKey::from_raw(Some("TCFLL5dx5ZJdKnWuesXxi1VPwjLVmWZZy9"));
        let usdt = AssetTokenKey::from_raw(Some("TXLAQ63Xg1NAzckPwKHvzw7CSEmLMEqcdj"));

        let mut refresh_plan = HashMap::new();

        record_asset_refresh(&mut refresh_plan, address, jst.clone());
        record_asset_refresh(&mut refresh_plan, address, jst.clone());
        record_asset_refresh(&mut refresh_plan, "TSecondAddress", usdt.clone());

        assert_eq!(refresh_plan.len(), 2);
        assert_eq!(refresh_plan.get(&jst).expect("jst token refresh").len(), 1);
        assert!(
            refresh_plan.get(&jst).expect("jst token refresh").contains(address),
            "JST backend asset update must schedule a JST refresh, not only native TRX"
        );
        assert!(refresh_plan.get(&usdt).expect("usdt token refresh").contains("TSecondAddress"));
        assert!(!refresh_plan.contains_key(&AssetTokenKey::Native));
    }
}

#[cfg(test)]
mod import_notification_tests;
