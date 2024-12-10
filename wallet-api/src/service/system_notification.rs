use wallet_database::{
    dao::{bill::BillDao, multisig_account::MultisigAccountDaoV1},
    entities::system_notification::CreateSystemNotificationEntity,
    repositories::system_notification::SystemNotificationRepoTrait,
};

pub struct SystemNotificationService<T: SystemNotificationRepoTrait> {
    pub repo: T,
}

impl<T: SystemNotificationRepoTrait> SystemNotificationService<T> {
    pub fn new(repo: T) -> Self {
        Self { repo }
    }

    pub async fn add_system_notification(
        self,
        id: &str,
        notification: crate::system_notification::Notification,
        status: i8,
    ) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        let r#type = notification.type_name();
        let content = notification.serialize()?;
        tx.upsert(id, &r#type, content, status)
            .await
            .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?;

        Ok(())
    }

    pub async fn add_system_notification_with_key_value(
        self,
        id: &str,
        notification: crate::system_notification::Notification,
        status: i8,
        key: Option<String>,
        value: Option<String>,
    ) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        let r#type = notification.type_name();
        let content = notification.serialize()?;
        tx.upsert_with_key_value(id, &r#type, content, status, key, value)
            .await
            .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?;
        Ok(())
    }

    pub async fn add_multi_system_notification_with_key_value(
        self,
        reqs: &[CreateSystemNotificationEntity],
    ) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        tx.upsert_multi_with_key_value(reqs)
            .await
            .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?;
        Ok(())
    }

    pub async fn update_system_notification_status(
        self,
        id: Option<String>,
        status: i8,
    ) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        tx.update_status(id, status)
            .await
            .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?;

        Ok(())
    }

    pub async fn get_system_notification_list(
        self,
        page: i64,
        page_size: i64,
    ) -> Result<
        wallet_database::pagination::Pagination<
            crate::response_vo::system_notification::SystemNotification,
        >,
        crate::ServiceError,
    > {
        let pool = crate::Context::get_global_sqlite_pool()?;
        let mut tx = self.repo;
        let list = tx
            .list(page, page_size)
            .await
            .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?;

        let data = list.data;
        let mut res = Vec::new();
        for notif in data {
            let no: crate::system_notification::Notification =
                wallet_utils::serde_func::serde_from_str(&notif.content)?;
            let val = match no {
                crate::system_notification::Notification::Multisig(multisig_notification) => {
                    match MultisigAccountDaoV1::find_by_id(
                        &multisig_notification.multisig_account_id,
                        &*pool,
                    )
                    .await?
                    {
                        Some(_) => (notif, true).into(),
                        None => (notif, false).into(),
                    }
                }
                crate::system_notification::Notification::Transaction(transaction_notification) => {
                    let hash = transaction_notification.transaction_hash;
                    match BillDao::get_one_by_hash(&hash, &*pool).await? {
                        Some(_) => (notif, true).into(),
                        None => (notif, false).into(),
                    }
                }
            };
            res.push(val);
        }

        let list = wallet_database::pagination::Pagination {
            page,
            page_size,
            total_count: list.total_count,
            data: res,
        };

        Ok(list)
    }
}

// use crate::global_context::GlobalContext;

// use super::Service;

// impl Service {
//     pub async fn add_system_notification(
//         &self,
//         r#type: i8,
//         content: String,
//         status: i8,
//     ) -> Result<(), crate::ServiceError> {
//         self.get_global_sqlite_context()?
//             .add_system_notification(r#type, content, status)
//             .await
//             .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?;

//         Ok(())
//     }

//     pub async fn update_system_notification_status(
//         &self,
//         id: Option<i32>,
//         status: i8,
//     ) -> Result<(), crate::ServiceError> {
//         self.get_global_sqlite_context()?
//             .update_system_notification_status(id, status)
//             .await
//             .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))
//     }

//     pub async fn get_system_notification_list(
//         &self,
//         page: i64,
//         page_size: i64,
//     ) -> Result<
//         wallet_database::pagination::Pagination<
//             wallet_database::sqlite::logic::system_notification::SystemNotificationEntity,
//         >,
//         crate::ServiceError,
//     > {
//         self.get_global_sqlite_context()?
//             .system_notification_list(page, page_size)
//             .await
//             .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))
//     }
// }
