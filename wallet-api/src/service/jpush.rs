use wallet_database::entities::task_queue::TaskQueueEntity;

pub struct JPushService {}

impl JPushService {
    pub async fn jpush(message: &str) -> Result<(), crate::ServiceError> {
        Self::jpush_multi(vec![message.to_string()], "JG").await?;
        Ok(())
    }

    pub async fn jpush_multi(
        messages: Vec<String>,
        source: &str,
    ) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let mut ids = Vec::new();
        for message in messages {
            let payload: crate::mqtt::payload::incoming::Message =
                wallet_utils::serde_func::serde_from_str(message.as_str())?;
            let id = payload.msg_id.clone();
            if TaskQueueEntity::get_task_queue(pool.as_ref(), &payload.msg_id)
                .await?
                .is_none()
            {
                crate::mqtt::handle::exec_payload(payload).await?;
            };
            ids.push(wallet_transport_backend::request::SendMsgConfirm::new(
                id, source,
            ));
        }
        if !ids.is_empty() {
            tokio::spawn(async move {
                if let Ok(backend_api) = crate::manager::Context::get_global_backend_api() {
                    let req = wallet_transport_backend::request::SendMsgConfirmReq::new(ids);
                    if let Err(e) = backend_api.send_msg_confirm(&req).await {
                        tracing::error!("send_msg_confirm error: {}", e);
                    }
                };
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_() {
        let message = "{\"clientId\":\"wenjing\",\"sn\":\"device457\",\"deviceType\":\"ANDROID\",\"bizType\":\"ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG\",\"body\":{\"status\":1,\"multisigAccountId\":\"order-1\",\"addressList\":[],\"acceptStatus\":false,\"acceptAddressList\":[\"THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB\"]}}";

        let _payload: crate::mqtt::payload::incoming::Message =
            wallet_utils::serde_func::serde_from_str(message).unwrap();
    }

    #[test]
    fn test_2() {
        let message = "{\"status\":1,\"multisigAccountId\":\"order-1\",\"addressList\":[],\"acceptStatus\":false,\"acceptAddressList\":[\"THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB\"]}";

        let _payload: crate::mqtt::payload::incoming::signature::OrderMultiSignAcceptCompleteMsg =
            wallet_utils::serde_func::serde_from_str(message).unwrap();
    }
}
