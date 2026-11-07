use crate::{api::ReturnType, manager::WalletManager, service::announcement::AnnouncementService};
use wallet_database::{entities::announcement::AnnouncementEntity, pagination::Pagination};

impl WalletManager {
    pub async fn add_announcement(
        &self,
        input: Vec<wallet_database::entities::announcement::CreateAnnouncementVo>,
    ) -> ReturnType<()> {
        AnnouncementService::new(self.ctx).add(input).await
    }

    pub async fn pull_announcement(&self) -> ReturnType<()> {
        AnnouncementService::new(self.ctx).pull_announcement().await
    }

    pub async fn get_announcement_list(
        &self,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<AnnouncementEntity>> {
        AnnouncementService::new(self.ctx).get_announcement_list(page, page_size).await
    }

    pub async fn get_announcement_by_id(&self, id: &str) -> ReturnType<Option<AnnouncementEntity>> {
        AnnouncementService::new(self.ctx).get_announcement_by_id(id).await
    }

    pub async fn read_announcement(&self, id: Option<&str>) -> ReturnType<()> {
        AnnouncementService::new(self.ctx).read(id).await
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use crate::testkit::env::get_manager;
    use anyhow::Result;
    use wallet_database::entities::announcement::CreateAnnouncementVo;

    #[tokio::test]
    async fn test_init_some_announcement() -> Result<()> {
        wallet_utils::init_test_log();
        let (wallet_manager, _test_params) = get_manager().await?;

        let an = CreateAnnouncementVo {
            id: "1".to_string(),
            title: "test".to_string(),
            content: "test".to_string(),
            language: "en".to_string(),
            status: 1,
            send_time: Some("1732527339".to_string()),
        };
        let announcements = vec![an]; // Create an empty Vec<CreateAnnouncementVo>
        let announcement_list = wallet_manager.add_announcement(announcements).await?;
        tracing::info!("get_announcement_list: {announcement_list:?}");
        let res = wallet_utils::serde_func::serde_to_string(&announcement_list)?;
        tracing::info!("res: {res}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_announcement_list() -> Result<()> {
        wallet_utils::init_test_log();
        let (wallet_manager, _test_params) = get_manager().await?;

        let announcement_list = wallet_manager.get_announcement_list(0, 10).await?;
        tracing::info!("get_announcement_list: {announcement_list:?}");
        let res = wallet_utils::serde_func::serde_to_string(&announcement_list)?;
        tracing::info!("res: {res}");
        Ok(())
    }

    #[tokio::test]
    async fn test_pull_announcement() -> Result<()> {
        wallet_utils::init_test_log();
        let (wallet_manager, _test_params) = get_manager().await?;

        let announcement_list = wallet_manager.pull_announcement().await?;
        tracing::info!("pull_announcement: {announcement_list:?}");
        let res = wallet_utils::serde_func::serde_to_string(&announcement_list)?;
        tracing::info!("res: {res}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_announcement_by_id() -> Result<()> {
        wallet_utils::init_test_log();
        let (wallet_manager, _test_params) = get_manager().await?;

        let id = "66fd0176b038070e17edd202";
        let announcement = wallet_manager.get_announcement_by_id(id).await?;
        tracing::info!("get_announcement_by_id: {announcement:?}");
        let res = wallet_utils::serde_func::serde_to_string(&announcement)?;
        tracing::info!("res: {res}");
        Ok(())
    }

    #[tokio::test]
    async fn test_read_announcement() -> Result<()> {
        wallet_utils::init_test_log();
        let (wallet_manager, _test_params) = get_manager().await?;

        let id = None;
        let read_announcement = wallet_manager.read_announcement(id).await?;
        tracing::info!("read_announcement: {read_announcement:?}");
        Ok(())
    }
}
