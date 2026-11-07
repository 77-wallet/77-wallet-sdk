use crate::{
    domain::{
        api_wallet::coin::ApiCoinDomain,
        app::{config::ConfigDomain, mqtt::MqttDomain},
        multisig::MultisigQueueDomain,
    },
    infrastructure::task_queue::task::{TaskTrait, task_type::TaskType},
    service::{announcement::AnnouncementService, coin::CoinService},
};
use wallet_database::entities::task_queue::{KnownTaskName, TaskName};

// 先定义枚举
pub(crate) enum InitializationTask {
    PullAnnouncement,
    PullHotCoins,
    PullApiWalletCoins,
    // ProcessUnconfirmMsg,
    SetBlockBrowserUrl,
    SetFiat,
    RecoverQueueData,
    InitMqtt,
}

// 然后实现Trait
#[async_trait::async_trait]
impl TaskTrait for InitializationTask {
    fn get_name(&self) -> TaskName {
        match self {
            InitializationTask::PullAnnouncement => {
                TaskName::Known(KnownTaskName::PullAnnouncement)
            }
            InitializationTask::PullHotCoins => TaskName::Known(KnownTaskName::PullHotCoins),
            InitializationTask::PullApiWalletCoins => {
                TaskName::Known(KnownTaskName::PullApiWalletCoins)
            }
            InitializationTask::SetBlockBrowserUrl => {
                TaskName::Known(KnownTaskName::SetBlockBrowserUrl)
            }
            InitializationTask::SetFiat => TaskName::Known(KnownTaskName::SetFiat),
            InitializationTask::RecoverQueueData => {
                TaskName::Known(KnownTaskName::RecoverQueueData)
            }
            InitializationTask::InitMqtt => TaskName::Known(KnownTaskName::InitMqtt),
        }
    }
    fn get_type(&self) -> TaskType {
        TaskType::Initialization
    }
    fn get_body(&self) -> Result<Option<String>, crate::error::service::ServiceError> {
        Ok(None)
    }

    async fn execute(
        &self,
        _id: &str,
        ctx: &'static crate::context::Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        match self {
            InitializationTask::PullAnnouncement => {
                let announcement_service = AnnouncementService::new(ctx);
                let res = announcement_service.pull_announcement().await;

                res?;
            }
            InitializationTask::PullHotCoins => {
                let coin_service = CoinService::new(ctx);
                coin_service.pull_hot_coins().await?;

                coin_service.init_token_price().await?;
            }
            InitializationTask::PullApiWalletCoins => {
                // 从后端获取最新的币数据
                let coins = ApiCoinDomain::pull_api_coins(ctx).await?;

                // 只有当成功获取到新数据时，才更新币价和初始化
                if !coins.is_empty() {
                    ApiCoinDomain::init_token_price(ctx).await?;

                    // let list = ApiCoinRepo::coin_list(&pool).await?;

                    // // 准备批量初始化币价的数据
                    // let mut coins_to_initialize = Vec::with_capacity(list.len());
                    // for coin in list.iter() {
                    //     if let Ok(price_real) = wallet_utils::unit::string_to_f64(&coin.price) {
                    //         coins_to_initialize.push(crate::infrastructure::asset_calc::actor_model::CoinInitializationData {
                    //             symbol: coin.symbol.clone(),
                    //             chain_code: coin.chain_code.clone(),
                    //             name: coin.name.clone(),
                    //             token_address: coin.token_address.clone(),
                    //             price_real,
                    //             decimals: coin.decimals,
                    //         });
                    //     }
                    // }

                    // // 批量初始化币价
                    // if !coins_to_initialize.is_empty() {
                    //     let asset_calc_actor_manager = crate::context::CONTEXT
                    //         .get()
                    //         .unwrap()
                    //         .get_global_asset_calc_actor_manager()
                    //         .await?;
                    //     asset_calc_actor_manager
                    //         .batch_initialize_prices(coins_to_initialize)
                    //         .await?;
                    // }

                    // 添加支持的币种
                    ApiCoinDomain::add_supported_coin(ctx, coins).await?;
                } else {
                    tracing::warn!("No new coin data received from backend API");
                }
            }
            InitializationTask::SetBlockBrowserUrl => {
                let mut app_service = crate::service::app::AppService::new(ctx);
                app_service.set_block_browser_url().await?;
            }
            InitializationTask::SetFiat => {
                ConfigDomain::init_currency(ctx).await?;
            }
            InitializationTask::RecoverQueueData => {
                MultisigQueueDomain::recover_all_uid_queue_data_with_ctx(ctx).await?;
            }
            InitializationTask::InitMqtt => {
                tracing::debug!("init mqtt start");
                MqttDomain::init_mqtt(ctx).await?;
                tracing::debug!("init mqtt end");
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
