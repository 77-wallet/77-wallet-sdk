pub mod account;
pub mod address_book;
pub mod announcement;
pub mod api_wallet;
pub mod app;
pub mod asset;
pub mod chain;
pub mod coin;
pub mod device;
pub mod multisig_account;
pub mod multisig_transaction;
pub mod node;
pub mod permission;
pub mod phrase;
pub mod stake;
pub mod swap;
pub mod system_notification;
// #[cfg(feature = "transaction")]
pub mod transaction;
pub mod wallet;

pub type ReturnType<T> = Result<T, crate::error::service::ServiceError>;

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use anyhow::Result;

    use crate::testkit::env::get_manager;

    #[tokio::test]
    async fn test_process_jpush_message() -> Result<()> {
        wallet_utils::init_test_log();
        let (wallet_manager, _test_params) = get_manager().await?;

        // let chain_code = "trx";
        // let account_name = "account_name1";
        // let derivation_path = Some("m/44'/60'/0'/0/1".to_string());

        let message = "{\"clientId\":\"666\",\"sn\":\"device457\",\"deviceType\":\"ANDROID\",\"bizType\":\"ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG\",\"body\":{\"status\":1,\"multisigAccountId\":\"order-1\",\"addressList\":[],\"acceptStatus\":false,\"acceptAddressList\":[\"THx9ao6pdLUFoS3CSc98pwj1HCrmGHoVUB\"]}}";

        let account = wallet_manager.process_jpush_message(message).await?;
        tracing::info!("[test_process_jpush_message] account: {account:?}");

        Ok(())
    }

    #[tokio::test]
    async fn test_get_task_queue_status() -> Result<()> {
        wallet_utils::init_test_log();
        let (wallet_manager, _test_params) = get_manager().await?;

        let status = wallet_manager.get_task_queue_status().await?;
        tracing::info!("[test_get_task_queue_status] status: {status:?}");
        let res = wallet_utils::serde_func::serde_to_string(&status).unwrap();
        tracing::info!("[test_get_task_queue_status] res: {res}");
        Ok(())
    }
}
