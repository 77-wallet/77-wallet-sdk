use crate::context::Context;
use std::cmp::Ordering;
use wallet_crypto::KdfAlgorithm;
use wallet_database::{
    entities::config::{
        AppVersion, Currency, InviteCode, KeysResetStatus, MinValueSwitchConfig, MqttUrl,
        OfficialWebsite,
        config_key::{
            APP_DOWNLOAD_QR_CODE_URL, APP_VERSION, BLOCK_BROWSER_URL_LIST, CURRENCY, INVITE_CODE,
            KEYS_RESET_EPOCH, KEYS_RESET_STATUS, KEYSTORE_KDF_ALGORITHM, LANGUAGE, MQTT_URL,
            OFFICIAL_WEBSITE, WALLET_TREE_STRATEGY,
        },
    },
    repositories::config::ConfigRepo,
};
use wallet_transport_backend::response_vo::chain::ChainUrlInfo;

pub struct ConfigDomain;

impl ConfigDomain {
    pub async fn get_config_min_value_with_ctx(
        ctx: &Context,
        symbol: &str,
    ) -> Result<Option<f64>, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let sn = ctx.get_global_device().sn.clone();
        let key = MinValueSwitchConfig::get_key(symbol, &sn);

        if let Some(config) = ConfigRepo::find_by_key(&key, &pool).await? {
            let min_config = MinValueSwitchConfig::try_from(config.value)?;
            if !min_config.switch {
                return Ok(None);
            }

            return Ok(Some(min_config.value));
        };

        Ok(None)
    }

    pub async fn get_config_min_value(
        ctx: &Context,
        symbol: &str,
    ) -> Result<Option<f64>, crate::error::service::ServiceError> {
        Self::get_config_min_value_with_ctx(ctx, symbol).await
    }

    /// fetch the minimum filtering amount configuration to the backend each time a wallet is created.
    pub async fn fetch_min_config_with_ctx(
        ctx: &Context,
        sn: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let backend = ctx.get_global_backend_api();
        let res = backend.fetch_min_config(sn.to_string()).await?;

        for item in res.list {
            let key = MinValueSwitchConfig::get_key(&item.token_code.to_uppercase(), sn);
            let value = MinValueSwitchConfig::new(item.is_open, item.min_amount);

            if let Err(e) = ConfigRepo::upsert(&key, &value.to_json_str()?, Some(1), &pool).await {
                tracing::error!("从后端同步过滤最小金额失败{}", e)
            }
        }

        Ok(())
    }

    pub async fn fetch_min_config(
        ctx: &Context,
        sn: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        Self::fetch_min_config_with_ctx(ctx, sn).await
    }

    pub async fn set_config_with_ctx(
        ctx: &Context,
        key: &str,
        value: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;

        ConfigRepo::upsert(key, value, None, &pool).await?;

        Ok(())
    }

    pub async fn set_config(
        ctx: &Context,
        key: &str,
        value: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        Self::set_config_with_ctx(ctx, key, value).await
    }

    pub async fn set_official_website(
        ctx: &Context,
        website: Option<String>,
    ) -> Result<(), crate::error::service::ServiceError> {
        if let Some(official_website) = website {
            let config = OfficialWebsite { url: official_website.clone() };
            ConfigDomain::set_config(ctx, OFFICIAL_WEBSITE, &config.to_json_str()?).await?;
            let mut config = crate::app_state::APP_STATE.write().await;
            config.set_official_website(Some(official_website));
        }

        Ok(())
    }

    pub async fn set_invite_code(
        ctx: &Context,
        status: Option<bool>,
        code: Option<String>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let config = InviteCode { code, status };
        ConfigDomain::set_config(ctx, INVITE_CODE, &config.to_json_str()?).await?;

        Ok(())
    }

    pub async fn set_currency(
        ctx: &Context,
        currency: Option<Currency>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let mut config = crate::app_state::APP_STATE.write().await;
        let currency = if let Some(currency) = currency
            && currency.currency != config.currency()
        {
            config.set_fiat_from_str(&currency.currency);
            currency
        } else {
            Currency::default()
        };
        drop(config);
        ConfigDomain::set_config(ctx, CURRENCY, &currency.to_json_str()?).await?;

        Ok(())
    }

    pub async fn set_app_download_qr_code_url(
        ctx: &Context,
        app_download_qr_code_url: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        // let tx = &mut self.repo;
        let config = wallet_database::entities::config::AppInstallDownload {
            url: app_download_qr_code_url.to_string(),
        };
        ConfigDomain::set_config(ctx, APP_DOWNLOAD_QR_CODE_URL, &config.to_json_str()?).await?;
        let mut config = crate::app_state::APP_STATE.write().await;
        config.set_app_download_qr_code_url(Some(app_download_qr_code_url.to_string()));
        Ok(())
    }

    pub async fn init_app_install_download_url_with_ctx(
        ctx: &Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let app_install_download_url =
            ConfigRepo::find_by_key(APP_DOWNLOAD_QR_CODE_URL, &pool).await?;
        if let Some(app_install_download_url) = app_install_download_url {
            let app_install_download_url =
                OfficialWebsite::try_from(app_install_download_url.value)?;

            let mut config = crate::app_state::APP_STATE.write().await;
            config.set_app_download_qr_code_url(Some(app_install_download_url.url));
        }
        Ok(())
    }

    pub async fn init_app_install_download_url(
        ctx: &Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        Self::init_app_install_download_url_with_ctx(ctx).await
    }

    pub async fn init_official_website_with_ctx(
        ctx: &Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let official_website = ConfigRepo::find_by_key(OFFICIAL_WEBSITE, &pool).await?;
        if let Some(official_website) = official_website {
            let official_website = OfficialWebsite::try_from(official_website.value)?;

            let mut config = crate::app_state::APP_STATE.write().await;
            config.set_official_website(Some(official_website.url));
        }
        Ok(())
    }

    pub async fn init_official_website(
        ctx: &Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        Self::init_official_website_with_ctx(ctx).await
    }

    pub async fn init_currency_with_ctx(
        ctx: &Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let currency = ConfigRepo::find_by_key(CURRENCY, &pool).await?;
        if let Some(currency) = currency {
            let mut config = crate::app_state::APP_STATE.write().await;
            let currency = wallet_database::entities::config::Currency::try_from(currency.value)?;
            config.set_fiat_from_str(&currency.currency);
        } else {
            ConfigDomain::set_currency(ctx, None).await?;
        };
        Ok(())
    }

    pub async fn init_currency(ctx: &Context) -> Result<(), crate::error::service::ServiceError> {
        Self::init_currency_with_ctx(ctx).await
    }

    pub async fn init_language_with_ctx(
        ctx: &Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let language = ConfigRepo::find_by_key(LANGUAGE, &pool).await?;
        let mut config = crate::app_state::APP_STATE.write().await;
        if let Some(language) = language {
            let language = wallet_database::entities::config::Language::try_from(language.value)?;
            config.set_language(&language.language);
        } else {
            let l = config.language();
            let config = wallet_database::entities::config::Language::new(l);
            ConfigDomain::set_config(ctx, LANGUAGE, &config.to_json_str()?).await?;
        };
        drop(config);

        Ok(())
    }

    pub async fn init_language(ctx: &Context) -> Result<(), crate::error::service::ServiceError> {
        Self::init_language_with_ctx(ctx).await
    }

    pub(crate) async fn get_currency_with_ctx(
        ctx: &Context,
    ) -> Result<String, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let currency = ConfigRepo::find_by_key(CURRENCY, &pool).await?;
        if let Some(currency) = currency {
            let currency = wallet_database::entities::config::Currency::try_from(currency.value)?;
            Ok(currency.currency)
        } else {
            Ok(String::from("USD"))
        }
    }

    pub(crate) async fn get_currency(
        ctx: &Context,
    ) -> Result<String, crate::error::service::ServiceError> {
        Self::get_currency_with_ctx(ctx).await
    }

    pub(crate) async fn get_invite_code_with_ctx(
        ctx: &Context,
    ) -> Result<Option<InviteCode>, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let invite_code = ConfigRepo::find_by_key(INVITE_CODE, &pool).await?;

        invite_code
            .map(|invite_code| {
                let invite_code =
                    wallet_database::entities::config::InviteCode::try_from(invite_code.value)?;
                Ok(invite_code)
            })
            .transpose()
    }

    pub(crate) async fn get_invite_code(
        ctx: &Context,
    ) -> Result<Option<InviteCode>, crate::error::service::ServiceError> {
        Self::get_invite_code_with_ctx(ctx).await
    }

    pub async fn set_keys_reset_status(
        ctx: &Context,
        status: Option<bool>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let config = KeysResetStatus { status };
        ConfigDomain::set_config(ctx, KEYS_RESET_STATUS, &config.to_json_str()?).await?;

        Ok(())
    }

    pub(crate) async fn get_keys_reset_status_with_ctx(
        ctx: &Context,
    ) -> Result<Option<KeysResetStatus>, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;

        let keys_reset_status = ConfigRepo::find_by_key(KEYS_RESET_STATUS, &pool).await?;

        if let Some(keys_reset_status) = keys_reset_status {
            Ok(Some(KeysResetStatus::try_from(keys_reset_status.value)?))
        } else {
            Ok(None)
        }
    }

    pub(crate) async fn get_keys_reset_epoch_with_ctx(
        ctx: &Context,
    ) -> Result<u64, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;

        // 尝试从数据库获取当前epoch
        let keys_reset_epoch = ConfigRepo::find_by_key(KEYS_RESET_EPOCH, &pool).await?;

        if let Some(keys_reset_epoch) = keys_reset_epoch {
            // 解析epoch值
            Ok(keys_reset_epoch.value.parse::<u64>().map_err(|e| {
                crate::error::service::ServiceError::System(
                    crate::error::system::SystemError::Internal(format!(
                        "Failed to parse epoch: {}",
                        e
                    )),
                )
            })?)
        } else {
            // 如果不存在，自动创建并设置为0
            ConfigRepo::upsert(KEYS_RESET_EPOCH, "0", None, &pool).await?;
            Ok(0)
        }
    }

    pub(crate) async fn get_keys_reset_epoch(
        ctx: &Context,
    ) -> Result<u64, crate::error::service::ServiceError> {
        Self::get_keys_reset_epoch_with_ctx(ctx).await
    }

    pub(crate) async fn bump_keys_reset_epoch_with_ctx(
        ctx: &Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;

        // 先获取当前epoch
        let current_epoch = ConfigDomain::get_keys_reset_epoch(ctx).await?;

        // 递增epoch
        let new_epoch = current_epoch + 1;

        // 持久化新epoch
        ConfigRepo::upsert(KEYS_RESET_EPOCH, &new_epoch.to_string(), None, &pool).await?;

        Ok(())
    }

    pub(crate) async fn bump_keys_reset_epoch(
        ctx: &Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        Self::bump_keys_reset_epoch_with_ctx(ctx).await
    }

    pub(crate) async fn check_epoch_validity(
        ctx: &Context,
        task_epoch: u64,
    ) -> Result<bool, crate::error::service::ServiceError> {
        // 获取当前epoch
        let current_epoch = ConfigDomain::get_keys_reset_epoch(ctx).await?;

        // 检查任务epoch是否与当前epoch匹配
        Ok(task_epoch == current_epoch)
    }

    pub(crate) async fn get_app_version_with_ctx(
        ctx: &Context,
    ) -> Result<AppVersion, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;

        let app_version = ConfigRepo::find_by_key(APP_VERSION, &pool).await?.ok_or(
            crate::error::service::ServiceError::Business(
                crate::error::business::BusinessError::Config(
                    crate::error::business::config::ConfigError::NotFound(APP_VERSION.to_owned()),
                ),
            ),
        )?;
        Ok(AppVersion::try_from(app_version.value)?)
    }

    pub(crate) fn compare_versions(v1: &str, v2: &str) -> Ordering {
        let parse =
            |v: &str| v.split('.').map(|s| s.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();

        let mut v1_parts = parse(v1);
        let mut v2_parts = parse(v2);

        let max_len = v1_parts.len().max(v2_parts.len());
        v1_parts.resize(max_len, 0);
        v2_parts.resize(max_len, 0);

        for (a, b) in v1_parts.iter().zip(v2_parts.iter()) {
            match a.cmp(b) {
                Ordering::Equal => continue,
                non_eq => return non_eq,
            }
        }

        Ordering::Equal
    }

    pub(crate) async fn get_keystore_kdf_algorithm_with_ctx(
        ctx: &Context,
    ) -> Result<KdfAlgorithm, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let keystore_kdf_algorithm = ConfigRepo::find_by_key(KEYSTORE_KDF_ALGORITHM, &pool).await?;
        if let Some(keystore_kdf_algorithm) = keystore_kdf_algorithm {
            let keystore_kdf_algorithm =
                wallet_database::entities::config::KeystoreKdfAlgorithm::try_from(
                    keystore_kdf_algorithm.value,
                )?;
            Ok(keystore_kdf_algorithm.keystore_kdf_algorithm)
        } else {
            Ok(KdfAlgorithm::Scrypt)
            // Ok(KdfAlgorithm::Argon2id)
        }
    }

    pub(crate) async fn get_wallet_tree_strategy_with_ctx(
        ctx: &Context,
    ) -> Result<wallet_tree::WalletTreeStrategy, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let wallet_tree_strategy = ConfigRepo::find_by_key(WALLET_TREE_STRATEGY, &pool).await?;
        if let Some(wallet_tree_strategy) = wallet_tree_strategy {
            let wallet_tree_strategy =
                wallet_database::entities::config::WalletTreeStrategy::try_from(
                    wallet_tree_strategy.value,
                )?;
            Ok(wallet_tree_strategy.wallet_tree_strategy)
        } else {
            // Ok(wallet_tree::WalletTreeStrategy::V2)
            Ok(wallet_tree::WalletTreeStrategy::V1)
        }
    }

    pub async fn init_block_browser_url_list_with_ctx(
        ctx: &Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let block_browser_url_list = ConfigRepo::find_by_key(BLOCK_BROWSER_URL_LIST, &pool).await?;
        if let Some(block_browser_url_list) = block_browser_url_list {
            let mut config = crate::app_state::APP_STATE.write().await;
            let value = wallet_utils::serde_func::serde_from_str(&block_browser_url_list.value)?;

            config.set_block_browser_url(value);
        }

        Ok(())
    }

    pub async fn init_block_browser_url_list(
        ctx: &Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        Self::init_block_browser_url_list_with_ctx(ctx).await
    }

    pub(crate) async fn set_block_browser_url(
        ctx: &Context,
        list: &[ChainUrlInfo],
    ) -> Result<(), crate::error::service::ServiceError> {
        let block_browser_url_list = list
            .iter()
            .map(|info| {
                crate::request::init::BlockBrowserUrl::new(
                    info.chain_code.clone(),
                    info.address_url.clone(),
                    info.hash_url.clone(),
                    info.token_url.clone(),
                )
            })
            .collect();
        let value = wallet_utils::serde_func::serde_to_string(&block_browser_url_list)?;
        ConfigDomain::set_config(ctx, BLOCK_BROWSER_URL_LIST, &value).await?;
        let mut config = crate::app_state::APP_STATE.write().await;
        config.set_block_browser_url(block_browser_url_list);
        Ok(())
    }

    pub async fn init_url(ctx: &Context) -> Result<(), crate::error::service::ServiceError> {
        // Self::init_mqtt_url().await?;
        // crate::WalletManager::init_mqtt().await?;

        Self::init_official_website(ctx).await?;
        Self::init_block_browser_url_list(ctx).await?;
        Self::init_app_install_download_url(ctx).await?;
        Self::init_language(ctx).await?;

        Ok(())
    }

    // Attempt to get the MQTT URI from the backend.
    // If an error occurs or the URI is not found, use the URI from the database instead.
    pub async fn get_mqtt_uri_with_ctx(
        ctx: &Context,
    ) -> Result<Option<String>, crate::error::service::ServiceError> {
        let backend_api = ctx.get_global_backend_api();
        let pool = ctx.core_pool()?;

        if let Ok(mqtt_url) = backend_api.mqtt_init().await {
            let config = MqttUrl { url: mqtt_url.clone() };
            ConfigDomain::set_config(ctx, MQTT_URL, &config.to_json_str()?).await?;
            return Ok(Some(config.url_with_protocol()));
        }

        let config = ConfigRepo::find_by_key(MQTT_URL, &pool).await?;
        let uri = config
            .and_then(|c| MqttUrl::try_from(c.value).ok())
            .map(|mqtt| mqtt.url_with_protocol());

        Ok(uri)
    }

    pub async fn get_mqtt_uri(
        ctx: &Context,
    ) -> Result<Option<String>, crate::error::service::ServiceError> {
        Self::get_mqtt_uri_with_ctx(ctx).await
    }

    pub(crate) async fn get_keystore_kdf_algorithm(
        ctx: &Context,
    ) -> Result<KdfAlgorithm, crate::error::service::ServiceError> {
        Self::get_keystore_kdf_algorithm_with_ctx(ctx).await
    }

    pub(crate) async fn get_wallet_tree_strategy(
        ctx: &Context,
    ) -> Result<wallet_tree::WalletTreeStrategy, crate::error::service::ServiceError> {
        Self::get_wallet_tree_strategy_with_ctx(ctx).await
    }

    pub(crate) async fn get_app_version(
        ctx: &Context,
    ) -> Result<AppVersion, crate::error::service::ServiceError> {
        Self::get_app_version_with_ctx(ctx).await
    }

    pub(crate) async fn get_keys_reset_status(
        ctx: &Context,
    ) -> Result<Option<KeysResetStatus>, crate::error::service::ServiceError> {
        Self::get_keys_reset_status_with_ctx(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use crate::domain::app::config::ConfigDomain;

    #[test]
    fn test_equal_versions() {
        assert_eq!(ConfigDomain::compare_versions("1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(ConfigDomain::compare_versions("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(ConfigDomain::compare_versions("1.0.0.0", "1"), Ordering::Equal);
    }

    #[test]
    fn test_greater_versions() {
        assert_eq!(ConfigDomain::compare_versions("1.2.10", "1.2.2"), Ordering::Greater);
        assert_eq!(ConfigDomain::compare_versions("2.0", "1.999.999"), Ordering::Greater);
        assert_eq!(ConfigDomain::compare_versions("1.10.0", "1.2.99"), Ordering::Greater);
    }

    #[test]
    fn test_less_versions() {
        assert_eq!(ConfigDomain::compare_versions("0.9.9", "1.0.0"), Ordering::Less);
        assert_eq!(ConfigDomain::compare_versions("1.2.3", "1.2.4"), Ordering::Less);
        assert_eq!(ConfigDomain::compare_versions("1.2", "1.2.1"), Ordering::Less);
    }

    #[test]
    fn test_invalid_parts() {
        assert_eq!(ConfigDomain::compare_versions("1.2.alpha", "1.2.0"), Ordering::Equal); // "alpha" -> 0
        assert_eq!(ConfigDomain::compare_versions("1.a.3", "1.0.3"), Ordering::Equal);
        assert_eq!(ConfigDomain::compare_versions("a.b.c", "0.0.0"), Ordering::Equal);
    }

    #[test]
    fn test_empty_strings() {
        assert_eq!(ConfigDomain::compare_versions("", ""), Ordering::Equal);
        assert_eq!(ConfigDomain::compare_versions("1.2.3", ""), Ordering::Greater);
        assert_eq!(ConfigDomain::compare_versions("", "0.0.1"), Ordering::Less);
    }

    #[test]
    fn test_trailing_zeros() {
        assert_eq!(ConfigDomain::compare_versions("1.0.0.0", "1"), Ordering::Equal);
        assert_eq!(ConfigDomain::compare_versions("1.0.0.1", "1"), Ordering::Greater);
        assert_eq!(ConfigDomain::compare_versions("1", "1.0.0.1"), Ordering::Less);
    }

    #[test]
    fn test_long_versions() {
        assert_eq!(
            ConfigDomain::compare_versions("1.2.3.4.5.6.7", "1.2.3.4.5.6.7"),
            Ordering::Equal
        );
        assert_eq!(
            ConfigDomain::compare_versions("1.2.3.4.5.6.8", "1.2.3.4.5.6.7"),
            Ordering::Greater
        );
    }
}
