use crate::{
    api::ReturnType, manager::WalletManager, request::wallet::CreateWalletReq,
    response_vo::standard_wallet::wallet::CreateWalletRes, service::wallet::WalletService,
};

impl WalletManager {
    /// Derives a password value using the current device serial number.
    ///
    /// This is a pure derivation helper: it returns the derived value and does not
    /// create or update the wallet password stored by the SDK.
    #[deprecated(
        note = "This fixed-salt compatibility helper is not used by the current password flow; pass the user's password to validate_password instead"
    )]
    pub async fn encrypt_password(&self, password: &str) -> ReturnType<String> {
        WalletService::new(self.ctx).encrypt_password(password).await
    }

    pub async fn validate_password(&self, password: &str) -> ReturnType<()> {
        WalletService::new(self.ctx).validate_password(password).await
    }

    pub async fn switch_wallet(&self, wallet_address: &str) -> ReturnType<()> {
        WalletService::new(self.ctx).switch_wallet(wallet_address).await
    }

    pub async fn edit_wallet_name(
        &self,
        wallet_name: &str,
        wallet_address: &str,
    ) -> ReturnType<()> {
        WalletService::new(self.ctx).edit_wallet_name(wallet_name, wallet_address).await
    }

    pub async fn logic_reset(&self) -> ReturnType<()> {
        WalletService::new(self.ctx).logic_reset().await
    }

    pub async fn physical_reset(&self) -> ReturnType<()> {
        WalletService::new(self.ctx).physical_reset().await
    }

    pub async fn create_wallet(&self, req: CreateWalletReq) -> ReturnType<CreateWalletRes> {
        WalletService::new(self.ctx)
            .create_wallet(
                req.language_code,
                &req.phrase,
                &req.salt,
                &req.wallet_name,
                &req.account_name,
                req.is_default_name,
                &req.wallet_password,
                req.invite_code, // req.derive_password,
            )
            .await
    }

    pub async fn get_phrase(
        &self,
        wallet_address: &str,
        password: &str,
    ) -> ReturnType<crate::response_vo::standard_wallet::wallet::GetPhraseRes> {
        WalletService::new(self.ctx).get_phrase(wallet_address, password).await
    }

    pub async fn import_derivation_path(
        &self,
        path: &str,
        wallet_address: &str,
        wallet_password: &str,
        account_name: &str,
        is_default_name: bool,
    ) -> ReturnType<crate::response_vo::standard_wallet::wallet::ImportDerivationPathRes> {
        WalletService::new(self.ctx)
            .import_derivation_path(
                path,
                wallet_address,
                wallet_password,
                account_name,
                is_default_name,
            )
            .await
    }

    pub async fn export_derivation_path(
        &self,
        wallet_address: &str,
    ) -> ReturnType<crate::response_vo::standard_wallet::wallet::ExportDerivationPathRes> {
        WalletService::new(self.ctx).export_derivation_path(wallet_address).await
    }

    pub async fn get_wallet_list(
        &self,
        wallet_address: Option<String>,
        chain_code: Option<String>,
        account_id: Option<u32>,
    ) -> ReturnType<Vec<crate::response_vo::standard_wallet::wallet::WalletInfo>> {
        WalletService::new(self.ctx).get_wallet_list(wallet_address, chain_code, account_id).await
    }

    pub async fn logic_delete_wallet(&self, address: &str) -> ReturnType<()> {
        WalletService::new(self.ctx).logic_delete(address).await
    }

    pub async fn physical_delete_wallet(&self, address: &str) -> ReturnType<()> {
        WalletService::new(self.ctx).physical_delete(address).await
    }

    pub async fn recover_multisig_data(&self, wallet_address: &str) -> ReturnType<()> {
        WalletService::new(self.ctx).recover_multisig_data(wallet_address).await
    }

    pub async fn upgrade_algorithm(&self, password: &str) -> ReturnType<()> {
        WalletService::new(self.ctx).upgrade_algorithm(password).await
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use crate::testkit::env::get_manager;

    use anyhow::Result;
    use std::{env, path::PathBuf};

    #[tokio::test]
    async fn test_create_wallet() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, test_params) = get_manager().await?;
        let res = wallet_manager.create_wallet(test_params.create_wallet_req).await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_create_wallet2() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, mut test_params) = get_manager().await?;

        test_params.create_wallet_req.salt = "nb".to_string();

        test_params.create_wallet_req.wallet_name = "oh".to_string();

        let res = wallet_manager.create_wallet(test_params.create_wallet_req).await;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_encrypt_password() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

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
        let (wallet_manager, _test_params) = get_manager().await?;

        // let wallet_address = "0x3A616291F1b7CcA94E753DaAc8fC96806e21Ea26";
        let wallet_address = "0x2154Fcc3C1CEC3eB158Ed8984934bFD332b32A3d";
        let res = wallet_manager.logic_delete_wallet(wallet_address).await;
        tracing::info!("res: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_physical_del_wallet() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // let wallet_address = "0x668fb1D3Df02391064CEe50F6A3ffdbAE0CDb406";
        let wallet_address = "0x22fa9304db331864827C27305f1337FEB4a48a71";
        // let wallet_address = "0xd8dc4B7daEfc0C993d1A7d3E2D4Dc998436032b3";
        let res = wallet_manager.physical_delete_wallet(wallet_address).await;
        tracing::info!("res: {res:?}");

        Ok(())
    }

    // 恢复多签张账号
    #[tokio::test]
    async fn test_recover_uid_multisig_data() -> Result<()> {
        wallet_utils::init_test_log();
        let (manager, _) = get_manager().await?;

        // 前端的uid
        let uid = "c25bcbb0cbe65b55d579bfd84840fabc28a12841042e5cd6fb4331aaa6769df3";
        // let address = Some("TUe3T6ErJvnoHMQwVrqK246MWeuCEBbyuR".to_string());
        let address = None;
        let start_time = std::time::Instant::now();

        let res = crate::domain::multisig::MultisigDomain::recover_uid_multisig_data_with_ctx(
            manager.ctx,
            uid,
            address,
        )
        .await;
        let elapsed_time = start_time.elapsed();
        tracing::info!("test_recover_multisig_data elapsed time: {:?}", elapsed_time);
        tracing::info!("res: {res:?}");
        Ok(())
    }

    // 恢复多签队列数据
    #[tokio::test]
    async fn test_recover_queue_data() -> Result<()> {
        let uid = std::env::var("WALLET_TEST_RECOVERY_UID")?;
        wallet_utils::init_test_log();
        let (manager, _) = get_manager().await?;

        let res = crate::domain::multisig::MultisigQueueDomain::recover_queue_data_with_ctx(
            manager.ctx,
            &uid,
        )
        .await;
        tracing::info!("res: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_edit_wallet_name() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let wallet_address = "0x48914c12BbB44a4c32e6CA7A99831c46267533B0";
        let res = wallet_manager.edit_wallet_name("new wallet", wallet_address).await;
        tracing::info!("res: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_reset() -> Result<()> {
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let _address = wallet_manager.logic_reset().await?;
        tracing::info!("res: {_address:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_physical_reset() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let _address = wallet_manager.physical_reset().await?;
        tracing::info!("res: {_address:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_switch_wallet() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let wallet_address = "0x22fa9304db331864827C27305f1337FEB4a48a71";
        let res = wallet_manager.switch_wallet(wallet_address).await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_export_derivation_path() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let wallet_address = "0x0996dc2A80F35D7075C426bf0Ac6e389e0AB99Fc";
        let res = wallet_manager.export_derivation_path(wallet_address).await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_import_derivation_path() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, test_params) = get_manager().await?;
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
            .import_derivation_path(
                &storage_dir,
                wallet_address,
                &test_params.create_wallet_req.wallet_password,
                account_name,
                true,
            )
            .await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_wallet_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // let chain_code = Some("tron".to_string());
        let chain_code = None;
        let list = wallet_manager.get_wallet_list(None, chain_code, None).await?;
        let res = serde_json::to_string(&list).unwrap();
        tracing::info!("res: {res:?}");
        // tracing::info!("list: {list:#?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_phrase() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, test_params) = get_manager().await?;

        let wallet_address = "0xDA32fc1346Fa1DF9719f701cbdd6855c901027C1";

        let res = wallet_manager
            .get_phrase(wallet_address, &test_params.create_wallet_req.wallet_password)
            .await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_set_all_password() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        // let old_passwd = "123456";
        let old_passwd = "q1111111";
        // let old_passwd = "new_passwd";
        // let new_passwd = "q1111111";
        let new_passwd = "new_passwd";
        // let new_passwd = "123456";
        let res = wallet_manager
            // .service
            .set_all_password(old_passwd, new_passwd)
            .await?;
        tracing::info!("res: {res:?}");
        // let wallet_address = "0xDA32fc1346Fa1DF9719f701cbdd6855c901027C1";
        // let key = wallet_manager
        //     .get_account_private_key(new_passwd, wallet_address, 1)
        //     .await;
        // tracing::info!("key: {key:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_upgrade_algorithm() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        let res = wallet_manager
            // .upgrade_algorithm(&test_params.create_wallet_req.wallet_password)
            .upgrade_algorithm("q1111111")
            .await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_validate_password() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        let res = wallet_manager
            // .upgrade_algorithm(&test_params.create_wallet_req.wallet_password)
            .validate_password("q1111111")
            .await?;
        tracing::info!("res: {res:?}");
        Ok(())
    }
}
