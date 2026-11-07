//! Temporary WALLET-2981 diagnostics. Never forward raw tracing events or errors.
use super::{FrontendNotifyEvent, event::NotifyEvent, other::DebugFront};
use crate::context::{Context, FrontendNotifySender};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub(crate) struct DiagnosticEmitter {
    sender: Arc<RwLock<FrontendNotifySender>>,
    sn: String,
    upload_configured: bool,
}

impl DiagnosticEmitter {
    pub(crate) fn from_context(ctx: &Context) -> Self {
        Self {
            sender: ctx.get_global_frontend_notify_sender(),
            sn: ctx.get_sn().into(),
            upload_configured: ctx.get_log_upload_client().is_some(),
        }
    }

    // Best effort only: never wait for a lock, spawn work or log send failures.
    pub(crate) fn emit(&self, mut fields: serde_json::Value) {
        let Some(object) = fields.as_object_mut() else { return };
        object.insert("diagnostic".into(), "WALLET-2981-notify-v1".into());
        object.insert("sn".into(), self.sn.clone().into());
        object.insert("upload_configured".into(), self.upload_configured.into());
        object.insert("time_ms".into(), chrono::Utc::now().timestamp_millis().into());
        let Ok(sender) = self.sender.try_read() else { return };
        if let Some(sender) = sender.as_ref() {
            let _ = sender
                .send(FrontendNotifyEvent::new(NotifyEvent::Debug(DebugFront { message: fields })));
        }
    }

    #[cfg(test)]
    pub(crate) fn test_channel() -> (Self, tokio::sync::mpsc::UnboundedReceiver<FrontendNotifyEvent>)
    {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Self {
                sender: Arc::new(RwLock::new(Some(tx))),
                upload_configured: true,
                sn: "test-device".into(),
            },
            rx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_diagnostics_reach_existing_frontend_envelope_with_identity() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let emitter = DiagnosticEmitter {
            sender: Arc::new(RwLock::new(Some(tx))),
            upload_configured: true,
            sn: "test-device".into(),
        };
        emitter.emit(serde_json::json!({"component":"creation","stage":"total","event":"begin"}));
        let event = rx.try_recv().expect("diagnostic must reach App without the upload service");
        assert_eq!(event.event, "DEBUG");
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["data"]["message"]["diagnostic"], "WALLET-2981-notify-v1");
        assert_eq!(value["data"]["message"]["sn"], "test-device");
        assert_eq!(value["data"]["message"]["stage"], "total");
        assert!(value["data"]["message"]["time_ms"].is_i64());
    }

    #[test]
    fn unavailable_or_busy_frontend_never_blocks_diagnostics() {
        let sender = Arc::new(RwLock::new(None));
        let emitter = DiagnosticEmitter {
            sender: sender.clone(),
            upload_configured: true,
            sn: "test-device".into(),
        };
        emitter.emit(serde_json::json!({}));
        let held = sender.try_write().unwrap();
        emitter.emit(serde_json::json!({}));
        drop(held);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        *sender.try_write().unwrap() = Some(tx);
        drop(rx);
        emitter.emit(serde_json::json!({}));
    }
}
