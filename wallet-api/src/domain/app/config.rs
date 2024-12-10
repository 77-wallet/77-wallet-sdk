use wallet_database::{
    dao::config::ConfigDao,
    entities::config::{
        config_key::{
            APP_DOWNLOAD_QR_CODE_URL, APP_DOWNLOAD_URL, BLOCK_BROWSER_URL_LIST, MIN_VALUE_SWITCH,
            MQTT_URL, OFFICIAL_WEBSITE,
        },
        MinValueSwitchConfig, MqttUrl, OfficialWebsite,
    },
};
use wallet_transport_backend::{
    consts::{endpoint::VERSION_DOWNLOAD, BASE_URL},
    response_vo::app::SaveSendMsgAccount,
};

pub struct ConfigDomain;

impl ConfigDomain {
    // 获取配置过滤的最小币
    //    U的价值  = 配置的法币值  / 汇率
    //    币的数量  = U的价值     / 币的单价(币)
    pub async fn get_config_min_value(
        chain_code: &str,
        symbol: &str,
    ) -> Result<Option<f64>, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        if let Some(config) = ConfigDao::find_by_key(MIN_VALUE_SWITCH, pool.as_ref()).await? {
            let min_config = MinValueSwitchConfig::try_from(config.value)?;
            if !min_config.switch {
                return Ok(None);
            }
            // 币价格
            let token_currency = super::super::coin::TokenCurrencyGetter::get_currency(
                &min_config.currency,
                chain_code,
                symbol,
            )
            .await?;

            if let Some(price) = token_currency.price {
                if token_currency.rate == 0.0 || price == 0.0 {
                    return Ok(None);
                }

                return Ok(Some(min_config.value / token_currency.rate / price));
            } else {
                return Ok(Some(0.0));
            }
        };

        Ok(None)
    }

    /// Report the minimum filtering amount configuration to the backend each time a wallet is created.
    pub async fn report_backend(sn: &str) -> Result<(), crate::ServiceError> {
        let pool = crate::Context::get_global_sqlite_pool()?;

        let config = ConfigDao::find_by_key(
            wallet_database::entities::config::config_key::MIN_VALUE_SWITCH,
            pool.as_ref(),
        )
        .await?;

        let req = if let Some(config) = config {
            let min_config =
                wallet_database::entities::config::MinValueSwitchConfig::try_from(config.value)?;
            SaveSendMsgAccount {
                amount: min_config.value,
                sn: sn.to_string(),
                is_open: min_config.switch,
            }
        } else {
            SaveSendMsgAccount {
                amount: 0.0,
                sn: sn.to_string(),
                is_open: false,
            }
        };

        let backend = crate::Context::get_global_backend_api()?;
        if let Err(e) = backend.save_send_msg_account(req).await {
            tracing::error!(sn = sn, "report min value error:{} ", e);
        }
        Ok(())
    }

    pub async fn set_config(key: &str, value: &str) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        ConfigDao::upsert(key, value, pool.as_ref()).await?;

        Ok(())
    }

    pub async fn set_official_website(website: Option<String>) -> Result<(), crate::ServiceError> {
        if let Some(official_website) = website {
            let config = OfficialWebsite {
                url: official_website.clone(),
            };
            ConfigDomain::set_config(OFFICIAL_WEBSITE, &config.to_json_str()?).await?;
            let mut config = crate::app_state::APP_STATE.write().await;
            config.set_official_website(Some(official_website));
        }

        Ok(())
    }

    pub async fn set_mqtt_url(mqtt_url: Option<String>) -> Result<(), crate::ServiceError> {
        if let Some(mqtt_url) = mqtt_url {
            let config = MqttUrl {
                url: mqtt_url.clone(),
            };
            ConfigDomain::set_config(MQTT_URL, &config.to_json_str()?).await?;
            let mut config = crate::app_state::APP_STATE.write().await;
            config.set_mqtt_url(Some(mqtt_url));
        }

        Ok(())
    }

    pub async fn set_app_download_qr_code_url(
        app_download_qr_code_url: &str,
    ) -> Result<(), crate::ServiceError> {
        // let tx = &mut self.repo;
        let config = wallet_database::entities::config::AppInstallDownload {
            url: app_download_qr_code_url.to_string(),
        };
        ConfigDomain::set_config(APP_DOWNLOAD_QR_CODE_URL, &config.to_json_str()?).await?;
        let mut config = crate::app_state::APP_STATE.write().await;
        config.set_app_download_qr_code_url(Some(app_download_qr_code_url.to_string()));
        Ok(())
    }

    pub async fn set_version_download_url(
        app_install_download_url: &str,
    ) -> Result<(), crate::ServiceError> {
        // let tx = &mut self.repo;
        let encoded_url = urlencoding::encode(app_install_download_url);
        let url = format!("{}/{}/{}", BASE_URL, VERSION_DOWNLOAD, encoded_url);
        let config = wallet_database::entities::config::VersionDownloadUrl::new(&url);
        ConfigDomain::set_config(APP_DOWNLOAD_URL, &config.to_json_str()?).await?;
        let mut config = crate::app_state::APP_STATE.write().await;
        config.set_app_download_url(Some(url));
        Ok(())
    }

    pub async fn init_app_install_download_url() -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let app_install_download_url =
            ConfigDao::find_by_key(APP_DOWNLOAD_QR_CODE_URL, pool.as_ref()).await?;
        if let Some(app_install_download_url) = app_install_download_url {
            let app_install_download_url =
                OfficialWebsite::try_from(app_install_download_url.value)?;

            let mut config = crate::app_state::APP_STATE.write().await;
            config.set_app_download_qr_code_url(Some(app_install_download_url.url));
        }
        Ok(())
    }

    pub async fn init_official_website() -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let official_website = ConfigDao::find_by_key(OFFICIAL_WEBSITE, pool.as_ref()).await?;
        if let Some(official_website) = official_website {
            let official_website = OfficialWebsite::try_from(official_website.value)?;

            let mut config = crate::app_state::APP_STATE.write().await;
            config.set_official_website(Some(official_website.url));
        }
        Ok(())
    }

    pub async fn init_block_browser_url_list() -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let block_browser_url_list =
            ConfigDao::find_by_key(BLOCK_BROWSER_URL_LIST, pool.as_ref()).await?;
        if let Some(block_browser_url_list) = block_browser_url_list {
            let mut config = crate::app_state::APP_STATE.write().await;
            let value = wallet_utils::serde_func::serde_from_str(&block_browser_url_list.value)?;

            config.set_block_browser_url(value);
        }

        Ok(())
    }

    pub async fn init_url() -> Result<(), crate::ServiceError> {
        Self::init_official_website().await?;
        Self::init_block_browser_url_list().await?;
        Self::init_app_install_download_url().await?;

        Ok(())
    }
}
