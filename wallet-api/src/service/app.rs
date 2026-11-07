use std::path::{Path, PathBuf};

use wallet_database::{
    entities::{
        api_wallet::ApiWalletType,
        config::{ConfigEntity, MinValueSwitchConfig, config_key::LANGUAGE},
        multisig_account::{MultiAccountOwner, MultisigAccountStatus},
        multisig_queue::MultisigQueueStatus,
    },
    repositories::{
        announcement::AnnouncementRepo, config::ConfigRepo, device::DeviceRepo,
        multisig_account::MultisigAccountRepo, multisig_queue::MultisigQueueRepo,
        system_notification::SystemNotificationRepo, wallet::WalletRepo,
    },
};
use wallet_transport_backend::{
    request::{AppInstallSaveReq, VersionViewReq},
    response_vo::app::{AppVersionRes, GetFiatRes, GetOfficialWebsiteRes},
};

use crate::{
    api::ReturnType,
    context::Context,
    domain::{
        api_wallet::wallet::ApiWalletDomain,
        app::{DeviceDomain, config::ConfigDomain},
    },
    error::service::ServiceError,
    infrastructure::task_queue::{
        backend::{BackendApiTask, BackendApiTaskData},
        task::Tasks,
    },
    response_vo::standard_wallet::app::{GetConfigRes, GlobalMsg, MultisigAccountBase},
};

pub struct AppService {
    ctx: &'static Context,
}

impl AppService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn get_official_website(
        self,
    ) -> Result<GetOfficialWebsiteRes, crate::error::service::ServiceError> {
        let config = crate::app_state::APP_STATE.read().await;

        let official_website = config.get_official_website();
        Ok(GetOfficialWebsiteRes { official_website })
    }

    pub async fn get_config(self) -> Result<GetConfigRes, crate::error::service::ServiceError> {
        let config = crate::app_state::APP_STATE.read().await;
        // if config.url().official_website.is_none() {
        //     let official_website = self.app_domain.get_official_website().await.ok();
        //     if let Some(official_website) = official_website {
        //         config.set_official_website(official_website.official_website);
        //     }
        // };
        let url = config.url().clone();
        drop(config);
        if url.block_browser_url_list.is_empty() {
            ConfigDomain::init_block_browser_url_list(self.ctx).await?;
        }
        if url.official_website.is_none() {
            ConfigDomain::init_official_website(self.ctx).await?;
        }
        if url.app_download_qr_code_url.is_none() {
            ConfigDomain::init_app_install_download_url(self.ctx).await?;
        }
        let pool = self.ctx.core_pool()?;
        let standard_wallet_list = WalletRepo::wallet_list(pool.clone())
            .await?
            .into_iter()
            .map(|wallet| wallet.into())
            .collect();

        let api_wallet_list = ApiWalletDomain::new(self.ctx).get_config_api_wallet_list().await?;

        let sn = self.ctx.get_sn();
        let device_info = DeviceRepo::get_device_info(pool.clone(), sn).await?;

        let unread_announcement_count = AnnouncementRepo::count_unread(&pool).await?;
        let unread_system_notification_count = SystemNotificationRepo::count_unread(&pool).await?;

        let config = crate::app_state::APP_STATE.read().await;
        Ok(GetConfigRes {
            fiat: config.currency().to_string(),
            language: config.language().to_string(),
            standard_wallet_list,
            api_wallet_list,
            device_info,
            url: config.url().clone(),
            unread_count: crate::response_vo::standard_wallet::app::UnreadCount {
                system_notification: unread_system_notification_count,
                announcement: unread_announcement_count,
            },
        })
    }

    pub async fn get_unread_status(
        self,
    ) -> Result<
        crate::response_vo::standard_wallet::app::UnreadCount,
        crate::error::service::ServiceError,
    > {
        let pool = self.ctx.core_pool()?;
        let unread_announcement_count = AnnouncementRepo::count_unread(&pool).await?;
        let unread_system_notification_count = SystemNotificationRepo::count_unread(&pool).await?;
        Ok(crate::response_vo::standard_wallet::app::UnreadCount {
            system_notification: unread_system_notification_count,
            announcement: unread_announcement_count,
        })
    }

    pub async fn language_init(
        self,
        language: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let val = wallet_database::entities::config::Language::new(language);
        ConfigDomain::set_config(self.ctx, LANGUAGE, &val.to_json_str()?).await?;

        let pool = self.ctx.core_pool()?;
        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(pool, sn).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };
        let task = DeviceDomain::language_init(&device, language).await?;
        Tasks::new().push(task).send_with_ctx(self.ctx).await?;
        let mut config = crate::app_state::APP_STATE.write().await;
        config.set_language(language);

        Ok(())
    }

    // pub async fn set_config(self, language: SetConfigReq) -> Result<(), crate::ServiceError> {
    //     let mut tx = self.repo;

    //     Ok(())
    // }

    pub async fn check_version(
        self,
        r#type: &str,
    ) -> Result<AppVersionRes, crate::error::service::ServiceError> {
        let req = VersionViewReq::new(r#type);
        let backend = self.ctx.get_global_backend_api();

        let res = backend.version_view(req).await?;
        Ok(res)
    }

    // fiat  = CNY
    pub async fn set_fiat(
        &mut self,
        fiat: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let config = wallet_database::entities::config::Currency { currency: fiat.to_string() };
        ConfigDomain::set_currency(self.ctx, Some(config)).await?;

        Ok(())
    }

    pub async fn set_app_id(self, app_id: &str) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;
        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(pool.clone(), sn).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };
        let sn = self.ctx.get_sn();
        DeviceRepo::update_app_id(pool.clone(), sn, app_id).await?;

        let req = wallet_transport_backend::request::UpdateAppIdReq::new(&device.sn, app_id);
        let task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::DEVICE_UPDATE_APP_ID,
            &req,
        )?;
        Tasks::new().push(BackendApiTask::BackendApi(task_data)).send_with_ctx(self.ctx).await?;

        Ok(())
    }

    pub async fn get_fiat(self) -> Result<GetFiatRes, crate::error::service::ServiceError> {
        let config = crate::app_state::APP_STATE.read().await;

        Ok(GetFiatRes { fiat: config.currency().to_string() })
    }

    pub async fn set_block_browser_url(
        &mut self,
    ) -> Result<(), crate::error::service::ServiceError> {
        // let tx = &mut self.repo;
        let backend_api = self.ctx.get_global_backend_api();

        let app_version = ConfigDomain::get_app_version(self.ctx).await?;

        let req = wallet_transport_backend::request::ChainListReq::new(app_version.app_version);
        let list = backend_api.chain_list(req).await?.list;
        ConfigDomain::set_block_browser_url(self.ctx, &list).await?;
        Ok(())
    }

    pub async fn upload_log_file(
        self,
        req: Vec<crate::request::app::UploadLogFileReq>,
    ) -> Result<(), ServiceError> {
        let client = self
            .ctx
            .get_log_upload_client()
            .ok_or_else(|| ServiceError::Parameter("log upload is not configured".to_string()))?;
        let sn = self.ctx.get_sn();
        let log_dir = Self::canonical_log_upload_dir(&self.ctx.get_global_dirs().get_log_dir())?;
        for req in req.into_iter() {
            let src_file_path = Self::validate_log_upload_source(&log_dir, &req.src_file_path)?;
            let dst_file_name = Self::validate_log_upload_file_name(&req.dst_file_name)?;
            Self::upload_validated_log_file(client.clone(), sn, &src_file_path, dst_file_name)
                .await?;
        }

        Ok(())
    }

    async fn upload_validated_log_file(
        uploader: std::sync::Arc<dyn crate::infrastructure::log::upload_log::LogUploader>,
        sn: &str,
        path: &Path,
        filename: &str,
    ) -> Result<(), ServiceError> {
        let data = tokio::fs::read(path).await.map_err(|e| {
            ServiceError::Parameter(format!("failed to read log upload source: {e}"))
        })?;
        uploader.upload(sn, filename, data).await?;
        Ok(())
    }

    fn canonical_log_upload_dir(log_dir: &Path) -> Result<PathBuf, ServiceError> {
        log_dir
            .canonicalize()
            .map_err(|e| ServiceError::Parameter(format!("invalid log directory for upload: {e}")))
    }

    fn validate_log_upload_source(
        log_dir: &Path,
        src_file_path: &str,
    ) -> Result<PathBuf, ServiceError> {
        let src = Path::new(src_file_path)
            .canonicalize()
            .map_err(|e| ServiceError::Parameter(format!("invalid log upload source file: {e}")))?;

        if !src.starts_with(log_dir) {
            return Err(ServiceError::Parameter(
                "log upload source must be under log directory".to_string(),
            ));
        }
        if !src.is_file() {
            return Err(ServiceError::Parameter("log upload source must be a file".to_string()));
        }

        Ok(src)
    }

    fn validate_log_upload_file_name(file_name: &str) -> Result<&str, ServiceError> {
        crate::infrastructure::path_guard::validate_path_segment(
            file_name,
            "log upload destination file name",
        )?;
        if file_name.contains('\\') {
            return Err(ServiceError::Parameter(
                "invalid log upload destination file name".to_string(),
            ));
        }
        Ok(file_name)
    }

    pub async fn mqtt_subscribe(
        self,
        topics: Vec<String>,
        qos: Option<u8>,
    ) -> Result<(), crate::error::service::ServiceError> {
        // 获取全局 topics
        let global_topics = self.ctx.get_global_mqtt_topics();
        let mut global_topics = global_topics.write().await;
        let handles = self.ctx.get_handles_arc().await?;

        global_topics.subscribe(topics, qos, &handles).await?;

        Ok(())
    }

    pub async fn mqtt_unsubscribe_unsubscribe(
        self,
        topics: Vec<String>,
    ) -> Result<(), crate::error::service::ServiceError> {
        // 获取全局已订阅的主题
        let global_topics = self.ctx.get_global_mqtt_topics();
        let mut global_topics = global_topics.write().await;
        let handles = self.ctx.get_handles_arc().await?;

        global_topics.unsubscribe(topics, &handles).await?;

        Ok(())
    }

    pub async fn mqtt_resubscribe(self) -> Result<(), crate::error::service::ServiceError> {
        // 获取全局已订阅的主题
        let global_topics = self.ctx.get_global_mqtt_topics();
        let global_topics = global_topics.write().await;
        let handles = self.ctx.get_handles_arc().await?;

        global_topics.resubscribe(&handles).await?;

        Ok(())
    }

    pub async fn get_configs(
        self,
    ) -> Result<Vec<ConfigEntity>, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;
        let res = ConfigRepo::list_v2(&pool).await?;
        Ok(res)
    }

    pub async fn set_config(
        self,
        key: String,
        value: String,
    ) -> Result<ConfigEntity, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;

        // let min_config =
        //     wallet_database::entities::config::MinValueSwitchConfig::try_from(value.clone())?;

        let res = ConfigRepo::upsert(&key, &value, Some(0), &pool).await?;

        Ok(res)
    }

    pub async fn set_min_value_config(
        self,
        symbol: String,
        amount: f64,
        switch: bool,
    ) -> Result<MinValueSwitchConfig, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;

        let sn = self.ctx.get_global_device().sn.clone();

        let symbol = symbol.to_ascii_uppercase();
        let key = MinValueSwitchConfig::get_key(&symbol, &sn);
        let config = MinValueSwitchConfig::new(switch, amount);

        ConfigRepo::upsert(&key, &config.to_json_str()?, Some(1), &pool).await?;

        let req = wallet_transport_backend::response_vo::app::SaveSendMsgAccount {
            sn: sn.clone(),
            amount,
            symbol,
            is_open: switch,
        };
        let backend = self.ctx.get_global_backend_api();

        if let Err(e) = backend.save_send_msg_account(vec![req]).await {
            tracing::warn!("filter min value report faild sn = {} error = {}", sn, e);
        }

        Ok(config)
    }

    pub async fn get_min_value_config(
        self,
        symbol: String,
    ) -> Result<Option<MinValueSwitchConfig>, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;

        let symbol = symbol.to_uppercase();
        let sn = self.ctx.get_global_device().sn.clone();

        let key = MinValueSwitchConfig::get_key(&symbol, &sn);

        match ConfigRepo::find_by_key(&key, &pool).await? {
            Some(config) => Ok(Some(MinValueSwitchConfig::try_from(config.value)?)),
            None => Ok(None),
        }
    }

    pub async fn app_install_save(
        self,
        sn: &str,
        device_type: &str,
        channel: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let req = AppInstallSaveReq::new(sn, device_type, channel);
        // 1. 首先递增Epoch，切换世代，这是reset的核心事实
        // 确保reset开始后，所有后续操作都使用新世代的Epoch
        ConfigDomain::bump_keys_reset_epoch(self.ctx).await?;
        // 获取新的epoch值用于日志
        let new_epoch = ConfigDomain::get_keys_reset_epoch(self.ctx).await?;
        tracing::info!(
            epoch = new_epoch,
            sn = sn,
            "app_install_save: Epoch bumped, generation switched"
        );

        let app_install_save_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::APP_INSTALL_SAVE,
            &req,
        )?;
        let keys_reset_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::KEYS_RESET,
            &serde_json::json!({
                "sn": sn
            }),
        )?;
        Tasks::new()
            .push(BackendApiTask::BackendApi(app_install_save_data))
            .push(BackendApiTask::BackendApi(keys_reset_data))
            .send_with_ctx(self.ctx)
            .await?;
        // backend.keys_reset(sn).await?;
        Ok(())
    }

    pub async fn request_backend(
        self,
        endpoint: &str,
        body: String,
    ) -> Result<serde_json::Value, crate::error::service::ServiceError> {
        let backend = self.ctx.get_global_backend_api();

        let result = backend.post_req_string::<serde_json::Value>(endpoint, body).await?;
        Ok(result)
    }

    pub async fn global_msg(self) -> Result<GlobalMsg, crate::error::service::ServiceError> {
        let mut msg = GlobalMsg::default();

        let pool = self.ctx.core_pool()?;

        let queues = MultisigQueueRepo::pending_handle(&pool).await?;
        for queue in queues.iter() {
            if !queue.permission_id.is_empty() {
                msg.pending_multisig_trans.push(MultisigAccountBase::from(queue));

                continue;
            }

            // 多签交易需要判断是否是发起者：多签的发起者才可以执行交易
            if queue.status == MultisigQueueStatus::PendingExecution.to_i8() {
                if let Some(account) =
                    MultisigAccountRepo::find_by_id(&pool, &queue.account_id).await?
                {
                    if account.owner != MultiAccountOwner::Participant.to_i8() {
                        msg.pending_multisig_trans.push(MultisigAccountBase::from(queue));
                    }
                }
                continue;
            }

            msg.pending_multisig_trans.push(MultisigAccountBase::from(queue));
        }

        // 多签账号状态
        msg.pending_deploy_multisig =
            MultisigAccountRepo::pending_handle(&pool, MultisigAccountStatus::Confirmed)
                .await?
                .into_iter()
                .map(MultisigAccountBase::from)
                .collect();

        msg.pending_agree_multisig =
            MultisigAccountRepo::pending_handle(&pool, MultisigAccountStatus::Pending)
                .await?
                .into_iter()
                .map(MultisigAccountBase::from)
                .collect();

        Ok(msg)
    }

    pub async fn set_invite_code(
        self,
        invite_code: Option<String>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;
        let sn = self.ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(pool, sn).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };

        let is_invite = invite_code.is_some();
        let req = wallet_transport_backend::request::SetInviteeStatusReq {
            sn: device.sn,
            invitee: is_invite,
        };

        ConfigDomain::set_invite_code(self.ctx, Some(is_invite), invite_code).await?;
        let task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::DEVICE_EDIT_DEVICE_INVITEE_STATUS,
            &req,
        )?;
        Tasks::new().push(BackendApiTask::BackendApi(task_data)).send_with_ctx(self.ctx).await?;

        Ok(())
    }

    pub async fn backend_config(
        self,
    ) -> Result<std::collections::HashMap<String, String>, crate::error::service::ServiceError>
    {
        let backend = self.ctx.get_global_backend_api();
        Ok(backend.all_config().await?.configs)
    }

    pub async fn set_wallet_type(
        self,
        wallet_type: ApiWalletType,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.ctx.set_current_wallet_type(wallet_type).await
    }

    pub async fn get_current_wallet_type(&self) -> ReturnType<ApiWalletType> {
        self.ctx.get_current_wallet_type().await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{Arc, Mutex},
    };

    use super::AppService;
    use crate::infrastructure::log::upload_log::LogUploader;

    #[derive(Default)]
    struct FakeUploader {
        reject: bool,
        calls: Mutex<Vec<(String, String, Vec<u8>)>>,
    }

    #[async_trait::async_trait]
    impl LogUploader for FakeUploader {
        async fn upload(
            &self,
            sn: &str,
            filename: &str,
            data: Vec<u8>,
        ) -> Result<(), crate::error::system::SystemError> {
            self.calls.lock().unwrap().push((sn.into(), filename.into(), data));
            if self.reject {
                Err(crate::error::system::SystemError::Service("synthetic rejection".into()))
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn manual_log_upload_reads_exact_file_and_propagates_backend_failure() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log_file = dir.path().join("manual-log.txt");
        fs::write(&log_file, b"synthetic manual log").expect("write log file");
        let success = Arc::new(FakeUploader::default());

        AppService::upload_validated_log_file(
            success.clone(),
            "device-sn",
            &log_file,
            "manual-log.txt",
        )
        .await
        .expect("upload succeeds");

        assert_eq!(
            success.calls.lock().unwrap().as_slice(),
            &[("device-sn".into(), "manual-log.txt".into(), b"synthetic manual log".to_vec())]
        );
        let rejected = Arc::new(FakeUploader { reject: true, ..Default::default() });
        assert!(
            AppService::upload_validated_log_file(
                rejected,
                "device-sn",
                &log_file,
                "manual-log.txt",
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn upload_log_file_source_accepts_file_under_log_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log_dir = dir.path().join("log");
        fs::create_dir_all(&log_dir).expect("create log dir");
        let log_file = log_dir.join("log.txt");
        fs::write(&log_file, "hello").expect("write log file");
        let log_dir = AppService::canonical_log_upload_dir(&log_dir).expect("canonical log dir");

        let resolved = AppService::validate_log_upload_source(&log_dir, log_file.to_str().unwrap())
            .expect("log file under log dir should be valid");

        assert_eq!(resolved, log_file.canonicalize().expect("canonical log file"));
    }

    #[test]
    fn upload_log_file_source_rejects_file_outside_log_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log_dir = dir.path().join("log");
        fs::create_dir_all(&log_dir).expect("create log dir");
        let secret_file = dir.path().join("secret.txt");
        fs::write(&secret_file, "secret").expect("write secret file");
        let log_dir = AppService::canonical_log_upload_dir(&log_dir).expect("canonical log dir");

        let err = AppService::validate_log_upload_source(&log_dir, secret_file.to_str().unwrap())
            .expect_err("outside file must be rejected");

        assert!(err.to_string().contains("source must be under log directory"));
    }

    #[test]
    fn upload_log_file_name_rejects_nested_destination() {
        let err = AppService::validate_log_upload_file_name("../secret.txt")
            .expect_err("traversal destination must be rejected");

        assert!(err.to_string().contains("invalid log upload destination file name"));
    }

    #[test]
    fn upload_log_file_name_rejects_windows_separator() {
        let err = AppService::validate_log_upload_file_name(r"nested\secret.txt")
            .expect_err("windows separator destination must be rejected");

        assert!(err.to_string().contains("invalid log upload destination file name"));
    }
}
