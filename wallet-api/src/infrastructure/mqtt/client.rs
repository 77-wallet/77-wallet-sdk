use rumqttc::v5::{AsyncClient, EventLoop, MqttOptions};
use std::time::Duration;

use super::property::UserProperty;

pub struct MqttClientBuilder {
    client_id: String,
    url: String,
    username: String,
    password: String,
    user_property: Vec<(String, String)>,
}

impl MqttClientBuilder {
    pub fn new(url: &str, user_property: UserProperty) -> Self {
        Self {
            client_id: user_property.client_id.clone(),
            url: url.to_string(),
            user_property: user_property.to_vec(),
            username: user_property.username,
            password: user_property.password,
        }
    }

    pub fn build(self) -> Result<(AsyncClient, EventLoop), crate::error::system::SystemError> {
        let url = format!("{}?client_id={}", self.url, self.client_id);
        let mut mqtt_options = MqttOptions::parse_url(url)
            .map_err(|e| crate::error::system::SystemError::Service(e.to_string()))?;

        mqtt_options
            .set_transport(rumqttc::Transport::Tcp)
            .set_connection_timeout(20)
            .set_credentials(self.username, self.password)
            .set_keep_alive(Duration::from_secs(10))
            .set_user_properties(self.user_property)
            .set_clean_start(false)
            .set_manual_acks(true);

        let (client, eventloop) = AsyncClient::new(mqtt_options, 50);

        Ok((client, eventloop))
    }
}
