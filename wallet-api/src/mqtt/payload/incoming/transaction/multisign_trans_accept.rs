use crate::{
    service::system_notification::SystemNotificationService,
    system_notification::{Notification, NotificationType},
};
use wallet_database::{
    dao::{
        multisig_account::MultisigAccountDaoV1, multisig_queue::MultisigQueueDaoV1,
        multisig_signatures::MultisigSignatureDaoV1,
    },
    entities::multisig_queue::{MultisigQueueStatus, NewMultisigQueueEntity},
    factory::RepositoryFactory,
    repositories::multisig_queue::MultisigQueueRepo,
};

// 多签交易队列的创建 同步给所有人
use super::MultiSignTransAccept;

/*
    {
        "clientId": "wenjing",
        "sn": "device460",
        "deviceType": "typeE",
        "bizType": "MULTI_SIGN_TRANS_ACCEPT",
        "body": {
            "id": "tx123456789",
            "fromAddr": "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
            "toAddr": "0xReceiverAddress",
            "value": "1000",
            "expiration": 1698806400,
            "symbol": "eth",
            "chainCode": "eth",
            "tokenAddr": null,
            "msgHash": "0xMessageHash",
            "txHash": "0xTransactionHash",
            "rawData": "raw transaction data",
            "status": 0,
            "notes": "This is a test transaction",
            "createdAt": "2024-07-30T12:34:56Z"
        }
    }
*/

//MultiSignTransAccept
impl From<&MultiSignTransAccept> for NewMultisigQueueEntity {
    fn from(value: &MultiSignTransAccept) -> Self {
        Self {
            id: value.id.clone(),
            from_addr: value.from_addr.clone(),
            to_addr: value.to_addr.clone(),
            value: value.value.clone(),
            symbol: value.symbol.clone(),
            expiration: value.expiration,
            chain_code: value.chain_code.clone(),
            token_addr: value.token_addr.clone(),
            msg_hash: value.msg_hash.clone(),
            tx_hash: value.tx_hash.clone(),
            raw_data: value.raw_data.clone(),
            status: MultisigQueueStatus::PendingSignature,
            notes: value.notes.clone(),
            signatures: vec![],
            fail_reason: "".to_string(),
            account_id: value.account_id.clone(),
            create_at: value.created_at,
        }
    }
}

impl MultiSignTransAccept {
    pub(crate) async fn exec(self, _msg_id: &str) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = RepositoryFactory::repo(pool.clone());
        let MultiSignTransAccept {
            ref id,
            ref from_addr,
            ref to_addr,
            ref value,
            expiration,
            ref symbol,
            ref chain_code,
            ref token_addr,
            ref msg_hash,
            ref tx_hash,
            ref raw_data,
            status,
            ref notes,
            created_at,
            ref signatures,
            ref account_id,
        } = self;
        // 新增交易队列数据
        let params: NewMultisigQueueEntity = (&self).into();

        let _res = MultisigQueueDaoV1::create_queue(&params, pool.as_ref())
            .await
            .map_err(|e| crate::ServiceError::Database(e.into()))?;

        for sig in signatures {
            MultisigSignatureDaoV1::create_or_update(sig, pool.clone())
                .await
                .map_err(|e| crate::ServiceError::Database(e.into()))?;
        }

        if let Some(multisig_account) =
            MultisigAccountDaoV1::find_by_address(from_addr, pool.as_ref())
                .await
                .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?
        {
            // 同步签名的状态
            MultisigQueueRepo::sync_sign_status(id, account_id, multisig_account.threshold, pool)
                .await?;

            // 系统通知
            let notification = Notification::new_multisig_notification(
                &multisig_account.name,
                &multisig_account.address,
                &multisig_account.id,
                NotificationType::TransferConfirmation,
            );
            let system_notification_service = SystemNotificationService::new(repo);
            system_notification_service
                .add_system_notification(id, notification, 0)
                .await?;
        };

        let data = crate::notify::NotifyEvent::MultiSignTransAccept(
            crate::notify::event::transaction::MultiSignTransAcceptFrontend {
                id: id.to_string(),
                from_addr: from_addr.to_string(),
                to_addr: to_addr.to_string(),
                value: value.to_string(),
                expiration,
                symbol: symbol.to_string(),
                chain_code: chain_code.to_string(),
                token_addr: token_addr.clone(),
                msg_hash: msg_hash.to_string(),
                tx_hash: tx_hash.to_string(),
                raw_data: raw_data.to_string(),
                status,
                notes: notes.to_string(),
                created_at,
            },
        );
        crate::notify::FrontendNotifyEvent::new(data).send().await?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::mqtt::payload::incoming::transaction::MultiSignTransAccept;

    #[test]
    fn test_() {
        let raw = r#"
        {
            "id": 1,
            "withdrawId": "159814456979886080"
        }
        "#;
        let res = serde_json::from_str::<MultiSignTransAccept>(&raw);
        println!("res: {res:?}");
    }
}
