use wallet_database::dao::multisig_account::MultisigAccountDaoV1;

use crate::notify::event::multisig::OrderMultisignCanceledFrontend;

// 发起方取消多签账号消息，参与方同步自己多签账号的状态
use super::OrderMultiSignCancel;

impl OrderMultiSignCancel {
    pub(crate) async fn exec(self, _msg_id: &str) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let OrderMultiSignCancel {
            ref multisig_account_id,
        } = self;

        let multisig_account = MultisigAccountDaoV1::find_by_id(multisig_account_id, &*pool)
            .await
            .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?;

        // check
        if let Some(multisig_account) = multisig_account {
            MultisigAccountDaoV1::logic_del_multisig_account(multisig_account_id, &*pool)
                .await
                .map_err(|e| crate::ServiceError::Database(e.into()))?;

            // let r#type = SystemNotificationType::MultisigCanceled;
            // let content = Content::MultisigCanceled {
            //     multisig_account_id: multisig_account.id.clone(),
            //     multisig_account_address: multisig_account.address.clone(),
            //     multisig_account_name: multisig_account.name.clone(),
            // };

            // let mut system_notification_service = SystemNotificationService::new(repo);
            // use wallet_database::repositories::system_notification::SystemNotificationRepoTrait as _;

            // system_notification_service
            //     .repo
            //     .upsert(msg_id, r#type.to_i8(), content.serialize()?, 0)
            //     .await?;

            let data = crate::notify::NotifyEvent::OrderMultisignCanceled(
                OrderMultisignCanceledFrontend {
                    multisig_account_id: multisig_account.id,
                    multisig_account_address: multisig_account.address,
                    address_type: multisig_account.address_type,
                },
            );
            crate::notify::FrontendNotifyEvent::new(data).send().await?;
        }

        Ok(())
    }
}
