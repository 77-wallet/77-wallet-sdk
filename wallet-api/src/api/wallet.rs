use crate::api::ReturnType;
use crate::response_vo::wallet::{CreateWalletRes, ResetRootRes};
use crate::service::wallet::WalletService;

impl crate::WalletManager {
    pub async fn encrypt_password(&self, password: &str) -> ReturnType<String> {
        WalletService::new(self.repo_factory.resuource_repo())
            .encrypt_password(password)
            .await?
            .into()
    }

    pub async fn validate_password(&self, encrypted_password: &str) -> ReturnType<()> {
        WalletService::new(self.repo_factory.resuource_repo())
            .validate_password(encrypted_password)
            .await?
            .into()
    }

    pub async fn switch_wallet(&self, wallet_address: &str) -> ReturnType<()> {
        WalletService::new(self.repo_factory.resuource_repo())
            .switch_wallet(wallet_address)
            .await?
            .into()
    }

    pub async fn edit_wallet_name(
        &self,
        wallet_name: &str,
        wallet_address: &str,
    ) -> ReturnType<()> {
        WalletService::new(self.repo_factory.resuource_repo())
            .edit_wallet_name(wallet_name, wallet_address)
            .await?
            .into()
    }

    pub async fn logic_reset(&self) -> ReturnType<()> {
        WalletService::new(self.repo_factory.resuource_repo())
            .logic_reset()
            .await?
            .into()
    }

    pub async fn physical_reset(&self) -> ReturnType<()> {
        WalletService::new(self.repo_factory.resuource_repo())
            .physical_reset()
            .await?
            .into()
    }

    pub async fn create_wallet(
        &self,
        language_code: u8,
        phrase: &str,
        salt: &str,
        wallet_name: &str,
        account_name: &str,
        is_default_name: bool,
        wallet_password: &str,
        derive_password: Option<String>,
    ) -> ReturnType<CreateWalletRes> {
        WalletService::new(self.repo_factory.resuource_repo())
            .create_wallet(
                language_code,
                phrase,
                salt,
                wallet_name,
                account_name,
                is_default_name,
                wallet_password,
                derive_password,
            )
            .await?
            .into()
    }

    pub async fn get_phrase(
        &self,
        wallet_address: &str,
        password: &str,
    ) -> ReturnType<crate::response_vo::wallet::GetPhraseRes> {
        WalletService::new(self.repo_factory.resuource_repo())
            .get_phrase(wallet_address, password)
            .await?
            .into()
    }

    pub async fn import_derivation_path(
        &self,
        path: &str,
        wallet_address: &str,
        wallet_password: &str,
        account_name: &str,
        is_default_name: bool,
    ) -> ReturnType<crate::response_vo::wallet::ImportDerivationPathRes> {
        WalletService::new(self.repo_factory.resuource_repo())
            .import_derivation_path(
                path,
                wallet_address,
                wallet_password,
                account_name,
                is_default_name,
            )
            .await?
            .into()
    }

    pub async fn export_derivation_path(
        &self,
        wallet_address: &str,
    ) -> ReturnType<crate::response_vo::wallet::ExportDerivationPathRes> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        WalletService::new(repo)
            .export_derivation_path(wallet_address)
            .await?
            .into()
    }

    pub async fn get_wallet_list(
        &self,
        wallet_address: Option<String>,
        chain_code: Option<String>,
    ) -> ReturnType<Vec<crate::response_vo::wallet::WalletInfo>> {
        WalletService::new(self.repo_factory.resuource_repo())
            .get_wallet_list(wallet_address, chain_code)
            .await?
            .into()
    }

    pub async fn logic_delete_wallet(&self, address: &str) -> ReturnType<()> {
        WalletService::new(self.repo_factory.resuource_repo())
            .logic_delete(address)
            .await?
            .into()
    }

    pub async fn physical_delete_wallet(&self, address: &str) -> ReturnType<()> {
        WalletService::new(self.repo_factory.resuource_repo())
            .physical_delete(address)
            .await?
            .into()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn reset_root(
        &self,
        language_code: u8,
        phrase: &str,
        salt: &str,
        address: &str,
        new_password: &str,
        subkey_password: Option<String>,
    ) -> ReturnType<ResetRootRes> {
        WalletService::new(self.repo_factory.resuource_repo())
            .reset_root(
                language_code,
                phrase,
                salt,
                address,
                new_password,
                subkey_password,
            )
            .await?
            .into()
    }

    pub async fn recover_multisig_data(&self, wallet_address: &str) -> ReturnType<()> {
        WalletService::new(self.repo_factory.resuource_repo())
            .recover_multisig_data(wallet_address)
            .await?
            .into()
    }
}

#[cfg(test)]
mod test {
    use crate::test::env::{setup_test_environment, TestData, TestWalletEnv};

    use anyhow::Result;
    use std::env;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_create_wallet() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData {
            wallet_manager,
            wallet_env,
            ..
        } = setup_test_environment(None, None, false, None).await?;
        let TestWalletEnv {
            language_code: _,
            phrase: _,
            salt: _,
            wallet_name,
            password,
        } = &wallet_env;

        // let req = InitDeviceReq {
        //     device_type: "ANDROID".to_string(),
        //     sn: "wenjing".to_string(),
        //     code: "2".to_string(),
        //     system_ver: "3".to_string(),
        //     iemi: Some("4".to_string()),
        //     meid: Some("5".to_string()),
        //     iccid: Some("6".to_string()),
        //     mem: Some("7".to_string()),
        //     app_id: Some("8".to_string()),
        //     package_id: None,
        // };
        // let _res = wallet_manager.init_device(req).await;
        // println!("res: {res:?}");

        // let res = wallet_manager
        //     .service
        //     .get_global_sqlite_context()?
        //     .device_detail()
        //     .await
        //     .unwrap();

        // println!("res: {res:?}");
        // let phrase = "盖 狗 排 霉 狠 评 料 岁 答 尤 硬 性";
        let phrase =
            "hard boost cup illegal express interest spread mother weapon make repeat weapon";
        // let phrase = "wife smoke help special across among want screen solve anxiety worth enforse";
        let salt = "Test1234";
        let account_name = "账户";
        let _ = wallet_manager.init_data().await;
        let res = wallet_manager
            .create_wallet(
                1,
                phrase,
                salt,
                wallet_name,
                account_name,
                true,
                password,
                None,
            )
            .await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_create_wallet2() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData {
            wallet_manager,
            wallet_env,
            ..
        } = setup_test_environment(None, None, false, None).await?;
        let TestWalletEnv { language_code, .. } = &wallet_env;

        let phrase = "virtual muscle bracket drip tent undo design reason dice total ugly beach";
        let salt = "Muson@3962";
        let wallet_name = "导入6";
        let account_name = "账户";
        let password = "Muson@3962";
        let res = wallet_manager
            .create_wallet(
                *language_code,
                phrase,
                salt,
                wallet_name,
                account_name,
                true,
                password,
                None,
            )
            .await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_encrypt_password() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        // let wallet_address = "0x3A616291F1b7CcA94E753DaAc8fC96806e21Ea26";
        let password = "123456";
        let res = wallet_manager.encrypt_password(password).await;
        tracing::info!("res: {res:?}");

        Ok(())
    }
    #[tokio::test]
    async fn test_logic_delete_wallet() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        // let wallet_address = "0x3A616291F1b7CcA94E753DaAc8fC96806e21Ea26";
        let wallet_address = "0x25d438EF0C15FbA678B73C9D0b943cF7Fe581730";
        let res = wallet_manager.logic_delete_wallet(wallet_address).await;
        tracing::info!("res: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_physical_del_wallet() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let wallet_address = "0x2b3f8269917dE02b02f5ac22fe1B4291Ed94D10a";
        // let wallet_address = "0xd8dc4B7daEfc0C993d1A7d3E2D4Dc998436032b3";
        let res = wallet_manager.physical_delete_wallet(wallet_address).await;
        tracing::info!("res: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_recover_multisig_data() -> Result<()> {
        wallet_utils::init_test_log();
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        // 前端的uid
        let wallet_address = "0x3d669d78532F763118561b55daa431956ede4155";

        let res = wallet_manager.recover_multisig_data(wallet_address).await;

        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_recover_uid_multisig_data() -> Result<()> {
        wallet_utils::init_test_log();
        let TestData {
            wallet_manager: _, ..
        } = setup_test_environment(None, None, false, None).await?;

        // 前端的uid
        let uid = "137eb624118a0224f491d94f153c2ad3b6e55661dbf687d8a8ba8c59aa7ab358";
        let start_time = std::time::Instant::now();

        let res = crate::domain::multisig::MultisigDomain::recover_uid_multisig_data(uid).await;
        let elapsed_time = start_time.elapsed();
        tracing::info!(
            "test_recover_multisig_data elapsed time: {:?}",
            elapsed_time
        );
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_recover_queue_data() -> Result<()> {
        wallet_utils::init_test_log();
        let TestData {
            wallet_manager: _, ..
        } = setup_test_environment(None, None, false, None).await?;

        // let uid = "c447318b94179614d70e50644233b30a";
        let uid = "71512c7dcca484ad9a03a0f7798e7bdd45602891ed464e0a541657137328d92d";
        let res = crate::domain::multisig::MultisigQueueDomain::recover_queue_data(uid).await;
        tracing::info!("res: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_edit_wallet_name() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let wallet_address = "0x9e2BEf062f301C85589E342d569058Fd4a1334d7";
        let res = wallet_manager
            .edit_wallet_name("wenjing_wallet", wallet_address)
            .await;
        tracing::info!("res: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_reset() -> Result<()> {
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let _address = wallet_manager.logic_reset().await.result.unwrap();
        tracing::info!("res: {_address:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_physical_reset() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let _address = wallet_manager.physical_reset().await;
        tracing::info!("res: {_address:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_switch_wallet() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let wallet_address = "0x9e2BEf062f301C85589E342d569058Fd4a1334d7";
        let res = wallet_manager.switch_wallet(wallet_address).await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_export_derivation_path() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let wallet_address = "0x0996dc2A80F35D7075C426bf0Ac6e389e0AB99Fc";
        let res = wallet_manager.export_derivation_path(wallet_address).await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_import_derivation_path() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData {
            wallet_manager,
            wallet_env,
            ..
        } = setup_test_environment(None, None, false, None).await?;
        let TestWalletEnv { password, .. } = &wallet_env;
        let storage_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?)
            .join("data")
            .join("test")
            // .join("0x0996dc2A80F35D7075C426bf0Ac6e389e0AB99Fc")
            .join("0xc6f9823E95782FAff8C78Cd67BD9C03F3A54108d")
            .to_string_lossy()
            .to_string();
        let wallet_address = "0x3A616291F1b7CcA94E753DaAc8fC96806e21Ea26";
        let account_name = "账户";
        let res = wallet_manager
            .import_derivation_path(&storage_dir, wallet_address, password, account_name, true)
            .await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_wallet_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let list = wallet_manager.get_wallet_list(None, None).await;
        let res = serde_json::to_string(&list).unwrap();
        tracing::info!("res: {res:?}");
        tracing::info!("list: {list:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_phrase() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData {
            wallet_manager,
            wallet_env: env,
            ..
        } = setup_test_environment(None, None, false, None).await?;

        let wallet_address = "0x3A616291F1b7CcA94E753DaAc8fC96806e21Ea26";

        let res = wallet_manager
            .get_phrase(wallet_address, &env.password)
            .await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_set_all_password() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let old_passwd = "123456";
        // let old_passwd = "new_passwd";
        let new_passwd = "new_passwd";
        // let new_passwd = "123456";
        let res = wallet_manager
            // .service
            .set_all_password(old_passwd, new_passwd)
            .await;
        tracing::info!("res: {res:?}");
        let wallet_address = "0x3A616291F1b7CcA94E753DaAc8fC96806e21Ea26";
        let key = wallet_manager
            .get_account_private_key(new_passwd, wallet_address, 1)
            .await;
        tracing::info!("key: {key:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_reset_root() -> Result<()> {
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData {
            wallet_manager,
            wallet_env: env,
            ..
        } = setup_test_environment(None, None, false, None).await?;
        let TestWalletEnv {
            language_code: _,
            phrase,
            salt,
            wallet_name: _,
            password: _,
        } = env;
        let address = "0x0D517b8d9D1a2D862816Ba9d5656eC63548629e0";
        let new_passwd = "new_passwd";
        let derive_passwd = "new_passwd";
        let _address = wallet_manager
            .reset_root(
                1, &phrase,    // 使用环境中的助记词
                &salt,      // 使用环境中的盐
                address,    // 传入指定的地址
                new_passwd, // Some(derive_passwd.to_string()),
                None,
            )
            .await
            .result
            .unwrap();
        let derive_address = "0xA8eEE0468F2D87D7603ec72c988c5f24C11fEd32";
        let key = wallet_manager
            .get_account_private_key(derive_passwd, derive_address, 1)
            .await
            .result;
        println!("key: {key:?}");
        Ok(())
    }
}
