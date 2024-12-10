use crate::{api::ReturnType, service::system_notification::SystemNotificationService};

impl crate::WalletManager {
    pub async fn add_system_notification(
        &self,
        id: &str,
        notification: crate::system_notification::Notification,
        status: i8,
    ) -> ReturnType<()> {
        SystemNotificationService::new(self.repo_factory.resuource_repo())
            .add_system_notification(id, notification, status)
            .await?
            .into()
    }

    pub async fn get_system_notification_list(
        &self,
        page: i64,
        page_size: i64,
    ) -> ReturnType<
        wallet_database::pagination::Pagination<
            crate::response_vo::system_notification::SystemNotification,
        >,
    > {
        SystemNotificationService::new(self.repo_factory.resuource_repo())
            .get_system_notification_list(page, page_size)
            .await?
            .into()
    }

    pub async fn update_system_notification_status(
        &self,
        id: Option<String>,
        status: i8,
    ) -> ReturnType<()> {
        SystemNotificationService::new(self.repo_factory.resuource_repo())
            .update_system_notification_status(id, status)
            .await?
            .into()
    }
}

#[cfg(test)]
mod test {
    use crate::system_notification::{
        AccountType, Notification, NotificationType, TransactionStatus,
    };
    use crate::test::env::{setup_test_environment, TestData};
    use anyhow::Result;
    // 添加这个引用

    #[tokio::test]
    async fn test_add_system_notification() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

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
            let _res = wallet_manager
                .add_system_notification("1232", notification, status)
                .await;
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
            let _res = wallet_manager
                .add_system_notification("1238", notification, status)
                .await;
        }
        {
            let notification = Notification::new_multisig_notification(
                "Default Account",
                "TRX1234567890123456789012345678901234",
                "multisig123456",
                NotificationType::TransferConfirmation,
            );

            tracing::info!("notification: {notification:?}");
            // let business_id = Some("456456456".to_string());
            let status = 2;
            let _res = wallet_manager
                .add_system_notification("1237", notification, status)
                .await;
        }

        {
            let notification = Notification::new_transaction_notification(
                AccountType::Regular,
                "default_name",
                "default_address",
                1.0,
                "TRX",
                &TransactionStatus::NotSent,
                "0x0000000000000000000000000000000000000000",
                &NotificationType::TransferFailure,
            );
            tracing::info!("notification: {notification:?}");
            // let business_id = Some("789789789".to_string());
            let status = 3;
            let _res = wallet_manager
                .add_system_notification("1235", notification, status)
                .await;
        }

        {
            let notification = Notification::new_transaction_notification(
                AccountType::Multisig,
                "default_name",
                "default_address",
                1.0,
                "TRX",
                &TransactionStatus::Sent,
                "0x0000000000000000000000000000000000000000",
                &NotificationType::TransferSuccess,
            );

            tracing::info!("notification: {notification:?}");
            // let business_id = Some("321321321".to_string());
            let status = 4;
            let _res = wallet_manager
                .add_system_notification("1236", notification, status)
                .await;
        }
        {
            let notification = Notification::new_transaction_notification(
                AccountType::Multisig,
                "default_name",
                "default_address",
                1.0,
                "TRX",
                &TransactionStatus::Received,
                "0x0000000000000000000000000000000000000000",
                &NotificationType::ReceiveSuccess,
            );

            tracing::info!("notification: {notification:?}");
            // let business_id = Some("321321321".to_string());
            let status = 4;
            let _res = wallet_manager
                .add_system_notification("1239", notification, status)
                .await;
        }

        let res = wallet_manager.get_system_notification_list(0, 10).await;
        tracing::info!("res: {res:#?}");

        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("res: {res}");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_system_notification_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        // let status = 0;
        let res = wallet_manager.get_system_notification_list(0, 10).await;
        tracing::info!("res: {res:?}");
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("res: {res}");
        Ok(())
    }

    #[tokio::test]
    async fn test_update_system_notification_status() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let status = 1;
        let res = wallet_manager
            .update_system_notification_status(None, status)
            .await;
        tracing::info!("res: {res:?}");

        Ok(())
    }
}
