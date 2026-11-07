use std::sync::Arc;

use crate::handles::Handles;

#[derive(Debug, Clone)]
pub struct TopicData {
    pub qos: rumqttc::v5::mqttbytes::QoS,
    pub last_updated: std::time::SystemTime,
    #[allow(dead_code)]
    pub is_active: bool,
}

// 用于排序的结构体，包含 topic 和 last_updated
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct TopicEntry {
    topic: String,
    last_updated: std::time::SystemTime,
}

// 实现 TopicEntry 的排序逻辑
impl Ord for TopicEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.last_updated.cmp(&other.last_updated)
    }
}

impl PartialOrd for TopicEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Topics {
    pub(crate) data: std::collections::HashMap<String, TopicData>,
    pub(crate) entry: std::collections::BTreeSet<TopicEntry>,
}

impl Topics {
    pub fn new() -> Self {
        Topics::default()
    }

    pub async fn subscribe(
        &mut self,
        topics: Vec<String>,
        qos: Option<u8>,
        handles: &Arc<Handles>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let qos = match qos {
            Some(0) => rumqttc::v5::mqttbytes::QoS::AtMostOnce,
            Some(1) => rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
            Some(2) => rumqttc::v5::mqttbytes::QoS::ExactlyOnce,
            _ => rumqttc::v5::mqttbytes::QoS::AtMostOnce,
        };

        let subscribed_topics: std::collections::HashSet<String> =
            self.data.keys().cloned().collect();
        let unique_topics: Vec<String> =
            topics.into_iter().filter(|topic| !subscribed_topics.contains(topic)).collect();

        let mqtt_processor = handles.get_normal_wallet_mqtt();
        if let Some(mqtt_handle) = mqtt_processor.lock().await.as_ref() {
            for topic in unique_topics.iter() {
                match mqtt_handle.try_subscribe(topic, qos) {
                    Ok(_) => {
                        tracing::debug!("订阅主题成功: {}", topic);
                        let now = std::time::SystemTime::now();
                        self.data.insert(
                            topic.clone(),
                            TopicData { qos, last_updated: now, is_active: true },
                        );
                        self.entry.insert(TopicEntry { topic: topic.clone(), last_updated: now });
                    }
                    Err(e) => {
                        tracing::error!("订阅主题失败: {}, 错误信息：{:?}", topic, e);
                    }
                }
            }
            return Ok(());
        }

        Err(crate::error::service::ServiceError::System(
            crate::error::system::SystemError::MqttClientNotInit,
        ))
    }

    pub async fn unsubscribe(
        &mut self,
        topics: Vec<String>,
        handles: &Arc<Handles>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let subscribed_topics: std::collections::HashSet<String> =
            self.data.keys().cloned().collect();
        let unique_topics: Vec<String> =
            topics.into_iter().filter(|topic| subscribed_topics.contains(topic)).collect();

        if unique_topics.is_empty() {
            return Ok(());
        }

        let mqtt_processor = handles.get_normal_wallet_mqtt();
        if let Some(mqtt_handle) = mqtt_processor.lock().await.as_ref() {
            tracing::debug!("取消订阅的主题: {}", unique_topics.join(", "));
            for topic in unique_topics.iter() {
                match mqtt_handle.try_unsubscribe(topic) {
                    Ok(_) => {
                        tracing::debug!("取消订阅成功: {}", topic);
                        if let Some(topic_data) = self.data.remove(topic) {
                            self.entry.remove(&TopicEntry {
                                topic: topic.clone(),
                                last_updated: topic_data.last_updated,
                            });
                        }
                    }
                    Err(e) => {
                        tracing::error!("取消订阅失败: {}, 错误信息：{:?}", topic, e);
                    }
                }
            }
            tracing::debug!("取消订阅完成");
            return Ok(());
        }

        Err(crate::error::service::ServiceError::System(
            crate::error::system::SystemError::MqttClientNotInit,
        ))
    }

    pub async fn resubscribe(
        &self,
        handles: &Arc<Handles>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let mqtt_processor = handles.get_normal_wallet_mqtt();
        if let Some(mqtt_handle) = mqtt_processor.lock().await.as_ref() {
            for (topic, topic_data) in self.data.iter() {
                match mqtt_handle.try_subscribe(topic, topic_data.qos) {
                    Ok(_) => {
                        tracing::debug!("重新订阅成功: {}", topic);
                    }
                    Err(e) => {
                        tracing::error!("重新订阅失败: {}, 错误信息：{:?}", topic, e);
                    }
                }
            }
            return Ok(());
        }

        Err(crate::error::service::ServiceError::System(
            crate::error::system::SystemError::MqttClientNotInit,
        ))
    }
}
