use wallet_database::{
    dao::multisig_account::MultisigAccountDaoV1,
    entities::multisig_account::{MultisigAccountPayStatus, MultisigAccountStatus},
};

use crate::notify::event::{multisig::OrderMultiSignServiceCompleteFrontend, other::ErrFront};

/*
    {
        "clientId": "wenjing",
        "sn": "device458",
        "deviceType": "typeC",
        "bizType": "ORDER_MULTI_SIGN_SERVICE_COMPLETE",
        "body": {
            "orderId": "order-1",
            "status": true,
            "type": "1"
        }
    }
    {
        "clientId": "wenjing",
        "sn": "device458",
        "deviceType": "typeC",
        "bizType": "ORDER_MULTI_SIGN_SERVICE_COMPLETE",
        "body": {
            "orderId": "order-1",
            "status": true,
            "type": "2"
        }
    }
*/
// 部署完成后，服务费hash以及部署hash的同步。
use super::OrderMultiSignServiceComplete;

impl OrderMultiSignServiceComplete {
    pub(crate) async fn exec(self, _msg_id: &str) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let OrderMultiSignServiceComplete {
            ref multisig_account_id,
            status,
            r#type,
        } = self;
        let Some(account) = MultisigAccountDaoV1::find_by_id(multisig_account_id, pool.as_ref())
            .await
            .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?
        else {
            tracing::error!("[order multisig service complete] multisig account not found");
            let err = crate::ServiceError::Business(crate::MultisigAccountError::NotFound.into());

            let data = crate::notify::NotifyEvent::Err(ErrFront {
                event: self.name(),
                message: err.to_string(),
            });
            crate::notify::FrontendNotifyEvent::new(data).send().await?;
            return Err(err);
        };

        let multi_account_id = account.id;

        // 更新订单结果

        // 更新多签账户手续费状态
        if r#type == 1 {
            let status_i8 = if status {
                // service.query_tx_result(&tx_hash).await?;
                MultisigAccountStatus::OnChain.to_i8()
            } else {
                MultisigAccountStatus::OnChainFail.to_i8()
            };
            MultisigAccountDaoV1::update_status(
                &multi_account_id,
                Some(status_i8),
                None,
                pool.as_ref(),
            )
            .await
            .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?;
        }
        // 更新多签账户服务费状态
        else if r#type == 2 {
            let pay_status = if status {
                MultisigAccountPayStatus::Paid.to_i8()
            } else {
                MultisigAccountPayStatus::PaidFail.to_i8()
            };
            MultisigAccountDaoV1::update_status(
                &multi_account_id,
                None,
                Some(pay_status),
                pool.as_ref(),
            )
            .await
            .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?;
        }

        let data = crate::notify::NotifyEvent::OrderMultiSignServiceComplete(
            OrderMultiSignServiceCompleteFrontend {
                multisign_address: account.address,
                status,
                r#type,
            },
        );
        crate::notify::FrontendNotifyEvent::new(data).send().await?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::mqtt::payload::incoming::signature::OrderMultiSignServiceComplete;

    #[test]
    fn test_() {
        let raw = r#"
        {
            "hash": "dc91c064567fa7276d5888c7973e44f74c64a9b89b9517346ca677f550929cf1",
            "status": true,
            "type": 1,
            "orderId": "66b328e9eafe8b248415bbb3"
        }
        "#;
        let res = serde_json::from_str::<OrderMultiSignServiceComplete>(&raw);
        println!("res: {res:?}");
    }
}
