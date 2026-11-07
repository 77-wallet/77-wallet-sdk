use wallet_database::repositories::task_queue::TaskQueueRepo;
use wallet_utils::serde_func;

use crate::{
    context::Context,
    messaging::{mqtt::Message, notify::FrontendNotifyEvent},
};

pub struct JPushService {
    ctx: &'static Context,
}

impl JPushService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    // 前端发来的消息
    pub async fn jpush(&self, message: &str) -> Result<(), crate::error::service::ServiceError> {
        // Self::jpush_multi(vec![message.to_string()], "JG").await?;
        match serde_func::serde_from_str::<Message>(message) {
            Ok(data) => {
                let backend_api = self.ctx.get_global_backend_api();

                // 重新查询一次,前端给到的数据不全面
                let data = backend_api
                    .get_unconfirm_by_msg_id(&wallet_transport_backend::request::GetUnconfirmById {
                        msg_id: data.msg_id.to_string(),
                    })
                    .await?;

                if let Some(msg) = data.body {
                    self.jpush_multi(
                        vec![msg],
                        // MsgConfirmSource::Jg
                    )
                    .await?;
                };
            }
            Err(e) => {
                tracing::error!("[jpush] serde_from_str error: {}", e);
                if let Err(e) =
                    FrontendNotifyEvent::send_error_with_ctx(self.ctx, "jpush", e.to_string()).await
                {
                    tracing::error!("send_error error: {}", e);
                }
            }
        };

        Ok(())
    }

    pub async fn jpush_multi(
        &self,
        messages: Vec<String>,
        // source: MsgConfirmSource,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.task_pool()?;
        let handles = self.ctx.get_global_handles().await;
        if let Some(handles) = handles.upgrade() {
            let unconfirmed_msg_collector = handles.get_global_unconfirmed_msg_collector();
            for message in messages {
                let payload = match serde_func::serde_from_str::<Message>(message.as_str()) {
                    Ok(data) => data,
                    Err(e) => {
                        tracing::error!("[jpush_multi] serde_from_str error: {}", e);
                        if let Err(e) = FrontendNotifyEvent::send_error_with_ctx(
                            self.ctx,
                            "jpush_multi",
                            e.to_string(),
                        )
                        .await
                        {
                            tracing::error!("send_error error: {}", e);
                        }
                        continue;
                    }
                };

                let id = payload.msg_id.clone();
                if let Some(task_entity) =
                    TaskQueueRepo::task_detail(&pool, &payload.msg_id).await?
                    && task_entity.status == 2
                {
                    unconfirmed_msg_collector.submit(vec![id])?;
                } else if let Err(e) =
                    crate::messaging::mqtt::handle::exec_payload(payload, self.ctx).await
                {
                    if let Err(e) = FrontendNotifyEvent::send_error_with_ctx(
                        self.ctx,
                        "jpush_multi",
                        e.to_string(),
                    )
                    .await
                    {
                        tracing::error!("send_error error: {}", e);
                    }
                    tracing::error!("[jpush_multi] exec_payload error: {}", e);
                };
            }
        } else {
            tracing::error!("jpush must be called before they received");
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use crate::messaging::mqtt::{Message, topics::OrderMultiSignAcceptCompleteMsg};

    #[test]
    fn test_() {
        let message = "{\"clientId\":\"666\",\"sn\":\"device457\",\"deviceType\":\"ANDROID\",\"bizType\":\"ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG\",\"body\":{\"status\":1,\"multisigAccountId\":\"order-1\",\"addressList\":[],\"acceptStatus\":false,\"acceptAddressList\":[\"THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB\"]}}";

        let _payload: Message = wallet_utils::serde_func::serde_from_str(message).unwrap();
    }

    #[test]
    fn test_2() {
        let message = "{\"status\":1,\"multisigAccountId\":\"order-1\",\"addressList\":[],\"acceptStatus\":false,\"acceptAddressList\":[\"THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB\"]}";

        let _payload: OrderMultiSignAcceptCompleteMsg =
            wallet_utils::serde_func::serde_from_str(message).unwrap();
    }
}
