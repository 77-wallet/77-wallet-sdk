use crate::error::service::ServiceError;
use std::collections::HashSet;
use tokio::time::Instant;

#[derive(Debug, Clone)]
pub(crate) struct UnconfirmedMsgCollector {
    tx: tokio::sync::mpsc::UnboundedSender<Vec<String>>,
}

impl UnconfirmedMsgCollector {
    pub fn new(ctx: &'static crate::context::Context) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self::start_collect(rx, ctx);
        Self { tx }
    }

    pub fn submit(&self, ids: Vec<String>) -> Result<(), ServiceError> {
        self.tx
            .send(ids)
            .map_err(|e| crate::error::system::SystemError::ChannelSendFailed(e.to_string()))?;
        Ok(())
    }

    pub fn start_collect(
        mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<String>>,
        ctx: &'static crate::context::Context,
    ) {
        tokio::spawn(async move {
            let mut buffer = HashSet::new();
            let mut last_recv_time: Option<Instant> = None;

            loop {
                tokio::select! {
                    Some(ids) = rx.recv() => {
                        for id in ids {
                            buffer.insert(id);
                        }
                        if last_recv_time.is_none() {
                            last_recv_time = Some(Instant::now());
                        }
                    }
                    _ = async {
                        if let Some(start) = last_recv_time {
                            let elapsed = start.elapsed();
                            if elapsed < tokio::time::Duration::from_secs(5) {
                                tokio::time::sleep(tokio::time::Duration::from_secs(5) - elapsed).await;
                            }
                        } else {
                            // 初始 sleep，避免 busy loop
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        }
                    } => {
                        if !buffer.is_empty() {
                            let confirm_ids: Vec<_> = buffer.drain().collect();
                            tracing::debug!("批量确认消息: {:?}", confirm_ids.len());

                            let confirms = confirm_ids
                                .iter()
                                .map(|id| {
                                    wallet_transport_backend::request::SendMsgConfirm::new(
                                        id,
                                        wallet_transport_backend::request::MsgConfirmSource::Other,
                                    )
                                })
                                .collect::<Vec<_>>();

                            if let Err(e) = crate::domain::task_queue::TaskQueueDomain::send_msg_confirm_with_ctx(ctx, confirms).await {
                                tracing::error!("发送确认失败: {:?}", e);
                            }

                            last_recv_time = None;

                            let handles = ctx.get_global_handles().await;
                            if let Some(handles) = handles.upgrade() {
                                let notify = handles.get_global_notify();
                                notify.notify_one();
                            }
                            tracing::debug!("notify_one");
                        }else{
                            tracing::debug!("⏳ 等待消息确认");
                        }
                    }

                }
            }
        });
    }
}
