use crate::{api::ReturnType, response_vo::app::GetConfigRes, service::app::AppService};
use wallet_database::entities::config::ConfigEntity;
use wallet_transport_backend::response_vo::app::{
    AppVersionRes, GetFiatRes, GetOfficialWebsiteRes,
};

impl crate::WalletManager {
    // app版本检测接口
    pub async fn check_version(
        &self,
        device_type: Option<String>,
        r#type: Option<String>,
    ) -> ReturnType<AppVersionRes> {
        AppService::new(self.repo_factory.resuource_repo())
            .check_version(device_type, r#type)
            .await?
            .into()
    }

    pub async fn set_currency(&self, fiat: &str) -> ReturnType<()> {
        AppService::new(self.repo_factory.resuource_repo())
            .set_fiat(fiat)
            .await?
            .into()
    }

    pub async fn set_language(&self, language: &str) -> ReturnType<()> {
        AppService::new(self.repo_factory.resuource_repo())
            .language_init(language)
            .await?
            .into()
    }

    pub async fn set_app_id(&self, app_id: &str) -> ReturnType<()> {
        AppService::new(self.repo_factory.resuource_repo())
            .set_app_id(app_id)
            .await?
            .into()
    }

    pub async fn get_fiat(&self) -> ReturnType<GetFiatRes> {
        AppService::new(self.repo_factory.resuource_repo())
            .get_fiat()
            .await?
            .into()
    }

    pub async fn get_official_website(&self) -> ReturnType<GetOfficialWebsiteRes> {
        AppService::new(self.repo_factory.resuource_repo())
            .get_official_website()
            .await?
            .into()
    }

    pub async fn get_config(&self) -> ReturnType<GetConfigRes> {
        AppService::new(self.repo_factory.resuource_repo())
            .get_config()
            .await?
            .into()
    }

    pub async fn get_unread_status(&self) -> ReturnType<crate::response_vo::app::UnreadCount> {
        AppService::new(self.repo_factory.resuource_repo())
            .get_unread_status()
            .await?
            .into()
    }

    /// Platform Energy Subsidy Switch Configuration
    pub async fn delegate_witch(&self) -> ReturnType<bool> {
        let backend = crate::manager::Context::get_global_backend_api()?;
        let res = backend.delegate_is_open().await;

        match res {
            Ok(rs) => rs.into(),
            Err(e) => Err(crate::Errors::Service(e.into()))?,
        }
    }

    pub async fn upload_log_file(&self) -> ReturnType<()> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        AppService::new(repo).upload_log_file().await?.into()
    }

    pub async fn mqtt_subscribe(&self, topics: Vec<String>, qos: Option<u8>) -> ReturnType<()> {
        AppService::new(self.repo_factory.resuource_repo())
            .mqtt_subscribe(topics, qos)
            .await?
            .into()
    }

    pub async fn mqtt_unsubscribe(&self, topics: Vec<String>) -> ReturnType<()> {
        AppService::new(self.repo_factory.resuource_repo())
            .mqtt_unsubscribe_unsubscribe(topics)
            .await?
            .into()
    }

    pub async fn get_configs(&self) -> ReturnType<Vec<ConfigEntity>> {
        AppService::new(self.repo_factory.resuource_repo())
            .get_configs()
            .await?
            .into()
    }

    pub async fn set_config(&self, key: String, value: String) -> ReturnType<ConfigEntity> {
        AppService::new(self.repo_factory.resuource_repo())
            .set_config(key, value)
            .await?
            .into()
    }
}

#[cfg(test)]
mod test {
    use crate::test::env::{setup_test_environment, TestData};
    use anyhow::Result;

    #[tokio::test]
    async fn test_set_language() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let res = wallet_manager.set_language("ENGLISH").await;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_official_website() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let res = wallet_manager.get_official_website().await;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_config() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let res = wallet_manager.get_config().await;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_unread_status() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let res = wallet_manager.get_unread_status().await;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    pub async fn check_version() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let device_type = "ANDROID".to_string();
        // let device_type = "IOS".to_string();
        // let r#type = Some("android_google_shop".to_string());
        let r#type = Some("android_google_shop".to_string());
        // let r#type = None;
        let res = wallet_manager
            .check_version(Some(device_type), r#type)
            .await;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    pub async fn set_currency() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let currency = "CNY";
        let res = wallet_manager.set_currency(currency).await;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_delegate_swidth() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let res = wallet_manager.delegate_witch().await;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_upload_log_file() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let res = wallet_manager.upload_log_file().await;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_set_app_id() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let res = wallet_manager.set_app_id("aaa").await;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }
}
