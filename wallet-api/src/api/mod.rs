pub mod account;
pub mod address_book;
pub mod announcement;
pub mod app;
pub mod asset;
pub mod chain;
pub mod coin;
pub mod device;
pub mod multisig_account;
pub mod multisig_transaction;
pub mod node;
pub mod phrase;
pub mod stake;
pub mod system_notification;
use crate::response::Response;

#[cfg(not(feature = "result"))]
pub type ReturnType<T> = Response<T>;
#[cfg(feature = "result")]
pub type ReturnType<T> = Result<T, crate::ServiceError>;

// #[cfg(feature = "transaction")]
pub mod transaction;
pub mod wallet;

impl super::WalletManager {
    // /// Sets a new password for a wallet based on the provided wallet name, address, old password, and new password.
    // ///
    // /// This function calls the `set_password` function from the wallet manager handler to update the wallet's password.
    // /// It performs the following steps:
    // /// 1. Retrieves the paths to the root and subkeys directories for the specified wallet.
    // /// 2. Traverses the directory structure to get the current wallet tree.
    // /// 3. Calls the `set_password` function to update the password, passing in the necessary parameters.
    // ///
    // /// # Arguments
    // ///
    // /// * `wallet_name` - A `String` specifying the name of the wallet.
    // /// * `address` - A `String` specifying the address associated with the wallet.
    // /// * `old_password` - A `String` containing the current password for the wallet.
    // /// * `new_password` - A `String` containing the new password for the wallet.
    // ///
    // /// # Returns
    // ///
    // /// * `ReturnType<()>` - A response indicating the success or failure of the operation.
    // pub async fn set_password(
    //     &self,
    //     // wallet_name: &str,
    //     address: &str,
    //     chain_code: &str,
    //     old_password: &str,
    //     new_password: &str,
    // ) -> ReturnType<()> {
    //     let pool = crate::manager::Context::get_global_sqlite_pool()?;
    //     let repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

    //     AccountService::new(repo)
    //         .set_password(address, chain_code, old_password, new_password)
    //         .await?
    //         .into()
    // }

    pub async fn process_jpush_message(&self, message: &str) -> ReturnType<()> {
        crate::service::jpush::JPushService::jpush(message)
            .await
            .into()
    }
}

#[cfg(test)]
mod test {
    use anyhow::Result;

    use crate::test::env::{setup_test_environment, TestData};

    #[tokio::test]
    async fn test_process_jpush_message() -> Result<()> {
        wallet_utils::init_test_log();
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        // let chain_code = "trx";
        // let account_name = "account_name1";
        // let derivation_path = Some("m/44'/60'/0'/0/1".to_string());

        let message = "{\"clientId\":\"wenjing\",\"sn\":\"device457\",\"deviceType\":\"ANDROID\",\"bizType\":\"ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG\",\"body\":{\"status\":1,\"multisigAccountId\":\"order-1\",\"addressList\":[],\"acceptStatus\":false,\"acceptAddressList\":[\"THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB\"]}}";

        let account = wallet_manager.process_jpush_message(message).await;
        tracing::info!("[test_process_jpush_message] account: {account:?}");

        Ok(())
    }
}
