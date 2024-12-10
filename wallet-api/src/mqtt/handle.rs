use rumqttc::v5::mqttbytes::v5::{Packet, Publish};
use wallet_database::{entities::task_queue::TaskQueueEntity, factory::RepositoryFactory};

use crate::{
    domain::task_queue::{MqttTask, Task},
    notify::FrontendNotifyEvent,
    service::{app::AppService, device::DeviceService},
};

pub(crate) async fn exec_incoming(
    client: &rumqttc::v5::AsyncClient,
    packet: Packet,
) -> Result<(), Box<dyn std::error::Error>> {
    match packet {
        Packet::Connect(_, _, _) => {}
        Packet::ConnAck(conn_ack) => {
            exec_incoming_connack(&client, conn_ack).await?;
        }
        Packet::Publish(publish) => {
            exec_incoming_publish(&publish).await?;
            client.ack(&publish).await?;
        }
        Packet::PubAck(_) => {}
        Packet::PingReq(_) => {}
        Packet::PingResp(_) => {
            let data = crate::notify::NotifyEvent::KeepAlive;
            if let Err(e) = crate::notify::FrontendNotifyEvent::new(data).send().await {
                tracing::error!("[exec_incoming] send error: {e}");
            }
        }
        Packet::Subscribe(_) => {}
        Packet::SubAck(_) => {}
        Packet::PubRec(_) => {}
        Packet::PubRel(_) => {}
        Packet::PubComp(_) => {}
        Packet::Unsubscribe(_) => {}
        Packet::UnsubAck(_) => {}
        Packet::Disconnect(_) => {
            let data = crate::notify::NotifyEvent::MqttDisconnected;
            crate::notify::FrontendNotifyEvent::new(data).send().await?;
        }
    }

    Ok(())
}

pub async fn exec_incoming_publish(
    publish: &Publish,
    // client: &rumqttc::v5::AsyncClient,
) -> Result<(), anyhow::Error> {
    let pool = crate::manager::Context::get_global_sqlite_pool()?;
    let Publish {
        dup: _,
        qos: _,
        retain: _,
        topic,
        pkid: _,
        payload,
        properties: _,
    } = &publish;

    // let topic_ = String::from_utf8(topic.to_vec())?;
    let crate::mqtt::constant::TopicClientId {
        topic,
        client_id: _,
    } = super::constant::Topic::from_bytes(topic.to_vec())?;
    match topic {
        super::constant::Topic::Switch => {}
        #[cfg(feature = "token")]
        super::constant::Topic::Token => {
            let payload: crate::mqtt::payload::incoming::token::TokenPriceChange =
                serde_json::from_slice(&payload)?;
            payload.exec().await?;
        }
        super::constant::Topic::Order
        | super::constant::Topic::Common
        | super::constant::Topic::BulletinInfo
        | super::constant::Topic::RpcChange => {
            let payload: super::payload::incoming::Message = serde_json::from_slice(payload)?;

            match wallet_utils::serde_func::serde_to_value(payload.clone()) {
                Ok(message) => {
                    FrontendNotifyEvent::send_error(message).await?;
                }
                Err(e) => tracing::error!("[mqtt] debug message serde error: {e}"),
            }

            let id = payload.msg_id.clone();
            tokio::spawn(async move {
                if let Ok(backend_api) = crate::manager::Context::get_global_backend_api() {
                    let req = wallet_transport_backend::request::SendMsgConfirmReq::new(vec![
                        wallet_transport_backend::request::SendMsgConfirm::new(id, "MQTT"),
                    ]);
                    if let Err(e) = backend_api.send_msg_confirm(&req).await {
                        tracing::error!("send_msg_confirm error: {}", e);
                    }
                };
            });
            if TaskQueueEntity::get_task_queue(pool.as_ref(), &payload.msg_id)
                .await?
                .is_none()
            {
                if let Err(e) = exec_payload(payload).await {
                    tracing::error!("exec_payload error: {}", e);
                };
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) async fn exec_payload(
    payload: super::payload::incoming::Message,
) -> Result<(), crate::ServiceError> {
    let body: super::payload::incoming::Body =
        wallet_utils::serde_func::serde_from_value(payload.body)?;
    match (payload.biz_type, body) {
        (
            super::payload::incoming::BizType::OrderMultiSignAccept,
            super::payload::incoming::Body::OrderMultiSignAccept(data),
        ) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(
                    &payload.msg_id,
                    Task::Mqtt(Box::new(MqttTask::OrderMultiSignAccept(data))),
                )
                .send()
                .await?;
        }
        (
            super::payload::incoming::BizType::OrderMultiSignAcceptCompleteMsg,
            super::payload::incoming::Body::OrderMultiSignAcceptCompleteMsg(data),
        ) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(
                    &payload.msg_id,
                    Task::Mqtt(Box::new(MqttTask::OrderMultiSignAcceptCompleteMsg(data))),
                )
                .send()
                .await?;
        }
        (
            super::payload::incoming::BizType::OrderMultiSignServiceComplete,
            super::payload::incoming::Body::OrderMultiSignServiceComplete(data),
        ) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(
                    &payload.msg_id,
                    Task::Mqtt(Box::new(MqttTask::OrderMultiSignServiceComplete(data))),
                )
                .send()
                .await?;
        }
        (
            super::payload::incoming::BizType::OrderMultiSignCancel,
            super::payload::incoming::Body::OrderMultiSignCancel(data),
        ) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(
                    &payload.msg_id,
                    Task::Mqtt(Box::new(MqttTask::OrderMultiSignCancel(data))),
                )
                .send()
                .await?;
        }
        (
            super::payload::incoming::BizType::MultiSignTransAccept,
            super::payload::incoming::Body::MultiSignTransAccept(data),
        ) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(
                    &payload.msg_id,
                    Task::Mqtt(Box::new(MqttTask::MultiSignTransAccept(data))),
                )
                .send()
                .await?;
        }
        (
            super::payload::incoming::BizType::MultiSignTransAcceptCompleteMsg,
            super::payload::incoming::Body::MultiSignTransAcceptCompleteMsg(data),
        ) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(
                    &payload.msg_id,
                    Task::Mqtt(Box::new(MqttTask::MultiSignTransAcceptCompleteMsg(data))),
                )
                .send()
                .await?;
        }
        (
            super::payload::incoming::BizType::AcctChange,
            super::payload::incoming::Body::AcctChange(data),
        ) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(
                    &payload.msg_id,
                    Task::Mqtt(Box::new(MqttTask::AcctChange(data))),
                )
                .send()
                .await?;
        }
        (super::payload::incoming::BizType::Init, super::payload::incoming::Body::Init(data)) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(&payload.msg_id, Task::Mqtt(Box::new(MqttTask::Init(data))))
                .send()
                .await?;
        }
        (
            super::payload::incoming::BizType::OrderMultiSignCreated,
            super::payload::incoming::Body::OrderMultiSignCreated(data),
        ) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(
                    &payload.msg_id,
                    Task::Mqtt(Box::new(MqttTask::OrderMultiSignCreated(data))),
                )
                .send()
                .await?;
        }
        (
            super::payload::incoming::BizType::BulletinMsg,
            super::payload::incoming::Body::BulletinMsg(data),
        ) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(
                    &payload.msg_id,
                    Task::Mqtt(Box::new(MqttTask::BulletinMsg(data))),
                )
                .send()
                .await?;
        }
        (
            super::payload::incoming::BizType::MultiSignTransCancel,
            super::payload::incoming::Body::MultiSignTransCancel(data),
        ) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(
                    &payload.msg_id,
                    Task::Mqtt(Box::new(MqttTask::MultiSignTransCancel(data))),
                )
                .send()
                .await?;
        }
        (
            super::payload::incoming::BizType::RpcAddressChange,
            super::payload::incoming::Body::RpcChange(data),
        ) => {
            crate::domain::task_queue::Tasks::new()
                .push_with_id(
                    &payload.msg_id,
                    Task::Mqtt(Box::new(MqttTask::RpcChange(data))),
                )
                .send()
                .await?;
        }
        (_, _) => {
            return Err(crate::ServiceError::System(
                crate::SystemError::MessageWrong,
            ))
        }
    }
    Ok(())
}
async fn exec_incoming_connack(
    client: &rumqttc::v5::AsyncClient,
    conn_ack: rumqttc::v5::mqttbytes::v5::ConnAck,
) -> Result<(), anyhow::Error> {
    let pool = crate::manager::Context::get_global_sqlite_pool()?;
    let repo = RepositoryFactory::repo(pool.clone());
    let device_service = DeviceService::new(repo);

    if conn_ack.code == rumqttc::v5::mqttbytes::v5::ConnectReturnCode::Success {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());
        AppService::new(repo).mqtt_resubscribe().await?;
    }

    use wallet_database::repositories::wallet::WalletRepoTrait as _;
    let mut repo = RepositoryFactory::repo(pool);
    if let Some(wallet) = repo.wallet_latest().await?
        && let Some(device) = &device_service.get_device_info().await?
        && let Some(app_id) = &device.app_id
    {
        let client_id = crate::domain::app::DeviceDomain::client_id_by_device(device)?;
        let body = super::payload::outgoing::OutgoingPayload::SwitchWallet {
            uid: wallet.uid,
            sn: device.sn.clone(),
            app_id: app_id.to_string(),
            device_type: device.device_type.clone(),
            client_id,
        };
        client
            .publish(
                crate::mqtt::constant::Topic::Switch,
                rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
                false,
                body.to_vec()?,
            )
            .await?;
    }

    let data = crate::notify::NotifyEvent::MqttConnected;
    crate::notify::FrontendNotifyEvent::new(data).send().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        mqtt::handle::exec_incoming_publish,
        test::env::{setup_test_environment, TestData},
    };

    #[tokio::test]
    async fn test_multi_signature_transfer_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_multi_signature_transfer_receive_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_multi_signature_transfer_to_partner_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_multi_signature_transfer_to_partner_receive_is_successful() -> anyhow::Result<()>
    {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_common_transfer_to_multi_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_common_transfer_to_multi_receive_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_common_transfer_to_common_token_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_common_transfer_to_common_fees_is_successful() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_eth() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_eth2() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_eth3() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_eth4() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_acct_change_bug() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { .. } = setup_test_environment(None, None, false, None).await?;

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
        let result = exec_incoming_publish(&publish).await;
        assert!(
            result.is_ok(),
            "exec_incoming_publish failed: {:?}",
            result.err()
        );

        Ok(())
    }
}
