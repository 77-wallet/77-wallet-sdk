use crate::{
    api::ReturnType, manager::WalletManager, messaging::system_notification::Notification,
    service::system_notification::SystemNotificationService,
};

impl WalletManager {
    pub async fn add_system_notification(
        &self,
        id: &str,
        notification: Notification,
        status: i8,
    ) -> ReturnType<()> {
        SystemNotificationService::new(self.ctx)
            .add_system_notification(id, notification, status)
            .await
    }

    pub async fn get_system_notification_list(
        &self,
        page: i64,
        page_size: i64,
    ) -> ReturnType<
        wallet_database::pagination::Pagination<
            crate::response_vo::standard_wallet::system_notification::SystemNotification,
        >,
    > {
        SystemNotificationService::new(self.ctx).get_system_notification_list(page, page_size).await
    }

    pub async fn update_system_notification_status(
        &self,
        id: Option<String>,
        status: i8,
    ) -> ReturnType<()> {
        SystemNotificationService::new(self.ctx).update_system_notification_status(id, status).await
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use crate::{
        messaging::system_notification::{
            AccountType, Notification, NotificationType, TransactionStatus,
        },
        testkit::env::get_manager,
    };
    use anyhow::Result;
    use wallet_database::entities::bill::BillKind;
    // 添加这个引用

    #[tokio::test]
    async fn test_add_system_notification() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        //670e5b356b58cd4047fdf46f	1	{"txHash":"0d9c3c1b9a959b6a240612e2fe885e9a6598b0161e5bb2c9dca70c7c41bb23ae","walletName":"asdasd","accountName":"账户2","uid":"cd2ac48fa33ba24a8bc0d89e7658a2cd","transferType":0,"status":2}	1	1728994157	2024-10-15T12:09:38.106672222+00:00

        {
            let notification = Notification::new_multisig_notification(
                "Default Account",
                "TRX1234567890123456789012345678901234",
                "multisig123456",
                NotificationType::DeployCompletion,
            );
            tracing::info!("notification: {notification:?}");
            // let business_id = Some("123123123".to_string());
            let status = 1;
            let _res = wallet_manager.add_system_notification("1232", notification, status).await;
        }
        {
            let notification = Notification::new_multisig_notification(
                "Default Account",
                "TRX1234567890123456789012345678901234",
                "multisig123456",
                NotificationType::DeployInvite,
            );

            tracing::info!("notification: {notification:?}");
            // let business_id = Some("456456456".to_string());
            let status = 2;
            let _res = wallet_manager.add_system_notification("1238", notification, status).await;
        }
        {
            let notification = Notification::new_multisig_notification(
                "Default Account",
                "TRX1234567890123456789012345678901234",
                "multisig123456",
                NotificationType::Confirmation,
            );

            tracing::info!("notification: {notification:?}");
            // let business_id = Some("456456456".to_string());
            let status = 2;
            let _res = wallet_manager.add_system_notification("1237", notification, status).await;
        }

        {
            let notification = Notification::new_transaction_notification(
                AccountType::Regular,
                "default_name",
                "default_address",
                "default_address",
                "default_address",
                1.0,
                "TRX",
                "tron",
                &TransactionStatus::NotSent,
                "0x0000000000000000000000000000000000000000",
                Some("0x0000000000000000000000000000000000000000".to_string()),
                &NotificationType::TransferFailure,
            );
            tracing::info!("notification: {notification:?}");
            // let business_id = Some("789789789".to_string());
            let status = 3;
            let _res = wallet_manager.add_system_notification("1235", notification, status).await;
        }

        {
            let notification = Notification::new_transaction_notification(
                AccountType::Multisig,
                "default_name",
                "default_address",
                "default_address",
                "default_address",
                1.0,
                "TRX",
                "tron",
                &TransactionStatus::Sent,
                "0x0000000000000000000000000000000000000000",
                Some("0x0000000000000000000000000000000000000000".to_string()),
                &NotificationType::TransferSuccess,
            );

            tracing::info!("notification: {notification:?}");
            // let business_id = Some("321321321".to_string());
            let status = 4;
            let _res = wallet_manager.add_system_notification("1236", notification, status).await;
        }
        {
            let notification = Notification::new_transaction_notification(
                AccountType::Multisig,
                "default_name",
                "default_address",
                "default_address",
                "default_address",
                1.0,
                "TRX",
                "tron",
                &TransactionStatus::Received,
                "0x0000000000000000000000000000000000000000",
                Some("0x0000000000000000000000000000000000000000".to_string()),
                &NotificationType::ReceiveSuccess,
            );
            tracing::info!("notification: {notification:?}");
            let status = 4;
            let _res = wallet_manager.add_system_notification("1239", notification, status).await;
        }

        {
            let account_name = "Default Account";
            let account_address = "TRX1234567890123456789012345678901234";
            let multisig_account_id = "multisig123456";
            let r#type = BillKind::BatchDelegateBandwidth;
            let notification_type = NotificationType::Confirmation;

            let notification = Notification::new_confirmation_notification(
                account_name,
                account_address,
                multisig_account_id,
                r#type,
                notification_type,
            );

            tracing::info!("notification: {notification:?}");
            let status = 4;
            let _res = wallet_manager.add_system_notification("1240", notification, status).await;
        }

        let res = wallet_manager.get_system_notification_list(0, 10).await?;
        tracing::info!("res: {res:#?}");

        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("res: {res}");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_system_notification_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        // let status = 0;
        let res = wallet_manager.get_system_notification_list(0, 10).await?;
        tracing::info!("res: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("res: {res}");
        Ok(())
    }

    #[tokio::test]
    async fn test_update_system_notification_status() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        let status = 1;
        let res = wallet_manager.update_system_notification_status(None, status).await;
        tracing::info!("res: {res:?}");

        Ok(())
    }
}
