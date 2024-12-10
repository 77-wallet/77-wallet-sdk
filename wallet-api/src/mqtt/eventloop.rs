use std::{future::Future, pin::Pin};

use rumqttc::v5::{AsyncClient, EventLoop};

pub type EventLoopResponse = Pin<Box<dyn Future<Output = ()> + Send>>;
pub type EventLoopHandler = fn(
    tokio::sync::mpsc::UnboundedSender<rumqttc::v5::Event>,
    AsyncClient,
    EventLoop,
) -> EventLoopResponse;

pub struct MqttEventLoop {
    pub eventloop: EventLoop,
}

impl MqttEventLoop {
    pub fn new(eventloop: EventLoop) -> Self {
        Self { eventloop }
    }

    pub async fn process(
        self,
        tx: tokio::sync::mpsc::UnboundedSender<rumqttc::v5::Event>,
        client: AsyncClient,
        handle_eventloop: EventLoopHandler,
    ) -> Result<(), crate::ServiceError> {
        let f = move |tx, client, eventloop| -> EventLoopResponse {
            handle_eventloop(tx, client, eventloop)
        };
        f(tx, client, self.eventloop).await;
        Ok(())
    }
}
