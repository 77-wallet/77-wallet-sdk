use crate::{
    api::ReturnType,
    error::service::ResultExt,
    manager::WalletManager,
    request::account::CreateAccountReq,
    response_vo::standard_wallet::account::{
        CurrentAccountInfo, DerivedAddressesList, GetAccountPrivateKeyRes,
        QueryAccountDerivationPath,
    },
    service::account::AccountService,
};
use wallet_database::entities::account::AccountEntity;

impl WalletManager {
    pub async fn switch_account(&self, wallet_address: &str, account_id: u32) -> ReturnType<()> {
        AccountService::new(self.ctx).switch_account(wallet_address, account_id).await
    }

    pub async fn create_account(&self, req: CreateAccountReq) -> ReturnType<()> {
        AccountService::new(self.ctx)
            .create_account(
                &req.wallet_address,
                &req.root_password,
                // req.derive_password,
                req.derivation_path,
                req.index,
                &req.name,
                req.is_default_name,
            )
            .await
            .with_context("create_account")
    }

    pub async fn edit_account_name(
        &self,
        account_id: u32,
        wallet_address: &str,
        name: &str,
    ) -> ReturnType<()> {
        AccountService::new(self.ctx).edit_account_name(account_id, wallet_address, name).await
    }

    #[allow(dead_code)]
    pub(crate) async fn account_detail(&self, address: &str) -> ReturnType<Option<AccountEntity>> {
        AccountService::new(self.ctx).account_details(address).await
    }

    pub async fn get_account_list(
        &self,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
    ) -> ReturnType<Vec<AccountEntity>> {
        AccountService::new(self.ctx).get_account_list(wallet_address, account_id).await
    }

    pub async fn get_account_derivation_path(
        &self,
        wallet_address: &str,
        index: u32,
    ) -> ReturnType<Vec<QueryAccountDerivationPath>> {
        AccountService::new(self.ctx).get_account_derivation_path(wallet_address, index).await
    }

    pub async fn list_derived_addresses(
        &self,
        wallet_address: &str,
        index: i32,
        password: &str,
        all: bool,
    ) -> ReturnType<Vec<DerivedAddressesList>> {
        AccountService::new(self.ctx)
            .list_derived_addresses(wallet_address, index, password, all)
            .await
    }

    pub async fn current_chain_address(
        &self,
        address: String,
        account_id: u32,
        chain_code: String,
    ) -> ReturnType<Vec<QueryAccountDerivationPath>> {
        AccountService::new(self.ctx).current_chain_address(address, account_id, &chain_code).await
    }

    pub async fn current_account(
        &self,
        wallet_address: String,
        account_id: i32,
    ) -> ReturnType<Vec<CurrentAccountInfo>> {
        AccountService::new(self.ctx).current_accounts(&wallet_address, account_id).await
    }

    // /// Recovers a subkey associated with a given wallet name and address.
    // ///
    // /// This function attempts to recover a subkey by performing the following steps:
    // /// 1. Retrieves the path to the subkeys directory for the specified wallet.
    // /// 2. Traverses the directory structure to get the wallet tree.
    // /// 3. Calls the `recover_subkey` function from the wallet manager handler to perform the recovery.
    // ///
    // /// # Arguments
    // ///
    // /// * `wallet_name` - A `String` specifying the name of the wallet.
    // /// * `address` - A `String` specifying the address associated with the subkey.
    // ///
    // /// # Returns
    // ///
    // /// * `Response<()>` - A response indicating the success or failure of the operation.
    // pub fn recover_subkey(&self, wallet_name: &str, address: &str) -> ReturnType<()> {
    //     let pool = crate::manager::Context::get_global_sqlite_pool()?;
    //     let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

    //     AccountService::new(repo)
    //         .recover_subkey(wallet_name, address)?
    //         .into()
    // }

    pub async fn get_account_private_key(
        &self,
        password: &str,
        wallet_address: &str,
        account_id: u32,
    ) -> ReturnType<GetAccountPrivateKeyRes> {
        AccountService::new(self.ctx)
            .get_account_private_key(password, wallet_address, account_id)
            .await
    }

    pub async fn set_all_password(&self, old_password: &str, new_password: &str) -> ReturnType<()> {
        AccountService::new(self.ctx).set_all_password(old_password, new_password).await
    }

    pub async fn physical_delete_account(
        &self,
        wallet_address: &str,
        account_id: u32,
        password: &str,
    ) -> ReturnType<()> {
        AccountService::new(self.ctx)
            .physical_delete_account(wallet_address, account_id, password)
            .await
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use crate::testkit::env::get_manager;
    use anyhow::{Context, Result};

    #[tokio::test]
    async fn test_switch_account() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        let account =
            wallet_manager.switch_account("0x8E5424c1347d27B6816eba3AEE7FbCeDFa229C1F", 2).await?;
        tracing::info!("[test_switch_account] account: {account:?}");
        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[test_switch_account] account: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_account_detail() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        let account = wallet_manager.account_detail("TLzteCJi4jSGor5EDRYZcdQ4hsZRQQZ4XR").await?;
        tracing::info!("[test_account_detail] account: {account:?}");

        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[test_account_detail] account: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_account_private_key() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, test_params) = get_manager().await?;

        // let account_name = "account_name1";
        // let derivation_path = Some("m/44'/60'/0'/0/1".to_string());
        // let derivation_path = Some("m/44'/501'/1'/0".to_string());

        // let wallet_address = "0x668fb1D3Df02391064CEe50F6A3ffdbAE0CDb406";
        // let wallet_address = "0x65Eb73c5aeAD87688D639E796C959E23C2356681";
        let wallet_address = "0x57CF28DD99cc444A9EEEEe86214892ec9F295480";
        let password = &test_params.create_wallet_req.wallet_password;
        // let password = "new_passwd";
        let account = wallet_manager.get_account_private_key(password, wallet_address, 1).await?;
        tracing::info!("[get_account_private_key] account: {account:?}");

        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[get_account_private_key] account: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_create_account() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, test_params) =
            get_manager().await.with_context(|| "get_manager().await")?;

        let _address = test_params.create_account_req.wallet_address.clone();
        wallet_manager.create_wallet(test_params.create_wallet_req).await?;
        let _account = wallet_manager
            .create_account(test_params.create_account_req)
            .await
            .with_context(|| "create_account")?;
        // let account = account.unwrap();
        // tracing::info!("[test_] account: {account:?}");
        // let list = wallet_manager.get_account_list(Some(&address), None).await.result;
        // tracing::info!("[test_create_account] list: {list:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_query_account_derivation_path() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;
        // let wallet_address = "0xc6f9823E95782FAff8C78Cd67BD9C03F3A54108d";
        let wallet_address = "0x57CF28DD99cc444A9EEEEe86214892ec9F295480";
        let account =
            wallet_manager.get_account_derivation_path(wallet_address, 2147483648).await?;
        tracing::info!("[get_account_derivation_path] get_account_derivation_path: {account:?}");
        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[get_account_derivation_path] get_account_derivation_path: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_show_index_address() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, test_params) = get_manager().await?;
        // let wallet_address = "0xc6f9823E95782FAff8C78Cd67BD9C03F3A54108d";
        let wallet_address = "0x57CF28DD99cc444A9EEEEe86214892ec9F295480";
        let account = wallet_manager
            .list_derived_addresses(
                wallet_address,
                -1,
                &test_params.create_wallet_req.wallet_password,
                true,
            )
            .await?;
        tracing::info!("[test_show_index_address] show_index_address: {account:?}");
        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[test_show_index_address] show_index_address: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_physical_delete_account() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let account_id = 2;
        let wallet_address = "0xDA32fc1346Fa1DF9719f701cbdd6855c901027C1";
        let password = &_test_params.create_wallet_req.wallet_password;
        let account =
            wallet_manager.physical_delete_account(wallet_address, account_id, password).await?;
        tracing::info!("[test_] test_physical_delete_account: {account:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_edit_account_name() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let wallet_address = "0x57CF28DD99cc444A9EEEEe86214892ec9F295480";
        let account = wallet_manager.edit_account_name(1, wallet_address, "new_account").await?;
        tracing::info!("[test_] account: {account:?}");
        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[test_edit_account_name] account: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_account_details() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let wallet_address = "0x57CF28DD99cc444A9EEEEe86214892ec9F295480";
        let account_id = 1;
        let account =
            wallet_manager.get_account_list(Some(wallet_address), Some(account_id)).await?;
        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[test_] account: {res:?}");

        Ok(())
    }
}
