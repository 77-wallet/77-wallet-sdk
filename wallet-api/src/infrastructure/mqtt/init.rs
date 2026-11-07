use super::property::UserProperty;
use crate::{
    error::{service::ServiceError, system::SystemError},
    infrastructure::mqtt::client::MqttClientBuilder,
    messaging::notify::{FrontendNotifyEvent, event::NotifyEvent, other::ConnectionErrorFrontend},
};
use rumqttc::v5::{
    Event, EventLoop,
    mqttbytes::{QoS, v5::Packet},
};
use std::sync::Arc;
use tokio::{
    sync::{Mutex, broadcast, mpsc::UnboundedSender},
    task::JoinHandle,
};
use tokio_stream::StreamExt as _;

#[derive(Debug)]
pub(crate) struct ProcessMqttHandle {
    client_id: String,
    shutdown_tx: broadcast::Sender<()>,
    client: Arc<rumqttc::v5::AsyncClient>,
    ev_handle: Mutex<Option<JoinHandle<()>>>,
    e_handle: Mutex<Option<JoinHandle<Result<(), ServiceError>>>>,
}

impl ProcessMqttHandle {
    pub async fn new(
        user_property: UserProperty,
        url: String,
        ctx: &'static crate::context::Context,
    ) -> Result<Self, ServiceError> {
        let (shutdown_tx, _) = broadcast::channel(1);
        let shutdown_rx1 = shutdown_tx.subscribe();
        let shutdown_rx2 = shutdown_tx.subscribe();
        let client_id = user_property.client_id.clone();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);

        tracing::debug!("[init_mqtt_processor] url: {url}");
        let (client, event_loop) = MqttClientBuilder::new(&url, user_property.clone()).build()?;
        let client = Arc::new(client);

        let mut ev =
            ProcessMqttEventLoop::new(user_property.clone(), shutdown_rx1, tx, event_loop, ctx);
        let ev_handle = tokio::spawn(async move { ev.handle_eventloop().await });
        let mut e = ProcessMqttEvent::new(user_property, shutdown_rx2, rx, client.clone(), ctx);
        let e_handle = tokio::spawn(async move { e.exec_event().await });

        Ok(Self {
            client_id,
            shutdown_tx,
            client,
            ev_handle: Mutex::new(Some(ev_handle)),
            e_handle: Mutex::new(Some(e_handle)),
        })
    }

    pub fn try_subscribe<S: Into<String>>(&self, topic: S, qos: QoS) -> Result<(), ServiceError> {
        self.client.try_subscribe(topic, qos).map_err(|err| {
            tracing::warn!(error = %err, "mqtt subscribe failed");
            ServiceError::System(SystemError::MqttClientNotInit)
        })
    }

    pub fn try_unsubscribe<S: Into<String>>(&self, topic: S) -> Result<(), ServiceError> {
        self.client.try_unsubscribe(topic).map_err(|err| {
            tracing::warn!(error = %err, "mqtt unsubscribe failed");
            ServiceError::System(SystemError::MqttClientNotInit)
        })
    }

    pub(crate) async fn close(&self) -> Result<(), ServiceError> {
        tracing::debug!("[init_mqtt_processor] close =============================");
        let _ = self.shutdown_tx.send(());
        if let Some(handle) = self.ev_handle.lock().await.take() {
            if let Err(err) = handle.await {
                tracing::warn!(
                    client_id = %self.client_id,
                    error = %err,
                    "mqtt event loop join failed during close"
                );
            }
        }
        if let Some(handle) = self.e_handle.lock().await.take() {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    tracing::warn!(
                        client_id = %self.client_id,
                        error = %err,
                        "mqtt event task returned error during close"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        client_id = %self.client_id,
                        error = %err,
                        "mqtt event task join failed during close"
                    );
                }
            }
        }
        Ok(())
    }
}

struct ProcessMqttEventLoop {
    user_property: UserProperty,
    shutdown_rx: broadcast::Receiver<()>,
    tx: UnboundedSender<rumqttc::v5::Event>,
    event_loop: EventLoop,
    ctx: &'static crate::context::Context,
}

impl ProcessMqttEventLoop {
    fn new(
        user_property: UserProperty,
        shutdown_rx: broadcast::Receiver<()>,
        tx: UnboundedSender<rumqttc::v5::Event>,
        event_loop: EventLoop,
        ctx: &'static crate::context::Context,
    ) -> Self {
        Self { user_property, shutdown_rx, tx, event_loop, ctx }
    }

    async fn handle_eventloop(&mut self) {
        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    tracing::info!("closing {} mqtt event loop -------------------------------", &self.user_property.client_id);
                    break;
                }
                ev = self.event_loop.poll() => {
                    match ev {
                        Ok(event) => {
                            tracing::info!("mqtt event: {:?}", event);
                            if let Err(e) = self.tx.send(event) {
                                tracing::error!("[handle event loop] send channel error: {e}");
                            };
                        }
                        Err(err) => {
                            tracing::error!("[mqtt] connection error = {err:?}");
                            // if connect error  ,sleep 5s and reconnect
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            let data = NotifyEvent::ConnectionError(ConnectionErrorFrontend {
                                message: err.to_string(),
                            });
                            match FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await {
                                Ok(_) => tracing::debug!("[mqtt] sender send ok"),
                                Err(e) => tracing::error!("[mqtt] sender send error: {e}"),
                            };
                        }
                    }
                }
            }
        }
        tracing::info!(
            "closing {} mqtt event loop ------------------------------- end",
            &self.user_property.client_id
        );
    }
}

struct ProcessMqttEvent {
    user_property: UserProperty,
    shutdown_rx: broadcast::Receiver<()>,
    rx: tokio_stream::wrappers::UnboundedReceiverStream<Event>,
    client: Arc<rumqttc::v5::AsyncClient>,
    ctx: &'static crate::context::Context,
}

impl ProcessMqttEvent {
    fn new(
        user_property: UserProperty,
        shutdown_rx: broadcast::Receiver<()>,
        rx: tokio_stream::wrappers::UnboundedReceiverStream<Event>,
        client: Arc<rumqttc::v5::AsyncClient>,
        ctx: &'static crate::context::Context,
    ) -> Self {
        Self { user_property, shutdown_rx, rx, client, ctx }
    }

    async fn exec_event(&mut self) -> Result<(), ServiceError> {
        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    if let Err(err) = self.client.disconnect().await {
                        tracing::warn!(
                            client_id = %self.user_property.client_id,
                            error = %err,
                            "mqtt disconnect failed during close"
                        );
                    }
                    tracing::info!("closing {} mqtt event -------------------------------", &self.user_property.client_id);
                    break;
                }
                Some(event) = self.rx.next() => {
                    let res = match event {
                        rumqttc::v5::Event::Incoming(packet) => {
                            self.exec_incoming(packet).await
                        }
                        rumqttc::v5::Event::Outgoing(_) => Ok(()),
                    };

                    if let Err(e) = res {
                        tracing::error!("[exec_event] error: {e}");
                    }
                }
            }
        }
        tracing::info!(
            "closing {} mqtt event ------------------------------- end",
            &self.user_property.client_id
        );
        Ok(())
    }

    async fn exec_incoming(&self, packet: Packet) -> Result<(), Box<dyn std::error::Error>> {
        match packet {
            Packet::ConnAck(conn_ack) => {
                crate::messaging::mqtt::handle::exec_incoming_connack(
                    self.ctx,
                    &self.client,
                    conn_ack,
                )
                .await?;
            }
            Packet::Publish(publish) => {
                crate::messaging::mqtt::handle::exec_incoming_publish(self.ctx, &publish).await?;
                self.client.ack(&publish).await?;
            }
            Packet::PingResp(_) => {
                let data = NotifyEvent::KeepAlive;
                if let Err(e) = FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await {
                    tracing::error!("[exec_incoming] send error: {e}");
                }
            }
            Packet::Disconnect(_) => {
                let data = NotifyEvent::MqttDisconnected;
                FrontendNotifyEvent::new(data).send_with_ctx(self.ctx).await?;
            }
            _ => {}
        }

        Ok(())
    }
}
