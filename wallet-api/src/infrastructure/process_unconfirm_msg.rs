use crate::{
    domain::app::mqtt::MqttDomain, error::service::ServiceError,
    messaging::notify::FrontendNotifyEvent,
};
use std::sync::Arc;
use tokio::{
    sync::{Mutex, broadcast},
    task::JoinHandle,
};
use wallet_database::repositories::task_queue::TaskQueueRepo;
use wallet_transport_backend::request::api_wallet::msg::MsgAckExpiredResendReq;

#[derive(Debug)]
pub struct UnconfirmedMsgProcessorHandle {
    shutdown_tx: broadcast::Sender<()>,
    handle: Mutex<Option<JoinHandle<Result<(), ServiceError>>>>,
}

impl UnconfirmedMsgProcessorHandle {
    pub async fn new(
        ctx: &'static crate::context::Context,
        client_id: &str,
        notify: Arc<tokio::sync::Notify>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let shutdown_rx1 = shutdown_tx.subscribe();
        // 发交易
        let mut processor = UnconfirmedMsgProcessor::new(ctx, shutdown_rx1, client_id, notify);
        let tx_handle = tokio::spawn(async move { processor.start().await });
        Self { shutdown_tx, handle: Mutex::new(Some(tx_handle)) }
    }

    pub(crate) async fn close(&self) -> Result<(), ServiceError> {
        tracing::info!("Closing unconfirmed transactions ------------------------------- 1");
        let _ = self.shutdown_tx.send(());
        if let Some(handle) = self.handle.lock().await.take() {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    tracing::warn!(error = %err, "unconfirmed msg processor returned error during close");
                }
                Err(err) => {
                    tracing::warn!(error = %err, "unconfirmed msg processor join failed during close");
                }
            }
        }
        Ok(())
    }
}

struct UnconfirmedMsgProcessor {
    ctx: &'static crate::context::Context,
    shutdown_rx: broadcast::Receiver<()>,
    client_id: String,
    notify: Arc<tokio::sync::Notify>,
}

impl UnconfirmedMsgProcessor {
    pub fn new(
        ctx: &'static crate::context::Context,
        shutdown_rx: broadcast::Receiver<()>,
        client_id: &str,
        notify: Arc<tokio::sync::Notify>,
    ) -> Self {
        Self { ctx, shutdown_rx, client_id: client_id.into(), notify }
    }

    async fn handle_once(
        &self,
        ctx: &'static crate::context::Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.task_pool()?;

        // 判断数据库中是否存在大量的未处理消息,如果有则跳过
        if TaskQueueRepo::failed_task_queue(&pool).await?.len() < 500 {
            tracing::debug!("未完成的mqtt任务数小于500个,处理未确认消息");
        } else {
            tracing::debug!("未完成的mqtt任务达到500个,跳过处理未确认消息");
            return Ok(());
        }

        MqttDomain::process_unconfirm_msg(ctx, &self.client_id).await
    }

    async fn handle_and_report(&self, ctx: &'static crate::context::Context) {
        if let Err(e) = self.handle_once(ctx).await {
            tracing::error!("处理未确认消息失败: {}", e);
            if let Err(send_err) = FrontendNotifyEvent::send_error_with_ctx(
                self.ctx,
                "InitializationTask::ProcessUnconfirmMsg",
                e.to_string(),
            )
            .await
            {
                tracing::error!("发送错误通知失败: {}", send_err);
            }
        }
    }

    async fn api_wallet_msg_resend(&self, ctx: &'static crate::context::Context) {
        let backend = ctx.get_global_backend_api();
        let res = backend
            .msg_ack_expired_resend(MsgAckExpiredResendReq {
                client_id: self.client_id.to_string(),
            })
            .await;
        match res {
            Ok(_) => {}
            Err(e) => {
                tracing::error!(" ---- {}", e)
            }
        }
    }

    /// Runs once at startup, then repeats either when notified
    /// or every 30 seconds on a timer.
    pub async fn start(&mut self) -> Result<(), ServiceError> {
        let ctx = self.ctx;
        let notify = self.notify.clone();
        let mut interval_30sec = tokio::time::interval(std::time::Duration::from_secs(30));
        let mut interval_10min = tokio::time::interval(std::time::Duration::from_secs(60 * 3));

        // 启动的时候执行一次
        self.handle_and_report(ctx).await;
        let r = ctx.is_init_api_swap().await;
        if r {
            self.api_wallet_msg_resend(ctx).await;
        }
        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    tracing::info!("closing process unconfirm msg -------------------------------");
                    break;
                }
                 _ = notify.notified() => {
                     tracing::debug!("收到通知，开始处理");
                    // 定时执行
                    self.handle_and_report(ctx).await;
                 }
                 _ = interval_30sec.tick() => {
                     tracing::debug!("30秒超时,开始自动处理");
                    // 定时执行
                    self.handle_and_report(ctx).await;
                 }
                _ = interval_10min.tick() => {
                    let r = ctx.is_init_api_swap().await;
                    if r {
                    self.api_wallet_msg_resend(ctx).await;
                    }
                }
            }
        }
        tracing::info!("closing process unconfirm msg ------------------------------- end");
        Ok(())
    }
}
