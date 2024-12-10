use wallet_database::{
    dao::{multisig_account::MultisigAccountDaoV1, multisig_member::MultisigMemberDaoV1},
    entities::multisig_account::MultisigAccountStatus,
};

use crate::notify::event::{multisig::OrderMultiSignAcceptCompleteMsgFrontend, other::ErrFront};

use super::OrderMultiSignAcceptCompleteMsg;

// 参与方同意后、同步数据给其他的成员同步对应的状态数据(多签账号数据状态流转)
impl OrderMultiSignAcceptCompleteMsg {
    pub(crate) async fn exec(self, _msg_id: &str) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let event_name = self.name();
        let OrderMultiSignAcceptCompleteMsg {
            status,
            ref multisig_account_id,
            accept_address_list: _,
            address_list,
            accept_status,
        } = self;

        let Some(account) = MultisigAccountDaoV1::find_by_id(multisig_account_id, pool.as_ref())
            .await
            .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?
        else {
            tracing::error!("[order multisig accept complete msg] multisig account not found");
            let err = crate::ServiceError::Business(crate::MultisigAccountError::NotFound.into());

            let data = crate::notify::NotifyEvent::Err(ErrFront {
                event: event_name,
                message: err.to_string(),
            });
            crate::notify::FrontendNotifyEvent::new(data).send().await?;
            return Err(err);
        };

        for address in address_list.iter() {
            let multi_account_id = &account.id;
            MultisigMemberDaoV1::sync_confirmed_and_pubkey_status(
                multi_account_id,
                &address.address,
                &address.pubkey,
                address.status,
                &address.uid,
                pool.as_ref(),
            )
            .await
            .map_err(|e| crate::ServiceError::Database(e.into()))?;

            let member = MultisigMemberDaoV1::find_records_by_id(&account.id, pool.as_ref())
                .await
                .map_err(|e| crate::ServiceError::Database(e.into()))?;

            let mut flag = true;
            for item in member.0.iter() {
                if item.confirmed != 1 {
                    flag = false;
                    break;
                }
            }
            if flag {
                // 所有owner都确认过，将多签账户的状态设置为2
                MultisigAccountDaoV1::sync_status(
                    multi_account_id,
                    MultisigAccountStatus::Confirmed,
                    pool.as_ref(),
                )
                .await
                .map_err(crate::SystemError::Database)?;
            }
        }

        // 确认状态(0,未确认,1,已确认)
        // if status == 1 {
        //     let notification = Notification::new_multisig_notification(
        //         &account.name,
        //         &account.address,
        //         multisig_account_id,
        //         NotificationType::Invite,
        //     );
        //     let r#type = SystemNotificationType::MultisigAcceptJoin;
        //     // let content = Content::MultisigAcceptJoin {
        //     //     multisig_account_id: multisig_account_id.to_string(),
        //     //     multisig_account_address: account.address.clone(),
        //     //     multisig_account_name: account.name,
        //     //     accept_address_list,
        //     // };

        //     let mut system_notification_service = SystemNotificationService::new(repo);
        //     use wallet_database::repositories::system_notification::SystemNotificationRepoTrait as _;

        //     system_notification_service
        //         .repo
        //         .upsert(msg_id, r#type.to_i8(), notification.serialize()?, 0)
        //         .await?;
        // }

        let data = crate::notify::NotifyEvent::OrderMultiSignAcceptCompleteMsg(
            OrderMultiSignAcceptCompleteMsgFrontend {
                status: status as i8,
                multisign_address: account.address,
                address_list,
                accept_status,
            },
        );
        crate::notify::FrontendNotifyEvent::new(data).send().await?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::mqtt::payload::incoming::Message;

    #[test]
    fn test_() {
        let raw = r#"
        {
            "bizType": "ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG",
            "body": {
                "orderId": "66b328e9eafe8b248415bbb3",
                "status": 1,
                "acceptStatus": false,
                "confirmList": [
                    {
                        "acceptAddress": "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
                        "acceptStatus": true,
                        "createTime": "2024-08-07 15:57:37.268",
                        "id": "66b328f1eafe8b248415bbb5",
                        "messageContent": "{\"id\":\"159780934751752192\",\"orderId\":\"66b328e9eafe8b248415bbb3\",\"name\":\"Multisig-tron-1\",\"initiatorAddr\":\"TJk5nUGoaMFmcrmSubFD11w6DVf5uX5yi6\",\"address\":\"TJk5nUGoaMFmcrmSubFD11w6DVf5uX5yi6\",\"chainCode\":\"tron\",\"threshold\":2,\"memeber\":[{\"name\":\"alice\",\"address\":\"THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB\"},{\"name\":\"bob\",\"address\":\"TByQCQiBUtbLQNh6r1ZPNwBJC1jLgZjkuk\"},{\"name\":\"bo\",\"address\":\"TJk5nUGoaMFmcrmSubFD11w6DVf5uX5yi6\"}]}",
                        "orderId": "66b328e9eafe8b248415bbb3",
                        "sendMessage": true,
                        "status": 1,
                        "updateTime": "2024-08-07 15:57:37.268"
                    }
                ],
                "acceptAddressList": [
                    "THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB",
                    "TByQCQiBUtbLQNh6r1ZPNwBJC1jLgZjkuk"
                ]
            },
            "clientId": "guangxiang",
            "deviceType": "ANDROID",
            "sn": "guangxiang"
        }
        "#;

        let res = serde_json::from_str::<Message>(&raw);
        println!("res: {res:?}");
    }
}
