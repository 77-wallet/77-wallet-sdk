use super::{
    Message,
    message::BizType,
    topics::{
        AcctChange, BulletinMsg, ChainChange, CleanPermission, MultiSignTransAccept,
        MultiSignTransAcceptCompleteMsg, MultiSignTransCancel, MultiSignTransExecute,
        OrderAllConfirmed, OrderMultiSignAccept, OrderMultiSignAcceptCompleteMsg,
        OrderMultiSignCancel, OrderMultiSignCreated, OrderMultiSignServiceComplete,
        PermissionAccept, RpcChange, Topic,
    },
};
use crate::{
    context::Context,
    error::service::ServiceError,
    infrastructure::task_queue::{
        MqttTask,
        mqtt_api::{ApiMqttStruct, EventType},
        task::{Tasks, dispatch_task_entities_with_ctx},
    },
    messaging::{
        mqtt::topics::{
            OutgoingPayload,
            api_wallet::{
                acct_change::ApiWalletAcctChange,
                address_insufficient::AwmAddressInsufficient,
                cmd::{
                    address_allock::AwmCmdAddrExpandMsg, dev_change::AwmCmdDevChangeMsg,
                    fee_res::AwmCmdFeeResMsg, unbind_uid::AwmCmdUidUnbindMsg,
                    wallet_activation::AwmCmdActiveMsg,
                },
                trans::AwmOrderTransMsg,
                trans_result::AwmOrderTransResMsg,
            },
        },
        notify::{FrontendNotifyEvent, event::NotifyEvent},
    },
    service::{app::AppService, device::DeviceService},
};
use rumqttc::v5::mqttbytes::v5::Publish;
use wallet_database::{
    entities::task_queue::WalletType,
    repositories::{task_queue::TaskQueueRepo, wallet::WalletRepo},
};
use wallet_transport_backend::response::api_response::{
    ApiBackendData, ApiBackendDataBody, ApiBackendResponse,
};
use wallet_utils::serde_func;

pub async fn exec_incoming_publish(
    ctx: &'static Context,
    publish: &Publish,
) -> Result<(), anyhow::Error> {
    let pool = ctx.task_pool()?;
    let topic = Topic::from_bytes_v3(publish.topic.to_vec())?;

    match topic.topic {
        Topic::Switch => {}
        // #[cfg(feature = "token")]
        crate::messaging::mqtt::topics::Topic::Token => {
            let payload: crate::messaging::mqtt::topics::TokenPriceChange =
                serde_json::from_slice(&publish.payload)?;
            payload.exec(ctx).await?;
        }
        Topic::RpcChange => {
            let payload: RpcChange = serde_json::from_slice(&publish.payload)?;

            if let Err(e) = FrontendNotifyEvent::send_debug_with_ctx(ctx, &payload).await {
                tracing::error!("[exec_incoming_publish] send debug error: {e}");
            };

            payload.exec(ctx).await?;
        }
        Topic::ChainChange => {
            let payload: ChainChange = serde_json::from_slice(&publish.payload)?;
            payload.exec(ctx).await?;
        }
        Topic::Order
        | Topic::Common
        | Topic::BulletinInfo
        | Topic::WalletMerchantCmd
        | Topic::MerchantTrans
        // | Topic::WalletMerchantTrans 
        => {
            let payload: Message = serde_json::from_slice(&publish.payload)?;
            if let Err(e) = FrontendNotifyEvent::send_debug_with_ctx(ctx, &payload).await {
                tracing::error!("[exec_incoming_publish] send debug error: {e}");
            };

            // 目前任务执行完后，会自动发送 send_msg_confirm，所以这里不需要再发送
            // 是否有相同的队列
            match TaskQueueRepo::task_detail(&pool, &payload.msg_id).await? {
                None => {
                    let msg_id = payload.msg_id.clone();
                    let biz_type = payload.biz_type.clone();
                    let event = serde_func::serde_to_string(&payload.biz_type)?;
                    if let Err(e) = exec_payload(payload, ctx).await {
                        tracing::error!(
                            msg_id = %msg_id,
                            biz_type = ?biz_type,
                            "exec_payload error: {}",
                            e
                        );
                        if let Err(e) =
                            FrontendNotifyEvent::send_error_with_ctx(ctx, &event, e.to_string()).await
                        {
                            tracing::error!("send_error error: {}", e);
                        }
                    };
                }
                Some(existing) => {
                    // status: 0 pending, 1 running, 2 success, 3 failed, 4 hang up
                    // 如果 task 已存在但未完成，说明：
                    // - 可能上一次 dispatch 未成功（handles 尚未 ready / channel 满）
                    // - 或 task 执行失败/挂起，backend 会重投 MQTT
                    // 这里不应静默丢弃，否则会导致永远不 ACK。
                    //
                    // ⚠️ 安全约束：
                    // - 仅对 pending(0) 做“补发 dispatch”，避免对已执行过但未成功落库的任务造成额外重复执行风险
                    if existing.status == 0 {
                        tracing::warn!(
                            msg_id = %payload.msg_id,
                            biz_type = ?payload.biz_type,
                            status = %existing.status,
                            "MQTT msg_id already persisted but task not completed; redispatching existing task"
                        );
                        if let Err(e) = dispatch_task_entities_with_ctx(ctx, vec![existing]).await {
                            tracing::error!(
                                msg_id = %payload.msg_id,
                                biz_type = ?payload.biz_type,
                                "Redispatch existing task failed: {}",
                                e
                            );
                        }
                    } else if existing.status != 2 {
                        tracing::warn!(
                            msg_id = %payload.msg_id,
                            biz_type = ?payload.biz_type,
                            status = %existing.status,
                            "MQTT msg_id already persisted but task not completed; skip redispatch to avoid duplicate side effects"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

pub(crate) async fn exec_payload(
    payload: Message,
    ctx: &'static Context,
) -> Result<(), crate::error::service::ServiceError> {
    match payload.biz_type {
        BizType::OrderMultiSignAccept => {
            exec_task::<OrderMultiSignAccept, _, _>(&payload, ctx, MqttTask::OrderMultiSignAccept)
                .await?
        }
        BizType::OrderMultiSignAcceptCompleteMsg => {
            exec_task::<OrderMultiSignAcceptCompleteMsg, _, _>(
                &payload,
                ctx,
                MqttTask::OrderMultiSignAcceptCompleteMsg,
            )
            .await?
        }
        BizType::OrderMultiSignServiceComplete => {
            exec_task::<OrderMultiSignServiceComplete, _, _>(
                &payload,
                ctx,
                MqttTask::OrderMultiSignServiceComplete,
            )
            .await?
        }
        BizType::OrderMultiSignCancel => {
            exec_task::<OrderMultiSignCancel, _, _>(&payload, ctx, MqttTask::OrderMultiSignCancel)
                .await?
        }
        BizType::MultiSignTransAccept => {
            exec_task::<MultiSignTransAccept, _, _>(&payload, ctx, MqttTask::MultiSignTransAccept)
                .await?
        }
        BizType::MultiSignTransAcceptCompleteMsg => {
            exec_task::<MultiSignTransAcceptCompleteMsg, _, _>(
                &payload,
                ctx,
                MqttTask::MultiSignTransAcceptCompleteMsg,
            )
            .await?
        }
        BizType::AcctChange => match account_change_route(payload.wallet_type.as_ref())? {
            AccountChangeRoute::NormalWallet => {
                exec_task::<AcctChange, _, _>(&payload, ctx, MqttTask::AcctChange).await?
            }
            AccountChangeRoute::ApiWallet => {
                exec_task::<ApiWalletAcctChange, _, _>(&payload, ctx, MqttTask::ApiWalletAcctChange)
                    .await?
            }
            AccountChangeRoute::Missing => (),
        },
        BizType::OrderMultiSignCreated => {
            exec_task::<OrderMultiSignCreated, _, _>(&payload, ctx, MqttTask::OrderMultiSignCreated)
                .await?
        }
        BizType::BulletinMsg => {
            exec_task::<BulletinMsg, _, _>(&payload, ctx, MqttTask::BulletinMsg).await?
        }
        BizType::MultiSignTransCancel => {
            exec_task::<MultiSignTransCancel, _, _>(&payload, ctx, MqttTask::MultiSignTransCancel)
                .await?
        }
        BizType::PermissionAccept => {
            exec_task::<PermissionAccept, _, _>(&payload, ctx, MqttTask::PermissionAccept).await?
        }
        BizType::MultiSignTransExecute => {
            exec_task::<MultiSignTransExecute, _, _>(&payload, ctx, MqttTask::MultiSignTransExecute)
                .await?
        }
        BizType::OrderMultiSignAllMemberAccepted => {
            exec_task::<OrderAllConfirmed, _, _>(&payload, ctx, MqttTask::OrderAllConfirmed).await?
        }
        BizType::CleanPermission => {
            exec_task::<CleanPermission, _, _>(&payload, ctx, MqttTask::CleanPermission).await?
        }
        BizType::AwmCfgChange => {
            let change =
                super::topics::api_wallet::cfg_change::AwmCfgChange::from_message(&payload)?;
            Tasks::new()
                .push_with_id(&payload.msg_id, MqttTask::AwmCfgChange(change))
                .send_with_ctx(ctx)
                .await?;
        }
        BizType::AwmAddressInsufficient => {
            let alert = AwmAddressInsufficient::from_message(&payload)?;
            Tasks::new()
                .push_with_id(&payload.msg_id, MqttTask::AwmAddressInsufficient(alert))
                .send_with_ctx(ctx)
                .await?;
        }
        // api wallet
        BizType::AwmOrderTrans
        | BizType::AwmOrderTransRes
        | BizType::AwmCmdRscRes
        | BizType::AwmCmdAddrExpand
        | BizType::AwmCmdFeeRes
        | BizType::AwmCmdActive
        | BizType::AwmCmdUidUnbind
        | BizType::AddressUse
        | BizType::AwmCmdDevChange => {
            let mut api_mqtt_st =
                serde_func::serde_from_value::<ApiMqttStruct>(payload.body.clone())?;
            if api_mqtt_st.sign.is_none() {
                return Err(ServiceError::Parameter("missing sign".to_string()));
            }
            if api_mqtt_st.secret.is_none() {
                return Err(ServiceError::Parameter("missing secret".to_string()));
            }
            // 验签
            let data: String = serde_func::serde_from_value(api_mqtt_st.data.clone())?;
            let res = ApiBackendResponse {
                success: true,
                code: 999,
                msg: Some("mqtt".to_string()),
                data: Some(ApiBackendData {
                    sign: api_mqtt_st.sign.clone().unwrap(),
                    body: ApiBackendDataBody { key: api_mqtt_st.secret.clone().unwrap(), data },
                }),
            };

            let data = exec_verify_api_mqtt_st(api_mqtt_st.event_type.clone(), &res).await?;
            api_mqtt_st.data = data;
            let mut payload = payload.clone();
            payload.body = serde_func::serde_to_value(api_mqtt_st)?;
            exec_task::<ApiMqttStruct, _, _>(&payload, ctx, MqttTask::ApiMqttStruct).await?
        }
        // 如果没有匹配到任何已知的 BizType，则返回错误
        biztype => {
            return Err(crate::error::service::ServiceError::System(
                crate::error::system::SystemError::MessageWrong(biztype, payload.body),
            ));
        }
    }

    Ok(())
}

async fn exec_task<T, F, R>(
    payload: &Message,
    ctx: &'static crate::context::Context,
    task_ctor: F,
) -> Result<(), crate::error::service::ServiceError>
where
    T: serde::de::DeserializeOwned,
    F: FnOnce(T) -> R,
    R: crate::infrastructure::task_queue::task::TaskTrait + 'static,
{
    let data = serde_func::serde_from_value::<T>(payload.body.clone())?;
    Tasks::new()
        .push_with_id(
            &payload.msg_id,
            task_ctor(data), // Task::Mqtt(Box::new())
        )
        .send_with_ctx(ctx)
        .await?;
    Ok(())
}

pub async fn exec_incoming_connack(
    context: &'static Context,
    client: &rumqttc::v5::AsyncClient,
    conn_ack: rumqttc::v5::mqttbytes::v5::ConnAck,
) -> Result<(), anyhow::Error> {
    let pool = context.core_pool()?;
    let device_service = DeviceService::new(context);
    let sn = context.get_sn();

    if conn_ack.code == rumqttc::v5::mqttbytes::v5::ConnectReturnCode::Success {
        AppService::new(context).mqtt_resubscribe().await?;
    }

    if let Some(wallet) = WalletRepo::wallet_latest(pool.clone()).await?
        && let Some(device) = &device_service.get_device_info(&sn).await?
        && let Some(app_id) = &device.app_id
    {
        let client_id = crate::domain::app::DeviceDomain::client_id_by_device(device)?;
        let body = OutgoingPayload::SwitchWallet {
            uid: wallet.uid,
            sn: device.sn.clone(),
            app_id: app_id.to_string(),
            device_type: device.device_type.clone(),
            client_id,
        };
        client
            .publish(Topic::Switch, rumqttc::v5::mqttbytes::QoS::AtLeastOnce, false, body.to_vec()?)
            .await?;
    }

    let data = NotifyEvent::MqttConnected;
    FrontendNotifyEvent::new(data).send_with_ctx(context).await?;
    Ok(())
}

async fn exec_verify_api_mqtt_st(
    ev: EventType,
    res: &ApiBackendResponse,
) -> Result<serde_json::Value, ServiceError> {
    match ev {
        EventType::AwmOrderTrans => {
            let data: Option<AwmOrderTransMsg> = res.process()?;
            if let Some(data) = data {
                return Ok(serde_func::serde_to_value(data)?);
            } else {
                return Err(ServiceError::Parameter("missing data".to_string()));
            }
        }
        EventType::AwmOrderTransRes => {
            let data: Option<AwmOrderTransResMsg> = res.process()?;
            if let Some(data) = data {
                Ok(serde_func::serde_to_value(data)?)
            } else {
                Err(ServiceError::Parameter("missing data".to_string()))
            }
        }
        EventType::AwmCmdRscRes => {
            let data: Option<AwmOrderTransResMsg> = res.process()?;
            if let Some(data) = data {
                Ok(serde_func::serde_to_value(data)?)
            } else {
                Err(ServiceError::Parameter("missing data".to_string()))
            }
        }
        EventType::AwmCmdAddrExpand => {
            let data: Option<AwmCmdAddrExpandMsg> = res.process()?;
            if let Some(data) = data {
                Ok(serde_func::serde_to_value(data)?)
            } else {
                Err(ServiceError::Parameter("missing data".to_string()))
            }
        }
        EventType::AwmCmdUidUnbind => {
            let data: Option<AwmCmdUidUnbindMsg> = res.process()?;
            if let Some(data) = data {
                Ok(serde_func::serde_to_value(data)?)
            } else {
                Err(ServiceError::Parameter("missing data".to_string()))
            }
        }
        EventType::AwmCmdActive => {
            let data: Option<AwmCmdActiveMsg> = res.process()?;
            if let Some(data) = data {
                Ok(serde_func::serde_to_value(data)?)
            } else {
                Err(ServiceError::Parameter("missing data".to_string()))
            }
        }
        EventType::AwmCmdFeeRes => {
            let data: Option<AwmCmdFeeResMsg> = res.process()?;
            if let Some(data) = data {
                Ok(serde_func::serde_to_value(data)?)
            } else {
                Err(ServiceError::Parameter("missing data".to_string()))
            }
        }
        EventType::AwmCmdDevChange => {
            let data: Option<AwmCmdDevChangeMsg> = res.process()?;
            if let Some(data) = data {
                Ok(serde_func::serde_to_value(data)?)
            } else {
                Err(ServiceError::Parameter("missing data".to_string()))
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AccountChangeRoute {
    NormalWallet,
    ApiWallet,
    Missing,
}

fn account_change_route(
    wallet_type: Option<&WalletType>,
) -> Result<AccountChangeRoute, ServiceError> {
    match wallet_type {
        Some(WalletType::NotFound) => {
            Err(crate::error::system::SystemError::WalletTypeNotSet.into())
        }
        Some(WalletType::NormalWallet) => Ok(AccountChangeRoute::NormalWallet),
        Some(WalletType::ApiRaw | WalletType::ApiWaw) => Ok(AccountChangeRoute::ApiWallet),
        None => Ok(AccountChangeRoute::Missing),
    }
}

#[cfg(test)]
mod wallet_type_tests {
    use super::{AccountChangeRoute, account_change_route};
    use crate::error::{service::ServiceError, system::SystemError};
    use wallet_database::entities::task_queue::WalletType;

    #[test]
    fn account_change_not_found_wallet_type_returns_system_error() {
        let error = account_change_route(Some(&WalletType::NotFound)).unwrap_err();

        assert!(matches!(error, ServiceError::System(SystemError::WalletTypeNotSet)));
    }

    #[test]
    fn account_change_known_and_missing_wallet_types_keep_existing_routing() {
        assert!(matches!(
            account_change_route(Some(&WalletType::NormalWallet)).unwrap(),
            AccountChangeRoute::NormalWallet
        ));
        assert_eq!(
            account_change_route(Some(&WalletType::ApiRaw)).unwrap(),
            AccountChangeRoute::ApiWallet
        );
        assert_eq!(account_change_route(None).unwrap(), AccountChangeRoute::Missing);
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use crate::{messaging::mqtt::handle::exec_incoming_publish, testkit::env::get_manager};

    async fn test_ctx() -> anyhow::Result<&'static crate::context::Context> {
        let (manager, _) = get_manager().await?;
        Ok(manager.ctx)
    }

    #[tokio::test]
    async fn test_multi_signature_transfer_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "1699855163567",
            "appId": "170976fa8a0772b7e2c",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 67090948,
                "chainCode": "tron",
                "fromAddr": "TBk86hq1e8C1gNX6RDXhk1wLamwzKnotmo",
                "isMultisig": 1,
                "notes": "时间",
                "queueId": "197131136378474496",
                "status": true,
                "symbol": "trx",
                "toAddr": "TAqUJ9enU8KkZYySA51iQim7TxbbdLR2wn",
                "token": "",
                "transactionFee": 1.401,
                "transactionTime": "2024-11-18 09:36:51",
                "transferType": 1,
                "txHash": "d92dbe05593f0c6a5e9dd76ecfb181903ffc8de77c43fa50fd4a6f53042c3371",
                "txKind": 1,
                "value": 1
            },
            "clientId": "dc09781f5671e2bc244ba492b8cfb0af",
            "deviceType": "ANDROID",
            "sn": "9580b55ec4a1d3d3af85077ae0c4c901885b1123e50f830cbd5bfbbe0cb161a3"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_multi_signature_transfer_receive_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "11111111",
            "appId": "170976fa8a0772b7e2c",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 67090948,
                "chainCode": "tron",
                "fromAddr": "TAqUJ9enU8KkZYySA51iQim7TxbbdLR2wn",
                "isMultisig": 0,
                "notes": "时间",
                "queueId": "197131136378474496",
                "status": true,
                "symbol": "trx",
                "toAddr": "TBk86hq1e8C1gNX6RDXhk1wLamwzKnotmo",
                "token": "",
                "transactionFee": 1.401,
                "transactionTime": "2024-11-18 09:36:51",
                "transferType": 0,
                "txHash": "d92dbe05593f0c6a5e9dd76ecfb181903ffc8de77c43fa50fd4a6f53042c3371",
                "txKind": 1,
                "value": 1
            },
            "clientId": "dc09781f5671e2bc244ba492b8cfb0af",
            "deviceType": "ANDROID",
            "sn": "9580b55ec4a1d3d3af85077ae0c4c901885b1123e50f830cbd5bfbbe0cb161a3"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_multi_signature_transfer_to_partner_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "222222",
            "appId": "170976fa8a0772b7e2c",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 67090948,
                "chainCode": "tron",
                "fromAddr": "TBk86hq1e8C1gNX6RDXhk1wLamwzKnotmo",
                "isMultisig": 1,
                "notes": "时间",
                "queueId": "197131136378474496",
                "status": true,
                "symbol": "trx",
                "toAddr": "TXBLuUhnfofYZAHZTWxYiDjgoaYLtwPzw3",
                "token": "",
                "transactionFee": 1.401,
                "transactionTime": "2024-11-18 09:36:51",
                "transferType": 1,
                "txHash": "d92dbe05593f0c6a5e9dd76ecfb181903ffc8de77c43fa50fd4a6f53042c3371",
                "txKind": 1,
                "value": 1
            },
            "clientId": "dc09781f5671e2bc244ba492b8cfb0af",
            "deviceType": "ANDROID",
            "sn": "9580b55ec4a1d3d3af85077ae0c4c901885b1123e50f830cbd5bfbbe0cb161a3"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_multi_signature_transfer_to_partner_receive_is_successful() -> anyhow::Result<()>
    {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "333333333",
            "appId": "170976fa8a0772b7e2c",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 67090948,
                "chainCode": "tron",
                "fromAddr": "TBk86hq1e8C1gNX6RDXhk1wLamwzKnotmo",
                "isMultisig": 1,
                "notes": "时间",
                "queueId": "197131136378474496",
                "status": true,
                "symbol": "trx",
                "toAddr": "TXBLuUhnfofYZAHZTWxYiDjgoaYLtwPzw3",
                "token": "",
                "transactionFee": 1.401,
                "transactionTime": "2024-11-18 09:36:51",
                "transferType": 0,
                "txHash": "d92dbe05593f0c6a5e9dd76ecfb181903ffc8de77c43fa50fd4a6f53042c3371",
                "txKind": 1,
                "value": 1
            },
            "clientId": "dc09781f5671e2bc244ba492b8cfb0af",
            "deviceType": "ANDROID",
            "sn": "9580b55ec4a1d3d3af85077ae0c4c901885b1123e50f830cbd5bfbbe0cb161a3"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_common_transfer_to_multi_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "123123",
            "appId": "170976fa8a0772b7e2c",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 67090948,
                "chainCode": "tron",
                "fromAddr": "TXBLuUhnfofYZAHZTWxYiDjgoaYLtwPzw3",
                "isMultisig": 0,
                "notes": "时间",
                "queueId": "197131136378474496",
                "status": true,
                "symbol": "trx",
                "toAddr": "TBk86hq1e8C1gNX6RDXhk1wLamwzKnotmo",
                "token": "",
                "transactionFee": 1.401,
                "transactionTime": "2024-11-18 09:36:51",
                "transferType": 1,
                "txHash": "d92dbe05593f0c6a5e9dd76ecfb181903ffc8de77c43fa50fd4a6f53042c3371",
                "txKind": 1,
                "value": 1
            },
            "clientId": "dc09781f5671e2bc244ba492b8cfb0af",
            "deviceType": "ANDROID",
            "sn": "9580b55ec4a1d3d3af85077ae0c4c901885b1123e50f830cbd5bfbbe0cb161a3"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_common_transfer_to_multi_receive_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "779799",
            "appId": "170976fa8a0772b7e2c",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 67090948,
                "chainCode": "tron",
                "fromAddr": "TXBLuUhnfofYZAHZTWxYiDjgoaYLtwPzw3",
                "isMultisig": 0,
                "notes": "时间",
                "queueId": "197131136378474496",
                "status": true,
                "symbol": "trx",
                "toAddr": "TBk86hq1e8C1gNX6RDXhk1wLamwzKnotmo",
                "token": "",
                "transactionFee": 1.401,
                "transactionTime": "2024-11-18 09:36:51",
                "transferType": 0,
                "txHash": "d92dbe05593f0c6a5e9dd76ecfb181903ffc8de77c43fa50fd4a6f53042c3371",
                "txKind": 1,
                "value": 1
            },
            "clientId": "dc09781f5671e2bc244ba492b8cfb0af",
            "deviceType": "ANDROID",
            "sn": "9580b55ec4a1d3d3af85077ae0c4c901885b1123e50f830cbd5bfbbe0cb161a3"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_common_transfer_to_common_token_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "123123",
            "appId": "120c83f760a33a6c663",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 67174479,
                "chainCode": "tron",
                "fromAddr": "TUDrRQ6zvwXhW3ScTxwGv8nwicLShVVWoF",
                "isMultisig": 0,
                "status": true,
                "symbol": "win",
                "toAddr": "TRbHD77Y6WWDaz9X5esrVKwEVwRM4gTw6N",
                "token": "TLa2f6VPqDgRE67v1736s7bJ8Ray5wYjU7",
                "transactionFee": 0,
                "transactionTime": "2024-11-21 07:14:36",
                "transferType": 1,
                "txHash": "c6a82ee6cd87419116965ceb8060e1318692cf60f90df09c86acf9014a73bec0",
                "txKind": 1,
                "value": 1000
            },
            "clientId": "5931ca06de5ebac2c527ee3e191b6e79",
            "deviceType": "ANDROID",
            "sn": "bf8da3b72e30e0179614e736f34f4a01e379e1383962d7c831922cdabca488b7"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_common_transfer_to_common_fees_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "321321",
            "appId": "191e35f7e0c29077f08",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 67174479,
                "chainCode": "tron",
                "fromAddr": "TUDrRQ6zvwXhW3ScTxwGv8nwicLShVVWoF",
                "isMultisig": 0,
                "status": true,
                "symbol": "trx",
                "toAddr": "TLa2f6VPqDgRE67v1736s7bJ8Ray5wYjU7",
                "token": "",
                "transactionFee": 5.8414,
                "transactionTime": "2024-11-21 07:14:36",
                "transferType": 1,
                "txHash": "c6a82ee6cd87419116965ceb8060e1318692cf60f90df09c86acf9014a73bec0",
                "txKind": 1,
                "value": 0
            },
            "clientId": "e0c7f98723a4227477968524c580af6c",
            "deviceType": "IOS",
            "sn": "7eeff112cb87de43c54f2fa4b44664a84e469c19c64ab87625857267b5d063c0"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_eth() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "321321",
            "appId": "191e35f7e0c29077f08",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 21328166,
                "chainCode": "eth",
                "fromAddr": "0x148805B49819371EEF9A822f7F880b42Cf67834D",
                "isMultisig": 1,
                "status": true,
                "symbol": "ETH",
                "toAddr": "0x8F1E2a99CB688587c02B8b836Ba9Ca39dC60D63B",
                "token": "",
                "transactionFee": 0.000515902158014156,
                "transferType": 0,
                "txHash": "0xba1bbff453e350a47a4d41b395b4c3367ecf68b75bf41c1ee57030e413ec6d5b",
                "txKind": 1,
                "value": 0.000025,
            },
            "clientId": "e0c7f98723a4227477968524c580af6c",
            "deviceType": "IOS",
            "sn": "7eeff112cb87de43c54f2fa4b44664a84e469c19c64ab87625857267b5d063c0"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_eth2() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "321321",
            "appId": "18171adc038afd5c4ac",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 21342032,
                "chainCode": "eth",
                "fromAddr": "0x8f1e2a99cb688587c02b8b836ba9ca39dc60d63b",
                "isMultisig": 1,
                "notes": "test",
                "queueId": "203611409211330560",
                "status": true,
                "symbol": "usdt",
                "toAddr": "0x148805b49819371eef9a822f7f880b42cf67834d",
                "token": "0xdac17f958d2ee523a2206206994597c13d831ec7",
                "transactionFee": 0.001107569750308281,
                "transactionTime": "2024-12-06 07:42:23",
                "transferType": 1,
                "txHash": "0xa9143a51d9927cbc010e2b35dfe4c9d4bfc37bd97b02cf7e6e1b2e3f37f91d98",
                "txKind": 1,
                "value": 0.5
            },
            "clientId": "ed8a6aab27108febe9909ff1ae10d31c",
            "deviceType": "IOS",
            "sn": "845edde901f5ea0f7c17d4b2e891679955b0a7b648da1b6ba247dfb1d1ac9902"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_eth3() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "321321",
            "appId": "191e35f7e0c290d7abd",
            "bizType": "ACCT_CHANGE",
            "body": {
            "blockHeight": 21342785,
            "chainCode": "eth",
            "fromAddr": "0x8F1E2a99CB688587c02B8b836Ba9Ca39dC60D63B",
            "isMultisig": 1,
            "notes": "0.11223",
            "queueId": "203663273910996992",
             "status": true,
            "symbol": "usdt",
            "toAddr": "0x148805B49819371EEF9A822f7F880b42Cf67834D",
            "token": "0xdac17f958d2ee523a2206206994597c13d831ec7",
            "transactionFee": 0.00135940441821096,
            "transactionTime": "2024-12-06 10:13:47",
            "transferType": 1,
            "txHash": "0xaaa362dfd318f4da95e2d1e71c8c2a2ceabc8fd5df85e7c144843e6fc55f25e0",
            "txKind": 1,
            "value": 0.1112
            },
            "clientId": "7552bd49a9407eb98164c129d11da7e2",
            "deviceType": "IOS",
            "sn": "5bb0eada7cb7290b5d196362e6def48dcb9703e1468c0fb28eb7dd61073875e6"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_eth4() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "msgId": "321321",
            "appId": "191e35f7e0c290d7abd",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 21342785,
                "chainCode": "eth",
                "fromAddr": "0x8F1E2a99CB688587c02B8b836Ba9Ca39dC60D63B",
                "isMultisig": 1,
                "notes": "0.11223",
                "queueId": "203663273910996992",
                "status": true,
                "symbol": "usdt",
                "toAddr": "0x148805B49819371EEF9A822f7F880b42Cf67834D",
                "token": "0xdac17f958d2ee523a2206206994597c13d831ec7",
                "transactionFee": 0.00135940441821096,
                "transactionTime": "2024-12-06 10:13:47",
                "transferType": 1,
                "txHash": "0xaaa362dfd318f4da95e2d1e71c8c2a2ceabc8fd5df85e7c144843e6fc55f25e0",
                "txKind": 1,
                "value": 0.1112
            },
            "clientId": "7552bd49a9407eb98164c129d11da7e2",
            "deviceType": "IOS",
            "sn": "5bb0eada7cb7290b5d196362e6def48dcb9703e1468c0fb28eb7dd61073875e6"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_acct_change_bug() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "appId": "191e35f7e0c290d7abd",
            "bizType": "ACCT_CHANGE",
            "body": {
                "blockHeight": 21336350,
                "chainCode": "eth",
                "fromAddr": "0x148805B49819371EEF9A822f7F880b42Cf67834D",
                "status": false,
                "symbol": "eth",
                "toAddr": "0x8F1E2a99CB688587c02B8b836Ba9Ca39dC60D63B",
                "token": "eth",
                "transactionFee": 0.000596818853647818,
                "transactionTime": "2024-12-05 12:39:11",
                "transferType": 1,
                "txHash": "0xbf49c59702eb886675eec8929b0320fa8ae5e533c9d8f2ab3a845568e4b594ef",
                "txKind": 1,
                "value": 0
            },
            "clientId": "7552bd49a9407eb98164c129d11da7e2",
            "deviceType": "IOS",
            "sn": "5bb0eada7cb7290b5d196362e6def48dcb9703e1468c0fb28eb7dd61073875e6",
            "msgId": "67519ef5a442286e3d1110cd"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }

    #[tokio::test]
    async fn test_acct_exec() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let ctx = test_ctx().await?;

        use rumqttc::v5::mqttbytes::v5::Publish;
        use serde_json::json;

        // 模拟 JSON 数据
        let json_data = json!({
            "appId": "xxx",
             "bizType": "MULTI_SIGN_TRANS_EXECUTE",
            "body": {
                 "withdrawId": "254342466625474560"
            },
            "clientId": "0e567a03c12ca24c5925790ccb348d1f",
            "deviceType": "ANDROID",
            "sn": "guangxiang1",
            "msgId": "680b2ca2db87aa7fc92529e9"
        });

        // 将 JSON 数据转换为 payload
        let payload = serde_json::to_vec(&json_data).expect("Failed to serialize JSON");

        // 创建模拟的 Publish 数据包
        let publish = Publish {
            dup: false,
            qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            retain: false,
            topic: "wallet/order".into(),
            pkid: 0,
            payload: payload.into(),
            properties: Default::default(),
        };

        // 调用 exec_incoming_publish 并断言结果
        let result = exec_incoming_publish(ctx, &publish).await;
        assert!(result.is_ok(), "exec_incoming_publish failed: {:?}", result.err());

        Ok(())
    }
}
