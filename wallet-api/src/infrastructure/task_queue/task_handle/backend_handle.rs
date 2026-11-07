use async_trait::async_trait;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Weak},
};
use tokio::sync::Mutex;
use wallet_database::{
    entities::address_query_state::AddressQueryStatus,
    repositories::{
        account::AccountRepo,
        api_wallet::{
            account::ApiAccountRepo, address_query_state::AddressQueryStateRepo,
            asset_query_state::AssetQueryStateRepo, wallet::ApiWalletRepo,
        },
        device::DeviceRepo,
        wallet::WalletRepo,
    },
};
use wallet_transport_backend::{
    api::BackendApi,
    consts::endpoint,
    request::{
        FindConfigByKey,
        api_wallet::address::{AddressListReq, AssetListReq},
    },
    response_vo::{app::FindConfigByKeyRes, coin::TokenRates},
};

use crate::{
    domain::{
        api_wallet::{account::ApiAccountDomain, chain::ApiChainDomain, wallet::ApiWalletDomain},
        app::config::ConfigDomain,
        chain::ChainDomain,
        node::NodeDomain,
    },
    infrastructure::{
        chain_node::chain_node_ensurer::ChainNodeEnsurer,
        expand_init::executor::do_init_with_ctx,
        task_queue::{
            backend::{BackendApiTask, BackendApiTaskData},
            task::Tasks,
        },
    },
    messaging::notify::FrontendNotifyEvent,
};
pub struct BackendTaskHandle;

static DEFAULT_ENDPOINTS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        endpoint::multisig::SIGNED_ORDER_ACCEPT,
        endpoint::multisig::SIGNED_ORDER_CANCEL,
        endpoint::multisig::SIGNED_ORDER_UPDATE_RECHARGE_HASH,
        endpoint::multisig::SIGNED_ORDER_UPDATE_SIGNED_HASH,
        endpoint::multisig::SIGNED_TRAN_CREATE,
        endpoint::multisig::SIGNED_TRAN_ACCEPT,
        endpoint::multisig::SIGNED_TRAN_UPDATE_TRANS_HASH,
        endpoint::multisig::SIGNED_ORDER_SAVE_RAW_DATA,
        // endpoint::DEVICE_DELETE,
        // endpoint::DEVICE_BIND_ADDRESS,
        endpoint::DEVICE_UNBIND_ADDRESS,
        endpoint::SEND_MSG_CONFIRM,
        endpoint::multisig::PERMISSION_ACCEPT,
        endpoint::UPLOAD_PERMISSION_TRANS,
        endpoint::DEVICE_UPDATE_APP_ID,
        endpoint::KEYS_UPDATE_WALLET_NAME,
        endpoint::ADDRESS_UPDATE_ACCOUNT_NAME,
        endpoint::APP_INSTALL_SAVE,
        endpoint::TOKEN_BALANCE_REFRESH,
        endpoint::SWAP_APPROVE_CANCEL,
        endpoint::SWAP_APPROVE_SAVE,
    ]
    .iter()
    .cloned()
    .collect()
});

static QUERY_ADDRESS_LIST_LOCKS: Lazy<DashMap<String, Weak<Mutex<()>>>> = Lazy::new(DashMap::new);

fn query_address_list_lock_key(uid: &str, chain_code: &str) -> String {
    format!("{uid}:{chain_code}")
}

fn query_address_list_lock(key: &str) -> Arc<Mutex<()>> {
    if let Some(entry) = QUERY_ADDRESS_LIST_LOCKS.get(key) {
        if let Some(lock) = entry.value().upgrade() {
            return lock;
        }
    }
    let lock = Arc::new(Mutex::new(()));
    QUERY_ADDRESS_LIST_LOCKS.insert(key.to_string(), Arc::downgrade(&lock));
    lock
}

fn body_field_count(body: &serde_json::Value) -> usize {
    body.as_object().map(|map| map.len()).unwrap_or(0)
}

impl BackendTaskHandle {
    pub async fn do_handle(
        ctx: &'static crate::context::Context,
        endpoint: &str,
        body: serde_json::Value,
        backend: Arc<BackendApi>,
        // wallet_type: WalletType,
    ) -> Result<(), crate::error::service::ServiceError> {
        if Self::endpoint_requires_system_ready(endpoint) {
            tracing::info!(endpoint = endpoint, "endpoint requires system_ready gate, waiting");
            crate::infrastructure::system_ready::wait_system_ready().await;
            tracing::info!(
                endpoint = endpoint,
                "system_ready gate passed, continue endpoint handling"
            );
        }

        let handler = Self::get_handler(endpoint);
        let body_kind = body_field_count(&body);
        tracing::info!(endpoint = endpoint, body_fields = body_kind, "handling backend task");
        handler.handle(ctx, endpoint, body, backend.as_ref()).await?;

        Ok(())
    }

    pub(crate) fn is_default_endpoint(endpoint: &str) -> bool {
        DEFAULT_ENDPOINTS.contains(&endpoint)
    }

    fn endpoint_requires_system_ready(endpoint: &str) -> bool {
        matches!(
            endpoint,
            endpoint::api_wallet::QUERY_ADDRESS_LIST | endpoint::api_wallet::QUERY_ASSET_LIST
        )
    }

    /// 获取对应的处理策略
    fn get_handler(endpoint: &str) -> Box<dyn EndpointHandler + Send + Sync> {
        if Self::is_default_endpoint(endpoint) {
            Box::new(DefaultHandler)
        } else {
            Box::new(SpecialHandler)
        }
    }
}

/// 定义一个处理策略的 trait
#[async_trait::async_trait]
trait EndpointHandler {
    async fn handle(
        &self,
        ctx: &'static crate::context::Context,
        endpoint: &str,
        body: serde_json::Value,
        backend: &BackendApi,
        // wallet_type: WalletType,
    ) -> Result<(), crate::error::service::ServiceError>;
}

/// 默认的处理策略
struct DefaultHandler;

#[async_trait::async_trait]
impl EndpointHandler for DefaultHandler {
    async fn handle(
        &self,
        ctx: &'static crate::context::Context,
        endpoint: &str,
        body: serde_json::Value,
        backend: &BackendApi,
        // _wallet_type: WalletType,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let sn = ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(pool, sn).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };

        if device.is_init != 1 {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        }
        // let invite_code = ConfigDomain::get_invite_code().await?;
        // if invite_code.status.is_none() {
        //     return Err(
        //         crate::BusinessError::Device(crate::DeviceError::InviteStatusNotConfirmed).into(),
        //     );
        // }
        // 实现具体的处理逻辑
        let _res = backend.post_default(endpoint, &body).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::BackendTaskHandle;
    use wallet_transport_backend::consts::endpoint;

    #[test]
    fn endpoint_requires_system_ready_for_api_recovery_endpoints() {
        assert!(BackendTaskHandle::endpoint_requires_system_ready(
            endpoint::api_wallet::QUERY_ADDRESS_LIST
        ));
        assert!(BackendTaskHandle::endpoint_requires_system_ready(
            endpoint::api_wallet::QUERY_ASSET_LIST
        ));
    }

    #[test]
    fn endpoint_requires_system_ready_is_false_for_common_endpoints() {
        assert!(!BackendTaskHandle::endpoint_requires_system_ready(endpoint::CHAIN_LIST));
        assert!(!BackendTaskHandle::endpoint_requires_system_ready(endpoint::LANGUAGE_INIT));
        assert!(!BackendTaskHandle::endpoint_requires_system_ready(endpoint::ADDRESS_BATCH_INIT));
    }

    #[test]
    fn body_field_count_uses_only_summary_information() {
        let body = serde_json::json!({
            "password": "super-secret",
            "tradeNo": "trade-1",
        });

        assert_eq!(super::body_field_count(&body), 2);
        assert_eq!(super::body_field_count(&serde_json::Value::Null), 0);
    }
}

/// 特殊的处理策略
struct SpecialHandler;

#[async_trait]
impl EndpointHandler for SpecialHandler {
    async fn handle(
        &self,
        ctx: &'static crate::context::Context,
        endpoint: &str,
        body: serde_json::Value,
        backend: &BackendApi,
        // TODO： 完全不需要这个
        // wallet_type: WalletType,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = ctx.core_pool()?;
        let api_pool = ctx.api_wallet_pool()?;
        // let mut repo = wallet_database::factory::RepositoryFactory::repo(core_pool.into_inner());
        let sn = ctx.get_sn();
        match endpoint {
            endpoint::DEVICE_QUERY_SEND_MSG_AMOUNT => {
                ConfigDomain::fetch_min_config(ctx, sn).await?;
            }
            endpoint::DEVICE_INIT => {
                let res = backend.post_req_str::<Option<()>>(endpoint, &body).await;
                res?;
                DeviceRepo::device_init(core_pool.clone(), sn).await?;
            }
            endpoint::KEYS_V2_INIT => {
                let status = ConfigDomain::get_keys_reset_status(&ctx).await?;
                if let Some(status) = status
                    && let Some(false) = status.status
                {
                    return Err(crate::error::business::BusinessError::Config(
                        crate::error::business::config::ConfigError::KeysNotReset,
                    )
                    .into());
                }

                let res = backend.post_req_str::<Option<()>>(endpoint, &body).await;

                // TODO 先单独在这里发送事件，后期修改为统一处理
                #[cfg(not(feature = "prod"))]
                if let Err(ref e) = res {
                    let message = serde_json::json!({
                        "event": "keys_init_fail",
                        "message": e.to_string(),
                    });
                    let _r = FrontendNotifyEvent::send_debug_with_ctx(ctx, message).await;
                }
                res?;

                let req: wallet_transport_backend::request::KeysInitReq =
                    wallet_utils::serde_func::serde_from_value(body)?;
                WalletRepo::wallet_init(core_pool, &req.uid).await?;
            }
            endpoint::old_wallet::OLD_KEYS_V2_INIT => {
                let status = ConfigDomain::get_keys_reset_status(&ctx).await?;
                if let Some(status) = status
                    && let Some(false) = status.status
                {
                    return Err(crate::error::business::BusinessError::Config(
                        crate::error::business::config::ConfigError::KeysNotReset,
                    )
                    .into());
                }

                let res = backend.post_req_str::<Option<()>>(endpoint, &body).await;

                // TODO 先单独在这里发送事件，后期修改为统一处理
                #[cfg(not(feature = "prod"))]
                if let Err(ref e) = res {
                    let message = serde_json::json!({
                        "event": "keys_init_fail",
                        "message": e.to_string(),
                    });
                    let _r = FrontendNotifyEvent::send_debug_with_ctx(ctx, message).await;
                }
                res?;
                let req: wallet_transport_backend::request::KeysInitReq =
                    wallet_utils::serde_func::serde_from_value(body)?;
                ApiWalletRepo::mark_init(&api_pool, &req.uid).await?;
            }

            endpoint::api_wallet::ADDRESS_INIT => {
                tracing::info!("开始处理地址初始化请求: {:?}", body);
                let req: wallet_transport_backend::request::api_wallet::address::ApiAddressInitReq =
                    wallet_utils::serde_func::serde_from_value(body.clone())?;

                // 直接转发给do_init执行，统一处理逻辑
                do_init_with_ctx(ctx, req).await?;

                tracing::info!("地址初始化请求处理完成");
            }
            endpoint::old_wallet::OLD_ADDRESS_BATCH_INIT => {
                let status = ConfigDomain::get_keys_reset_status(&ctx).await?;
                if let Some(status) = status
                    && let Some(false) = status.status
                {
                    return Err(crate::error::business::BusinessError::Config(
                        crate::error::business::config::ConfigError::KeysNotReset,
                    )
                    .into());
                }

                let req: wallet_transport_backend::request::AddressBatchInitReq =
                    wallet_utils::serde_func::serde_from_value(body.clone())?;

                for address in req.0 {
                    let wallet = ApiWalletRepo::find_by_uid(&api_pool, &address.uid).await?;

                    match wallet {
                        Some(wallet) => {
                            if wallet.is_init == 1 {
                                ApiAccountRepo::init(
                                    &api_pool,
                                    &address.address,
                                    &address.chain_code,
                                )
                                .await?;
                                continue;
                            } else {
                                return Err(crate::error::business::BusinessError::ApiWallet(
                                    crate::error::business::api_wallet::ApiWalletError::WalletNotInit,
                                )
                                .into());
                            }
                        }
                        None => {
                            return Err(crate::error::business::BusinessError::ApiWallet(
                                crate::error::business::api_wallet::ApiWalletError::WalletNotInit,
                            )
                            .into());
                        }
                    }
                }

                let res = backend.post_req_str::<()>(endpoint, &body).await;
                res?;
            }

            endpoint::DEVICE_EDIT_DEVICE_INVITEE_STATUS => {
                let pool = ctx.core_pool()?;
                let Some(device) = DeviceRepo::get_device_info(pool, sn).await? else {
                    return Err(crate::error::business::BusinessError::Device(
                        crate::error::business::device::DeviceError::Uninitialized,
                    )
                    .into());
                };

                if device.is_init != 1 {
                    return Err(crate::error::business::BusinessError::Device(
                        crate::error::business::device::DeviceError::Uninitialized,
                    )
                    .into());
                }

                let req: wallet_transport_backend::request::SetInviteeStatusReq =
                    wallet_utils::serde_func::serde_from_value(body.clone())?;
                let res = backend.post_req_str::<Option<()>>(endpoint, &body).await;

                res?;
                let code = ConfigDomain::get_invite_code(&ctx).await?.and_then(|c| c.code);

                ConfigDomain::set_invite_code(&ctx, Some(req.invitee), code).await?;
            }
            endpoint::LANGUAGE_INIT => {
                backend.post_req_str::<()>(endpoint, &body).await?;
                DeviceRepo::language_init(core_pool.clone(), sn).await?;
                crate::domain::announcement::AnnouncementDomain::pull_announcement(
                    &core_pool, &ctx,
                )
                .await?;
            }
            endpoint::ADDRESS_BATCH_INIT => {
                let status = ConfigDomain::get_keys_reset_status(&ctx).await?;
                if let Some(status) = status
                    && let Some(false) = status.status
                {
                    return Err(crate::error::business::BusinessError::Config(
                        crate::error::business::config::ConfigError::KeysNotReset,
                    )
                    .into());
                }

                let req: wallet_transport_backend::request::AddressBatchInitReq =
                    wallet_utils::serde_func::serde_from_value(body.clone())?;

                for address in req.0 {
                    let wallet =
                        WalletRepo::wallet_detail_by_uid(core_pool.clone(), &address.uid).await?;

                    match wallet {
                        Some(wallet) => {
                            if wallet.is_init == 1 {
                                AccountRepo::account_init(
                                    core_pool.clone(),
                                    &address.address,
                                    &address.chain_code,
                                )
                                .await?;
                                continue;
                            } else {
                                return Err(crate::error::business::BusinessError::Wallet(
                                    crate::error::business::wallet::WalletError::NotInit,
                                )
                                .into());
                            }
                        }
                        None => {
                            return Err(crate::error::business::BusinessError::Wallet(
                                crate::error::business::wallet::WalletError::NotFound,
                            )
                            .into());
                        }
                    }
                }

                let res = backend.post_req_str::<()>(endpoint, &body).await;
                res?;
            }
            endpoint::TOKEN_CUSTOM_TOKEN_INIT => {
                let res = backend.post_req_str::<bool>(endpoint, &body).await;
                res?;

                crate::service::coin::CoinService::new(ctx).init_token_price().await?;
            }

            endpoint::TOKEN_QUERY_RATES => {
                let rates: TokenRates = backend.post_req_str::<TokenRates>(endpoint, &body).await?;

                let exchange_rate_service =
                    crate::service::exchange_rate::ExchangeRateService::new(ctx);
                exchange_rate_service.init(rates).await?;
            }
            endpoint::SYS_CONFIG_FIND_CONFIG_BY_KEY => {
                let req: FindConfigByKey =
                    wallet_utils::serde_func::serde_from_value(body.clone())?;
                match req.key.as_str() {
                    "OFFICIAL:WEBSITE" => {
                        let res =
                            backend.post_req_str::<FindConfigByKeyRes>(endpoint, &body).await?;
                        ConfigDomain::set_official_website(&ctx, res.value).await?;
                    }
                    _ => {
                        tracing::warn!("unknown key: {}", req.key);
                    }
                }
            }
            endpoint::APP_INSTALL_DOWNLOAD => {
                let url = backend.post_req_str::<String>(endpoint, &body).await?;
                ConfigDomain::set_app_download_qr_code_url(&ctx, &url).await?;
                // ConfigDomain::set_version_download_url(&url).await?;
            }
            endpoint::VERSION_VIEW => {
                // let app_version_res = backend
                //     .post_req_str::<wallet_transport_backend::response_vo::app::AppVersionRes>(
                //         endpoint, &body,
                //     )
                //     .await?;
                // ConfigDomain::set_version_download_url(app_version_res.download_url)
            }
            endpoint::CHAIN_LIST => {
                let input = backend
                    .post_req_str::<wallet_transport_backend::response_vo::chain::ChainList>(
                        endpoint, &body,
                    )
                    .await?;
                // 1. 后端 chains → upsert 到本地
                ChainDomain::init_load_backend_chains_with_ctx(ctx, input).await?;
                // 2. 基于本地 chains → 触发去拉 nodes
                NodeDomain::init_sync_nodes_with_ctx(ctx).await?;
                // 3. 兜底保证每条链都有 node
                let ensurer = ChainNodeEnsurer::new(core_pool.clone(), api_pool.clone());
                ensurer.ensure_all().await?;
                crate::domain::coin::CoinDomain::sync_default_coins_by_bound_nodes_with_ctx(ctx)
                    .await?;
            }
            endpoint::api_wallet::API_WALLET_CHAIN_LIST => {
                let body: HashMap<String, String> =
                    wallet_utils::serde_func::serde_from_value(body)?;
                let app_version_code = body.get("appVersionCode");
                let input = backend.api_wallet_chain_list(app_version_code.unwrap()).await?;
                //先插入再过滤
                ApiChainDomain::upsert_multi_api_chain_than_toggle(ctx, input).await?;
                ApiChainDomain::reconcile_accounts_if_unlocked(ctx).await?;
            }
            endpoint::CHAIN_RPC_LIST => {
                let input = backend
                    .post_req_str::<wallet_transport_backend::response_vo::chain::ChainInfos>(
                        endpoint, &body,
                    )
                    .await?;
                NodeDomain::upsert_chain_rpc(&core_pool, input).await?;
                let ensurer = ChainNodeEnsurer::new(core_pool.clone(), api_pool.clone());
                ensurer.ensure_all().await?;
                crate::domain::api_wallet::coin::ApiCoinDomain::sync_dev_native_coins(ctx).await?;
                ApiChainDomain::reconcile_accounts_if_unlocked(ctx).await?;
                crate::domain::coin::CoinDomain::sync_default_coins_by_bound_nodes_with_ctx(ctx)
                    .await?;
            }
            endpoint::old_wallet::OLD_CHAIN_RPC_LIST => {
                let input = backend
                    .post_req_str::<wallet_transport_backend::response_vo::chain::ChainInfos>(
                        endpoint, &body,
                    )
                    .await?;
                NodeDomain::upsert_chain_rpc(&core_pool, input).await?;
                let ensurer = ChainNodeEnsurer::new(core_pool.clone(), api_pool.clone());
                ensurer.ensure_all().await?;
                crate::domain::coin::CoinDomain::sync_default_coins_by_bound_nodes_with_ctx(ctx)
                    .await?;
            }
            endpoint::MQTT_INIT => {
                // 1.4 version 注释掉,
                // let mqtt_url = backend
                //     .post_req_str::<String>(endpoint, &body)
                //     .await?;
                // ConfigDomain::set_mqtt_url(Some(mqtt_url)).await?;
            }
            endpoint::KEYS_RESET => {
                // 1. 先调用backend reset接口
                match backend.post_req_str::<Option<()>>(endpoint, &body).await {
                    Ok(_) => {
                        // 2. reset成功后，只设置status，epoch已在physical_reset中设置
                        ConfigDomain::set_keys_reset_status(&ctx, Some(true)).await?;
                    }
                    Err(err) => {
                        // 3. reset失败，status设为false
                        ConfigDomain::set_keys_reset_status(&ctx, Some(false)).await?;
                        return Err(err.into());
                    }
                };
            }
            endpoint::api_wallet::QUERY_ADDRESS_LIST => {
                use std::time::Instant;
                let start_total = Instant::now();

                // 1. 反序列化请求体
                let start_deserialize = Instant::now();
                let mut req =
                    wallet_utils::serde_func::serde_from_value::<AddressListReq>(body.clone())?;
                ctx.recovery_coordinator().wait_for_turn(Some(&req.uid)).await;
                tracing::debug!(
                    "[PERF] QUERY_ADDRESS_LIST start: uid={}, chain_code={}",
                    req.uid,
                    req.chain_code
                );
                let lock_key = query_address_list_lock_key(&req.uid, &req.chain_code);
                let query_lock = query_address_list_lock(&lock_key);
                let _lock_guard = query_lock.lock().await;
                tracing::debug!(
                    "[PERF] QUERY_ADDRESS_LIST: deserialized request in {:?}, uid={}, chain_code={}",
                    start_deserialize.elapsed(),
                    req.uid,
                    req.chain_code
                );

                // 2. 查询地址查询状态，决定起始页码
                let start_check_state = Instant::now();
                let state = AddressQueryStateRepo::get_by_uid_and_chain(
                    &api_pool,
                    &req.uid,
                    &req.chain_code,
                )
                .await?;

                let start_page = match state {
                    None => {
                        // 第一次查询，插入状态
                        let query_state = AddressQueryStateRepo::build_create_state(
                            &req.uid,
                            &req.chain_code,
                            AddressQueryStatus::Running,
                        );
                        AddressQueryStateRepo::upsert(&api_pool, query_state).await?;
                        0
                    }
                    Some(s) if s.status == AddressQueryStatus::Done => {
                        // 已经完成，直接返回
                        tracing::info!(
                            "Address query already done for uid={}, chain_code={}",
                            req.uid,
                            req.chain_code
                        );
                        return Ok(());
                    }
                    Some(s) => {
                        // 从正确的页码开始：last_page < 0 时从 0 开始，否则从 last_page + 1 开始
                        if s.last_page < 0 {
                            0 // 表示从未成功拉到任何页
                        } else {
                            s.last_page + 1
                        }
                    }
                };

                // 设置请求页码
                req.page_num = start_page as i32;
                tracing::debug!(
                    "[PERF] QUERY_ADDRESS_LIST: checked query state in {:?}, uid={}, chain_code={}, start_page={}",
                    start_check_state.elapsed(),
                    req.uid,
                    req.chain_code,
                    start_page
                );

                // 4. 调用后端查询地址列表
                let start_backend_query = Instant::now();
                let res = backend.query_used_address_list(&req).await?;
                let backend_query_elapsed = start_backend_query.elapsed();
                tracing::debug!(
                    "[PERF] QUERY_ADDRESS_LIST: backend query in {:?}, uid={}, chain_code={}, total_elements={}",
                    backend_query_elapsed,
                    req.uid,
                    req.chain_code,
                    res.total_elements
                );

                if res.total_elements == 0 {
                    // 没有地址，直接标记为完成
                    AddressQueryStateRepo::update_status(
                        &api_pool,
                        &req.uid,
                        &req.chain_code,
                        AddressQueryStatus::Done,
                    )
                    .await?;
                    return Ok(());
                }
                // 更新总远程地址数
                AddressQueryStateRepo::update_total_remote(
                    &api_pool,
                    &req.uid,
                    &req.chain_code,
                    res.total_elements as i64,
                )
                .await?;

                let list = res.content;
                tracing::debug!("query_used_address_list req: {:?}", req);
                tracing::debug!("query_used_address_list list: {:?}", list);

                let mut done = 0;
                tracing::debug!("查询地址列表： total_elements: {}", res.total_elements);

                // 5. 处理地址列表
                let start_batch_process = Instant::now();

                // 5.1 获取后端地址索引集合
                let backend_indices: Vec<i32> = list.iter().map(|addr| addr.index).collect();
                let backend_indices_set: std::collections::HashSet<i32> =
                    backend_indices.iter().cloned().collect();

                // 5.1.1 持久化资产查询任务（以本页后端索引为准，避免部分恢复导致漏查余额）
                let mut backend_indices_sorted: Vec<i32> =
                    backend_indices_set.iter().cloned().collect();
                backend_indices_sorted.sort_unstable();
                let index_list_json =
                    serde_json::to_string(&backend_indices_sorted).map_err(|e| {
                        crate::error::service::ServiceError::System(
                            crate::error::system::SystemError::Internal(e.to_string()),
                        )
                    })?;
                AssetQueryStateRepo::upsert_pending(
                    &api_pool,
                    &req.uid,
                    &req.chain_code,
                    res.number as i64,
                    &index_list_json,
                )
                .await?;

                // 5.2 查询本地数据库中已存在的地址索引
                let wallet = ApiWalletRepo::find_by_uid(&api_pool, &req.uid).await?.ok_or(
                    crate::error::business::BusinessError::ApiWallet(
                        crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
                    ),
                )?;

                let local_indices_tuples = ApiAccountRepo::list_inited_indices_by_candidates(
                    &api_pool,
                    &wallet.address,
                    &req.chain_code,
                    &backend_indices_sorted,
                )
                .await?;
                let local_indices: Vec<i32> =
                    local_indices_tuples.iter().map(|(idx,)| *idx).collect();
                let local_indices_set: std::collections::HashSet<i32> =
                    local_indices.iter().cloned().collect();

                // 5.3 计算需要恢复的地址（后端有但本地没有）
                let need_recover: std::collections::HashSet<i32> =
                    backend_indices_set.difference(&local_indices_set).cloned().collect();

                tracing::debug!(
                    "地址处理分析：后端索引数={}, 本地索引数={}, 需要恢复={}",
                    backend_indices.len(),
                    local_indices.len(),
                    need_recover.len()
                );

                // 5.5 批量恢复已有地址（直接插入，不调用create）
                if !need_recover.is_empty() {
                    let start_recover = Instant::now();

                    // 获取需要恢复的地址详情
                    let addresses_to_recover: Vec<_> = list
                        .into_iter()
                        .filter(|addr| need_recover.contains(&addr.index))
                        .collect();

                    // 批量恢复已有地址，使用优化后的create_api_account
                    // 提取需要恢复的索引列表
                    let input_indices: Vec<_> =
                        addresses_to_recover.iter().map(|addr| addr.index).collect();

                    // 调用优化后的create_api_account，使用快速路径+慢速路径模式
                    ApiAccountDomain::recover_prefiltered_missing_accounts_with_ctx(
                        ctx,
                        &wallet.address,
                        req.chain_code.to_string(),
                        &input_indices,
                        wallet.api_wallet_type,
                        res.last,          // ⭐ 新传参：是否最后一页
                        res.number as i64, // ⭐ 新传参：当前页码
                    )
                    .await?;

                    done += addresses_to_recover.len();
                    tracing::debug!(
                        "[PERF] QUERY_ADDRESS_LIST: recovered {} addresses in {:?}, uid={}, chain_code={}",
                        addresses_to_recover.len(),
                        start_recover.elapsed(),
                        req.uid,
                        req.chain_code
                    );
                }

                // 5.7 投递地址恢复进度到统一通知聚合器（由聚合器负责节流、去重和最终发送）
                let start_send_notify = Instant::now();
                if let Err(e) = ApiAccountDomain::enqueue_address_recovery_progress_with_ctx(
                    ctx,
                    &req.uid,
                    &req.chain_code,
                    res.number as i32,
                    done as u32,
                    res.total_elements as u32,
                    res.last,
                )
                .await
                {
                    tracing::warn!("Failed to enqueue address recovery progress: {}", e);
                }
                tracing::debug!(
                    "[PERF] QUERY_ADDRESS_LIST: enqueued recovery notify in {:?}, uid={}, done={}, total={}, page={}, last={}",
                    start_send_notify.elapsed(),
                    req.uid,
                    done,
                    res.total_elements,
                    res.number,
                    res.last
                );

                tracing::debug!("查询地址列表：处理完成，共恢复 {} 个地址", need_recover.len());
                let batch_process_elapsed = start_batch_process.elapsed();
                tracing::debug!(
                    "[PERF] QUERY_ADDRESS_LIST: batch processing completed in {:?}, uid={}, chain_code={}, done={}",
                    batch_process_elapsed,
                    req.uid,
                    req.chain_code,
                    done
                );

                tracing::debug!("查询地址列表： create_api_account done: done: {done}");

                // 9. 更新进度 - 成功处理本页后推进 last_page / Done（必须在资产查询任务持久化后）
                let start_update_progress = Instant::now();
                AddressQueryStateRepo::update_last_page(
                    &api_pool,
                    &req.uid,
                    &req.chain_code,
                    res.number as i64,
                )
                .await?;
                if res.last {
                    AddressQueryStateRepo::update_status(
                        &api_pool,
                        &req.uid,
                        &req.chain_code,
                        AddressQueryStatus::Done,
                    )
                    .await?;
                }
                if !res.last {
                    // 直接递归调用下一页，不需要 tasks 系统
                    let next_page = res.number + 1;
                    let query_address_list_req =
                        AddressListReq::new(&req.uid, &req.chain_code, next_page, 1000);
                    let query_address_list_body = serde_json::to_value(query_address_list_req)
                        .map_err(|e| {
                            crate::error::service::ServiceError::System(
                                crate::error::system::SystemError::Internal(e.to_string()),
                            )
                        })?;

                    tracing::debug!(
                        "[PERF] QUERY_ADDRESS_LIST: updating progress in {:?}, uid={}, chain_code={}, next_page={}",
                        start_update_progress.elapsed(),
                        req.uid,
                        req.chain_code,
                        next_page
                    );

                    // 直接调用下一页，实现断点续查
                    let query_address_list_task_data = BackendApiTaskData::new(
                        wallet_transport_backend::consts::endpoint::api_wallet::QUERY_ADDRESS_LIST,
                        &query_address_list_body,
                    )?;

                    Tasks::new()
                        .push(BackendApiTask::BackendApi(query_address_list_task_data))
                        .send_with_ctx(ctx)
                        .await?;
                }
                // 地址同步完成，发送HintScan事件通知Scanner检查状态
                // 只有在数据库事实已形成后发送
                if let Some(event_tx) = ctx.get_expand_event_tx().await {
                    use crate::infrastructure::expand_address::event::ExpandEvent;
                    if let Err(err) = event_tx.send(ExpandEvent::HintScan).await {
                        tracing::warn!(
                            error = %err,
                            uid = %req.uid,
                            chain_code = %req.chain_code,
                            "failed to send HintScan event"
                        );
                    } else {
                        tracing::debug!(
                            "Sent HintScan event for uid={}, chain_code={}",
                            req.uid,
                            req.chain_code
                        );
                    }
                }

                tracing::debug!("地址同步完成: uid={}, chain_code={}", req.uid, req.chain_code);
                tracing::debug!(
                    "[PERF] QUERY_ADDRESS_LIST: processed pagination in {:?}, uid={}, chain_code={}",
                    start_update_progress.elapsed(),
                    req.uid,
                    req.chain_code
                );
                tracing::info!(
                    metric = "query_address_list_page",
                    uid = %req.uid,
                    chain_code = %req.chain_code,
                    page = res.number,
                    recovered_count = need_recover.len(),
                    done = done,
                    total_elements = res.total_elements,
                    last = res.last,
                    backend_ms = backend_query_elapsed.as_secs_f64() * 1000.0,
                    batch_ms = batch_process_elapsed.as_secs_f64() * 1000.0,
                    total_ms = start_total.elapsed().as_secs_f64() * 1000.0,
                    "address-list page completed"
                );
            }
            endpoint::api_wallet::QUERY_ASSET_LIST => {
                let req = wallet_utils::serde_func::serde_from_value::<AssetListReq>(body.clone())?;
                crate::infrastructure::api_wallet_assets_sync::query_and_upsert_assets(
                    ctx, &api_pool, backend, &req,
                )
                .await?;
            }
            _ => {
                // 未知的 endpoint
                tracing::warn!("unknown endpoint: {}", endpoint);
                Err(crate::error::service::ServiceError::System(
                    crate::error::system::SystemError::BackendEndpointNotFound,
                ))?;
            }
        }

        Ok(())
    }
}
