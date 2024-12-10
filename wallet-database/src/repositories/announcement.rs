use crate::{
    entities::announcement::{AnnouncementEntity, CreateAnnouncementVo},
    pagination::Pagination,
};

// pub struct ChainRepo {
//     // pub repo: ResourcesRepo,
// }

// impl ChainRepo {
//     pub fn new(db_pool: crate::DbPool) -> Self {
//         Self {
//             // repo: ResourcesRepo::new(db_pool),
//         }
//     }
// }

// impl ChainRepoTrait for ChainRepo {}

#[async_trait::async_trait]
pub trait AnnouncementRepoTrait: super::TransactionTrait {
    async fn add(&mut self, input: Vec<CreateAnnouncementVo>) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AnnouncementEntity::upsert, input)
    }

    async fn update_existing(
        &mut self,
        input: Vec<CreateAnnouncementVo>,
    ) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AnnouncementEntity::update_existing, input)
    }

    async fn list(&mut self) -> Result<Vec<AnnouncementEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AnnouncementEntity::list,)
    }

    async fn get_announcement_list(
        &mut self,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<AnnouncementEntity>, crate::Error> {
        let executor = self.get_db_pool();
        AnnouncementEntity::get_announcement_list(executor, page, page_size).await
    }

    async fn get_announcement_by_id(
        &mut self,
        id: &str,
    ) -> Result<Option<AnnouncementEntity>, crate::Error> {
        let executor = self.get_db_pool();
        AnnouncementEntity::get_announcement_by_id(executor, id).await
    }

    async fn read(&mut self, id: Option<&str>) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AnnouncementEntity::update_status, id, 1)?;
        Ok(())
    }

    async fn count_unread_status(&mut self) -> Result<i64, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AnnouncementEntity::count_status_zero,)
    }

    async fn physical_delete(&mut self, id: &str) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AnnouncementEntity::physical_delete, id)
    }
}
