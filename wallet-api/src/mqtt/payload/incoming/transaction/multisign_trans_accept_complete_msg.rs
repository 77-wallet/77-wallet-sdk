use wallet_database::{
    dao::{multisig_queue::MultisigQueueDaoV1, multisig_signatures::MultisigSignatureDaoV1},
    entities::multisig_signatures::{MultisigSignatureStatus, NewSignatureEntity},
    repositories::multisig_queue::MultisigQueueRepo,
};

// 签名的结果同步给所有人
use super::{MultiSignTransAcceptCompleteMsg, MultiSignTransAcceptCompleteMsgBody};

impl MultiSignTransAcceptCompleteMsg {
    pub(crate) async fn exec(self, _msg_id: &str) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let MultiSignTransAcceptCompleteMsg(body) = &self;

        for msg in body {
            let params: NewSignatureEntity = msg.try_into()?;

            MultisigSignatureDaoV1::create_or_update(&params, pool.clone())
                .await
                .map_err(|e| crate::ServiceError::Database(e.into()))?;

            let data = crate::notify::NotifyEvent::MultiSignTransAcceptCompleteMsg(msg.to_owned());
            crate::notify::FrontendNotifyEvent::new(data).send().await?;
        }

        // sync sign status
        if let Some(item) = body.first() {
            let account =
                MultisigQueueDaoV1::find_by_id_with_account(&item.queue_id, pool.as_ref())
                    .await
                    .map_err(|e| crate::ServiceError::Database(e.into()))?;

            if let Some(account) = account {
                MultisigQueueRepo::sync_sign_status(
                    &item.queue_id,
                    &account.account_id,
                    account.threshold,
                    pool.clone(),
                )
                .await?;
            }
        }

        Ok(())
    }
}

impl TryFrom<&MultiSignTransAcceptCompleteMsgBody> for NewSignatureEntity {
    fn try_from(value: &MultiSignTransAcceptCompleteMsgBody) -> Result<Self, Self::Error> {
        let status: MultisigSignatureStatus = (value.status as i32).try_into()?;
        Ok(Self {
            queue_id: value.queue_id.to_string(),
            address: value.address.to_string(),
            signature: value.signature.to_string(),
            status,
        })
    }

    type Error = crate::ServiceError;
}

#[cfg(test)]
mod test {
    use crate::mqtt::payload::incoming::transaction::MultiSignTransAcceptCompleteMsg;

    #[test]
    fn test_() {
        let raw = r#"
        [
                {
                    "address": "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
                    "queue_id": "160055304875282432",
                    "signature": "cde600c686b6ac9359c926a78cddaca34c3894629694225b60ade7b309abdd9e6e1f38b5d346ea99114eaaad9cbd390f7c8cea2dd3499d04065ab8641b9d831300",
                    "status": 1
                },
                {
                    "address": "TByQCQiBUtbLQNh6r1ZPNwBJC1jLgZjkuk",
                    "queue_id": "160055304875282432",
                    "signature": "bd2a584a46c1207b5bf0b0f8e7f9b9f3a40cc74911001b7fb47a71fd029c18f8241dc07de7b9fd1fe13b97d29bc2c42541f3406cf7f6d3a91d0e621810cfa01400",
                    "status": 1
                }
            ]
        "#;
        let res = serde_json::from_str::<MultiSignTransAcceptCompleteMsg>(&raw);
        println!("res: {res:?}");
    }
}
