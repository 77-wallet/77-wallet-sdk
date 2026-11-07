use wallet_database::entities::task_queue::{KnownTaskName, TaskName};

use crate::{
    infrastructure::task_queue::{
        mqtt_api::ApiMqttStruct,
        task::{TaskTrait, task_type::TaskType},
    },
    messaging::mqtt::topics::{self},
};

pub(crate) enum MqttTask {
    OrderMultiSignAccept(topics::OrderMultiSignAccept),
    OrderMultiSignAcceptCompleteMsg(topics::OrderMultiSignAcceptCompleteMsg),
    OrderMultiSignServiceComplete(topics::OrderMultiSignServiceComplete),
    OrderMultiSignCreated(topics::OrderMultiSignCreated),
    OrderAllConfirmed(topics::OrderAllConfirmed),
    OrderMultiSignCancel(topics::OrderMultiSignCancel),
    MultiSignTransAccept(topics::MultiSignTransAccept),
    MultiSignTransCancel(topics::MultiSignTransCancel),
    MultiSignTransAcceptCompleteMsg(topics::MultiSignTransAcceptCompleteMsg),
    MultiSignTransExecute(topics::MultiSignTransExecute),
    AcctChange(topics::AcctChange),
    ApiWalletAcctChange(topics::api_wallet::acct_change::ApiWalletAcctChange),
    AwmCfgChange(topics::api_wallet::cfg_change::AwmCfgChange),
    AwmAddressInsufficient(topics::api_wallet::address_insufficient::AwmAddressInsufficient),
    BulletinMsg(topics::BulletinMsg),
    PermissionAccept(topics::PermissionAccept),
    CleanPermission(topics::CleanPermission),

    ApiMqttStruct(ApiMqttStruct),
}

#[async_trait::async_trait]
impl TaskTrait for MqttTask {
    fn get_name(&self) -> TaskName {
        match self {
            MqttTask::OrderMultiSignAccept(_) => {
                TaskName::Known(KnownTaskName::OrderMultiSignAccept)
            }
            MqttTask::OrderMultiSignAcceptCompleteMsg(_) => {
                TaskName::Known(KnownTaskName::OrderMultiSignAcceptCompleteMsg)
            }
            MqttTask::OrderMultiSignServiceComplete(_) => {
                TaskName::Known(KnownTaskName::OrderMultiSignServiceComplete)
            }
            MqttTask::OrderMultiSignCreated(_) => {
                TaskName::Known(KnownTaskName::OrderMultiSignCreated)
            }
            MqttTask::OrderMultiSignCancel(_) => {
                TaskName::Known(KnownTaskName::OrderMultiSignCancel)
            }
            MqttTask::MultiSignTransAccept(_) => {
                TaskName::Known(KnownTaskName::MultiSignTransAccept)
            }
            MqttTask::MultiSignTransCancel(_) => {
                TaskName::Known(KnownTaskName::MultiSignTransCancel)
            }
            MqttTask::MultiSignTransAcceptCompleteMsg(_) => {
                TaskName::Known(KnownTaskName::MultiSignTransAcceptCompleteMsg)
            }
            MqttTask::AcctChange(_) => TaskName::Known(KnownTaskName::AcctChange),
            MqttTask::BulletinMsg(_) => TaskName::Known(KnownTaskName::BulletinMsg),

            MqttTask::PermissionAccept(_) => TaskName::Known(KnownTaskName::PermissionAccept),
            MqttTask::MultiSignTransExecute(_) => {
                TaskName::Known(KnownTaskName::MultiSignTransExecute)
            }
            MqttTask::CleanPermission(_) => TaskName::Known(KnownTaskName::CleanPermission),
            MqttTask::OrderAllConfirmed(_) => TaskName::Known(KnownTaskName::OrderAllConfirmed),
            // api wallet
            MqttTask::ApiMqttStruct(api_mqtt_struct) => api_mqtt_struct.get_name(),
            MqttTask::ApiWalletAcctChange(_) => TaskName::Known(KnownTaskName::ApiWalletAcctChange),
            MqttTask::AwmCfgChange(_) => TaskName::Known(KnownTaskName::AwmCfgChange),
            MqttTask::AwmAddressInsufficient(_) => {
                TaskName::Known(KnownTaskName::AwmAddressInsufficient)
            }
        }
    }
    fn get_type(&self) -> TaskType {
        TaskType::Mqtt
    }
    fn get_body(&self) -> Result<Option<String>, crate::error::service::ServiceError> {
        let res = match self {
            MqttTask::OrderMultiSignAccept(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::OrderMultiSignAcceptCompleteMsg(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::OrderMultiSignServiceComplete(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::OrderMultiSignCancel(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::MultiSignTransAccept(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::MultiSignTransCancel(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::MultiSignTransAcceptCompleteMsg(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::OrderMultiSignCreated(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::AcctChange(req) => Some(wallet_utils::serde_func::serde_to_string(req)?),
            MqttTask::BulletinMsg(req) => Some(wallet_utils::serde_func::serde_to_string(req)?),
            MqttTask::PermissionAccept(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::MultiSignTransExecute(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::OrderAllConfirmed(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::CleanPermission(req) => Some(wallet_utils::serde_func::serde_to_string(req)?),
            MqttTask::ApiMqttStruct(api_mqtt_struct) => api_mqtt_struct.get_body()?,
            MqttTask::ApiWalletAcctChange(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
            MqttTask::AwmCfgChange(req) => Some(wallet_utils::serde_func::serde_to_string(req)?),
            MqttTask::AwmAddressInsufficient(req) => {
                Some(wallet_utils::serde_func::serde_to_string(req)?)
            }
        };
        Ok(res)
    }

    async fn execute(
        &self,
        id: &str,
        ctx: &'static crate::context::Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        match self {
            MqttTask::OrderMultiSignAccept(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::OrderMultiSignAcceptCompleteMsg(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::OrderMultiSignServiceComplete(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::OrderMultiSignCreated(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::OrderMultiSignCancel(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::MultiSignTransAccept(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::MultiSignTransCancel(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::MultiSignTransAcceptCompleteMsg(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::AcctChange(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::BulletinMsg(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::PermissionAccept(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::MultiSignTransExecute(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::CleanPermission(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::OrderAllConfirmed(data) => data.exec_with_ctx(id, ctx).await?,
            MqttTask::ApiMqttStruct(api_mqtt_struct) => api_mqtt_struct.execute(id, ctx).await?,
            MqttTask::ApiWalletAcctChange(data) => data.exec(ctx, id).await?,
            MqttTask::AwmCfgChange(data) => data.exec(ctx, id).await?,
            MqttTask::AwmAddressInsufficient(data) => data.exec(ctx, id).await?,
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
