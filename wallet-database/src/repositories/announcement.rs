use crate::{
    CoreDbPool,
    dao::announcement::AnnouncementDao,
    entities::announcement::{AnnouncementEntity, CreateAnnouncementVo},
    pagination::Pagination,
};

pub struct AnnouncementRepo {}

impl AnnouncementRepo {
    pub async fn add(
        pool: &CoreDbPool,
        input: Vec<CreateAnnouncementVo>,
    ) -> Result<(), crate::Error> {
        AnnouncementDao::upsert(pool.write_ref(), input).await
    }

    pub async fn update_existing(
        pool: &CoreDbPool,
        input: Vec<CreateAnnouncementVo>,
    ) -> Result<(), crate::Error> {
        AnnouncementDao::update_existing(pool.write_ref(), input).await
    }

    pub async fn list(pool: &CoreDbPool) -> Result<Vec<AnnouncementEntity>, crate::Error> {
        AnnouncementDao::list(pool.read_ref()).await
    }

    pub async fn get_announcement_list(
        pool: &CoreDbPool,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<AnnouncementEntity>, crate::Error> {
        AnnouncementDao::get_announcement_list(pool.read_ref(), page, page_size).await
    }

    pub async fn get_announcement_by_id(
        pool: &CoreDbPool,
        id: &str,
    ) -> Result<Option<AnnouncementEntity>, crate::Error> {
        AnnouncementDao::get_announcement_by_id(pool.read_ref(), id).await
    }

    pub async fn read(pool: &CoreDbPool, id: Option<&str>) -> Result<(), crate::Error> {
        AnnouncementDao::update_status(pool.write_ref(), id, 1).await?;
        Ok(())
    }

    pub async fn count_unread(pool: &CoreDbPool) -> Result<i64, crate::Error> {
        AnnouncementDao::count_status_zero(pool.read_ref()).await
    }

    pub async fn delete(pool: &CoreDbPool, id: &str) -> Result<(), crate::Error> {
        AnnouncementDao::physical_delete(pool.write_ref(), id).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        dao::announcement::AnnouncementDao,
        entities::announcement::CreateAnnouncementVo,
        repositories::{announcement::AnnouncementRepo, test_helper::setup_core_pool},
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
    async fn announcement_repo_queries_work_without_explicit_transaction() {
        let dir = make_temp_dir("wallet_db_repo_announcement");
        let ctx = crate::SqliteContext::new(&dir, Some("data.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();
        let core_pool = crate::CoreDbPool::new(pool);

        AnnouncementRepo::add(
            &core_pool,
            vec![CreateAnnouncementVo {
                id: "a1".to_string(),
                title: "title".to_string(),
                content: "content".to_string(),
                language: "en".to_string(),
                status: 0,
                send_time: None,
            }],
        )
        .await
        .unwrap();

        let one =
            AnnouncementRepo::get_announcement_by_id(&core_pool, "a1").await.unwrap().unwrap();
        assert_eq!(one.id, "a1");

        let page = AnnouncementRepo::get_announcement_list(&core_pool, 0, 10).await.unwrap();
        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].id, "a1");
    }

    #[tokio::test]
    async fn announcement_repo_empty_add_keeps_list_empty() {
        let pool = setup_core_pool("wallet_db_repo_announcement_edge").await;
        AnnouncementRepo::add(&pool, vec![]).await.unwrap();
        let list = AnnouncementRepo::list(&pool).await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn announcement_repo_tx_rollback_keeps_announcement_absent() {
        let pool = setup_core_pool("wallet_db_repo_announcement_rollback").await;

        let mut tx = pool.write_ref().begin().await.unwrap();
        AnnouncementDao::upsert(
            tx.as_mut(),
            vec![CreateAnnouncementVo {
                id: "a_rb".to_string(),
                title: "rollback".to_string(),
                content: "rollback".to_string(),
                language: "en".to_string(),
                status: 0,
                send_time: None,
            }],
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();

        let found = AnnouncementRepo::get_announcement_by_id(&pool, "a_rb").await.unwrap();
        assert!(found.is_none());
    }
}
