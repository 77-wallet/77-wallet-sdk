use wallet_database::repositories::device::DeviceRepo;

use crate::{
    domain::{self, chain::ChainDomain, node::NodeDomain},
    infrastructure::{
        chain_node::chain_node_ensurer::ChainNodeEnsurer,
        task_queue::{
            backend::{BackendApiTask, BackendApiTaskData},
            initialization::InitializationTask,
            task::Tasks,
        },
    },
};

pub(crate) async fn init_some_data(
    ctx: &'static crate::context::Context,
) -> Result<(), crate::error::service::ServiceError> {
    crate::domain::app::config::ConfigDomain::init_url(ctx).await?;
    let core_pool = ctx.core_pool()?;
    let api_wallet_pool = ctx.api_wallet_pool()?;
    // // 1. 先初始化链兜底
    NodeDomain::init_load_default_nodes_with_ctx(ctx).await?;
    ChainDomain::init_chain_info_with_ctx(ctx).await?;
    let ensurer = ChainNodeEnsurer::new(core_pool.clone(), api_wallet_pool.clone());
    ensurer.ensure_all().await?;

    // // if !ApiChainDomain::sync_chains().await?.is_empty() {
    // //     let unlock_token = ApiWalletDomain::get_wallet_unlock_token().await?;
    // //     ApiChainDomain::sync_wallet_chain_data(&password).await?;
    // // }
    //
    // // 2. 初始化节点
    // let repo = RepositoryFactory::repo(pool.clone());
    // let mut node_service = NodeService::new(repo);
    // node_service.init_node_info().await?;

    // let asset_calc_actor_manager =
    // asset_calc_actor_manager.init_account_cache().await?;
    crate::domain::coin::CoinDomain::init_coins(&core_pool).await?;
    crate::domain::coin::CoinDomain::sync_default_coins_by_bound_nodes_with_ctx(ctx).await?;

    let token_query_rates_req = BackendApiTaskData::new(
        wallet_transport_backend::consts::endpoint::TOKEN_QUERY_RATES,
        &(),
    )?;

    let set_official_website_req = BackendApiTaskData::new(
        wallet_transport_backend::consts::endpoint::SYS_CONFIG_FIND_CONFIG_BY_KEY,
        &wallet_transport_backend::request::FindConfigByKey::new("OFFICIAL:WEBSITE"),
    )?;

    let set_app_install_download_req = BackendApiTaskData::new(
        wallet_transport_backend::consts::endpoint::APP_INSTALL_DOWNLOAD,
        &(),
    )?;

    // let mqtt_init_req =
    //     BackendApiTaskData::new(wallet_transport_backend::consts::endpoint::MQTT_INIT, &())?;

    let sn = ctx.get_sn();
    let fetch_min_value_config_req = min_value_config_task(sn)?;

    let device = DeviceRepo::get_device_info(core_pool, sn).await?;

    let mut tasks = Tasks::new().push(InitializationTask::InitMqtt);
    if let Some(device) = device
        && device.language_init == 1
    {
        tasks = tasks
            .push(domain::app::DeviceDomain::language_init(&device, "CHINESE_SIMPLIFIED").await?);
    } else {
        tasks = tasks.push(InitializationTask::PullAnnouncement);
    }
    tasks
        .push(InitializationTask::PullHotCoins)
        .push(InitializationTask::SetBlockBrowserUrl)
        .push(InitializationTask::SetFiat)
        .push(InitializationTask::RecoverQueueData)
        .push(BackendApiTask::BackendApi(token_query_rates_req))
        .push(BackendApiTask::BackendApi(set_official_website_req))
        .push(BackendApiTask::BackendApi(set_app_install_download_req))
        .push(BackendApiTask::BackendApi(fetch_min_value_config_req))
        .send_with_ctx(ctx)
        .await?;

    Ok(())
}

fn min_value_config_task(
    sn: &str,
) -> Result<BackendApiTaskData, crate::error::service::ServiceError> {
    BackendApiTaskData::new(
        wallet_transport_backend::consts::endpoint::DEVICE_QUERY_SEND_MSG_AMOUNT,
        &serde_json::json!({ "sn": sn }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_value_config_sync_uses_persistent_backend_task_endpoint() {
        // Arrange
        let sn = "device-sn";

        // Act
        let task = min_value_config_task(sn).expect("build min-value config task");

        // Assert
        assert_eq!(
            task.endpoint,
            wallet_transport_backend::consts::endpoint::DEVICE_QUERY_SEND_MSG_AMOUNT
        );
        assert_eq!(task.body, serde_json::json!({ "sn": sn }));
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub(crate) sn: String,
    pub(crate) client_id: String,
}
impl DeviceInfo {
    pub fn new(sn: &str, client_id: &str) -> Self {
        Self { sn: sn.to_owned(), client_id: client_id.to_owned() }
    }
}

#[derive(Debug, Clone)]
pub struct RpcToken {
    pub token: String,
    pub instance: tokio::time::Instant,
}

impl Default for RpcToken {
    fn default() -> Self {
        Self { token: String::new(), instance: tokio::time::Instant::now() }
    }
}
