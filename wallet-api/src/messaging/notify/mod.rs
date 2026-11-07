// 发送给前端事件的
use crate::context::Context;
use event::NotifyEvent;
use other::{DebugFront, ErrFront};

pub(crate) mod api_wallet;
pub(crate) mod diagnostic;
pub(crate) mod event;
pub(crate) mod multisig;
pub(crate) mod other;
pub(crate) mod permission;
pub(crate) mod transaction;
pub(crate) mod writer_diagnostics;

#[derive(Debug, serde::Serialize)]
pub struct FrontendNotifyEvent {
    pub event: String,
    pub data: NotifyEvent,
}

impl FrontendNotifyEvent {
    pub(crate) fn new(data: NotifyEvent) -> Self {
        FrontendNotifyEvent { event: data.event_name(), data }
    }

    pub(crate) async fn send_debug_with_ctx<T: serde::Serialize>(
        ctx: &'static Context,
        message: T,
    ) -> Result<(), crate::error::service::ServiceError> {
        let message = wallet_utils::serde_func::serde_to_value(message)?;
        let data = NotifyEvent::Debug(DebugFront { message });
        match FrontendNotifyEvent::new(data).send_with_ctx(ctx).await {
            Ok(_) => tracing::debug!("[mqtt] send debug message ok"),
            Err(e) => tracing::error!("[mqtt] send debug message error: {e}"),
        };
        Ok(())
    }

    #[deprecated(note = "pass Context explicitly with send_debug_with_ctx")]
    pub(crate) async fn send_debug<T: serde::Serialize>(
        message: T,
    ) -> Result<(), crate::error::service::ServiceError> {
        Self::send_debug_with_ctx(crate::get_context()?, message).await
    }

    pub(crate) async fn send_error_with_ctx<T: serde::Serialize>(
        ctx: &'static Context,
        event: &str,
        message: T,
    ) -> Result<(), crate::error::service::ServiceError> {
        let message = wallet_utils::serde_func::serde_to_string(&message)?;
        let data = NotifyEvent::Err(ErrFront { event: event.to_string(), message });
        match FrontendNotifyEvent::new(data).send_with_ctx(ctx).await {
            Ok(_) => tracing::debug!("[mqtt] send err message ok"),
            Err(e) => tracing::error!("[mqtt] send err message error: {e}"),
        };
        Ok(())
    }

    #[deprecated(note = "pass Context explicitly with send_error_with_ctx")]
    pub(crate) async fn send_error<T: serde::Serialize>(
        event: &str,
        message: T,
    ) -> Result<(), crate::error::service::ServiceError> {
        Self::send_error_with_ctx(crate::get_context()?, event, message).await
    }

    #[deprecated(note = "pass Context explicitly with send_with_ctx")]
    pub(crate) async fn send(self) -> Result<(), crate::error::service::ServiceError> {
        self.send_with_ctx(crate::get_context()?).await
    }

    pub(crate) async fn send_with_ctx(
        self,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let sender = ctx.get_global_frontend_notify_sender();
        let sender = sender.read().await;
        if let Some(sender) = sender.as_ref() {
            sender.send(self).map_err(|e| {
                crate::error::service::ServiceError::System(
                    crate::error::system::SystemError::ChannelSendFailed(e.to_string()),
                )
            })?;
        } else {
            return Err(crate::error::service::ServiceError::System(
                crate::error::system::SystemError::FrontendNotifySenderUnset,
            ));
        }
        Ok(())
    }
}
