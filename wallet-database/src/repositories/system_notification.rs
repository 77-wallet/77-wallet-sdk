use crate::{
    CoreDbPool,
    dao::system_notification::SystemNotificationDao,
    entities::system_notification::{CreateSystemNotificationEntity, SystemNotificationEntity},
    pagination::Pagination,
};

pub struct SystemNotificationRepo;

impl SystemNotificationRepo {
    pub fn build_create(
        id: &str,
        r#type: &str,
        content: &str,
        status: i8,
        key: Option<String>,
        value: Option<String>,
    ) -> CreateSystemNotificationEntity {
        CreateSystemNotificationEntity::new(id, r#type, content, status, key, value)
    }

    pub async fn upsert(
        pool: &CoreDbPool,
        id: &str,
        r#type: &str,
        content: String,
        status: i8,
    ) -> Result<Vec<SystemNotificationEntity>, crate::Error> {
        SystemNotificationDao::upsert(pool.write_ref(), id, r#type, content, status).await
    }

    pub async fn upsert_with_key_value(
        pool: &CoreDbPool,
        id: &str,
        r#type: &str,
        content: String,
        status: i8,
        key: Option<String>,
        value: Option<String>,
    ) -> Result<Vec<SystemNotificationEntity>, crate::Error> {
        SystemNotificationDao::upsert_with_key_value(
            pool.write_ref(),
            id,
            r#type,
            content,
            status,
            key,
            value,
        )
        .await
    }

    pub async fn upsert_multi_with_key_value(
        pool: &CoreDbPool,
        reqs: &[CreateSystemNotificationEntity],
    ) -> Result<(), crate::Error> {
        SystemNotificationDao::upsert_multi_with_key_value(pool.write_ref(), reqs).await
    }

    pub async fn list_page(
        pool: &CoreDbPool,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<SystemNotificationEntity>, crate::Error> {
        SystemNotificationDao::system_notification_list_page(pool.read_ref(), page, page_size).await
    }

    pub async fn update_status(
        pool: &CoreDbPool,
        id: Option<String>,
        status: i8,
    ) -> Result<(), crate::Error> {
        SystemNotificationDao::update_system_notification_status(pool.write_ref(), id, status).await
    }

    pub async fn delete(pool: &CoreDbPool, id: &str) -> Result<(), crate::Error> {
        SystemNotificationDao::delete_system_notification(pool.write_ref(), id).await
    }

    pub async fn count_unread(pool: &CoreDbPool) -> Result<i64, crate::Error> {
        SystemNotificationDao::count_status_zero(pool.read_ref()).await
    }

    pub async fn find_by_key_value(
        pool: &CoreDbPool,
        key: Option<&str>,
        value: Option<&str>,
    ) -> Result<Option<SystemNotificationEntity>, crate::Error> {
        SystemNotificationDao::detail(pool.read_ref(), key, value, None).await
    }

    pub async fn find_by_id(
        id: &str,
        pool: &CoreDbPool,
    ) -> Result<Option<SystemNotificationEntity>, crate::Error> {
        SystemNotificationDao::detail(pool.read_ref(), None, None, Some(id)).await
    }
}

#[cfg(test)]
mod tests {
    use super::SystemNotificationRepo;
    use crate::{
        dao::system_notification::SystemNotificationDao, repositories::test_helper::setup_core_pool,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn make_temp_dir(prefix: &str) -> String {
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "{}_{}_{}_{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
            seq
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().to_string()
    }

    #[tokio::test]
    async fn system_notification_repo_list_and_detail_work_without_explicit_transaction() {
        let dir = make_temp_dir("wallet_db_repo_system_notification");
        let ctx = crate::SqliteContext::new(&dir, Some("data.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();
        let core_pool = crate::CoreDbPool::new(pool.clone());

        SystemNotificationRepo::upsert_multi_with_key_value(
            &core_pool,
            &[SystemNotificationRepo::build_create(
                "n1",
                "system",
                "hello",
                0,
                Some("k".to_string()),
                Some("v".to_string()),
            )],
        )
        .await
        .unwrap();

        let detail =
            super::SystemNotificationRepo::find_by_id("n1", &core_pool).await.unwrap().unwrap();
        assert_eq!(detail.id, "n1");

        let page = super::SystemNotificationRepo::list_page(&core_pool, 0, 10).await.unwrap();
        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].id, "n1");

        let find =
            super::SystemNotificationRepo::find_by_key_value(&core_pool, Some("k"), Some("v"))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(find.id, "n1");
    }

    #[tokio::test]
    async fn system_notification_repo_missing_id_returns_none() {
        let pool = setup_core_pool("wallet_db_repo_system_notification_edge").await;
        let missing = super::SystemNotificationRepo::find_by_id("n_missing", &pool).await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn system_notification_repo_tx_rollback_keeps_row_absent() {
        let pool = setup_core_pool("wallet_db_repo_system_notification_rollback").await;

        let mut tx = pool.write_ref().begin().await.unwrap();
        SystemNotificationDao::upsert_multi_with_key_value(
            tx.as_mut(),
            &[SystemNotificationRepo::build_create(
                "n_rb",
                "system",
                "rollback",
                0,
                Some("rk".to_string()),
                Some("rv".to_string()),
            )],
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();

        let found = super::SystemNotificationRepo::find_by_id("n_rb", &pool).await.unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn system_notification_repo_build_create_maps_fields() {
        let entity = SystemNotificationRepo::build_create(
            "id-1",
            "system",
            "content",
            2,
            Some("k".to_string()),
            Some("v".to_string()),
        );

        assert_eq!(entity.id, "id-1");
        assert_eq!(entity.r#type, "system");
        assert_eq!(entity.content, "content");
        assert_eq!(entity.status, 2);
        assert_eq!(entity.key.as_deref(), Some("k"));
        assert_eq!(entity.value.as_deref(), Some("v"));
    }

    #[test]
    fn system_notification_repo_build_create_accepts_empty_key_value() {
        let entity = SystemNotificationRepo::build_create("id-2", "tx", "payload", 0, None, None);
        assert_eq!(entity.id, "id-2");
        assert_eq!(entity.r#type, "tx");
        assert_eq!(entity.content, "payload");
        assert_eq!(entity.status, 0);
        assert!(entity.key.is_none());
        assert!(entity.value.is_none());
    }
}
