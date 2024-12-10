use crate::{
    entities::system_notification::{CreateSystemNotificationEntity, SystemNotificationEntity},
    pagination::Pagination,
};

#[async_trait::async_trait]
pub trait SystemNotificationRepoTrait: super::TransactionTrait {
    async fn detail(&mut self, id: &str) -> Result<Option<SystemNotificationEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        let req = crate::entities::system_notification::QueryReq {
            key: None,
            value: None,
            id: Some(id.to_string()),
        };
        crate::execute_with_executor!(executor, SystemNotificationEntity::detail, &req)
    }

    async fn upsert(
        &mut self,
        id: &str,
        r#type: &str,
        content: String,
        status: i8,
    ) -> Result<Vec<SystemNotificationEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            SystemNotificationEntity::upsert,
            id,
            r#type,
            content,
            status
        )
    }

    async fn upsert_with_key_value(
        &mut self,
        id: &str,
        r#type: &str,
        content: String,
        status: i8,
        key: Option<String>,
        value: Option<String>,
    ) -> Result<Vec<SystemNotificationEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            SystemNotificationEntity::upsert_with_key_value,
            id,
            r#type,
            content,
            status,
            key,
            value
        )
    }

    async fn upsert_multi_with_key_value(
        &mut self,
        reqs: &[CreateSystemNotificationEntity],
    ) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            SystemNotificationEntity::upsert_multi_with_key_value,
            reqs
        )
    }

    async fn detail_by_key(
        &mut self,
        key: Option<String>,
        value: Option<String>,
    ) -> Result<Option<SystemNotificationEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        let req = crate::entities::system_notification::QueryReq {
            key,
            value,
            id: None,
        };
        crate::execute_with_executor!(executor, SystemNotificationEntity::detail, &req)
    }

    async fn list(
        &mut self,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<SystemNotificationEntity>, crate::Error> {
        let executor = self.get_db_pool();
        SystemNotificationEntity::system_notification_list_page(executor, page, page_size).await
    }

    async fn update_status(&mut self, id: Option<String>, status: i8) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            SystemNotificationEntity::update_system_notification_status,
            id,
            status
        )
    }

    async fn count_unread_status(&mut self) -> Result<i64, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, SystemNotificationEntity::count_status_zero,)
    }
}
