use wallet_database::entities::{
    multisig_queue::QueueTaskEntity,
    task_queue::{KnownTaskName, TaskName},
};
use wallet_transport_backend::request::TokenQueryPriceReq;

use crate::{
    domain::{
        api_wallet::account::{ApiAccountDomain, CreateAccountDeferredData},
        multisig::{MultisigDomain, MultisigQueueDomain},
        permission::PermissionDomain,
    },
    error::service::ServiceError,
    infrastructure::task_queue::task::{TaskTrait, task_type::TaskType},
    messaging::notify::{FrontendNotifyEvent, event::NotifyEvent},
    service::coin::CoinService,
};

#[async_trait::async_trait]
impl TaskTrait for CommonTask {
    fn get_name(&self) -> TaskName {
        match self {
            CommonTask::CheckApiAddressBalance(_) => {
                TaskName::Known(KnownTaskName::CheckApiAddressBalance)
            }
            CommonTask::QueryCoinPrice(_) => TaskName::Known(KnownTaskName::QueryCoinPrice),
            CommonTask::QueryQueueResult(_) => TaskName::Known(KnownTaskName::QueryQueueResult),
            CommonTask::RecoverMultisigAccountData(_) => {
                TaskName::Known(KnownTaskName::RecoverMultisigAccountData)
            }
            CommonTask::CreateApiAccountDeferred(_) => {
                TaskName::Known(KnownTaskName::CreateApiAccountDeferred)
            }
        }
    }
    fn get_type(&self) -> TaskType {
        TaskType::Common
    }

    fn get_body(&self) -> Result<Option<String>, ServiceError> {
        let res = match self {
            CommonTask::CheckApiAddressBalance(data) => {
                Some(wallet_utils::serde_func::serde_to_string(data)?)
            }
            CommonTask::QueryCoinPrice(query_coin_price) => {
                Some(wallet_utils::serde_func::serde_to_string(query_coin_price)?)
            }
            CommonTask::QueryQueueResult(queue) => {
                Some(wallet_utils::serde_func::serde_to_string(queue)?)
            }
            CommonTask::RecoverMultisigAccountData(recover_data) => {
                Some(wallet_utils::serde_func::serde_to_string(recover_data)?)
            }
            CommonTask::CreateApiAccountDeferred(data) => {
                Some(wallet_utils::serde_func::serde_to_string(data)?)
            }
        };
        Ok(res)
    }

    async fn execute(
        &self,
        _id: &str,
        ctx: &'static crate::context::Context,
    ) -> Result<(), ServiceError> {
        match self {
            CommonTask::CheckApiAddressBalance(data) => {
                crate::infrastructure::system_ready::wait_system_ready().await;
                data.execute(ctx).await?;
            }
            CommonTask::QueryCoinPrice(data) => {
                CoinService::new(ctx).query_token_price(data).await?;
            }
            CommonTask::QueryQueueResult(data) => {
                MultisigQueueDomain::sync_queue_status_with_ctx(ctx, &data.id).await?
            }
            CommonTask::RecoverMultisigAccountData(body) => {
                MultisigDomain::recover_uid_multisig_data_with_ctx(ctx, &body.uid, None).await?;
                if let Some(address) = &body.tron_address {
                    PermissionDomain::recover_permission(ctx, vec![address.clone()]).await?;
                }

                MultisigQueueDomain::recover_all_queue_data_with_ctx(ctx, &body.uid).await?;

                // 恢复完成后发送事件给前端
                let data = NotifyEvent::RecoverComplete;
                FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
            }
            CommonTask::CreateApiAccountDeferred(data) => {
                ctx.recovery_coordinator().wait_for_turn(data.recovery_uid()).await;
                ApiAccountDomain::create_api_account_deferred_with_ctx(ctx, data.clone()).await?;
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub(crate) enum CommonTask {
    CheckApiAddressBalance(crate::application::api_wallet_balance_check::ApiAddressBalanceCheck),
    QueryCoinPrice(TokenQueryPriceReq),
    QueryQueueResult(QueueTaskEntity),
    RecoverMultisigAccountData(RecoverDataBody),
    CreateApiAccountDeferred(CreateAccountDeferredData),
    // SyncNodesAndLinkToChains(Vec<NodeEntity>),
    // EncryptPrivateKey(EncryptPrivateKeyTask),
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct RecoverDataBody {
    pub uid: String,
    // 波场恢复权限使用的地址
    pub tron_address: Option<String>,
}

// #[derive(Debug, serde::Serialize, serde::Deserialize)]
// pub struct EncryptPrivateKeyTask {
//     pub address: String,
//     pub address_type: AddressType,
//     pub account_index: u32,
//     pub wallet_address: String,
//     pub chain_code: String,
//     pub api_wallet_type: ApiWalletType,
// }

// impl EncryptPrivateKeyTask {
//     pub fn new(
//         address: &str,
//         address_type: AddressType,
//         account_index: u32,
//         wallet_address: &str,
//         chain_code: &str,
//         api_wallet_type: ApiWalletType,
//     ) -> Self {
//         Self {
//             address: address.to_string(),
//             address_type,
//             account_index,
//             wallet_address: wallet_address.to_string(),
//             chain_code: chain_code.to_string(),
//             api_wallet_type,
//         }
//     }
// }

impl RecoverDataBody {
    pub fn new(uid: &str) -> Self {
        Self { uid: uid.to_string(), tron_address: None }
    }
}
