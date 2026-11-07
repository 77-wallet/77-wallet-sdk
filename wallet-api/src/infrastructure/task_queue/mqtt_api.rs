use crate::{
    error::service::ServiceError,
    infrastructure::task_queue::task::{TaskTrait, task_type::TaskType},
    messaging::mqtt::topics::api_wallet::{
        cmd::{
            address_allock::AwmCmdAddrExpandMsg, dev_change::AwmCmdDevChangeMsg,
            fee_res::AwmCmdFeeResMsg, unbind_uid::AwmCmdUidUnbindMsg,
            wallet_activation::AwmCmdActiveMsg,
        },
        trans::AwmOrderTransMsg,
        trans_result::AwmOrderTransResMsg,
    },
};
use wallet_database::entities::task_queue::{KnownTaskName, TaskName};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) enum EventType {
    #[serde(rename = "1")]
    AwmOrderTrans,
    #[serde(rename = "2")]
    AwmOrderTransRes,
    #[serde(rename = "3")]
    AwmCmdAddrExpand,
    #[serde(rename = "4")]
    AwmCmdUidUnbind,
    #[serde(rename = "5")]
    AwmCmdActive,
    #[serde(rename = "6")]
    AwmCmdFeeRes,
    #[serde(rename = "7")]
    AwmCmdDevChange,
    #[serde(rename = "8")]
    AwmCmdRscRes,
    // AddressUse,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiMqttStruct {
    pub(crate) event_no: String,
    /// 1交易事件 / 2交易最终结果 / 3地址扩容 / 4平台解绑 / 5激活钱包
    /// / 6交易手续费结果 / 7设备变更 / 8资源结果
    pub(crate) event_type: EventType,
    pub(crate) data: serde_json::Value,
    pub(crate) time: u64,
    pub(crate) sign: Option<String>,
    pub(crate) secret: Option<String>,
}

#[async_trait::async_trait]
impl TaskTrait for ApiMqttStruct {
    fn get_name(&self) -> TaskName {
        match self.event_type {
            EventType::AwmOrderTrans => TaskName::Known(KnownTaskName::AwmOrderTrans),
            EventType::AwmOrderTransRes => TaskName::Known(KnownTaskName::AwmOrderTransRes),
            EventType::AwmCmdAddrExpand => TaskName::Known(KnownTaskName::AwmCmdAddrExpand),
            EventType::AwmCmdUidUnbind => TaskName::Known(KnownTaskName::AwmCmdUidUnbind),
            EventType::AwmCmdFeeRes => TaskName::Known(KnownTaskName::AwmCmdFeeRes),
            EventType::AwmCmdActive => TaskName::Known(KnownTaskName::AwmCmdActive),
            EventType::AwmCmdDevChange => TaskName::Known(KnownTaskName::AwmCmdDevChange),
            // 资源结果复用交易结果任务队列通道，实际业务分流由 tradeType 完成。
            EventType::AwmCmdRscRes => TaskName::Known(KnownTaskName::AwmOrderTransRes),
        }
    }

    fn get_type(&self) -> TaskType {
        TaskType::Mqtt
    }

    fn get_body(&self) -> Result<Option<String>, ServiceError> {
        Ok(Some(wallet_utils::serde_func::serde_to_string(self)?))
    }

    async fn execute(
        &self,
        id: &str,
        ctx: &'static crate::context::Context,
    ) -> Result<(), ServiceError> {
        match &self.event_type {
            EventType::AwmOrderTrans => {
                let data: AwmOrderTransMsg =
                    wallet_utils::serde_func::serde_from_value(self.data.clone())?;
                data.exec(ctx, id).await?
            }
            EventType::AwmOrderTransRes => {
                let data: AwmOrderTransResMsg =
                    wallet_utils::serde_func::serde_from_value(self.data.clone())?;
                data.exec(ctx, id).await?
            }
            EventType::AwmCmdRscRes => {
                let data: AwmOrderTransResMsg =
                    wallet_utils::serde_func::serde_from_value(self.data.clone())?;
                data.exec_resource_result(ctx, id).await?
            }
            EventType::AwmCmdAddrExpand => {
                let data: AwmCmdAddrExpandMsg =
                    wallet_utils::serde_func::serde_from_value(self.data.clone())?;
                data.exec(ctx, id).await?
            }
            EventType::AwmCmdUidUnbind => {
                let data: AwmCmdUidUnbindMsg =
                    wallet_utils::serde_func::serde_from_value(self.data.clone())?;
                data.exec(ctx, id).await?
            }
            EventType::AwmCmdFeeRes => {
                let data: AwmCmdFeeResMsg =
                    wallet_utils::serde_func::serde_from_value(self.data.clone())?;
                data.exec(ctx, id).await?
            }
            EventType::AwmCmdActive => {
                let data: AwmCmdActiveMsg =
                    wallet_utils::serde_func::serde_from_value(self.data.clone())?;
                data.exec(ctx, id).await?
            }
            EventType::AwmCmdDevChange => {
                let data: AwmCmdDevChangeMsg =
                    wallet_utils::serde_func::serde_from_value(self.data.clone())?;
                data.exec(ctx, id).await?
            }
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiMqttStruct, EventType};
    use crate::infrastructure::task_queue::task::TaskTrait;
    use wallet_database::entities::task_queue::{KnownTaskName, TaskName};

    #[test]
    fn awm_cmd_rsc_res_event_type_uses_resource_result_route() {
        let payload = r#"{
            "eventNo":"2056995714306813952",
            "eventType":"8",
            "data":{"tradeNo":"C2056937784291237888","tradeType":"5","status":true,"failType":0,"uid":"uid"},
            "time":1779260970
        }"#;

        let api_mqtt: ApiMqttStruct = serde_json::from_str(payload).unwrap();
        assert!(matches!(api_mqtt.event_type, EventType::AwmCmdRscRes));
        assert!(matches!(api_mqtt.get_name(), TaskName::Known(KnownTaskName::AwmOrderTransRes)));
    }
}
