use crate::api::ReturnType;
use crate::response_vo::account::DerivedAddressesList;
use crate::service::account::AccountService;
use wallet_database::entities::account::AccountEntity;

impl crate::WalletManager {
    pub async fn switch_account(&self, wallet_address: &str, account_id: u32) -> ReturnType<()> {
        AccountService::new(self.repo_factory.resuource_repo())
            .switch_account(wallet_address, account_id)
            .await?
            .into()
    }

    pub async fn create_account(
        &self,
        wallet_address: &str,
        root_password: &str,
        derive_password: Option<String>,
        derivation_path: Option<String>,
        index: Option<i32>,
        name: &str,
        is_default_name: bool,
    ) -> ReturnType<()> {
        AccountService::new(self.repo_factory.resuource_repo())
            .create_account(
                wallet_address,
                root_password,
                derive_password,
                derivation_path,
                index,
                name,
                is_default_name,
            )
            .await?
            .into()
    }

    pub async fn edit_account_name(
        &self,
        account_id: u32,
        wallet_address: &str,
        name: &str,
    ) -> ReturnType<()> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());
        AccountService::new(repo)
            .edit_account_name(account_id, wallet_address, name)
            .await?
            .into()
    }

    #[allow(dead_code)]
    pub(crate) async fn account_detail(&self, address: &str) -> ReturnType<Option<AccountEntity>> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());
        AccountService::new(repo)
            .account_details(address)
            .await?
            .into()
    }

    pub async fn get_account_list(
        &self,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
    ) -> ReturnType<Vec<AccountEntity>> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        AccountService::new(repo)
            .get_account_list(wallet_address, account_id)
            .await?
            .into()
    }

    pub async fn list_derived_addresses(
        &self,
        wallet_address: &str,
        index: i32,
        password: &str,
        all: bool,
    ) -> ReturnType<Vec<DerivedAddressesList>> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        AccountService::new(repo)
            .list_derived_addresses(wallet_address, index, password, all)
            .await?
            .into()
    }

    /// Recovers a subkey associated with a given wallet name and address.
    ///
    /// This function attempts to recover a subkey by performing the following steps:
    /// 1. Retrieves the path to the subkeys directory for the specified wallet.
    /// 2. Traverses the directory structure to get the wallet tree.
    /// 3. Calls the `recover_subkey` function from the wallet manager handler to perform the recovery.
    ///
    /// # Arguments
    ///
    /// * `wallet_name` - A `String` specifying the name of the wallet.
    /// * `address` - A `String` specifying the address associated with the subkey.
    ///
    /// # Returns
    ///
    /// * `Response<()>` - A response indicating the success or failure of the operation.
    pub fn recover_subkey(&self, wallet_name: &str, address: &str) -> ReturnType<()> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        AccountService::new(repo)
            .recover_subkey(wallet_name, address)?
            .into()
    }

    pub async fn get_account_private_key(
        &self,
        password: &str,
        wallet_address: &str,
        account_id: u32,
    ) -> ReturnType<crate::response_vo::account::GetAccountPrivateKeyRes> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        AccountService::new(repo)
            .get_account_private_key(password, wallet_address, account_id)
            .await?
            .into()
    }

    pub async fn get_account_address(
        &self,
        wallet_address: &str,
        account_id: u32,
    ) -> ReturnType<crate::response_vo::account::GetAccountAddressRes> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        AccountService::new(repo)
            .get_account_address(wallet_address, account_id)
            .await?
            .into()
    }

    pub async fn set_all_password(&self, old_password: &str, new_password: &str) -> ReturnType<()> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        AccountService::new(repo)
            .set_all_password(old_password, new_password)
            .await?
            .into()
    }

    pub async fn physical_delete_account(
        &self,
        wallet_address: &str,
        account_id: u32,
    ) -> ReturnType<()> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        AccountService::new(repo)
            .physical_delete_account(wallet_address, account_id)
            .await?
            .into()
    }
}

#[cfg(test)]
mod test {
    use crate::test::env::{setup_test_environment, TestData};
    use anyhow::Result;

    #[tokio::test]
    async fn test_switch_account() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let account = wallet_manager
            .switch_account("0x8E5424c1347d27B6816eba3AEE7FbCeDFa229C1F", 2)
            .await;
        tracing::info!("[test_switch_account] account: {account:?}");
        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[test_switch_account] account: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_account_detail() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let account = wallet_manager
            .account_detail("TLzteCJi4jSGor5EDRYZcdQ4hsZRQQZ4XR")
            .await;
        tracing::info!("[test_account_detail] account: {account:?}");

        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[test_account_detail] account: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_account_private_key() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData {
            wallet_manager,
            wallet_env,
            ..
        } = setup_test_environment(None, None, false, None).await?;

        // let account_name = "account_name1";
        // let derivation_path = Some("m/44'/60'/0'/0/1".to_string());
        // let derivation_path = Some("m/44'/501'/1'/0".to_string());
        let password = &wallet_env.password;
        // let password = "new_passwd";
        let account = wallet_manager
            .get_account_private_key(password, "0x3A616291F1b7CcA94E753DaAc8fC96806e21Ea26", 1)
            .await;
        tracing::info!("[get_account_private_key] account: {account:?}");

        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[get_account_private_key] account: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_create_account() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData {
            wallet_manager,
            wallet_env: _,
            ..
        } = setup_test_environment(None, None, false, None).await?;

        // let chain_code = "trx";
        // let account_name = "account_name1";
        // let derivation_path = Some("m/44'/60'/0'/0/1".to_string());
        // let derivation_path = Some("m/44'/501'/1'/0".to_string());
        let derivation_path = None;
        // let index = Some(2147478971);
        // let index = Some(2);
        let index = Some(3);
        // let index = Some(i32::MIN);
        // let address = "0x3A616291F1b7CcA94E753DaAc8fC96806e21Ea26";
        // let address = "0x2b3f8269917dE02b02f5ac22fe1B4291Ed94D10a";
        let address = "0x76ab490669d54F312e15C9a8f0d24E6476a5CDc0";

        // let password = wallet_env.password;
        let password = "123456";
        let account = wallet_manager
            .create_account(
                address,
                &password,
                Some(password.to_string()),
                derivation_path,
                index,
                "牛逼",
                true,
            )
            .await
            .message;
        tracing::info!("[test_] account: {account:?}");
        // let address = "0xfc6A4Ed634335cde2701553B7dbB2C362510FBd9";
        let list = wallet_manager
            .get_account_list(Some(address), None)
            .await
            .result;
        tracing::info!("[test_create_account] list: {list:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_show_index_address() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData {
            wallet_manager,
            wallet_env,
            ..
        } = setup_test_environment(None, None, false, None).await?;
        // let wallet_address = "0xc6f9823E95782FAff8C78Cd67BD9C03F3A54108d";
        let wallet_address = "0x1AEc788620fE374A3D1D3CC3860F066b508bC86e";
        let account = wallet_manager
            .list_derived_addresses(wallet_address, -1, &wallet_env.password, false)
            .await;
        tracing::info!("[test_show_index_address] show_index_address: {account:?}");
        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[test_show_index_address] show_index_address: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_physical_delete_account() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let account_id = 1;
        let wallet_address = "0x2b3f8269917dE02b02f5ac22fe1B4291Ed94D10a";
        let account = wallet_manager
            .physical_delete_account(wallet_address, account_id)
            .await;
        tracing::info!("[test_] test_physical_delete_account: {account:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_edit_account_name() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let wallet_address = "0x3A616291F1b7CcA94E753DaAc8fC96806e21Ea26";
        let account = wallet_manager
            .edit_account_name(1, wallet_address, "new_account")
            .await;
        tracing::info!("[test_] account: {account:?}");
        let res = serde_json::to_string(&account).unwrap();
        tracing::info!("[test_edit_account_name] account: {res:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_account_details() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let wallet_address = "0x3A616291F1b7CcA94E753DaAc8fC96806e21Ea26";
        let account_id = 1;
        let account = wallet_manager
            .get_account_list(Some(wallet_address), Some(account_id))
            .await;
        tracing::info!("[test_] account: {account:?}");

        Ok(())
    }
}
